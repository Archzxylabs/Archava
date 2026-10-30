//! Server-side authority: the membership grant, the scope rule, the bound
//! approval.
//!
//! Audit RA01's core: "request body does not become authority for
//! tenant/role/grants/approval." Everything in this module exists to make that
//! structural. The router builds its `ActionRequest` — the input the policy
//! gate has always trusted — from *here*, and `here` reads only:
//!
//! - the session's principal (resolved by [`crate::identity`] from a token
//!   only this server could have issued),
//! - the membership row that principal holds in the requested tenant
//!   (`active`, with the role and capability columns migration 004 added —
//!   the grant is a database fact, not a JSON claim),
//! - the bound approval row a distinct human stamped for this exact command.
//!
//! The submission body contributes the *requested target* (which tenant,
//! which action, which input, which revision, which idempotency key) and the
//! narrowing signals (`available_on_page`, `confirmed`, `declined`). Those can
//! only ever subtract authority the server already granted; none of them can
//! add any.
//!
//! The pure pieces ([`derive_request`], [`input_fingerprint`]) are unit-tested
//! without a database on purpose: a scope rule that can only be checked
//! against PostgreSQL is a scope rule that ships untested on machines without
//! one.

use archava_adapters::rls::{TenantContextError, TenantTransaction};
use archava_core::actions::{ActionLevel, ActionRegistry};
use archava_core::roles::{role_name_of, CapabilityTier, RoleName};
use archava_core::tenant::TenantId;
use archava_domain::authority::ActionRequest;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// Why an authority resolution refused.
///
/// Separate from the gate's `PolicyDecision` on purpose: a `PolicyDecision` is
/// a policy answer to a request the server could build; these are the reasons
/// the server could not build — or could not trust — one.
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    /// A database, ledger or runtime failure. The caller is told the service
    /// is unavailable; the operator gets the detail from the log.
    #[error(transparent)]
    Context(#[from] TenantContextError),

    /// The identity HMAC key is below the baseline's 32-byte minimum.
    /// Construction-time only: no request can reach it.
    #[error("identity secret must contain at least 32 random bytes")]
    IdentitySecretTooShort,

    /// The ERP assembler named a namespace that is not mounted by this host.
    #[error("unknown ERP readiness ledger")]
    InvalidErpLedger,

    /// The session's principal has no ACTIVE membership in the requested
    /// tenant.
    ///
    /// One variant covers "no such tenant", "no such principal", "membership
    /// deleted" and "membership revoked" — deliberately indistinguishable,
    /// because telling a caller which of those four it is would confirm the
    /// existence of tenants and principals the caller is not entitled to
    /// know about (PRD §15).
    #[error("the principal holds no active membership in the requested tenant")]
    MissingMembership,

    /// The action's level requires `scope = all` and the membership is
    /// `assigned` — see [`SCOPE_GATE_LEVEL`].
    #[error("the membership's scope does not cover this action")]
    MissingScope,

    /// A stored grant value is outside its vocabulary.
    ///
    /// Fail-closed: an unreadable grant is refused, never defaulted upward.
    /// This is an operator-facing data problem, so it is logged at error.
    #[error("the stored membership grant is not usable")]
    UnusableGrant,

    /// The submission itself is invalid before any authority question (e.g. a
    /// revision below 1, which the approvals CHECK would refuse anyway).
    #[error("the submission is invalid")]
    InvalidSubmission,
}

impl AuthorityError {
    /// The database-failure shape, labelled with the ledger it happened under.
    ///
    /// The label is a parameter rather than a constant because this host is
    /// ledger-agnostic by construction (`Ledger` is supplied, not assumed) and
    /// an error that named the wrong ledger would send an operator to the
    /// wrong database.
    pub(crate) fn database(ledger: &'static str, source: sqlx::Error) -> Self {
        Self::Context(TenantContextError::Database { ledger, source })
    }
}

/// The level at which a command requires `scope = all`.
///
/// The rule, stated once: L3 and above ("sends data to an external system and
/// cannot be undone", payments, account changes, administrative changes) is a
/// command whose effect reaches beyond the records personally assigned to the
/// actor, so it requires the unassigned-wide grant. An `assigned`-scope
/// membership keeps the whole read/L1/L2 surface and is refused, by name,
/// above this line.
///
/// This is a server-side derivation from the membership row. It is not
/// negotiable per request, and the body has no field that speaks to it.
pub(crate) const SCOPE_GATE_LEVEL: ActionLevel = ActionLevel::L3;

/// The kinds a membership can be, mirroring the schema CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MembershipKind {
    Human,
    Service,
}

/// The scopes a membership can carry, mirroring the schema CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    All,
    Assigned,
}

