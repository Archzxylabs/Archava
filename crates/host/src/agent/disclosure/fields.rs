//! Closed provider projection. New response fields/collections default to withheld.
use archava_own::{DataClass, DisclosurePolicy};
use serde_json::{Map, Value};

pub(super) fn collections(module: &str) -> &'static [&'static str] {
    match module {
        "crm" => &["items", "contacts"],
        "commerce" => &["products", "orders"],
        "purchasing" => &["suppliers", "offers", "orders", "documents"],
        "billing" => &["invoices"],
        "support" => &["items", "cases"],
        "organizations" => &["companies", "parties", "organizations", "affiliations"],
        "projects" => &["projects", "tasks", "milestones"],
        "proposals" => &[
            "proposals",
            "scope_items",
            "scope_versions",
            "change_requests",
        ],
        "foodservice" => &[
            "tables",
            "menu_items",
            "recipe_versions",
            "recipe_components",
            "tickets",
            "ticket_lines",
        ],
        "scheduling" => &["resources", "reservations"],
        "accommodation" => &["properties", "room_types", "rooms", "stays"],
        "assets" => &["assets", "custody_events", "maintenance_records"],
        "subscriptions" => &["subscriptions"],
        "attendance" => &["sessions"],
        "laundry" => &["orders"],
        "field" | "fulfillment" => &["jobs"],
        "education" => &["enrollments"],
        "accounting" => &[
            "books",
            "accounts",
            "periods",
            "journals",
            "invoice_policies",
            "invoice_sources",
            "purchase_policies",
            "purchase_sources",
            "stock_policies",
            "stock_sources",
        ],
        _ => &[],
    }
}
fn key(field: &str) -> String {
    field
        .chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}
