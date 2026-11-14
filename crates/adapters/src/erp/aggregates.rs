//! Closed aggregate reads: a count or a sum over the whole filtered set of one
//! collection, computed by one SQL statement inside the tenant transaction.
//!
//! The public vocabulary (which collection, which amount fields, which
//! dimensions and filters) is declared in the module registry
//! (`config/modules.v1.json`, `agent.aggregates`). This file is the other
//! half: the fixed SQL behind each declared name. A request is served only
//! when the registry lists the name AND this file implements it; a caller
//! value never becomes SQL text, only a bound parameter.
//!
//! An aggregate sees exactly the rows the collection's page read
//! (`workspace_view_filtered`) can show its caller:
//!
//! - the module's read permission, resolved by the same context resolver;
//! - the membership scope (`all`, or only rows the caller owns);
//! - company access: a row without a company, or a row of a company the
//!   caller can read through `companies.read` under the Organizations scope —
//!   the rule `company_trade::resolve` applies before a company page read;
//! - the tenant, by FORCE RLS and the explicit predicate.
//!
//! The caller may narrow the company scope further (the owner's AI sharing
//! selection); it can never widen it.
//!
//! Money is never added across currencies. Every sum is grouped by the
//! record's currency first: an IDR row is whole Rupiah, a row in any other
//! currency is that currency's ISO minor unit
//! (`docs/architecture/MULTI_CURRENCY.md`).
use super::{billing, commerce_kernel, companies, company_trade, purchasing, PurchaseError};
use archava_domain::erp::{purchasing::PurchasePrincipal, ErpError, ErpScope};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// Groups returned per request, across every currency section.
pub const GROUP_LIMIT: i64 = 50;
/// Distinct currencies one aggregate reports. More is refused, not truncated.
pub const CURRENCY_LIMIT: i64 = 50;

/// The fixed SQL behind one declared aggregate collection.
struct Source {
    module: &'static str,
    collection: &'static str,
    ledger: &'static str,
    table: &'static str,
    /// A fixed predicate the page read also applies.
    base: &'static str,
    /// The state filter and dimension: public name, column, accepted values.
    state: (&'static str, &'static str, &'static [&'static str]),
    /// The `timestamptz` column behind `dateFrom`/`dateTo` and `month`.
    date: (&'static str, &'static str),
    /// Amount name → `bigint` column, in the row's own currency unit.
    sums: &'static [(&'static str, &'static str)],
    /// Counterparty dimension: public name, grouping key, label.
    party: (&'static str, &'static str, &'static str),
}

const SOURCES: &[Source] = &[
    Source {
        module: "billing",
        collection: "invoices",
        ledger: "billing",
        table: "archava_billing.invoices",
        base: "true",
        state: ("status", "t.status", &["draft", "issued", "settled", "void"]),
        date: ("issued", "t.issued_at"),
        sums: &[
            ("gross", "t.gross_minor"),
            ("outstanding", "t.outstanding_minor"),
            ("tax", "t.tax_minor"),
        ],
        // One customer is the recorded party or contact; an invoice without
        // either is grouped by the customer text as written.
        party: (
            "customer",
            "coalesce('party:'||t.customer_party_id::text,'contact:'||t.customer_contact_id::text,'name:'||t.customer_ref)",
            "t.customer_ref",
        ),
    },
    Source {
        module: "commerce",
        collection: "orders",
        ledger: "commerce",
        table: "archava_commerce.orders",
        // Foodservice-managed orders are not sales orders of the workspace.
        base: "t.managed_by IS NULL",
        state: (
            "state",
            "t.state",
            &["draft", "reserved", "fulfilled", "cancelled", "returned"],
        ),
        date: ("created", "t.created_at"),
        sums: &[("total", "t.total_estimate")],
        party: (
            "customer",
            "coalesce('party:'||t.customer_party_id::text,'contact:'||t.customer_contact_id::text,'name:'||t.customer_name)",
            "t.customer_name",
        ),
    },
    Source {
        module: "purchasing",
        collection: "orders",
        ledger: "purchasing",
        table: "archava_purchasing.orders",
        base: "true",
        state: (
            "state",
            "t.state",
            &[
                "draft",
                "confirmed",
                "partial",
                "received",
                "cancelled",
                "short_closed",
            ],
        ),
        date: ("created", "t.created_at"),
        sums: &[("total", "t.total_estimate")],
        party: ("supplier", "t.supplier_id::text", "t.supplier_name"),
    },
];

