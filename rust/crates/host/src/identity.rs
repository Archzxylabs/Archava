//! The identity port: users, sessions, scrypt passwords, login buckets.
//!
//! This is a line-for-line port of `packages/identity-pg` (`password.ts` and
//! `sessions.ts`) plus the schema its migration defines, and it exists because
//! of audit RA01: the router used to take tenant, role, capability and human
//! approval from the request body, so an unauthenticated caller could assert
//! any authority it liked. The fix is not a stricter body parser — it is a
//! server-side identity whose session the caller can only *present*, never
//! *forge*:
//!
//! - `POST /login` exchanges a username and password (scrypt, the baseline's
//!   fixed OWASP parameters) for a 32-random-byte base64url token. Only the
//!   HMAC digest of the token is stored; the token itself never touches the
//!   database, so a database reader cannot impersonate a live session.
//! - Every later request presents the token; [`Identity::authenticate`]
//!   re-reads the session row and re-checks the user is active and unversioned
//!   (a password change bumps `version` and therefore invalidates every
//!   existing session — the baseline's revocation semantics).
//! - CSRF is derived, not stored: the expected header is
//!   `HMAC(key, "csrf\0" + token)`, which a cross-site page cannot compute
//!   because it can neither read the token nor predict the key.
//!
//! The HMAC key is supplied by the host (≥ 32 random bytes, enforced at
//! construction). It is process configuration the way the baseline's
//! `PostgresIdentity.create(pool, key)` key is: short enough to remember is
//! not a shape this module accepts.
//!
//! Rate limiting is the baseline's, bucket for bucket: a per-source budget and
//! a per-account budget, every attempted login consuming both, a successful
//! one clearing the account bucket. The source identifier is supplied by the
//! transport layer (the router passes a constant for now — a global login
//! throttle — and names that as a known simplification rather than pretending
//! it has client-IP semantics it cannot verify).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use scrypt::{scrypt, Params};
use sha2::Sha256;
use sqlx::{PgPool, Postgres, Row, Transaction};

/// The identity ledger label, mirroring the baseline's error strings
/// (`Identity runtime requires a non-owner application role`).
pub(crate) const IDENTITY_LEDGER: &str = "Identity";
/// The identity schema — same name the baseline's migration creates.
pub(crate) const IDENTITY_NAMESPACE: &str = "archava_identity";

/// The identity HMAC key's minimum length, from `PostgresIdentity.create`.
const MIN_KEY_BYTES: usize = 32;
/// The password KDF's memory ceiling, matching the baseline's `maxmem` so the
/// same parameters cannot be reinterpreted as a cheaper request.
const SCRYPT_MAXMEM: usize = 48 * 1024 * 1024;

/// Concurrent scrypt derivations before the baseline's `busy` refusal. Two,
/// like the original: bounded concurrency keeps memory predictable under a
/// login flood, and the third caller is refused rather than queued.
///
/// A `const`, not a mutable static, and there is no setter. That is RN07/RN04:
/// an earlier version exported `set_max_active_derivations` so the integration
/// suites could raise the ceiling to 32, and the override shipped in the normal
/// build — an importer of this crate as an ordinary dependency could take the
/// production budget to zero and make every `encode_password` return `Busy`,
/// and every suite that raised it was measuring a concurrency policy the
/// product does not have. The suites now run against this value; the overload
/// proof is `tests/kdf_budget.rs`, which asserts the third concurrent
/// derivation is refused *because* the default is 2.
const MAX_ACTIVE_DERIVATIONS: usize = 2;

/// Concurrency counter for [`derive`], the port of `active` in `password.ts`.
static ACTIVE_DERIVATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The production concurrent-derivation budget, for the overload proof to
/// assert against rather than restate.
///
/// Exposed as a number so `tests/kdf_budget.rs` proves the *default* is two
/// (by observing exactly one refusal among three concurrent derivations)
/// instead of taking the value on trust. It is a read-only view of a `const`
/// — nothing can change what it reports.
#[doc(hidden)]
pub const fn max_active_derivations() -> usize {
    MAX_ACTIVE_DERIVATIONS
}

