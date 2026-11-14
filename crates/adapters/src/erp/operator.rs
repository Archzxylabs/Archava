//! Compose existing scoped readers into native workspaces without broad SQL reads.
use super::{
    attendance, field_services, formal_education, fulfillment, laundry,
    projections::{camel, WorkspaceDomain, WorkspacePage},
    subscriptions, PurchaseError,
};
use archava_domain::erp::{
    field_services::FieldJobPageQuery, formal_education::EnrollmentPageQuery,
    fulfillment::JobPageQuery, laundry::OrderPageQuery, purchasing::PurchasePrincipal, ErpError,
    ErpScope,
};
use serde_json::{json, Value};
use sqlx::PgConnection;
impl From<formal_education::FormalEducationError> for PurchaseError {
    fn from(value: formal_education::FormalEducationError) -> Self {
        use formal_education::FormalEducationError as E;
        match value {
            E::Forbidden => Self::Domain(ErpError::Forbidden),
            E::NotFound => Self::Domain(ErpError::NotFound),
            E::Database { ledger, source } => Self::Database { ledger, source },
            _ => Self::Domain(ErpError::Conflict),
        }
    }
}
fn document(value: impl serde::Serialize) -> Result<Value, PurchaseError> {
    serde_json::to_value(value)
        .map(camel)
        .map_err(|_| PurchaseError::Domain(ErpError::Unavailable))
}
fn rows(mut value: Value) -> Value {
    if let Some(items) = value.as_array_mut() {
        for row in items {
            if row.get("id").is_none() {
                for key in [
                    "jobId",
                    "orderId",
                    "enrollmentId",
                    "sessionId",
                    "subscriptionId",
                ] {
                    if let Some(id) = row.get(key).cloned() {
                        row["id"] = id;
                        break;
                    }
                }
            }
        }
    }
    value
}
pub async fn workspace_view(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    domain: WorkspaceDomain,
    page: WorkspacePage<'_>,
) -> Result<Value, PurchaseError> {
    let WorkspacePage {
        search,
        collection: wanted_collection,
        cursor,
        limit,
        selected,
    } = page;
    let offset = if matches!(domain, WorkspaceDomain::Field | WorkspaceDomain::Education)
        || cursor.is_empty()
    {
        0
    } else {
        cursor
            .parse::<i64>()
            .ok()
            .filter(|n| (0..=1_000_000).contains(n))
            .ok_or(PurchaseError::Domain(ErpError::Conflict))?
    };
    let next = if cursor.is_empty() {
        None
    } else {
        Some(cursor.to_string())
    };
    macro_rules! detail {
        ($id:ident,$call:expr) => {
            if let Some($id) = selected {
                document(
                    $call
                        .await
                        .map_err(PurchaseError::from)?
                        .ok_or(PurchaseError::Domain(ErpError::NotFound))?,
                )?
            } else {
                Value::Null
            }
        };
    }
    let (collection, label, permissions, scope, page, detail) = match domain {
        WorkspaceDomain::Subscriptions => {
            let a =
                subscriptions::resolve_subscriptions_context(&mut *conn, tenant, principal).await?;
            let list = subscriptions::list_subscriptions(
                &mut *conn,
                tenant,
                &a,
                &subscriptions::SubscriptionPageQuery {
                    limit: limit + 1,
                    offset,
                    status: None,
                    contact_id: None,
                },
            )
            .await?;
            let d = detail!(
                id,
                subscriptions::read_subscription_detail(&mut *conn, tenant, &a, id)
            );
            (
                "subscriptions",
                "Subscriptions",
                a.permissions,
                json!(a.scope),
                document(list)?,
                d,
            )
        }
        WorkspaceDomain::Attendance => {
            let a = attendance::resolve_attendance_context(&mut *conn, tenant, principal).await?;
            let list = attendance::list_attendance_sessions(
                &mut *conn,
                tenant,
                &a,
                &attendance::AttendancePageQuery {
                    limit: limit + 1,
                    offset,
                    status: None,
                },
            )
            .await?;
            let d = detail!(
                id,
                attendance::read_attendance_detail(&mut *conn, tenant, &a, id)
            );
            (
                "sessions",
                "Sessions",
                a.permissions,
                json!(a.scope),
                document(list)?,
                d,
            )
        }
        WorkspaceDomain::Laundry => {
            let a = laundry::resolve_laundry_context(&mut *conn, tenant, principal).await?;
            let list = laundry::list_laundry_orders(
                &mut *conn,
                tenant,
                &a,
                &OrderPageQuery {
                    limit: limit + 1,
                    offset,
                    status: None,
                    kind: None,
                },
            )
            .await?;
            let d = detail!(
                id,
                laundry::get_laundry_order_detail(&mut *conn, tenant, &a, id)
            );
            (
                "orders",
                "Laundry orders",
                a.permissions,
                json!(if a.scope == ErpScope::All {
                    "all"
                } else {
                    "assigned"
                }),
                document(list)?,
                d,
            )
        }
        WorkspaceDomain::Field => {
            let a = field_services::resolve_field_context(&mut *conn, tenant, principal).await?;
            let list = field_services::list_field_jobs(
                &mut *conn,
                tenant,
                principal,
                FieldJobPageQuery {
                    limit: limit + 1,
                    cursor: next,
                    status: None,
                },
            )
            .await?;
            let d = detail!(
                id,
                field_services::get_field_job_detail(&mut *conn, tenant, principal, id)
            );
            (
                "jobs",
                "Service jobs",
                a.permissions,
                json!(if a.scope == ErpScope::All {
                    "all"
                } else {
                    "assigned"
                }),
                document(list)?,
                d,
            )
        }
        WorkspaceDomain::Education => {
            let a =
                formal_education::resolve_education_context(&mut *conn, tenant, principal).await?;
            let list = formal_education::list_education_enrollments(
                &mut *conn,
                tenant,
                principal,
                EnrollmentPageQuery {
                    limit: limit + 1,
                    cursor: next,
                    status: None,
                },
            )
            .await?;
            let d = detail!(
                id,
                formal_education::get_education_enrollment_detail(
                    &mut *conn, tenant, principal, id
                )
            );
            (
                "enrollments",
                "Enrollments",
                a.permissions,
                json!(a.scope.as_str()),
                document(list)?,
                d,
            )
        }
        WorkspaceDomain::Fulfillment => {
            let a = fulfillment::resolve_fulfillment_context(&mut *conn, tenant, principal).await?;
            let list = fulfillment::list_fulfillment_jobs(
                &mut *conn,
                tenant,
                principal,
                JobPageQuery {
                    limit: limit + 1,
                    offset,
                    status: None,
                    kind: None,
                },
            )
            .await?;
            let d = detail!(
                id,
                fulfillment::get_fulfillment_job_detail(&mut *conn, tenant, principal, id)
            );
            (
                "jobs",
                "Fulfillment jobs",
                a.permissions,
                json!(if a.scope == ErpScope::All {
                    "all"
                } else {
                    "assigned"
                }),
                document(list)?,
                d,
            )
        }
        _ => return Err(PurchaseError::Domain(ErpError::NotFound)),
    };
    if wanted_collection.is_some_and(|wanted| wanted != collection) {
        return Err(PurchaseError::Domain(ErpError::NotFound));
    }
    let mut page = rows(page);
    let items = page
        .as_array_mut()
        .ok_or(PurchaseError::Domain(ErpError::Unavailable))?;
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let next = if more {
        if matches!(domain, WorkspaceDomain::Field | WorkspaceDomain::Education) {
            items
                .last()
                .and_then(|r| r["id"].as_str())
                .map(str::to_owned)
        } else {
            Some((offset + limit).to_string())
        }
    } else {
        None
    };
    if !search.is_empty() {
        let needle = search.to_lowercase();
        items.retain(|row| row.to_string().to_lowercase().contains(&needle));
    }
    let mut result = json!({"collections":[{"id":collection,"label":label,"primaryKey":"id","hasMore":more,"nextCursor":next}],"permissions":permissions,"scope":scope,"detail":detail});
    result[collection] = page;
    Ok(result)
}