/// Why a request is not served. Each is a closed reason the caller can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The registry declares no aggregate for this module and collection.
    CollectionNotListed,
    /// The sum names an amount field the collection does not declare.
    MeasureNotListed,
    /// The dimension is not declared for the collection.
    DimensionNotListed,
    /// The filter is not declared for the collection.
    FilterNotListed,
    /// A filter value is not a value this collection can hold.
    FilterInvalid,
    /// The caller lacks the collection's read permission.
    NotPermitted,
    /// The company filter names a company the caller cannot read.
    CompanyNotReadable,
    /// More currencies than one aggregate reports.
    TooManyCurrencies,
    /// The statement exceeded its time limit; nothing is reported.
    TimedOut,
}
impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::CollectionNotListed => "aggregate_collection_not_listed",
            Self::MeasureNotListed => "aggregate_measure_not_listed",
            Self::DimensionNotListed => "aggregate_dimension_not_listed",
            Self::FilterNotListed => "aggregate_filter_not_listed",
            Self::FilterInvalid => "aggregate_filter_invalid",
            Self::NotPermitted => "aggregate_not_permitted",
            Self::CompanyNotReadable => "aggregate_company_not_readable",
            Self::TooManyCurrencies => "aggregate_too_many_currencies",
            Self::TimedOut => "aggregate_timed_out",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AggregateError {
    #[error("aggregate refused: {}", .0.code())]
    Refused(Refusal),
    #[error(transparent)]
    Storage(#[from] PurchaseError),
}
impl From<Refusal> for AggregateError {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

/// Which company rows the caller lets the aggregate count, at most. Access is
/// still decided per row; this can only narrow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyScope {
    /// `None`: every company the caller can read. `Some`: only these.
    pub companies: Option<Vec<Uuid>>,
    /// Whether rows without a company count.
    pub unassigned: bool,
}

/// One aggregate, in the registry's vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request<'a> {
    pub module: &'a str,
    pub collection: &'a str,
    /// `None` is a count; `Some(field)` the sum of that amount field.
    pub sum: Option<&'a str>,
    pub group_by: Option<&'a str>,
    /// Filter name → value, as declared for the collection.
    pub filters: &'a [(&'a str, &'a str)],
    pub scope: CompanyScope,
}

/// The filter as it was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedFilter {
    /// The state filter's public name and value.
    pub state: Option<(&'static str, String)>,
    /// Which date the bounds read (`issued`, `created`).
    pub date: &'static str,
    /// Inclusive UTC calendar dates.
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub company: Option<Uuid>,
    /// Whether rows of other owners were visible to the caller.
    pub every_owner: bool,
    pub scope: CompanyScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// `None` when the dimension has no value on these rows (for example the
    /// issue month of an invoice that was never issued).
    pub label: Option<String>,
    pub rows: i64,
    pub total: Option<i128>,
}

/// Groups beyond the limit, folded so the section total stays exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fold {
    pub groups: i64,
    pub rows: i64,
    pub total: Option<i128>,
}

/// The rows of one currency. A count has a single section without a currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub currency: Option<String>,
    pub rows: i64,
    pub total: Option<i128>,
    /// Distinct groups in this section, returned and folded.
    pub group_count: i64,
    /// Ordered by the measure, largest first.
    pub groups: Vec<Group>,
    pub other: Option<Fold>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub filter: AppliedFilter,
    /// Rows the aggregate covers: the whole filtered set.
    pub rows: i64,
    /// UTC time of the read, `YYYY-MM-DDTHH:MM:SSZ`.
    pub observed_at: String,
    pub sections: Vec<Section>,
}