/// How many derivations hold the budget right now — test-only, and read-only
/// even though it is.
///
/// `#[cfg(test)]` rather than `pub` on purpose: the shipped build has no
/// handle on this counter at all, so no importer can widen the budget, drain
/// it or rely on its value across a request. The in-crate overload proof uses
/// it to WAIT (deterministically) until the budget is genuinely held, rather
/// than estimating how long two concurrent `scrypt` calls stay in flight —
/// which is a race, and races in the striped check belong to the stripe,
/// not the proof.
#[cfg(test)]
fn active_derivations() -> usize {
    ACTIVE_DERIVATIONS.load(std::sync::atomic::Ordering::SeqCst)
}

/// Guards the tests that OBSERVE the derivation budget.
///
/// The counter is process-wide — correctly, since the budget it bounds is a
/// process-wide property of the shipped library — and the test harness runs
/// unit tests on parallel threads. Two budget tests running at once would
/// measure each other's concurrent derivations, and "the third is Busy" could
/// then be satisfied (or not) by somebody else's work. This mutex makes each
/// budget observation the only one in flight. Test infrastructure, `cfg(test)`
// both.
#[cfg(test)]
static BUDGET_OBSERVATION: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

#[cfg(test)]
fn budget_observation() -> std::sync::MutexGuard<'static, ()> {
    BUDGET_OBSERVATION
        .get_or_init(std::sync::Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Everything that can go wrong on the identity path.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// Wrong username, wrong password, expired session, deactivated user, or
    /// a session whose user version no longer matches. Deliberately one
    /// variant: telling a caller *which* half of a credential pair was wrong
    /// is a free oracle.
    #[error("invalid credentials")]
    InvalidCredentials,
    /// A login bucket is over budget.
    #[error("rate limited")]
    RateLimited,
    /// Too many scrypt derivations in flight.
    #[error("identity is busy")]
    Busy,
    /// No usable session token was presented.
    #[error("unauthenticated")]
    Unauthenticated,
    /// A database failure on the identity path. Logged server-side; the
    /// caller is told the service is unavailable, never which query failed.
    #[error("identity store unavailable")]
    Unavailable,
}

impl From<sqlx::Error> for IdentityError {
    fn from(error: sqlx::Error) -> Self {
        // This is the only place the actual database failure still exists —
        // the variant the caller receives deliberately carries no detail
        // (the response must say "unavailable", never which query failed,
        // D09). The operator path needs the real error, so it is logged here,
        // at the boundary, rather than being dropped entirely: without this
        // line a production 503 `identity` is uninvestigable, and the test
        // suite observed exactly that hole — `(503, "identity")` with nowhere
        // to look.
        tracing::error!(error = %error, "sqlx failure on the identity path");
        Self::Unavailable
    }
}

/// The port of `PostgresIdentity`.
pub(crate) struct Identity {
    pool: PgPool,
    key: Vec<u8>,
}

/// `Debug` with the key redacted. The derived version would print the HMAC
/// key in full — a credential leaking into whatever log or assertion result
/// happens to carry the state's `Debug`, which is not a trade the debugging
/// convenience is worth.
impl std::fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("pool", &self.pool)
            .field("key", &"<redacted>")
            .finish()
    }
}

/// A session that authenticated: the principal it binds, nothing more.
/// Tenant and grants are resolved per request against commerce memberships —
/// a session is identity, not authority.
#[derive(Debug)]
pub(crate) struct AuthenticatedSession {
    pub(crate) principal_id: String,
}