fn class(field: &str) -> Option<DataClass> {
    use DataClass::*;
    Some(match key(field).as_str() {
        "id"
        | "revision"
        | "expectedrevision"
        | "stockrevision"
        | "projectrevision"
        | "companyid"
        | "companyrevision"
        | "ownerid"
        | "actorid"
        | "principalid"
        | "contactid"
        | "contactrevision"
        | "customercontactid"
        | "customercontactrevision"
        | "suppliercontactid"
        | "suppliercontactrevision"
        | "customerpartyid"
        | "customerpartyrevision"
        | "supplierpartyid"
        | "supplierpartyrevision"
        | "partyid"
        | "partyrevision"
        | "organizationid"
        | "sourcerevision"
        | "sourceorganizationid"
        | "supplierid"
        | "productid"
        | "offerid"
        | "orderid"
        | "lineid"
        | "invoiceid"
        | "invoicerevision"
        | "documentid"
        | "bookid"
        | "accountid"
        | "accountrevision"
        | "expectedaccountrevision"
        | "periodid"
        | "journalid"
        | "policyid"
        | "policyrevision"
        | "sourceid"
        | "originalsourceid"
        | "reversalof"
        | "projectid"
        | "taskid"
        | "milestoneid"
        | "proposalid"
        | "resourceid"
        | "reservationid"
        | "roomid"
        | "roomtypeid"
        | "propertyid"
        | "stayid"
        | "assetid"
        | "jobid"
        | "caseid"
        | "subscriptionid"
        | "sessionid"
        | "enrollmentid"
        | "menuid"
        | "ticketid"
        | "tableid"
        | "parentid"
        | "requestid"
        | "allocationid"
        | "receiptid"
        | "originid" => RecordReferences,
        "name" | "fullname" | "displayname" | "organization" | "organizationname" | "guestname"
        | "studentname" | "participantname" | "accountname" | "accountcode" | "legalname"
        | "title" | "label" | "code" | "sku" | "companyname" | "companycode" | "customername"
        | "suppliername" | "producttitle" | "invoicecode" | "contactname" | "assetname" => Names,
        "email" | "phone" | "phonenumber" | "customeremail" | "customerphone" | "address"
        | "street" | "city" | "postcode" | "country" => ContactDetails,
        "notes" | "note" | "memo" | "description" | "reason" | "sourceref" | "subject" => Notes,
        "price"
        | "unitcost"
        | "unitcostminor"
        | "costbasisminor"
        | "total"
        | "amount"
        | "amountminor"
        | "totalminor"
        | "debitminor"
        | "creditminor"
        | "goodsminor"
        | "landedfeeminor"
        | "totalcostminor"
        | "recognizedcostminor"
        | "balance"
        | "estimatedvalue"
        | "totalestimatedvalue"
        | "totalvalue"
        | "costdeltavalue"
        | "amountpaid"
        | "refundedamount"
        | "outstandingamount"
        | "priceminor"
        | "depositminor"
        | "rateminor"
        | "subtotalminor"
        | "taxminor"
        | "discountminor" => FinancialAmounts,
        "state" | "status" | "active" | "kind" | "type" | "unit" | "trackstock" | "quantity"
        | "reserved" | "available" | "returnedquantity" | "acceptedquantity"
        | "rejectedquantity" | "currency" | "minorunits" | "currencycatalogue" | "sourceunit"
        | "sourceauthority" | "class" | "cashrole" | "role" | "effect" | "sourcestate"
        | "independentposting" | "date" | "sourcedate" | "startdate" | "enddate" | "duedate"
        | "createdat" | "updatedat" | "postedat" | "startsat" | "endsat" | "starttime"
        | "endtime" | "capacity" | "priority" | "stage" | "version" | "durationminutes"
        | "sequence" | "position" | "completed" | "observedat" | "expiresat" | "checkin"
        | "checkout" | "nights" => BusinessFacts,
        _ => return None,
    })
}
fn container(field: &str) -> bool {
    matches!(
        key(field).as_str(),
        "lines"
            | "legs"
            | "items"
            | "components"
            | "mappings"
            | "policy"
            | "accounts"
            | "inventory"
            | "revenue"
            | "receivable"
            | "payable"
            | "clearing"
            | "grni"
            | "feeaccrual"
            | "cogsaccount"
            | "sourcedraft"
            | "journal"
            | "originalaccounts"
            | "accountallocations"
            | "inventoryaccount"
            | "cogsaccountref"
    )
}
/// A company-owned row cannot inherit permission from the workspace flag.
/// Only Accounting's company-filtered response can supply inherited scope.
pub(super) fn record(
    value: &Value,
    policy: &DisclosurePolicy,
    inherited_company: Option<&str>,
    company_record: bool,
) -> Option<Value> {
    let scope = if company_record {
        Some(value.get("id")?.as_str()?)
    } else if let Some(id) = value.get("companyId").or_else(|| value.get("company_id")) {
        if id.is_null() {
            None
        } else {
            Some(id.as_str()?)
        }
    } else {
        inherited_company
    };
    if !policy.company_allowed(scope) {
        return None;
    }
    project(value, policy, 0)
}
fn project(value: &Value, policy: &DisclosurePolicy, depth: usize) -> Option<Value> {
    if depth > 12 {
        return None;
    }
    match value {
        Value::Object(o) => {
            let own_scope = o.get("companyId").or_else(|| o.get("company_id"));
            if let Some(id) = own_scope {
                if !policy.company_allowed(if id.is_null() {
                    None
                } else {
                    Some(id.as_str()?)
                }) {
                    return None;
                }
            }
            let mut out = Map::new();
            for (k, v) in o {
                if container(k) && (v.is_object() || v.is_array()) {
                    if let Some(v) = project(v, policy, depth + 1) {
                        out.insert(k.clone(), v);
                    }
                } else if class(k).is_some_and(|c| policy.data_classes.contains(&c))
                    && !v.is_object()
                    && !v.is_array()
                {
                    out.insert(k.clone(), sanitize(v));
                }
            }
            Some(Value::Object(out))
        }
        Value::Array(rows) => Some(Value::Array(
            rows.iter()
                .filter_map(|v| project(v, policy, depth + 1))
                .collect(),
        )),
        _ => None,
    }
}
fn sanitize(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(archava_context::decision::turn::credential_text(s)),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn policy() -> DisclosurePolicy {
        serde_json::from_value(json!({"providerBinding":"a".repeat(64),"modules":["crm"],"dataClasses":["record_references","business_facts"],"companies":["00000000-0000-4000-8000-000000000001"],"includeWorkspaceRecords":true})).unwrap()
    }
    #[test]
    fn unknown_fields_and_categories_never_become_implicitly_shared() {
        let p = policy();
        let value = json!({"id":"exact-reference","revision":3,"quantity":7,"fullName":"Private Name","email":"private@example.test","notes":"Private note","price":705100,"apiKey":"never-share","newServerField":"never-share","lines":[{"productId":"product","quantity":2,"price":100,"new":true}]});
        let v = record(&value, &p, None, false).unwrap();
        assert_eq!(
            v,
            json!({"id":"exact-reference","revision":3,"quantity":7,"lines":[{"productId":"product","quantity":2}]})
        );
        assert_eq!(value["email"], "private@example.test");
    }
    #[test]
    fn workspace_permission_does_not_include_foreign_companies_or_nested_company_rows() {
        let p = policy();
        let a = "00000000-0000-4000-8000-000000000001";
        let b = "00000000-0000-4000-8000-000000000002";
        assert!(record(&json!({"id":b}), &p, None, true).is_none());
        assert!(record(&json!({"id":"row","companyId":b}), &p, None, false).is_none());
        let v=record(&json!({"id":"row","companyId":a,"lines":[{"id":"foreign","companyId":b},{"id":"same","companyId":a}]}),&p,None,false).unwrap();
        assert_eq!(v["lines"].as_array().unwrap().len(), 1);
        assert_eq!(v["lines"][0]["id"], "same");
        let mut explicit = p.clone();
        explicit.include_workspace_records = false;
        assert!(record(&json!({"id":"row"}), &explicit, None, false).is_none());
        assert!(record(&json!({"id":"book-child"}), &explicit, Some(a), false).is_some());
    }
    #[test]
    fn exact_financial_strings_survive_only_their_selected_category() {
        let mut p = policy();
        p.data_classes.insert(DataClass::FinancialAmounts);
        let v=record(&json!({"id":"source","amountMinor":"9007199254740993","lines":[{"accountId":"original","debitMinor":"705100","creditMinor":"0"}]}),&p,None,false).unwrap();
        assert_eq!(v["amountMinor"], "9007199254740993");
        assert_eq!(v["lines"][0]["debitMinor"], "705100");
        assert!(collections("unknown").is_empty());
        assert!(!collections("organizations").contains(&"secrets"));
    }
}