fn source(module: &str, collection: &str) -> Result<&'static Source, Refusal> {
    // Both halves must agree: declared in the registry, implemented here.
    archava_domain::modules::aggregate(module, collection).ok_or(Refusal::CollectionNotListed)?;
    SOURCES
        .iter()
        .find(|s| s.module == module && s.collection == collection)
        .ok_or(Refusal::CollectionNotListed)
}

/// A real calendar date written `YYYY-MM-DD`.
fn calendar_date(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = &text[range];
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (year >= 1900 && (1..=days).contains(&day)).then(|| text.to_owned())
}

struct Planned {
    source: &'static Source,
    amount: Option<&'static str>,
    /// Grouping key and label expressions.
    group: Option<(&'static str, &'static str)>,
    month: bool,
    state: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    company: Option<Uuid>,
}

/// Checks a request against the registry and this file. No database access.
fn plan(request: &Request<'_>) -> Result<Planned, Refusal> {
    let source = source(request.module, request.collection)?;
    let declared = archava_domain::modules::aggregate(request.module, request.collection)
        .ok_or(Refusal::CollectionNotListed)?;
    let amount = match request.sum {
        None => None,
        Some(field) => Some(
            source
                .sums
                .iter()
                .find(|(name, _)| *name == field && declared.sums.contains(name))
                .map(|(_, column)| *column)
                .ok_or(Refusal::MeasureNotListed)?,
        ),
    };
    let mut month = false;
    let group = match request.group_by {
        None => None,
        Some(dimension) => {
            if !declared.dimensions.iter().any(|d| d.id == dimension) {
                return Err(Refusal::DimensionNotListed);
            }
            if dimension == source.state.0 {
                Some((source.state.1, source.state.1))
            } else if dimension == source.party.0 {
                Some((source.party.1, source.party.2))
            } else if dimension == "month" {
                month = true;
                None
            } else {
                return Err(Refusal::DimensionNotListed);
            }
        }
    };
    let (mut state, mut date_from, mut date_to, mut company) = (None, None, None, None);
    for (index, (name, value)) in request.filters.iter().enumerate() {
        if !declared.filters.contains(name)
            || request.filters[..index]
                .iter()
                .any(|(seen, _)| seen == name)
        {
            return Err(Refusal::FilterNotListed);
        }
        if *name == source.state.0 {
            if !source.state.2.contains(value) {
                return Err(Refusal::FilterInvalid);
            }
            state = Some((*value).to_owned());
        } else if *name == "dateFrom" {
            date_from = Some(calendar_date(value).ok_or(Refusal::FilterInvalid)?);
        } else if *name == "dateTo" {
            date_to = Some(calendar_date(value).ok_or(Refusal::FilterInvalid)?);
        } else if *name == "companyId" {
            company = Some(Uuid::parse_str(value).map_err(|_| Refusal::FilterInvalid)?);
        } else {
            return Err(Refusal::FilterNotListed);
        }
    }
    if date_from
        .as_ref()
        .zip(date_to.as_ref())
        .is_some_and(|(from, to)| from > to)
    {
        return Err(Refusal::FilterInvalid);
    }
    Ok(Planned {
        source,
        amount,
        group,
        month,
        state,
        date_from,
        date_to,
        company,
    })
}

