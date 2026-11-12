//! Complete period controls and immutable human reconciliation attestations.
use super::*;

async fn load(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    policy: Uuid,
    period: Uuid,
) -> Result<Value, PurchaseError> {
    let value:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('bankPolicyId',p.id,'policyRevision',p.revision,'periodId',r.id,'periodRevision',r.revision,'periodState',r.state,'bankAccount',jsonb_build_object('id',b.id,'code',b.code,'name',b.name),'periodCode',r.code,'check',archava_accounting.bank_period_check(p.tenant_id,p.id,r.id),'importerCannotReview',p.independent_review AND EXISTS(SELECT 1 FROM archava_accounting.bank_statements s WHERE s.tenant_id=p.tenant_id AND s.bank_account_id=p.bank_account_id AND s.from_date>=r.start_date AND s.to_date<=r.end_date AND s.created_by=$4),'reviewed',EXISTS(SELECT 1 FROM archava_accounting.bank_reconciliations x WHERE x.tenant_id=p.tenant_id AND x.bank_policy_id=p.id AND x.period_id=r.id AND x.policy_revision=p.revision AND x.reviewed_period_revision=CASE WHEN r.state='closed' THEN r.revision-1 ELSE r.revision END AND x.check_snapshot=archava_accounting.bank_period_check(p.tenant_id,p.id,r.id))) FROM archava_accounting.bank_policies p JOIN archava_accounting.periods r ON r.tenant_id=p.tenant_id AND r.book_id=p.book_id JOIN archava_accounting.bank_accounts b ON b.tenant_id=p.tenant_id AND b.id=p.bank_account_id WHERE p.tenant_id=$1 AND p.id=$2 AND r.id=$3")
        .bind(tenant).bind(policy).bind(period).bind(actor).fetch_optional(conn).await.map_err(db)?;
    value.ok_or_else(|| ErpError::NotFound.into())
}

pub async fn readiness(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    policy: Uuid,
    period: Uuid,
) -> Result<Value, PurchaseError> {
    require(access, "bank.read")?;
    let key = book_for(conn, tenant, "bank_policy", policy).await?;
    let book = book(conn, tenant, principal, access, key, false).await?;
    if book.source != SourceAuthority::Native.as_str() {
        return Err(ErpError::Conflict.into());
    }
    let mut value = load(conn, tenant, &principal.id, policy, period).await?;
    value["canConfirm"] = json!(
        access.holds(&["bank.reconcile"])
            && value["periodState"] == "open"
            && value["check"]["ready"] == true
            && value["importerCannotReview"] == false
    );
    Ok(value)
}

pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    book: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    match command {
        Command::BankPolicySet {
            bank_account_id,
            expected_revision,
            from_date,
            independent_review,
            ..
        } => {
            let bank = id(bank_account_id)?;
            let existing:Option<(Uuid,i32)>=sqlx::query_as("SELECT id,revision FROM archava_accounting.bank_policies WHERE tenant_id=$1 AND bank_account_id=$2 AND book_id=$3 FOR UPDATE")
                .bind(tenant).bind(bank).bind(book.id).fetch_optional(&mut *conn).await.map_err(db)?;
            let (key, revision) = if let Some((key, revision)) = existing {
                if revision != *expected_revision {
                    return Err(ErpError::Conflict.into());
                }
                let updated:Option<i32>=sqlx::query_scalar("UPDATE archava_accounting.bank_policies SET from_date=$4::text::date,independent_review=$5,revision=revision+1 WHERE tenant_id=$1 AND id=$2 AND revision=$3 RETURNING revision")
                    .bind(tenant).bind(key).bind(expected_revision).bind(from_date).bind(independent_review).fetch_optional(&mut *conn).await.map_err(db)?;
                (key, updated.ok_or(ErpError::Conflict)?)
            } else {
                if *expected_revision != 0 {
                    return Err(ErpError::Conflict.into());
                }
                let key = Uuid::new_v4();
                sqlx::query("INSERT INTO archava_accounting.bank_policies(tenant_id,id,book_id,bank_account_id,from_date,independent_review,created_by) VALUES($1,$2,$3,$4,$5::text::date,$6,$7)")
                    .bind(tenant).bind(key).bind(book.id).bind(bank).bind(from_date).bind(independent_review).bind(&principal.id).execute(&mut *conn).await.map_err(db)?;
                (key, 1)
            };
            Ok(("bank_policy", key, revision))
        }
        Command::BankReconciliationConfirm {
            bank_policy_id,
            expected_policy_revision,
            period_id,
            expected_period_revision,
            expected_check_hash,
            ..
        } => {
            let policy = id(bank_policy_id)?;
            let period = id(period_id)?;
            let value = load(conn, tenant, &principal.id, policy, period).await?;
            if value["policyRevision"] != *expected_policy_revision
                || value["periodRevision"] != *expected_period_revision
                || value["periodState"] != "open"
                || value["check"]["ready"] != true
                || value["check"]["evidenceSha256"] != expected_check_hash.as_str()
            {
                return Err(ErpError::Conflict.into());
            }
            if value["importerCannotReview"] == true {
                return Err(ErpError::Forbidden.into());
            }
            let key = Uuid::new_v4();
            sqlx::query("INSERT INTO archava_accounting.bank_reconciliations(tenant_id,id,book_id,bank_policy_id,period_id,policy_revision,reviewed_period_revision,check_snapshot,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                .bind(tenant).bind(key).bind(book.id).bind(policy).bind(period).bind(expected_policy_revision).bind(expected_period_revision).bind(&value["check"]).bind(&principal.id).execute(conn).await.map_err(db)?;
            Ok(("bank_reconciliation", key, 1))
        }
        _ => Err(ErpError::Conflict.into()),
    }
}