/// A resolved membership grant: everything the server knows about what this
/// principal may do in this tenant, read from the row that says so.
#[derive(Debug, Clone)]
pub(crate) struct Membership {
    pub(crate) kind: MembershipKind,
    pub(crate) role: RoleName,
    pub(crate) capability: CapabilityTier,
    pub(crate) scope: Scope,
}

/// Resolve the principal's ACTIVE membership in `tenant`.
///
/// `active` is in the WHERE clause, not a post-filter: a revoked membership
/// is indistinguishable from an absent one, which is the property every
/// refusal path in this host relies on. An out-of-vocabulary stored value
/// surfaces as [`AuthorityError::UnusableGrant`] — fail closed, never
/// defaulted upward.
pub(crate) async fn resolve_membership(
    connection: &mut PgConnection,
    namespace: &str,
    ledger: &'static str,
    tenant: &TenantId,
    principal: &str,
) -> Result<Option<Membership>, AuthorityError> {
    let query = format!(
        "SELECT kind,role,capability,scope FROM {namespace}.memberships \
         WHERE tenant_id=$1 AND principal_id=$2 AND active"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(tenant.as_str())
        .bind(principal)
        .fetch_optional(connection)
        .await
        .map_err(|error| AuthorityError::database(ledger, error))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let kind = match row.get::<String, _>("kind").as_str() {
        "human" => MembershipKind::Human,
        "service" => MembershipKind::Service,
        other => {
            tracing::error!(kind = other, "membership kind is outside its vocabulary");
            return Err(AuthorityError::UnusableGrant);
        }
    };
    let scope = match row.get::<String, _>("scope").as_str() {
        "all" => Scope::All,
        "assigned" => Scope::Assigned,
        other => {
            tracing::error!(scope = other, "membership scope is outside its vocabulary");
            return Err(AuthorityError::UnusableGrant);
        }
    };
    let role = role_name_of(&row.get::<String, _>("role")).ok_or_else(|| {
        tracing::error!("membership role is outside its vocabulary");
        AuthorityError::UnusableGrant
    })?;
    let capability =
        CapabilityTier::try_from(row.get::<String, _>("capability")).map_err(|error| {
            tracing::error!(error = %error, "membership capability is outside its vocabulary");
            AuthorityError::UnusableGrant
        })?;
    Ok(Some(Membership {
        kind,
        role,
        capability,
        scope,
    }))
}

/// Build the gate's `ActionRequest` from server-resolved facts.
///
/// `human_approved` is a SERVER-side fact: whether a bound, unexpired,
/// unconsumed approval row exists for this exact command. The submission has
/// no field of that name.
pub(crate) fn derive_request(
    tenant: TenantId,
    membership: &Membership,
    action_id: &str,
    available_on_page: Option<bool>,
    confirmed: Option<bool>,
    declined: Option<bool>,
    human_approved: bool,
) -> Result<ActionRequest, AuthorityError> {
    // The scope rule fires before the gate so the refusal is `missing_scope`
    // rather than a role error that happens to also apply. An unregistered
    // action skips this check on purpose: the gate refuses unknown actions by
    // name, and duplicating that lookup here would only split the difference
    // between two refusal paths.
    if let Some(action) = ActionRegistry::try_get(action_id) {
        if action.level >= SCOPE_GATE_LEVEL && membership.scope != Scope::All {
            return Err(AuthorityError::MissingScope);
        }
    }
    Ok(ActionRequest {
        tenant,
        role: membership.role,
        capability: membership.capability,
        action_id: action_id.to_string(),
        available_on_page,
        confirmed,
        declined,
        human_approved: Some(human_approved),
        // Never client-supplied. `None` means "no tenant allow-list
        // narrowing", which is a server configuration decision; the field the
        // old body used to fill is gone from the submission struct, so no
        // future serde default can quietly resurrect it.
        enabled_action_ids: None,
    })
}

/// The command's fingerprint: sha256 over action, revision and input bytes.
///
/// This is the value the approval binds to and the receipt records. The input
/// is treated as opaque bytes — canonicalisation is the client's job, and a
/// server that re-serialised it would be hashing something the client never
/// signed.
pub(crate) fn input_fingerprint(action_id: &str, revision: i32, input: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(action_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(revision.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(input.unwrap_or("").as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// --- Bound approvals (the database half) ------------------------------------
//
// Every statement here is tenant-scoped by predicate AND runs inside a
// `TenantTransaction`, so row-level security and the query agree; neither is
// load-bearing alone.

/// Whether an approval exists that is bound to this exact command, approved,
/// unconsumed and unexpired.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn load_bound_approval(
    connection: &mut PgConnection,
    namespace: &str,
    tenant: &TenantId,
    approval_id: Uuid,
    actor: &str,
    action_id: &str,
    input_hash: &str,
    revision: i32,
) -> Result<bool, sqlx::Error> {
    let query = format!(
        "SELECT 1 FROM {namespace}.approvals WHERE tenant_id=$1 AND approval_id=$2 \
         AND actor_id=$3 AND action_id=$4 AND input_hash=$5 AND revision=$6 \
         AND approved_at IS NOT NULL AND consumed_at IS NULL AND expires_at>now()"
    );
    let found = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(tenant.as_str())
        .bind(approval_id)
        .bind(actor)
        .bind(action_id)
        .bind(input_hash)
        .bind(revision)
        .fetch_optional(connection)
        .await?;
    Ok(found.is_some())
}

/// Create a pending approval bound to (tenant, actor, action, input hash,
/// revision) with an expiry. Nothing else about it is set until a distinct
/// human stamps it through [`approve_approval`].
#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_approval(
    connection: &mut PgConnection,
    namespace: &str,
    tenant: &TenantId,
    approval_id: Uuid,
    actor: &str,
    action_id: &str,
    input_hash: &str,
    revision: i32,
    ttl_seconds: i32,
) -> Result<(), sqlx::Error> {
    let query = format!(
        "INSERT INTO {namespace}.approvals(tenant_id,approval_id,actor_id,action_id,input_hash,revision,expires_at) \
         VALUES($1,$2,$3,$4,$5,$6,now()+make_interval(secs=>$7::double precision))"
    );
    sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(tenant.as_str())
        .bind(approval_id)
        .bind(actor)
        .bind(action_id)
        .bind(input_hash)
        .bind(revision)
        .bind(ttl_seconds as f64)
        .execute(connection)
        .await?;
    Ok(())
}

/// The facts the approve route branches on. Timestamps are booleans computed
/// in SQL on purpose: the router needs "is it still stampable", never a
/// calendar value, and asking for a typed timestamp would drag a time-crate
/// feature into every caller.
pub(crate) struct ApprovalState {
    pub(crate) actor_id: String,
    pub(crate) approved: bool,
    pub(crate) consumed: bool,
    pub(crate) expired: bool,
}

/// Load the pending-approval facts, scoped to the tenant transaction.
pub(crate) async fn load_approval_state(
    connection: &mut PgConnection,
    namespace: &str,
    tenant: &TenantId,
    approval_id: Uuid,
) -> Result<Option<ApprovalState>, sqlx::Error> {
    let query = format!(
        "SELECT actor_id, approved_at IS NOT NULL AS approved, \
         consumed_at IS NOT NULL AS consumed, expires_at<=now() AS expired \
         FROM {namespace}.approvals WHERE tenant_id=$1 AND approval_id=$2"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(tenant.as_str())
        .bind(approval_id)
        .fetch_optional(connection)
        .await?;
    Ok(row.map(|row| ApprovalState {
        actor_id: row.get::<String, _>("actor_id"),
        approved: row.get::<bool, _>("approved"),
        consumed: row.get::<bool, _>("consumed"),
        expired: row.get::<bool, _>("expired"),
    }))
}

/// Stamp the approver onto a pending approval. The WHERE clause is the
/// single-use contract: approved-once, unconsumed, unexpired.
pub(crate) async fn approve_approval(
    connection: &mut PgConnection,
    namespace: &str,
    tenant: &TenantId,
    approval_id: Uuid,
    approver: &str,
) -> Result<bool, sqlx::Error> {
    let query = format!(
        "UPDATE {namespace}.approvals SET approver_id=$3, approved_at=now() \
         WHERE tenant_id=$1 AND approval_id=$2 AND approved_at IS NULL \
         AND consumed_at IS NULL AND expires_at>now()"
    );
    let updated = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(tenant.as_str())
        .bind(approval_id)
        .bind(approver)
        .execute(connection)
        .await?;
    Ok(updated.rows_affected() == 1)
}

/// Consume an approval. `false` means someone else (or an earlier retry)
/// already consumed it — the race the single-use contract exists to lose
/// loudly rather than win twice.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn consume_approval(
    connection: &mut PgConnection,
    namespace: &str,
    tenant: &TenantId,
    approval_id: Uuid,
    actor: &str,
    action_id: &str,
    input_hash: &str,
    revision: i32,
) -> Result<bool, sqlx::Error> {
    let query = format!(
        "UPDATE {namespace}.approvals SET consumed_at=now() WHERE tenant_id=$1 AND approval_id=$2 \
         AND actor_id=$3 AND action_id=$4 AND input_hash=$5 AND revision=$6 \
         AND approved_at IS NOT NULL AND consumed_at IS NULL AND expires_at>now()"
    );
    let updated = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(tenant.as_str())
        .bind(approval_id)
        .bind(actor)
        .bind(action_id)
        .bind(input_hash)
        .bind(revision)
        .execute(connection)
        .await?;
    Ok(updated.rows_affected() == 1)
}

/// Read a stored command receipt, scoped to the tenant transaction.
///
/// Returns `(actor_id, receipt)` — the caller compares the actor itself,
/// because "whose receipt is this" is a host rule, not a SQL one.
pub(crate) async fn load_receipt(
    connection: &mut PgConnection,
    namespace: &str,
    tenant: &TenantId,
    request_id: Uuid,
) -> Result<Option<(String, Option<serde_json::Value>)>, sqlx::Error> {
    let query = format!(
        "SELECT actor_id,receipt FROM {namespace}.commands WHERE tenant_id=$1 AND request_id=$2"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(tenant.as_str())
        .bind(request_id)
        .fetch_optional(connection)
        .await?;
    Ok(row.map(|row| (row.get::<String, _>("actor_id"), row.get("receipt"))))
}

/// Open the tenant transaction the authority reads (and later the write)
/// happen inside.
pub(crate) async fn begin<'c>(
    pool: &'c sqlx::PgPool,
    ledger: crate::Ledger,
    tenant: TenantId,
) -> Result<TenantTransaction<'c>, AuthorityError> {
    TenantTransaction::begin(pool, ledger.namespace, ledger.ledger, tenant)
        .await
        .map_err(AuthorityError::from)
}

/// Resolve membership inside a verified tenant transaction with RLS context.
///
/// Fail closed:
/// - If transaction cannot be opened or database error occurs -> Err(AuthorityError)
/// - If membership row is missing -> Ok(None)
/// - If membership row is present -> Ok(Some(Membership))
pub(crate) async fn resolve_tenant_membership(
    pool: &sqlx::PgPool,
    ledger: crate::Ledger,
    tenant: &TenantId,
    principal: &str,
) -> Result<Option<Membership>, AuthorityError> {
    let mut transaction = begin(pool, ledger, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;
    let found = resolve_membership(
        connection,
        ledger.namespace,
        ledger.ledger,
        tenant,
        principal,
    )
    .await?;
    let _ = transaction.commit().await;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn membership(scope: Scope) -> Membership {
        Membership {
            kind: MembershipKind::Service,
            role: RoleName::ArchavaAssistant,
            capability: CapabilityTier::Act,
            scope,
        }
    }

    #[test]
    fn the_fingerprint_binds_action_revision_and_input() {
        let base = input_fingerprint("crm.lead.create", 3, Some("lead-42"));
        assert_eq!(base.len(), 64, "the approvals CHECK pins sha256 hex");
        // Every bound component changes it; nothing else about the format
        // matters, but a collision across any of the three would silently
        // un-bind approvals.
        assert_ne!(
            base,
            input_fingerprint("crm.lead.create", 4, Some("lead-42"))
        );
        assert_ne!(
            base,
            input_fingerprint("crm.lead.create", 3, Some("lead-43"))
        );
        assert_ne!(base, input_fingerprint("email.send", 3, Some("lead-42")));
        // Absent input is empty bytes, not the string "None".
        assert_eq!(
            input_fingerprint("x", 1, None),
            input_fingerprint("x", 1, Some(""))
        );
    }

    #[test]
    fn an_assigned_scope_is_refused_above_the_scope_gate() {
        let tenant = TenantId::new("acme-retail").expect("fixture tenant");
        let error = derive_request(
            tenant.clone(),
            &membership(Scope::Assigned),
            "email.send",
            None,
            Some(true),
            None,
            false,
        )
        .expect_err("L3 with assigned scope must refuse");
        assert!(matches!(error, AuthorityError::MissingScope));
    }

    #[test]
    fn an_assigned_scope_keeps_the_surface_below_the_gate() {
        let tenant = TenantId::new("acme-retail").expect("fixture tenant");
        let request = derive_request(
            tenant,
            &membership(Scope::Assigned),
            "lead.capture",
            None,
            None,
            None,
            false,
        )
        .expect("L2 with assigned scope is inside the grant");
        assert_eq!(request.role, RoleName::ArchavaAssistant);
    }

    #[test]
    fn the_request_carries_server_facts_and_never_a_body_grant() {
        let tenant = TenantId::new("acme-retail").expect("fixture tenant");
        let request = derive_request(
            tenant,
            &membership(Scope::All),
            "product.read",
            Some(false),
            Some(true),
            Some(false),
            true,
        )
        .expect("a valid derivation");
        // The narrowing signals survive — they can only subtract.
        assert_eq!(request.available_on_page, Some(false));
        assert_eq!(request.confirmed, Some(true));
        assert_eq!(request.declined, Some(false));
        // The grant facts come from the membership row.
        assert_eq!(request.capability, CapabilityTier::Act);
        // Human approval is the server's lookup result, passed in, never
        // read from a body field.
        assert_eq!(request.human_approved, Some(true));
        // And the allow-list stays server-owned.
        assert!(request.enabled_action_ids.is_none());
    }
}