/// The values a collection's state filter accepts, with the filter's name.
pub fn states(module: &str, collection: &str) -> Option<(&'static str, &'static [&'static str])> {
    source(module, collection)
        .ok()
        .map(|source| (source.state.0, source.state.2))
}

/// Whether the registry and this file would serve the request's names. Used to
/// refuse before any transaction is opened; `run` checks again.
pub fn check(request: &Request<'_>) -> Result<(), Refusal> {
    plan(request).map(|_| ())
}

/// The statement of one planned aggregate. Only fixed fragments are composed;
/// every caller value is a bound parameter.
fn statement(planned: &Planned) -> String {
    let source = planned.source;
    let date = source.date.1;
    let (key, label) = if planned.month {
        let month = format!("to_char({date} AT TIME ZONE 'UTC','YYYY-MM')");
        (month.clone(), month)
    } else {
        planned
            .group
            .map(|(key, label)| (key.to_owned(), label.to_owned()))
            .unwrap_or_else(|| ("NULL::text".to_owned(), "NULL::text".to_owned()))
    };
    // A count is not money: it has no currency section.
    let (currency, amount, measure) = match planned.amount {
        Some(column) => ("t.currency", format!("{column}::numeric"), "total"),
        None => ("NULL::text", "NULL::numeric".to_owned(), "n"),
    };
    format!(
        "WITH scoped AS MATERIALIZED (\
           SELECT {currency} AS currency,{amount} AS amount,{key} AS grp,{label} AS label,t.created_at AS seen \
           FROM {table} t \
           WHERE t.tenant_id=$1 AND {base} \
             AND ($2 OR t.owner_id=$3) \
             AND ((t.company_id IS NULL AND $4) \
               OR (t.company_id IS NOT NULL AND ($5::uuid[] IS NULL OR t.company_id=ANY($5)) \
                 AND EXISTS(SELECT 1 FROM archava_organizations.companies c \
                   WHERE c.tenant_id=t.tenant_id AND c.id=t.company_id AND $6 AND ($7 OR c.owner_id=$8)))) \
             AND ($9::text IS NULL OR {state}=$9) \
             AND ($10::text IS NULL OR ({date} AT TIME ZONE 'UTC')::date>=$10::text::date) \
             AND ($11::text IS NULL OR ({date} AT TIME ZONE 'UTC')::date<=$11::text::date)),\
         grouped AS (\
           SELECT currency,grp,(array_agg(label ORDER BY seen DESC))[1] AS label,count(*) AS n,coalesce(sum(amount),0) AS total \
           FROM scoped GROUP BY currency,grp),\
         sections AS (SELECT count(*) AS currencies FROM (SELECT DISTINCT currency FROM grouped) d),\
         ranked AS (\
           SELECT g.*,row_number() OVER (PARTITION BY g.currency ORDER BY g.{measure} DESC,g.n DESC,g.label NULLS LAST,g.grp NULLS LAST) AS position,\
             GREATEST(1,$12::bigint/GREATEST(1,s.currencies)) AS quota,s.currencies \
           FROM grouped g CROSS JOIN sections s) \
         SELECT currency,kept,position,label,n,total::text AS total,folded,currencies FROM (\
           SELECT currency,true AS kept,position,label,n::bigint AS n,total,1::bigint AS folded,currencies FROM ranked WHERE position<=quota \
           UNION ALL \
           SELECT currency,false,NULL::bigint,NULL::text,sum(n)::bigint,sum(total),count(*)::bigint,max(currencies) FROM ranked WHERE position>quota GROUP BY currency) x \
         ORDER BY currency NULLS FIRST,kept DESC,position \
         LIMIT $13",
        table = source.table,
        base = source.base,
        state = source.state.1,
    )
}

fn amount(
    text: Option<String>,
    sum: bool,
    ledger: &'static str,
) -> Result<Option<i128>, PurchaseError> {
    if !sum {
        return Ok(None);
    }
    text.as_deref()
        .and_then(|digits| digits.parse::<i128>().ok())
        .map(Some)
        .ok_or_else(|| PurchaseError::Database {
            ledger,
            source: sqlx::Error::Decode("aggregate total is not an integer".into()),
        })
}

/// Computes one aggregate over the whole filtered set.
///
/// The caller owns the tenant transaction and has already checked that the
/// module is enabled, as it does before the collection's page read.
pub async fn run(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    request: &Request<'_>,
) -> Result<Outcome, AggregateError> {
    let planned = plan(request)?;
    let source = planned.source;
    let ledger = source.ledger;
    let db = move |error: sqlx::Error| super::purchase_database_error(ledger, error);
    let denied = |error: PurchaseError| match error {
        PurchaseError::Domain(ErpError::Forbidden | ErpError::NotFound) => {
            AggregateError::Refused(Refusal::NotPermitted)
        }
        other => AggregateError::Storage(other),
    };
    // The module's own read permission and row scope, from the resolver its
    // page read uses.
    let (every_owner, owner_id) = match source.module {
        "billing" => {
            let access = billing::resolve_billing_context(&mut *connection, tenant, principal)
                .await
                .map_err(denied)?;
            if !access.holds("billing.read") {
                return Err(Refusal::NotPermitted.into());
            }
            (access.scope == ErpScope::All, access.owner_id)
        }
        "commerce" => {
            let context =
                commerce_kernel::resolve_commerce_context(&mut *connection, tenant, principal)
                    .await
                    .map_err(denied)?;
            commerce_kernel::verify_commerce_context(&mut *connection, tenant).await?;
            if !context.access.holds(&["orders.read"]) {
                return Err(Refusal::NotPermitted.into());
            }
            (context.access.scope == ErpScope::All, context.owner_id)
        }
        "purchasing" => {
            let context =
                purchasing::resolve_purchasing_context(&mut *connection, tenant, principal)
                    .await
                    .map_err(denied)?;
            let access = context.context().narrowed().purchasing;
            if !access.permissions.iter().any(|p| p == "orders.read") {
                return Err(Refusal::NotPermitted.into());
            }
            (access.scope == ErpScope::All, access.owner_id)
        }
        _ => return Err(Refusal::CollectionNotListed.into()),
    };
    // Company rows count only under `companies.read`, within the caller's
    // Organizations scope: what `company_trade::resolve` requires per company.
    let (company_read, every_company, company_owner) =
        match companies::access(&mut *connection, tenant, principal).await {
            Ok(access) => (
                access.holds(&["companies.read"]),
                access.scope.covers_all(),
                access.owner_id,
            ),
            Err(PurchaseError::Domain(ErpError::Forbidden | ErpError::NotFound)) => {
                (false, false, String::new())
            }
            Err(other) => return Err(other.into()),
        };
    let mut scope = request.scope.clone();
    if let Some(company) = planned.company {
        match company_trade::resolve(&mut *connection, tenant, principal, company).await {
            Ok(_) => {}
            Err(PurchaseError::Domain(ErpError::Forbidden | ErpError::NotFound)) => {
                return Err(Refusal::CompanyNotReadable.into())
            }
            Err(other) => return Err(other.into()),
        }
        if scope
            .companies
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(&company))
        {
            return Err(Refusal::CompanyNotReadable.into());
        }
        scope = CompanyScope {
            companies: Some(vec![company]),
            unassigned: false,
        };
    }
    // The same bound the other bounded reads run under.
    sqlx::query("SET LOCAL statement_timeout='10s'")
        .execute(&mut *connection)
        .await
        .map_err(db)?;
    let statement = statement(&planned);
    let rows = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .bind(every_owner)
        .bind(&owner_id)
        .bind(scope.unassigned)
        .bind(scope.companies.as_deref())
        .bind(company_read)
        .bind(every_company)
        .bind(&company_owner)
        .bind(planned.state.as_deref())
        .bind(planned.date_from.as_deref())
        .bind(planned.date_to.as_deref())
        .bind(GROUP_LIMIT)
        // At most the limit plus one fold per currency; see the check below.
        .bind(GROUP_LIMIT + CURRENCY_LIMIT + 1)
        .fetch_all(&mut *connection)
        .await
        .map_err(|error| {
            let timed_out = error
                .as_database_error()
                .is_some_and(|e| e.code().as_deref() == Some("57014"));
            if timed_out {
                AggregateError::Refused(Refusal::TimedOut)
            } else {
                AggregateError::Storage(db(error))
            }
        })?;
    let observed_at: String = sqlx::query_scalar(
        "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(db)?;
    let sum = planned.amount.is_some();
    let grouped = planned.group.is_some() || planned.month;
    let mut sections: Vec<Section> = Vec::new();
    for row in rows {
        let currencies: i64 = row.try_get("currencies").map_err(db)?;
        if currencies > CURRENCY_LIMIT {
            return Err(Refusal::TooManyCurrencies.into());
        }
        let currency: Option<String> = row.try_get("currency").map_err(db)?;
        let kept: bool = row.try_get("kept").map_err(db)?;
        let n: i64 = row.try_get("n").map_err(db)?;
        let total = amount(row.try_get("total").map_err(db)?, sum, ledger)?;
        let folded: i64 = row.try_get("folded").map_err(db)?;
        if sections.last().is_none_or(|s| s.currency != currency) {
            sections.push(Section {
                currency: currency.clone(),
                rows: 0,
                total: sum.then_some(0),
                group_count: 0,
                groups: Vec::new(),
                other: None,
            });
        }
        let section = sections.last_mut().expect("a section was just ensured");
        section.rows += n;
        section.group_count += folded;
        if let (Some(sum), Some(part)) = (section.total.as_mut(), total) {
            *sum += part;
        }
        if kept {
            if grouped {
                section.groups.push(Group {
                    label: row.try_get("label").map_err(db)?,
                    rows: n,
                    total,
                });
            }
        } else {
            section.other = Some(Fold {
                groups: folded,
                rows: n,
                total,
            });
        }
    }
    if !grouped {
        for section in &mut sections {
            section.group_count = 0;
        }
    }
    Ok(Outcome {
        filter: AppliedFilter {
            state: planned.state.map(|value| (source.state.0, value)),
            date: source.date.0,
            date_from: planned.date_from,
            date_to: planned.date_to,
            company: planned.company,
            every_owner,
            scope,
        },
        rows: sections.iter().map(|s| s.rows).sum(),
        observed_at,
        sections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(
        module: &'a str,
        collection: &'a str,
        sum: Option<&'a str>,
        group_by: Option<&'a str>,
        filters: &'a [(&'a str, &'a str)],
    ) -> Request<'a> {
        Request {
            module,
            collection,
            sum,
            group_by,
            filters,
            scope: CompanyScope {
                companies: None,
                unassigned: true,
            },
        }
    }

    #[test]
    fn the_registry_and_the_sql_table_declare_exactly_the_same_names() {
        let mut declared = 0;
        for module in archava_domain::modules::all() {
            for aggregate in module.aggregates {
                declared += 1;
                let source = source(module.id, aggregate.collection).unwrap_or_else(|_| {
                    panic!("{}.{} has no SQL", module.id, aggregate.collection)
                });
                assert_eq!(
                    aggregate.sums,
                    source
                        .sums
                        .iter()
                        .map(|(name, _)| *name)
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    aggregate
                        .dimensions
                        .iter()
                        .map(|d| d.id)
                        .collect::<Vec<_>>(),
                    [source.state.0, source.party.0, "month"]
                );
                assert_eq!(
                    aggregate.filters,
                    [source.state.0, "dateFrom", "dateTo", "companyId"]
                );
                assert_eq!(aggregate.date, source.date.0);
                // Counterparty labels are names; everything else is a fact.
                for dimension in aggregate.dimensions {
                    assert_eq!(
                        dimension.class == "names",
                        dimension.id == source.party.0,
                        "{}",
                        dimension.id
                    );
                }
            }
        }
        assert_eq!(declared, SOURCES.len(), "SQL without a registry entry");
    }

    #[test]
    fn anything_outside_the_allowlist_is_refused_with_its_own_reason() {
        let none: &[(&str, &str)] = &[];
        assert!(check(&request("billing", "invoices", None, None, none)).is_ok());
        assert!(check(&request(
            "billing",
            "invoices",
            Some("outstanding"),
            Some("customer"),
            &[
                ("status", "issued"),
                ("dateFrom", "2026-11-01"),
                ("dateTo", "2026-11-30")
            ]
        ))
        .is_ok());
        for (request, refusal) in [
            (
                request("billing", "payment_events", None, None, none),
                Refusal::CollectionNotListed,
            ),
            (
                request("accounting", "journals", None, None, none),
                Refusal::CollectionNotListed,
            ),
            (
                request("crm", "contacts", None, None, none),
                Refusal::CollectionNotListed,
            ),
            (
                request("billing", "invoices", Some("settled"), None, none),
                Refusal::MeasureNotListed,
            ),
            (
                request("commerce", "orders", Some("gross"), None, none),
                Refusal::MeasureNotListed,
            ),
            (
                request("billing", "invoices", Some("gross_minor"), None, none),
                Refusal::MeasureNotListed,
            ),
            (
                request("billing", "invoices", None, Some("owner"), none),
                Refusal::DimensionNotListed,
            ),
            (
                request("billing", "invoices", None, Some("supplier"), none),
                Refusal::DimensionNotListed,
            ),
            (
                request("purchasing", "orders", None, Some("customer"), none),
                Refusal::DimensionNotListed,
            ),
            (
                request("billing", "invoices", None, None, &[("state", "issued")]),
                Refusal::FilterNotListed,
            ),
            (
                request("billing", "invoices", None, None, &[("ownerId", "alice")]),
                Refusal::FilterNotListed,
            ),
            (
                request(
                    "billing",
                    "invoices",
                    None,
                    None,
                    &[("status", "issued"), ("status", "void")],
                ),
                Refusal::FilterNotListed,
            ),
            (
                request("billing", "invoices", None, None, &[("status", "paid")]),
                Refusal::FilterInvalid,
            ),
            (
                request(
                    "billing",
                    "invoices",
                    None,
                    None,
                    &[("status", "issued' OR '1'='1")],
                ),
                Refusal::FilterInvalid,
            ),
            (
                request(
                    "billing",
                    "invoices",
                    None,
                    None,
                    &[("dateFrom", "2026-02-30")],
                ),
                Refusal::FilterInvalid,
            ),
            (
                request(
                    "billing",
                    "invoices",
                    None,
                    None,
                    &[("dateTo", "30/11/2026")],
                ),
                Refusal::FilterInvalid,
            ),
            (
                request(
                    "billing",
                    "invoices",
                    None,
                    None,
                    &[("dateFrom", "2026-12-01"), ("dateTo", "2026-11-30")],
                ),
                Refusal::FilterInvalid,
            ),
            (
                request("billing", "invoices", None, None, &[("companyId", "acme")]),
                Refusal::FilterInvalid,
            ),
        ] {
            assert_eq!(check(&request), Err(refusal), "{request:?}");
        }
    }

    #[test]
    fn calendar_dates_are_real_days() {
        assert_eq!(calendar_date("2024-02-29").as_deref(), Some("2024-02-29"));
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-11-31",
            "2026-1-01",
            "2026-11-1",
            "20261101",
            " 2026-11-01",
            "2026-11-01T00:00:00Z",
            "0001-01-01",
            "",
        ] {
            assert!(calendar_date(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_statement_is_composed_of_fixed_text_and_bound_parameters_only() {
        let filters = [("status", "issued")];
        let planned = plan(&request(
            "billing",
            "invoices",
            Some("outstanding"),
            Some("customer"),
            &filters,
        ))
        .unwrap();
        let sql = statement(&planned);
        assert!(!sql.contains("issued'"), "a filter value is never SQL text");
        assert!(sql.contains("t.currency AS currency"));
        assert!(sql.contains("t.outstanding_minor::numeric AS amount"));
        assert!(sql.contains("($2 OR t.owner_id=$3)"));
        assert!(sql.contains("archava_organizations.companies"));
        let count =
            statement(&plan(&request("commerce", "orders", None, Some("month"), &[])).unwrap());
        assert!(
            count.contains("NULL::text AS currency"),
            "a count has no currency"
        );
        assert!(count.contains("t.managed_by IS NULL"));
        assert!(count.contains("to_char(t.created_at AT TIME ZONE 'UTC','YYYY-MM')"));
    }
}