impl Identity {
    /// Opens the identity service, but only after readiness passes.
    ///
    /// The baseline's `PostgresIdentity.create` refuses three conditions and
    /// so does this: a key under 32 bytes, a runtime that could bypass RLS on
    /// `archava_identity`, and a migration history that is not exactly the
    /// one this build ships. The third check takes the expected `(id, sha256)`
    /// ledger as an argument for the same reason the host's commerce
    /// readiness does: which migrations "this build expects" belongs to
    /// whoever assembles the binary.
    pub(crate) async fn open(
        pool: PgPool,
        key: &[u8],
        expected_migrations: &[(String, String)],
    ) -> Result<Self, crate::AuthorityError> {
        if key.len() < MIN_KEY_BYTES {
            return Err(crate::AuthorityError::IdentitySecretTooShort);
        }
        archava_adapters::rls::assert_non_owner_runtime(&pool, IDENTITY_NAMESPACE, IDENTITY_LEDGER)
            .await
            .map_err(crate::AuthorityError::Context)?;

        let history: Vec<(String, String)> = sqlx::query_as(
            "SELECT id,checksum FROM archava_identity.schema_migrations ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .map_err(|error| crate::AuthorityError::database(IDENTITY_LEDGER, error))?;
        if history != expected_migrations {
            return Err(crate::AuthorityError::Context(
                archava_adapters::rls::TenantContextError::MigrationHistory {
                    ledger: IDENTITY_LEDGER,
                    expected: expected_migrations.len(),
                    found: history.len(),
                },
            ));
        }
        Ok(Self {
            pool,
            key: key.to_vec(),
        })
    }

    /// Builds identity WITHOUT the readiness checks — test-only, hence
    /// `#[cfg(test)]`, and `pub(crate)` so it stays inside this crate either
    /// way. The in-crate router tests drive refusals that fire before (or in
    /// spite of) database access, on a pool that never connects. The
    /// key-length rule is still enforced here — it is cheap, and a short test
    /// key is a bug in the test rather than a fixture to work around.
    /// External callers get [`Identity::open`] through
    /// [`crate::AppState::try_new`], which checks.
    #[cfg(test)]
    pub(crate) fn unchecked(pool: PgPool, key: &[u8]) -> Self {
        assert!(
            key.len() >= MIN_KEY_BYTES,
            "identity secret must contain at least {MIN_KEY_BYTES} random bytes"
        );
        Self {
            pool,
            key: key.to_vec(),
        }
    }

    /// `HMAC-SHA256(key, label + "\0" + value)`, hex — the baseline's
    /// `digest`, used for session hashes, csrf digests and bucket keys alike.
    pub(crate) fn digest(&self, label: &str, value: &str) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC accepts any key length");
        mac.update(label.as_bytes());
        mac.update(b"\0");
        mac.update(value.as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    /// The CSRF header value bound to `token` — derived, never stored.
    pub(crate) fn csrf(&self, token: &str) -> String {
        self.digest("csrf", token)
    }

    /// Timing-safe CSRF check. A wrong SHAPE (not 64 hex chars) is refused
    /// before any comparison, exactly as the baseline refuses early.
    pub(crate) fn valid_csrf(&self, token: &str, header: Option<&str>) -> bool {
        let Some(header) = header else { return false };
        let Ok(provided) = decode_hex(header) else {
            return false;
        };
        let expected = decode_hex(&self.csrf(token)).expect("our own digest is hex");
        ct_eq(&expected, &provided)
    }

    /// The port of `PostgresIdentity.login`.
    ///
    /// Order preserved from the original because each step's position is a
    /// security decision: rate-limit FIRST (a brute force costs budget before
    /// it costs scrypt), then the user lookup and a constant-work password
    /// verify (a missing user still pays a derivation, so the response time
    /// does not say whether the username exists), then the session insert
    /// that re-checks active + password hash inside its own `SELECT` — a user
    /// deactivated between the two reads lands in the refusal, not in a
    /// session.
    pub(crate) async fn login(
        &self,
        raw_username: &str,
        raw_password: &str,
        source: &str,
    ) -> Result<String, IdentityError> {
        let username = normalise_username(raw_username)?;
        if raw_password.len() > 128 || source.len() > 128 {
            return Err(IdentityError::InvalidCredentials);
        }
        self.rate(&username, source).await?;

        let found = sqlx::query(
            "SELECT id,password_hash,active FROM archava_identity.users WHERE username=$1",
        )
        .bind(&username)
        .fetch_optional(&self.pool)
        .await?;
        let (user_id, password_hash, active) = match found {
            Some(row) => (
                row.get::<String, _>("id"),
                row.get::<String, _>("password_hash"),
                row.get::<bool, _>("active"),
            ),
            None => (String::new(), String::new(), false),
        };
        let verified = verify_password(raw_password, Some(password_hash.as_str()))?;
        if !verified || !active || user_id.is_empty() {
            return Err(IdentityError::InvalidCredentials);
        }

        let token = random_token();
        let mut transaction = self.transaction().await?;
        let digest = self.digest("session", &token);
        let inserted = sqlx::query(
            "INSERT INTO archava_identity.sessions(token_hash,user_id,user_version) \
             SELECT $1,id,version FROM archava_identity.users \
             WHERE id=$2 AND active AND password_hash=$3 RETURNING user_id",
        )
        .bind(&digest)
        .bind(&user_id)
        .bind(&password_hash)
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() != 1 {
            transaction.rollback().await.ok();
            return Err(IdentityError::InvalidCredentials);
        }
        sqlx::query("INSERT INTO archava_identity.events(id,user_id,kind) VALUES($1,$2,'login')")
            .bind(uuid::Uuid::new_v4())
            .bind(&user_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM archava_identity.login_buckets WHERE key=$1")
            .bind(self.digest("account", &username))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(token)
    }

    /// The port of `PostgresIdentity.authenticate`.
    ///
    /// One statement, like the original: the `UPDATE ... RETURNING` both
    /// validates (user active, user version matching the session's, session
    /// unexpired and seen within the idle window) and refreshes
    /// `last_seen_at`. A token that fails any clause returns `None` — no
    /// error, no distinction between "never existed" and "expired".
    pub(crate) async fn authenticate(
        &self,
        token: &str,
    ) -> Result<Option<AuthenticatedSession>, IdentityError> {
        if !token_shape(token) {
            return Ok(None);
        }
        let digest = self.digest("session", token);
        let row = sqlx::query(
            "UPDATE archava_identity.sessions s SET last_seen_at=now() \
             FROM archava_identity.users u \
             WHERE s.token_hash=$1 AND u.id=s.user_id AND u.active \
             AND u.version=s.user_version AND s.expires_at>now() \
             AND s.last_seen_at>now()-interval '30 minutes' RETURNING s.user_id",
        )
        .bind(&digest)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| AuthenticatedSession {
            principal_id: row.get::<String, _>("user_id"),
        }))
    }

    /// The port of `PostgresIdentity.rate`: two buckets, source-first lock
    /// order, every attempt consuming budget whether or not it succeeds.
    async fn rate(&self, username: &str, source: &str) -> Result<(), IdentityError> {
        let mut transaction = self.transaction().await?;
        let mut blocked = false;
        for (label, value, seconds, limit) in [
            ("source", source, 300i32, 60i64),
            ("account", username, 900i32, 5i64),
        ] {
            let key = self.digest(label, value);
            let row = sqlx::query(
                "INSERT INTO archava_identity.login_buckets(key,attempts) VALUES($1,1) \
                 ON CONFLICT(key) DO UPDATE SET \
                 attempts=CASE WHEN login_buckets.window_start<now()-make_interval(secs=>$2::double precision) \
                 THEN 1 ELSE LEAST(login_buckets.attempts+1,1000) END, \
                 window_start=CASE WHEN login_buckets.window_start<now()-make_interval(secs=>$2::double precision) \
                 THEN now() ELSE login_buckets.window_start END RETURNING attempts",
            )
            .bind(&key)
            .bind(seconds as f64)
            .fetch_one(&mut *transaction)
            .await?;
            if row.get::<i32, _>("attempts") as i64 > limit {
                blocked = true;
            }
        }
        sqlx::query(
            "DELETE FROM archava_identity.login_buckets WHERE key IN \
             (SELECT key FROM archava_identity.login_buckets \
             WHERE window_start<now()-interval '1 day' LIMIT 100)",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "DELETE FROM archava_identity.sessions WHERE token_hash IN \
             (SELECT token_hash FROM archava_identity.sessions \
             WHERE expires_at<now() OR last_seen_at<now()-interval '30 minutes' LIMIT 100)",
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        if blocked {
            return Err(IdentityError::RateLimited);
        }
        Ok(())
    }

    /// A transaction with the baseline's statement/lock timeouts, so a login
    /// storm cannot hold identity rows hostage.
    async fn transaction(&self) -> Result<Transaction<'_, Postgres>, IdentityError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET LOCAL statement_timeout='10s'")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("SET LOCAL lock_timeout='3s'")
            .execute(&mut *transaction)
            .await?;
        Ok(transaction)
    }
}

/// The password KDF's parameters, from `password.ts` — fixed, never requested
/// by the caller (`verifyPassword`'s header: malformed/untrusted encodings
/// cannot request costly work).
fn scrypt_params() -> Params {
    // N=32768=2^15, r=8, p=3, 32-byte output: the baseline's exact profile.
    let params = Params::new(15, 8, 3, 32).expect("the baseline's scrypt parameters are valid");
    // Tie the profile to the baseline's `maxmem` (password.ts passes it on
    // every call): 128 * N * r is the memory scrypt will demand per
    // derivation, and someone raising N or r here must not be able to do so
    // silently past the ceiling the original enforces.
    let memory = 128usize * (1usize << params.log_n()) * (params.r() as usize);
    assert!(
        memory <= SCRYPT_MAXMEM,
        "the scrypt profile demands {memory} bytes, above the baseline's {SCRYPT_MAXMEM}"
    );
    params
}

/// One scrypt derivation, bounded by [`MAX_ACTIVE_DERIVATIONS`].
fn derive(password: &str, salt: &[u8]) -> Result<[u8; 32], IdentityError> {
    let previous = ACTIVE_DERIVATIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if previous >= MAX_ACTIVE_DERIVATIONS {
        ACTIVE_DERIVATIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        return Err(IdentityError::Busy);
    }
    let mut key = [0u8; 32];
    let result = scrypt(password.as_bytes(), salt, &scrypt_params(), &mut key);
    ACTIVE_DERIVATIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    result.map_err(|_| IdentityError::Busy)?;
    Ok(key)
}

/// The port of `hashPassword`, producing the same encoded form:
/// `scrypt$32768$8$3$<salt base64url>$<key base64url>`.
///
/// Public because a caller that provisions users (a fixture seeder, an
/// operator's first-run tool) must produce hashes this host can verify — and
/// hashing a password grants nobody any authority over one. The baseline's
/// minimum length of 15 is enforced here, at hash time, exactly where the
/// original enforces it.
pub fn encode_password(raw: &str) -> Result<String, IdentityError> {
    if raw.len() < 15 || raw.len() > 128 {
        return Err(IdentityError::InvalidCredentials);
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|_| IdentityError::Unavailable)?;
    let key = derive(raw, &salt)?;
    Ok(format!(
        "scrypt$32768$8$3${}${}",
        URL_SAFE_NO_PAD.encode(salt),
        URL_SAFE_NO_PAD.encode(key)
    ))
}

/// The port of `verifyPassword`: fixed parameters, constant work on a missing
/// or malformed record, timing-safe comparison.
/// Verify `password` against a stored record, or refuse for a reason that is
/// not "the password was wrong".
///
/// PW06/RN07: the old signature was `bool`, and [`derive`]'s `Err(Busy)` fell
/// into the same `else` branch as a failed comparison — so a login flood
/// beyond the derivation budget answered **401 invalid_credentials**. A
/// correct password, retried during a busy window, told its user the password
/// was wrong; the natural response to that answer is to retry and possibly
/// burn the account's rate budget, and a successful login that "becomes" a
/// failed one from server load is exactly the confusion the baseline's
/// with-reasons vocabulary exists to prevent. Overload must be its own
/// answer, which [`crate::router`] maps to the 503 `busy` shape.
///
/// `Ok(false)` still covers the cases where the password genuinely does not
/// match: a malformed stored record and a missing one are both indistinguishable
/// from a wrong password once they have paid a derivation to say so.
pub(crate) fn verify_password(
    password: &str,
    encoded: Option<&str>,
) -> Result<bool, IdentityError> {
    let parsed = encoded.and_then(parse_encoded);
    let (salt, expected) = match &parsed {
        Some((salt, key)) => (salt.as_slice(), Some(key.as_slice())),
        // A missing or malformed record still pays one derivation against a
        // dummy salt, so response time does not reveal that the user (or the
        // stored hash) does not exist.
        None => ([0u8; 16].as_slice(), None),
    };
    let actual = derive(password, salt)?;
    Ok(match expected {
        Some(expected) => ct_eq(expected, &actual),
        None => false,
    })
}

/// Parse `scrypt$32768$8$3$<salt>$<key>` into its decoded halves.
fn parse_encoded(encoded: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let parts: Vec<&str> = encoded.split('$').collect();
    if parts.len() != 6
        || parts[0] != "scrypt"
        || parts[1] != "32768"
        || parts[2] != "8"
        || parts[3] != "3"
        || parts[4].len() != 22
        || parts[5].len() != 43
    {
        return None;
    }
    let salt = URL_SAFE_NO_PAD.decode(parts[4]).ok()?;
    let key = URL_SAFE_NO_PAD.decode(parts[5]).ok()?;
    (salt.len() == 16 && key.len() == 32).then_some((salt, key))
}

/// A fresh session token: 32 random bytes, base64url, no padding — the
/// baseline's exact shape, which its 43-character schema pins.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system CSPRNG is available");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The token schema `sessions.ts` pins: 43 base64url characters, checked
/// before any database work so a hostile header cannot reach a query.
fn token_shape(token: &str) -> bool {
    token.len() == 43
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The baseline's `usernameSchema`: an email, lowercased, at most 254
/// characters. Refused rather than silently transformed when it is not one.
///
/// The shape check mirrors what zod's `.email()` actually rejects, not a
/// convenient approximation of it: exactly one `@` (a `split_once` would read
/// `two@@example.com` as local `two`, domain `@example.com` and let it past a
/// non-empty-and-contains-dot test), a non-empty local part, a domain with a
/// dot in it, and no whitespace anywhere in either half.
fn normalise_username(raw: &str) -> Result<String, IdentityError> {
    if raw.is_empty() || raw.len() > 254 || raw.chars().any(|c| c.is_control()) {
        return Err(IdentityError::InvalidCredentials);
    }
    let lowered = raw.to_lowercase();
    let mut parts = lowered.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(IdentityError::InvalidCredentials);
    };
    if local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || local.chars().any(char::is_whitespace)
        || domain.chars().any(char::is_whitespace)
    {
        return Err(IdentityError::InvalidCredentials);
    }
    Ok(lowered)
}

/// Constant-time byte comparison. Length is compared in the clear (both sides
/// are fixed-length digests here), everything else folds to a single OR.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Decode a hex string; anything that is not hex of even length is `None`.
fn decode_hex(text: &str) -> Result<Vec<u8>, ()> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).map_err(|_| ()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_shape_matches_the_baseline_schema() {
        assert!(token_shape(&random_token()));
        assert!(!token_shape("short"));
        assert!(!token_shape(&"a".repeat(44)));
        assert!(!token_shape(&"!".repeat(43)));
    }

    #[test]
    fn the_encoded_password_form_is_the_baseline_form() {
        // This runs a derivation, so it takes the same observation slot the
        // budget tests do: an unordered derivation landing under this test
        // would otherwise make "the third derivation is Busy" ambiguous about
        // which call was actually third.
        let _observation = budget_observation();
        let encoded = encode_password("correct horse battery staple").expect("hash");
        assert!(encoded.starts_with("scrypt$32768$8$3$"));
        assert!(parse_encoded(&encoded).is_some());
    }

    #[test]
    fn verification_pays_off_and_refuses_malformed_records() {
        let _observation = budget_observation();
        let encoded = encode_password("correct horse battery staple").expect("hash");
        assert!(
            verify_password("correct horse battery staple", Some(&encoded))
                .expect("a real record verifies")
        );
        // A wrong password is `Ok(false)` — an answer about the data, not a
        // server state.
        assert!(!verify_password("wrong password entirely", Some(&encoded))
            .expect("a mismatch is still an answer"));
        assert!(
            !verify_password("correct horse battery staple", Some("scrypt$1$2$3$aa$bb"))
                .expect("a malformed record is refused as data, not as a fault")
        );
        assert!(!verify_password("correct horse battery staple", None)
            .expect("a missing record is refused as data, not as a fault"));
    }

    /// The overload refusal is NOT `Ok(false)`: otherwise a busy server
    /// answers "the password was wrong", and a correct password retried during
    /// a login flood burns the account's rate budget chasing a credential
    /// problem that does not exist (PW06/RN07).
    ///
    /// [`active_derivations`] is what makes this deterministic rather than a
    /// race: the holder threads park derivations in flight, the main thread
    /// waits until the production counter actually reads the budget, and only
    /// then issues the third call — which must be refused. No timing estimate
    /// enters the proof; the deadline only bounds how long the test waits
    /// before declaring that it could not produce the state it needs.
    #[test]
    fn an_overloaded_server_is_busy_never_an_invalid_password() {
        let _observation = budget_observation();
        let holders: Vec<_> = (0..MAX_ACTIVE_DERIVATIONS)
            .map(|_| {
                std::thread::spawn(move || {
                    verify_password(
                        "correct horse battery staple",
                        Some(
                            "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$\
                             jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo",
                        ),
                    )
                })
            })
            .collect();

        // Wait until the holders are provably INSIDE their derivations. This
        // loop observes the production counter, not the clock, which is the
        // whole reason the test is not a probability.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while active_derivations() < MAX_ACTIVE_DERIVATIONS {
            assert!(
                std::time::Instant::now() < deadline,
                "the holders never reached the budget; this fixture cannot \
                 produce the state its assertion needs"
            );
            std::thread::yield_now();
        }

        // Both slots are taken. The third derivation is refused.
        let third = verify_password("correct horse battery staple", None);
        for holder in holders {
            assert!(
                holder.join().expect("the holders must finish").is_ok(),
                "each holder must complete its own derivation"
            );
        }

        assert!(
            matches!(third, Err(IdentityError::Busy)),
            "a third derivation against a budget of {MAX_ACTIVE_DERIVATIONS} \
             must be refused Busy, not mistaken for a wrong password, got {third:?}"
        );
        // And the counter returns to zero once the held derivations finish, so
        // a server that was overloaded stops refusing once it is not.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while active_derivations() != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the held derivations never returned the capacity they took"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn the_username_gate_normalises_and_refuses() {
        assert_eq!(
            normalise_username("Ops@Example.COM").expect("email"),
            "ops@example.com"
        );
        assert!(normalise_username("not-an-email").is_err());
        assert!(normalise_username("two@@example.com").is_err());
        assert!(normalise_username("").is_err());
    }

    #[test]
    fn csrf_comparison_is_exact_and_hex() {
        let identity_like_key = [7u8; 32];
        let digest = |value: &str| {
            let mut mac = Hmac::<Sha256>::new_from_slice(&identity_like_key).expect("hmac");
            mac.update(b"csrf\0");
            mac.update(value.as_bytes());
            hex(&mac.finalize().into_bytes())
        };
        let expected = digest("the-token");
        assert!(ct_eq(
            &decode_hex(&expected).unwrap(),
            &decode_hex(&expected).unwrap()
        ));
        assert!(!ct_eq(
            &decode_hex(&expected).unwrap(),
            &decode_hex(&digest("other")).unwrap()
        ));
        assert!(decode_hex("nothex").is_err());
    }
}
