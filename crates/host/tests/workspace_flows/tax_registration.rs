//! Tax registration identifiers (Organizations 005): a managed company's own
//! statement that it is registered for value-added tax, and the tax id a
//! business party keeps as the reviewed snapshot of its source organization.
//! Both are text and a flag the company entered; nothing is verified.
use super::*;

const TAX_ID: &str = "01.234.567.8-999.000";

async fn fixture() -> (Fixture, Router, Value) {
    let (f, app, s) = setup().await;
    for actor in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,$3,$4)").bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).bind(if actor=="reader"{vec!["companies.read","parties.read","organizations.read","audit.read"]}else{vec!["companies.read","companies.write","organizations.read","organizations.write","parties.read","parties.write","audit.read"]}).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,ARRAY['contacts.read','contacts.write'],$3)").bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
    }
    (f, app, s)
}
fn input(kind: &str, data: Value) -> Value {
    json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}})
}
async fn send(app: &Router, s: &Value, b: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/erp/organizations/commands",
        b.clone(),
        Some(s),
    )
    .await
}
async fn done(app: &Router, s: &Value, kind: &str, data: Value) -> Value {
    let (st, b) = send(app, s, &input(kind, data)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b["receipt"].clone()
}
async fn organization(app: &Router, s: &Value, name: &str, tax_id: Option<&str>) -> Value {
    let mut data = json!({"name":name,"legalName":format!("{name} Official")});
    if let Some(tax_id) = tax_id {
        data["taxId"] = json!(tax_id);
    }
    done(app, s, "create_organization", data).await
}
/// Changes the organization's tax id (and nothing else) at `revision`.
async fn retax(app: &Router, s: &Value, o: &Value, revision: i32, name: &str, tax_id: Value) {
    done(app,s,"update_organization",json!({"id":o["organizationId"],"revision":revision,"name":name,"legalName":format!("{name} Official"),"taxId":tax_id})).await;
}
fn registration(c: &Value, revision: i32, stated: bool) -> Value {
    input(
        "company.set_tax_registration",
        json!({"id":c["companyId"],"expectedRevision":revision,"vatRegistered":stated,"reason":"The company states its own tax registration"}),
    )
}
fn company_refresh(c: &Value, revision: i32, source: i32) -> Value {
    input(
        "company.refresh_source",
        json!({"id":c["companyId"],"expectedRevision":revision,"expectedSourceRevision":source,"reason":"Reviewed official source profile"}),
    )
}
fn party_refresh(p: &Value, revision: i32, source: i32) -> Value {
    input(
        "party.refresh_source",
        json!({"id":p["partyId"],"expectedRevision":revision,"expectedSourceRevision":source,"reason":"Reviewed source profile"}),
    )
}
async fn row(app: &Router, s: &Value, collection: &str, id: &Value) -> Value {
    let (st, b) = snapshot(app, s, "organizations").await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b[collection]
        .as_array()
        .unwrap_or_else(|| panic!("{b}"))
        .iter()
        .find(|r| &r["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("{b}"))
}
fn assert_refused_as(status: StatusCode, body: &Value, code: &str) {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["reason"], "conflict", "{body}");
    assert_eq!(body["detail"]["code"], code, "{body}");
    let refusal = archava_domain::erp::refusals::for_code(code).expect("a catalogued code");
    assert_eq!(body["message"], refusal.message.as_str(), "{body}");
}
/// The SQLSTATE and message of a statement the serving role is refused.
async fn runtime_refusal(f: &Fixture, statement: &str, id: &str) -> (String, String) {
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query(sqlx::AssertSqlSafe(statement))
        .bind(id)
        .execute(&mut *tx)
        .await
        .expect_err(statement);
    let database = error.as_database_error().expect("a database refusal");
    (
        database.code().unwrap_or_default().into_owned(),
        database.message().to_owned(),
    )
}

/// **Witness: a managed company states that it is registered for value-added
/// tax through its own reviewed command, in a revision of its own, and only
/// while its tax id (the reviewed copy of its organization's) is digits and
/// separators. The statement is read back with the company, replays return
/// the original receipt, a caller without `companies.write` is refused, a
/// refresh that would leave a registered company with another kind of tax id
/// is refused with the same reason, and the serving role cannot set the flag
/// around the rule by direct SQL.**
#[tokio::test]
async fn a_company_states_its_tax_registration_on_a_tax_id_of_digits_and_separators() {
    let (f, app, s) = fixture().await;
    let o = organization(&app, &s, "Seller", Some("DECLARED-A")).await;
    let c = done(&app,&s,"company.create_from_organization",json!({"code":"SELLER","organizationId":o["organizationId"],"expectedSourceRevision":1,"reason":"Declare our managed company"})).await;
    let id = c["companyId"].clone();
    let before = row(&app, &s, "companies", &id).await;
    assert_eq!(before["vatRegistered"], false, "{before}");
    assert_eq!(before["taxId"], "DECLARED-A");

    // A tax id that is not digits and separators cannot carry the statement.
    let (st, b) = send(&app, &s, &registration(&c, 1, true)).await;
    assert_refused_as(st, &b, "company_tax_registration_needs_tax_id");
    assert_eq!(row(&app, &s, "companies", &id).await, before);

    // The tax id is changed where it lives: on the organization, then reviewed
    // into the company.
    retax(&app, &s, &o, 1, "Seller", json!(TAX_ID)).await;
    let (st, b) = send(&app, &s, &company_refresh(&c, 1, 2)).await;
    assert_eq!(st, StatusCode::OK, "{b}");

    // Not for a reader, and not for a malformed statement.
    let reader = login(&app, "reader").await;
    let (st, b) = send(&app, &reader, &registration(&c, 2, true)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{b}");
    for (member, value) in [
        ("vatRegistered", json!("yes")),
        ("taxId", json!(TAX_ID)),
        ("verified", json!(true)),
    ] {
        let mut forged = registration(&c, 2, true);
        forged["mutation"]["data"][member] = value;
        let (st, b) = send(&app, &s, &forged).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{member} {b}");
    }
    assert_eq!(row(&app, &s, "companies", &id).await["revision"], 2);

    let command = registration(&c, 2, true);
    let (st, b) = send(&app, &s, &command).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["decision"], "receipt");
    assert_eq!(b["receipt"]["type"], "company.set_tax_registration");
    assert_eq!(b["receipt"]["companyId"], id);
    assert_eq!(b["receipt"]["revision"], 3);
    assert_eq!(b["receipt"]["replayed"], false);
    // The same request again is the original receipt.
    let (st, again) = send(&app, &s, &command).await;
    assert_eq!(st, StatusCode::OK, "{again}");
    assert_eq!(again["decision"], "replayed");
    assert_eq!(again["receipt"]["revision"], 3);
    assert_eq!(again["receipt"]["replayed"], true);
    let (st, status) = call(
        &app,
        "GET",
        &format!(
            "/erp/organizations/commands/status?tenant=acme&requestId={}",
            command["mutation"]["requestId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{status}");
    assert_eq!(status["decision"], "committed", "{status}");

    let stated = row(&app, &s, "companies", &id).await;
    assert_eq!(stated["vatRegistered"], true, "{stated}");
    assert_eq!(stated["taxId"], TAX_ID);
    assert_eq!(stated["revision"], 3);
    assert_eq!(stated["sourceRevision"], 2);
    assert_eq!(stated["active"], true);
    assert!(stated.get("verified").is_none() && stated.get("taxVerified").is_none());
    let history: Vec<(String, i32, Option<String>, String)> = sqlx::query_as("SELECT operation,revision,snapshot->>'vat_registered',reason FROM archava_organizations.company_audit WHERE company_id=$1::uuid ORDER BY revision")
        .bind(id.as_str().unwrap()).fetch_all(&f.admin_pool).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|h| (h.0.as_str(), h.1, h.2.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("company.create_from_organization", 1, Some("false")),
            ("company.refresh_source", 2, Some("false")),
            ("company.set_tax_registration", 3, Some("true")),
        ]
    );
    assert_eq!(history[2].3, "The company states its own tax registration");

    // No change, and a stale revision, are plain conflicts.
    for stale in [registration(&c, 3, true), registration(&c, 2, false)] {
        let (st, b) = send(&app, &s, &stale).await;
        assert_eq!(st, StatusCode::CONFLICT, "{b}");
        assert!(b.get("detail").is_none(), "{b}");
    }

    // While the statement stands, a refresh cannot bring in a tax id of
    // another kind; nothing is written.
    retax(&app, &s, &o, 2, "Seller", json!("REF-X")).await;
    let (st, b) = send(&app, &s, &company_refresh(&c, 3, 3)).await;
    assert_refused_as(st, &b, "company_tax_registration_needs_tax_id");
    assert_eq!(row(&app, &s, "companies", &id).await, stated);

    // The serving role cannot set the statement around the rule, nor together
    // with another change.
    let key = id.as_str().unwrap();
    let (st, b) = send(&app, &s, &registration(&c, 3, false)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let (st, b) = send(&app, &s, &company_refresh(&c, 4, 3)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let unstated = row(&app, &s, "companies", &id).await;
    assert_eq!(
        (
            &unstated["vatRegistered"],
            &unstated["taxId"],
            &unstated["revision"]
        ),
        (&json!(false), &json!("REF-X"), &json!(5))
    );
    assert_eq!(
        runtime_refusal(&f,"UPDATE archava_organizations.companies SET vat_registered=true,revision=revision+1 WHERE id=$1::uuid",key).await,
        ("23514".to_owned(),"A company registered for tax needs a tax id of digits and separators".to_owned())
    );
    assert_eq!(
        runtime_refusal(&f,"UPDATE archava_organizations.companies SET vat_registered=true,active=false,revision=revision+1 WHERE id=$1::uuid",key).await,
        ("23514".to_owned(),"A company tax registration changes in a revision of its own".to_owned())
    );
    let (st, b) = send(&app, &s, &registration(&c, 5, true)).await;
    assert_refused_as(st, &b, "company_tax_registration_needs_tax_id");
    assert_eq!(row(&app, &s, "companies", &id).await, unstated);
}

/// **Witness: a business party keeps the tax id of its source organization as
/// part of the reviewed snapshot, exactly as entered: set at registration,
/// changed only by a reviewed refresh after the organization changed, null for
/// an organization without one and for a party sourced from a contact. Every
/// version is in the party history, and the serving role cannot write a tax id
/// that is not its source's.**
#[tokio::test]
async fn a_party_keeps_the_tax_id_of_its_source_organization_as_entered() {
    let (f, app, s) = fixture().await;
    // Stored as entered: any text within the bound, no format is checked.
    let o = organization(&app, &s, "Buyer", Some("REG 02.345 / A-1")).await;
    let p = done(&app,&s,"party.create_from_organization",json!({"organizationId":o["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":false})).await;
    let id = p["partyId"].clone();
    let first = row(&app, &s, "parties", &id).await;
    assert_eq!(first["taxId"], "REG 02.345 / A-1", "{first}");
    assert_eq!(first["revision"], 1);

    // Changing the organization does not change the party until it is reviewed.
    retax(&app, &s, &o, 1, "Buyer", json!("02.345.678.9-012.000")).await;
    assert_eq!(row(&app, &s, "parties", &id).await, first);
    // The same source revision again is still refused.
    let (st, b) = send(&app, &s, &party_refresh(&p, 1, 1)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{b}");
    let (st, b) = send(&app, &s, &party_refresh(&p, 1, 2)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let second = row(&app, &s, "parties", &id).await;
    assert_eq!(second["taxId"], "02.345.678.9-012.000");
    assert_eq!(
        (&second["revision"], &second["sourceRevision"]),
        (&json!(2), &json!(2))
    );
    // A refresh with nothing newer is refused, as before.
    let (st, b) = send(&app, &s, &party_refresh(&p, 2, 2)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{b}");

    // Every version is in the party history with the tax id it had.
    let history: Vec<(i32, Option<String>)> = sqlx::query_as("SELECT revision,snapshot->>'tax_id' FROM archava_organizations.party_audit WHERE party_id=$1::uuid ORDER BY revision")
        .bind(id.as_str().unwrap()).fetch_all(&f.admin_pool).await.unwrap();
    assert_eq!(
        history,
        vec![
            (1, Some("REG 02.345 / A-1".to_owned())),
            (2, Some("02.345.678.9-012.000".to_owned())),
        ]
    );

    // An organization without a tax id, and a contact, give a party without one.
    let plain = organization(&app, &s, "Plain buyer", None).await;
    let q = done(&app,&s,"party.create_from_organization",json!({"organizationId":plain["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":false})).await;
    assert!(row(&app, &s, "parties", &q["partyId"]).await["taxId"].is_null());
    let (st, contact) = call(&app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.create","data":{"fullName":"A person","email":"person@example.test"}}}),Some(&s)).await;
    assert_eq!(st, StatusCode::OK, "{contact}");
    let person = done(&app,&s,"party.create_from_contact",json!({"contactId":contact["receipt"]["id"],"expectedSourceRevision":contact["receipt"]["revision"],"customer":true,"supplier":false})).await;
    assert!(row(&app, &s, "parties", &person["partyId"]).await["taxId"].is_null());

    // The serving role cannot give a party a tax id its source does not have.
    for (key, message) in [
        (
            id.as_str().unwrap(),
            "Party refresh must advance its source revision",
        ),
        (
            person["partyId"].as_str().unwrap(),
            "Party differs from the reviewed current source",
        ),
    ] {
        assert_eq!(
            runtime_refusal(&f,"UPDATE archava_organizations.parties SET tax_id='99',revision=revision+1 WHERE id=$1::uuid",key).await,
            ("23514".to_owned(), message.to_owned()),
        );
    }

    // A party registered before parties kept a tax id (planted by the owner
    // with the guard off) takes its organization's tax id at the revision it
    // was reviewed at, and nothing else; after that the usual rule applies.
    let old = organization(&app, &s, "Earlier buyer", Some("03.456.789.0-123.000")).await;
    let r = done(&app,&s,"party.create_from_organization",json!({"organizationId":old["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":false})).await;
    let earlier = r["partyId"].as_str().unwrap();
    for statement in [
        "ALTER TABLE archava_organizations.parties DISABLE TRIGGER USER",
        "ALTER TABLE archava_organizations.party_audit DISABLE TRIGGER USER",
    ] {
        sqlx::query(statement).execute(&f.admin_pool).await.unwrap();
    }
    sqlx::query("UPDATE archava_organizations.parties SET tax_id=NULL WHERE id=$1::uuid")
        .bind(earlier)
        .execute(&f.admin_pool)
        .await
        .unwrap();
    sqlx::query("UPDATE archava_organizations.party_audit SET snapshot=snapshot-'tax_id' WHERE party_id=$1::uuid")
        .bind(earlier)
        .execute(&f.admin_pool)
        .await
        .unwrap();
    for statement in [
        "ALTER TABLE archava_organizations.parties ENABLE TRIGGER USER",
        "ALTER TABLE archava_organizations.party_audit ENABLE TRIGGER USER",
    ] {
        sqlx::query(statement).execute(&f.admin_pool).await.unwrap();
    }
    assert!(row(&app, &s, "parties", &r["partyId"]).await["taxId"].is_null());
    let (st, b) = send(&app, &s, &party_refresh(&r, 1, 1)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let adopted = row(&app, &s, "parties", &r["partyId"]).await;
    assert_eq!(adopted["taxId"], "03.456.789.0-123.000", "{adopted}");
    assert_eq!(
        (&adopted["revision"], &adopted["sourceRevision"]),
        (&json!(2), &json!(1))
    );
    let (st, b) = send(&app, &s, &party_refresh(&r, 2, 1)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{b}");
}
