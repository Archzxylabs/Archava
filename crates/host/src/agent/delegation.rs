//! Task-local worker identity, never accepted from HTTP headers or model output.
use super::*;
use crate::identity::AuthenticatedSession;
#[derive(Clone)]
pub(super) struct Execution {
    pub tenant: TenantId,
    pub goal: Uuid,
    pub actor: String,
    pub session_hash: String,
}
tokio::task_local! {static EXECUTION:Execution;}
pub(super) async fn scope<F: std::future::Future>(context: Execution, future: F) -> F::Output {
    EXECUTION.scope(context, future).await
}
pub(crate) async fn authenticate(state: &AppState) -> Option<Result<AuthenticatedSession, ()>> {
    let execution = EXECUTION.try_with(Clone::clone).ok()?;
    async fn validate(state: &AppState, e: Execution) -> Result<AuthenticatedSession, ()> {
        let session = state
            .identity()
            .active_session_hash(&e.session_hash)
            .await
            .map_err(|_| ())?
            .ok_or(())?;
        if session.principal_id != e.actor {
            return Err(());
        }
        let mut tx = workflows::begin(state, &e.tenant).await.map_err(|_| ())?;
        let goal = store::goal(
            tx.checked_executor().await.map_err(|_| ())?,
            e.tenant.as_str(),
            &e.actor,
            e.goal,
        )
        .await
        .map_err(|_| ())?
        .ok_or(())?;
        let valid = matches!(goal["state"].as_str(), Some("approved" | "running"))
            && goal["approved_hash"] == goal["plan_hash"]
            && goal["session_hash"] == e.session_hash;
        tx.commit().await.map_err(|_| ())?;
        if !valid {
            return Err(());
        }
        Ok(session)
    }
    Some(validate(state, execution).await)
}
