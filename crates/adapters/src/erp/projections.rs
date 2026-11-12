//! Bounded operator projections. All identifiers come from this static registry;
//! membership is locked by the existing domain resolver before any row is read.
use super::{
    accommodation, assets, foodservice, organizations, projects, proposals, scheduling,
    PurchaseError,
};
use archava_domain::erp::{purchasing::PurchasePrincipal, ErpError, ErpScope};
use serde_json::{json, Value};
use sqlx::PgConnection;

#[derive(Clone, Copy, Debug)]
pub enum WorkspaceDomain {
    Organizations,
    Proposals,
    Projects,
    Foodservice,
    Scheduling,
    Accommodation,
    Assets,
    Subscriptions,
    Attendance,
    Laundry,
    Field,
    Education,
    Fulfillment,
}
impl WorkspaceDomain {
    /// Every workspace domain; the host mounts one read route per entry.
    pub const ALL: [Self; 13] = [
        Self::Organizations,
        Self::Proposals,
        Self::Projects,
        Self::Foodservice,
        Self::Scheduling,
        Self::Accommodation,
        Self::Assets,
        Self::Subscriptions,
        Self::Attendance,
        Self::Laundry,
        Self::Field,
        Self::Education,
        Self::Fulfillment,
    ];
    pub fn slug(self) -> &'static str {
        match self {
            Self::Organizations => "organizations",
            Self::Proposals => "proposals",
            Self::Projects => "projects",
            Self::Foodservice => "foodservice",
            Self::Scheduling => "scheduling",
            Self::Accommodation => "accommodation",
            Self::Assets => "assets",
            Self::Subscriptions => "subscriptions",
            Self::Attendance => "attendance",
            Self::Laundry => "laundry",
            Self::Field => "field",
            Self::Education => "education",
            Self::Fulfillment => "fulfillment",
        }
    }
    /// The schema that owns this workspace's tables, from the module
    /// registry (`config/modules.v1.json`) — the same value the form
    /// catalogue and the migration manifest carry for the module.
    pub fn namespace(self) -> &'static str {
        archava_domain::modules::get(self.slug())
            .expect("every workspace domain is a registered module")
            .namespace
    }
}
struct Collection {
    id: &'static str,
    label: &'static str,
    permission: &'static str,
    from: &'static str,
    ownership: &'static str,
    key: &'static str,
}
impl Collection {
    const fn new(
        id: &'static str,
        label: &'static str,
        permission: &'static str,
        from: &'static str,
        ownership: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            permission,
            from,
            ownership,
            key: "id",
        }
    }
}
fn collections(domain: WorkspaceDomain) -> Vec<Collection> {
    use WorkspaceDomain::*;
    match domain {
        Organizations=>vec![
            Collection::new("companies","Managed companies","companies.read","archava_organizations.companies r","r.owner_id=$2"),
            Collection::new("company_audit","Company history","audit.read","archava_organizations.company_audit r JOIN archava_organizations.companies p ON p.tenant_id=r.tenant_id AND p.id=r.company_id","p.owner_id=$2"),
            Collection::new("parties","Business parties","parties.read","archava_organizations.parties r","r.owner_id=$2"),
            Collection::new("party_audit","Party history","audit.read","archava_organizations.party_audit r JOIN archava_organizations.parties p ON p.tenant_id=r.tenant_id AND p.id=r.party_id","p.owner_id=$2"),
            Collection::new("organizations","Organizations","organizations.read","archava_organizations.organizations r","r.owner_id=$2"),
            Collection::new("affiliations","Contact affiliations","affiliations.read","archava_organizations.affiliations r JOIN archava_organizations.organizations p ON p.tenant_id=r.tenant_id AND p.id=r.organization_id","p.owner_id=$2")],
        Proposals=>vec![
            Collection::new("proposals","Proposals","proposals.read","archava_proposals.proposals r","r.owner_id=$2"),
            Collection::new("scope_items","Scope items","proposals.read","archava_proposals.scope_items r JOIN archava_proposals.proposals p ON p.tenant_id=r.tenant_id AND p.id=r.proposal_id","p.owner_id=$2"),
            Collection::new("scope_versions","Scope history","proposals.read","archava_proposals.scope_versions r JOIN archava_proposals.proposals p ON p.tenant_id=r.tenant_id AND p.id=r.proposal_id","p.owner_id=$2"),
            Collection::new("change_requests","Change requests","changerequests.read","archava_proposals.change_requests r JOIN archava_proposals.proposals p ON p.tenant_id=r.tenant_id AND p.id=r.proposal_id","r.owner_id=$2 AND p.owner_id=$2")],
        Projects=>vec![
            Collection::new("projects","Projects","projects.read","archava_projects.projects r","r.owner_id=$2"),
            Collection::new("tasks","Tasks","tasks.read","archava_projects.tasks r JOIN archava_projects.projects p ON p.tenant_id=r.tenant_id AND p.id=r.project_id","p.owner_id=$2"),
            Collection::new("milestones","Milestones","milestones.read","archava_projects.milestones r JOIN archava_projects.projects p ON p.tenant_id=r.tenant_id AND p.id=r.project_id","p.owner_id=$2")],
        Foodservice=>vec![
            Collection::new("tables","Dining tables","tables.read","archava_foodservice.tables r","true"),
            Collection::new("menu_items","Menu items","menu.read","(SELECT m.*,p.title,p.sku,p.price,p.active FROM archava_foodservice.menu_items m JOIN archava_commerce.products p ON p.tenant_id=m.tenant_id AND p.id=m.id) r","true"),
            Collection::new("recipe_versions","Recipe versions","menu.read","(SELECT v.*,v.menu_id::text||':'||v.version::text AS id FROM archava_foodservice.recipe_versions v) r","true"),
            Collection::new("recipe_components","Recipe components","menu.read","(SELECT c.*,c.menu_id::text||':'||c.version::text||':'||c.product_id::text AS id FROM archava_foodservice.recipe_components c) r","true"),
            Collection::new("tickets","Kitchen tickets","tickets.read","archava_foodservice.tickets r","r.owner_id=$2"),
            Collection::new("ticket_lines","Ticket items","tickets.read","archava_foodservice.ticket_lines r JOIN archava_foodservice.tickets p ON p.tenant_id=r.tenant_id AND p.id=r.ticket_id","p.owner_id=$2")],
        Scheduling=>vec![
            Collection::new("resources","Resources","resources.read","archava_scheduling.resources r","r.owner_id=$2"),
            Collection::new("reservations","Reservations","reservations.read","archava_scheduling.reservations r","r.owner_id=$2")],
        Accommodation=>vec![
            Collection::new("properties","Properties","properties.read","archava_accommodation.properties r","r.owner_id=$2"),
            Collection::new("room_types","Room types","rooms.read","archava_accommodation.room_types r JOIN archava_accommodation.properties p ON p.tenant_id=r.tenant_id AND p.id=r.property_id","p.owner_id=$2"),
            Collection::new("rooms","Rooms","rooms.read","archava_accommodation.rooms r JOIN archava_accommodation.properties p ON p.tenant_id=r.tenant_id AND p.id=r.property_id","p.owner_id=$2"),
            Collection::new("stays","Stays","stays.read","archava_accommodation.stays r","r.owner_id=$2")],
        Subscriptions|Attendance|Laundry|Field|Education|Fulfillment=>Vec::new(),
        Assets=>vec![
            Collection::new("assets","Assets","assets.read","archava_assets.assets r","r.owner_id=$2"),
            Collection::new("custody_events","Custody history","assets.read","archava_assets.custody_events r JOIN archava_assets.assets p ON p.tenant_id=r.tenant_id AND p.id=r.asset_id","p.owner_id=$2"),
            Collection::new("maintenance_records","Maintenance history","assets.read","archava_assets.maintenance_records r JOIN archava_assets.assets p ON p.tenant_id=r.tenant_id AND p.id=r.asset_id","p.owner_id=$2")],
    }
}
fn denied() -> PurchaseError {
    PurchaseError::Domain(ErpError::Forbidden)
}
pub(super) fn camel(value: Value) -> Value {
    match value {
        Value::Object(row) => Value::Object(
            row.into_iter()
                .map(|(key, value)| {
                    let mut name = String::new();
                    let mut upper = false;
                    for c in key.chars() {
                        if c == '_' {
                            upper = true;
                        } else if upper {
                            name.extend(c.to_uppercase());
                            upper = false;
                        } else {
                            name.push(c);
                        }
                    }
                    let money = key.ends_with("_minor")
                        || matches!(
                            key.as_str(),
                            "estimated_value"
                                | "total_estimated_value"
                                | "total_value"
                                | "cost_delta_value"
                        );
                    let value = if money && value.is_number() {
                        Value::String(value.to_string())
                    } else {
                        camel(value)
                    };
                    (name, value)
                })
                .collect(),
        ),
        Value::Array(rows) => Value::Array(rows.into_iter().map(camel).collect()),
        value => value,
    }
}
#[derive(Clone, Copy)]
pub struct WorkspacePage<'a> {
    pub search: &'a str,
    pub collection: Option<&'a str>,
    pub cursor: &'a str,
    pub limit: i64,
    pub selected: Option<&'a str>,
}
/// A collection selector is matched against this registry, never interpolated
/// from a request. Pagination is by canonical row identity, with a hard limit.
pub async fn workspace_view(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    domain: WorkspaceDomain,
    page: WorkspacePage<'_>,
) -> Result<Value, PurchaseError> {
    let WorkspacePage {
        search,
        collection,
        cursor,
        limit,
        selected,
    } = page;
    if matches!(
        domain,
        WorkspaceDomain::Subscriptions
            | WorkspaceDomain::Attendance
            | WorkspaceDomain::Laundry
            | WorkspaceDomain::Field
            | WorkspaceDomain::Education
            | WorkspaceDomain::Fulfillment
    ) {
        return super::operator::workspace_view(conn, tenant, principal, domain, page).await;
    }
    macro_rules! access {
        ($resolver:path) => {{
            let a = $resolver(&mut *conn, tenant, principal).await?;
            (a.permissions, a.owner_id, a.scope)
        }};
    }
    let (permissions, owner, scope) = match domain {
        WorkspaceDomain::Subscriptions
        | WorkspaceDomain::Attendance
        | WorkspaceDomain::Laundry
        | WorkspaceDomain::Field
        | WorkspaceDomain::Education
        | WorkspaceDomain::Fulfillment => unreachable!("handled by typed operator projections"),
        WorkspaceDomain::Organizations => access!(organizations::resolve_organizations_context),
        WorkspaceDomain::Proposals => access!(proposals::resolve_proposals_context),
        WorkspaceDomain::Projects => access!(projects::resolve_projects_context),
        WorkspaceDomain::Scheduling => access!(scheduling::resolve_scheduling_context),
        WorkspaceDomain::Accommodation => access!(accommodation::resolve_accommodation_context),
        WorkspaceDomain::Assets => access!(assets::resolve_assets_context),
        WorkspaceDomain::Foodservice => {
            let a = foodservice::resolve_food_context(&mut *conn, tenant, principal).await?;
            (a.food.permissions, a.food.owner_id, a.food.scope)
        }
    };
    let definitions = collections(domain);
    if collection.is_some_and(|wanted| !definitions.iter().any(|d| d.id == wanted)) {
        return Err(PurchaseError::Domain(ErpError::NotFound));
    }
    let allowed: Vec<_> = definitions
        .into_iter()
        .filter(|d| {
            permissions.iter().any(|p| p == d.permission)
                && (d.id != "party_audit" || permissions.iter().any(|p| p == "parties.read"))
                && (d.id != "company_audit" || permissions.iter().any(|p| p == "companies.read"))
        })
        .collect();
    if allowed.is_empty()
        || collection.is_some_and(|wanted| !allowed.iter().any(|d| d.id == wanted))
    {
        return Err(denied());
    }
    let needle = format!(
        "%{}%",
        search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut result = serde_json::Map::new();
    let mut metadata = Vec::new();
    for d in allowed {
        if collection.is_some_and(|wanted| wanted != d.id) {
            continue;
        }
        let key = if matches!(d.id, "party_audit" | "company_audit") {
            "request_id"
        } else if d.id == "ticket_lines" {
            "order_line_id"
        } else {
            d.key
        };
        let document = if matches!(domain, WorkspaceDomain::Projects)
            && matches!(d.id, "tasks" | "milestones")
        {
            "to_jsonb(r)||jsonb_build_object('project_revision',p.revision)"
        } else {
            "to_jsonb(r)"
        };
        let statement=format!("SELECT {document} FROM {} WHERE r.tenant_id=$1 AND ($3 OR ({})) AND to_jsonb(r)::text ILIKE $4 ESCAPE '\\' AND r.{}::text>$5 AND ($7::text IS NULL OR r.{}::text=$7) ORDER BY r.{}::text LIMIT $6",d.from,d.ownership,key,key,key);
        let mut rows: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
            .bind(tenant)
            .bind(&owner)
            .bind(scope == ErpScope::All)
            .bind(&needle)
            .bind(cursor)
            .bind(limit + 1)
            .bind(selected)
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| super::purchase_database_error(domain.slug(), e))?;
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next = if more {
            rows.last().and_then(|r| r[key].as_str()).map(str::to_owned)
        } else {
            None
        };
        let rows = rows
            .into_iter()
            .map(|mut row| {
                if row.get("id").is_none() {
                    row["id"] = row[key].clone();
                }
                camel(row)
            })
            .collect::<Vec<_>>();
        metadata.push(
            json!({"id":d.id,"label":d.label,"primaryKey":key,"hasMore":more,"nextCursor":next}),
        );
        result.insert(d.id.into(), json!(rows));
    }
    result.insert("collections".into(), json!(metadata));
    result.insert("permissions".into(), json!(permissions));
    result.insert(
        "scope".into(),
        json!(if scope == ErpScope::All {
            "all"
        } else {
            "assigned"
        }),
    );
    Ok(Value::Object(result))
}

#[cfg(test)]
mod registry_tests {
    use super::WorkspaceDomain;
    use std::collections::BTreeSet;

    #[test]
    fn every_workspace_domain_is_a_distinct_registered_module() {
        let slugs: BTreeSet<_> = WorkspaceDomain::ALL.iter().map(|d| d.slug()).collect();
        assert_eq!(slugs.len(), WorkspaceDomain::ALL.len());
        for domain in WorkspaceDomain::ALL {
            let module = archava_domain::modules::get(domain.slug())
                .unwrap_or_else(|| panic!("{} is not in the module registry", domain.slug()));
            assert_eq!(domain.namespace(), module.namespace);
            assert!(domain.namespace().starts_with("archava_"));
        }
    }
}
