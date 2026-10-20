//! P-R03-DOMAIN's HTTP half — the CRM conversation route against real HTTP
//! and real PostgreSQL.
//!
//! ## Why this suite exists
//!
//! The assistant crate's own tests prove the flow over a canned [`HostCrm`];
//! this suite re-proves the same decisions over the REAL router
//! (`tower::ServiceExt::oneshot`), a real session/CSRF boundary, and real SQL
//! against a disposable PostgreSQL whose `archava_crm` schema is the real
//! migration ledger. Every assertion is a fact about what the SERVER did:
//!
//! - the coverage reads ride the session's CRM membership — `contacts.read`
//!   plus scope, re-authorized inside each tool transaction, exactly like the
//!   other CRM routes;
//! - the preparers produce a REAL envelope over REAL rows (the prepared
//!   draft's revision comes from the actual contact row) and change NOTHING —
//!   the `followups` row count is asserted unchanged around every prepare;
//! - refusals surface as the recorded statuses with the masked error body —
//!   the detailed message stays in the log, so a refusal text is never an
//!   oracle about what the caller may not see.
//!
//! ## What this suite deliberately does NOT cover, and where it is covered
//!
//! - The revision-conflict branch (a contact moving between the flow's own
//!   summary read and the prepare read) needs an injection point BETWEEN two
//!   transactions inside one request. No honest HTTP surface can time that;
//!   the flow layer proves it with an injected host that bumps the revision
//!   (`archava-assistant`'s conversation tests). Here the envelope is proved
//!   to carry the REAL revision, which is the fact the conflict compares.
//! - The Gemini planner lane is a provider decision, not a CRM contract; this
//!   composition serves the local deterministic planner, the same one the TS
//!   fallback serves.
//! - The native-actions module gate is proved here against the persisted C03
//!   owner ledger (`crm-pipeline`); the assistant crate's flow-level tests
//!   never see a module decision, because it is a host composition rule.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use sqlx::types::Uuid;
use tower::ServiceExt;

const TABLES: &[&str] = &[
    "memberships",
    "products",
    "stock_balances",
    "orders",
    "order_lines",
    "allocations",
    "commands",
    "movements",
    "returns",
    "audit",
    "approvals",
];

/// A 32-byte identity HMAC key — the minimum `Identity::open` accepts.
const IDENTITY_KEY: &[u8] = &[7u8; 32];

/// The test password. The password gate enforces `length >= 15`.
const PASSWORD: &str = "correct horse battery staple";

/// A precomputed scrypt hash of [`PASSWORD`] — one hash serves every user.
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

/// Serving state that has passed the commerce, CRM and owner-ledger readiness
/// gates (`try_new_owner` composes all three).
async fn state(fixture: &Fixture) -> AppState {
    AppState::try_new_owner(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.crm_ledger,
        &fixture.owner_ledger,
    )
    .await
    .expect("a ready fixture must construct serving state")
}

const ACME: &str = "acme-retail";
const GLOBEX: &str = "globex-corp";

// --- The cast -----------------------------------------------------------------

struct Principal {
    id: &'static str,
    permissions: &'static [&'static str],
    scope: &'static str,
    active: bool,
    tenants: &'static [&'static str],
}

/// The conversation cast, in the vocabulary the memberships table carries.
/// The route has exactly one actor shape — a human session — so the cast
/// exercises grant *contents*, not principal kinds (the service-intersection
/// rules are the CRM command lane's, already pinned in `tests/crm.rs`):
///
/// - `alice` holds the full read/write surface including the pipeline grants,
///   scope `all` — the positive witness for everything she may see.
/// - `bob` holds the contact and follow-up reads but NO write grant — both
///   preparation refusals, and a summary whose activity section is empty
///   (his membership lacks `activities.read`).
/// - `carol` holds reads plus `followups.write`, scope `assigned`, owning two
///   contacts of her own — the scope-containment witness — and no
///   `pipeline.read`, which is the pipeline gate.
/// - `dave` holds only `followups.write` — the contact-read gate fires before
///   any tool, for every utterance: "+" "no CRM data without the read grant".
/// - `revoked` is `alice` with `active=false` — inactive reads as refused.
/// - `other` is a reader in a second workspace only — the cross-tenant shape.
const PRINCIPALS: &[Principal] = &[
    Principal {
        id: "alice",
        permissions: &[
            "contacts.read",
            "contacts.write",
            "activities.read",
            "activities.write",
            "followups.read",
            "followups.write",
            "pipeline.read",
            "pipeline.write",
            "audit.read",
        ],
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "bob",
        permissions: &["contacts.read", "followups.read"],
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "carol",
        permissions: &[
            "contacts.read",
            "followups.read",
            "followups.write",
            "activities.read",
        ],
        scope: "assigned",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "dave",
        permissions: &["followups.write"],
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "revoked",
        permissions: &["contacts.read", "followups.read", "followups.write"],
        scope: "all",
        active: false,
        tenants: &[ACME],
    },
    Principal {
        id: "other",
        permissions: &["contacts.read"],
        scope: "all",
        active: true,
        tenants: &[GLOBEX],
    },
];

// --- The seeded business records ----------------------------------------------

const ID_A: &str = "2c3e5a10-1111-4111-8111-0000000000a1";
const ID_B: &str = "2c3e5a10-1111-4111-8111-0000000000b1";
const ID_C: &str = "2c3e5a10-1111-4111-8111-0000000000b2";
const ID_W: &str = "2c3e5a10-1111-4111-8111-0000000000c1";
const ID_X: &str = "2c3e5a10-1111-4111-8111-0000000000c2";
const ID_G: &str = "2c3e5a10-1111-4111-8111-0000000000d1";

const ACT_A: &str = "2c3e5a10-1111-4111-8111-0000000000e1";

const F_LATE: &str = "2c3e5a10-1111-4111-8111-0000000000f1";
const F_NOW: &str = "2c3e5a10-1111-4111-8111-0000000000f2";
const F_SOON: &str = "2c3e5a10-1111-4111-8111-0000000000f3";
const F_DONE: &str = "2c3e5a10-1111-4111-8111-0000000000f4";
const F_C_LATE: &str = "2c3e5a10-1111-4111-8111-0000000000f5";

const OP_ALPHA: &str = "2c3e5a10-1111-4111-8111-0000000000e2";
const OP_BETA: &str = "2c3e5a10-1111-4111-8111-0000000000e3";
const OP_GAMMA: &str = "2c3e5a10-1111-4111-8111-0000000000e4";

/// The seeded due instants, anchored to the FIXTURE'S OWN database clock.
///
/// The flow stamps the DB clock for `occurredAt`, and the overview's `asOf`
/// IS the database clock — no request field pins it — so absolute seed dates
/// were a time bomb: the constants pinned `DUE_NOW` to 2026-10-18T08:00Z,
/// which held only while the wall clock sat before that instant, and the
/// overview's bucket counts detonated once it crossed. `seed` now reads the
/// clock once per fixture and derives every boundary from it; the worklist
/// test still gets its EXACT boundary equality by carrying the computed
/// instant back INTO the request as `followupAsOf` — the pin stays in the
/// caller's hands, and no assertion depends on when the machine runs.
struct Seeded {
    /// F_NOW's due time: one hour ahead of the clock, the boundary instant.
    pin: String,
    /// One day behind the clock (F_LATE; also F_DONE's completed due time).
    due_late: String,
    /// Three hours deeper into the past (F_C_LATE — keeps the
    /// [F_LATE, F_C_LATE] due order determinate).
    due_c_late: String,
    /// One day ahead of the clock (F_SOON).
    due_soon: String,
}

async fn seed(fixture: &Fixture) -> Seeded {
    let (due_late, due_c_late, pin, due_soon): (String, String, String, String) = sqlx::query_as(
        "SELECT \
         to_char((now() - interval '1 day') AT TIME ZONE 'UTC', \
                 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
         to_char((now() - interval '1 day' + interval '3 hours') AT TIME ZONE 'UTC', \
                 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
         to_char((now() + interval '1 hour') AT TIME ZONE 'UTC', \
                 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
         to_char((now() + interval '1 day') AT TIME ZONE 'UTC', \
                 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
    )
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("anchor the seeded due instants to the fixture's clock");
    let seeded = Seeded {
        pin,
        due_late,
        due_c_late,
        due_soon,
    };
    for principal in PRINCIPALS {
        sqlx::query(
            "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
             VALUES ($1, $2, $3, true, 1)",
        )
        .bind(principal.id)
        .bind(format!("{}@archava.test", principal.id))
        .bind(PASSWORD_HASH)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the identity user");

        for tenant in principal.tenants {
            sqlx::query(
                "INSERT INTO archava_crm.memberships \
                 (tenant_id, principal_id, kind, permissions, scope, active) \
                 VALUES ($1, $2, 'human', $3, $4, $5)",
            )
            .bind(tenant)
            .bind(principal.id)
            .bind(principal.permissions.to_vec())
            .bind(principal.scope)
            .bind(principal.active)
            .execute(&fixture.admin_pool)
            .await
            .expect("seed the CRM membership");
        }
    }

    // The contacts. Owner bindings must hold for the opportunity FK below
    // (002_sales_pipeline: opportunities reference (tenant,contact,owner)).
    for (tenant, id, full_name, owner) in [
        (ACME, ID_A, "Ada Lovelace", "alice"),
        (ACME, ID_B, "Carol Onwire", "carol"),
        (ACME, ID_C, "Carol Second", "carol"),
        (ACME, ID_W, "A%B Format", "bob"),
        (ACME, ID_X, "AXB Format", "bob"),
        (GLOBEX, ID_G, "Globex Lead", "other"),
    ] {
        sqlx::query(
            "INSERT INTO archava_crm.contacts \
             (tenant_id, id, full_name, email, organization, owner_id) \
             VALUES ($1, $2, $3, NULL, NULL, $4)",
        )
        .bind(tenant)
        .bind(Uuid::parse_str(id).expect("a test uuid"))
        .bind(full_name)
        .bind(owner)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the contact");
    }

    sqlx::query(
        "INSERT INTO archava_crm.activities (tenant_id, id, contact_id, kind, summary, actor_id) \
         VALUES ($1, $2, $3, 'note', 'Call booked at ten.', $4)",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ACT_A).expect("a test uuid"))
    .bind(Uuid::parse_str(ID_A).expect("a test uuid"))
    .bind("alice")
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the activity");

    for (id, contact, summary, due, status, revision) in [
        (
            F_LATE,
            ID_A,
            "Send the catalog",
            seeded.due_late.clone(),
            "open",
            1,
        ),
        (F_NOW, ID_A, "Boundary task", seeded.pin.clone(), "open", 2),
        (
            F_SOON,
            ID_A,
            "Book the demo",
            seeded.due_soon.clone(),
            "open",
            3,
        ),
        (
            F_DONE,
            ID_B,
            "Archive the note",
            seeded.due_late.clone(),
            "completed",
            1,
        ),
        (
            F_C_LATE,
            ID_C,
            "Resend the quote",
            seeded.due_c_late.clone(),
            "open",
            2,
        ),
    ] {
        sqlx::query(
            "INSERT INTO archava_crm.followups \
             (tenant_id, id, contact_id, summary, due_at, status, revision) \
             VALUES ($1, $2, $3, $4, $5::timestamptz, $6, $7)",
        )
        .bind(ACME)
        .bind(Uuid::parse_str(id).expect("a test uuid"))
        .bind(Uuid::parse_str(contact).expect("a test uuid"))
        .bind(summary)
        .bind(due)
        .bind(status)
        .bind(revision)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the followup");
    }

    // Pipeline rows. Each transition consumes a `commands` row (the FK), and
    // the record FK forces the opportunity's owner to be the CONTACT's owner.
    for (request, actor) in [
        ("2c3e5a10-1111-4111-8111-000000000001", "alice"),
        ("2c3e5a10-1111-4111-8111-000000000002", "carol"),
        ("2c3e5a10-1111-4111-8111-000000000003", "carol"),
        ("2c3e5a10-1111-4111-8111-000000000004", "carol"),
        ("2c3e5a10-1111-4111-8111-000000000005", "carol"),
        ("2c3e5a10-1111-4111-8111-000000000006", "carol"),
    ] {
        sqlx::query(
            "INSERT INTO archava_crm.commands (tenant_id, request_id, actor_id, fingerprint) \
             VALUES ($1, $2, $3, 'seed-assistant-transition')",
        )
        .bind(ACME)
        .bind(Uuid::parse_str(request).expect("a test uuid"))
        .bind(actor)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the transition's command row");
    }

    for (id, contact, owner, title, stage, revision) in [
        (OP_ALPHA, ID_A, "alice", "Ada Renewal", "new", 1),
        (OP_BETA, ID_C, "carol", "Carol Quote", "proposal", 2),
        (OP_GAMMA, ID_B, "carol", "Carol Renewal", "won", 3),
    ] {
        sqlx::query(
            "INSERT INTO archava_crm.opportunities \
             (tenant_id, id, contact_id, owner_id, title, stage, revision) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(ACME)
        .bind(Uuid::parse_str(id).expect("a test uuid"))
        .bind(Uuid::parse_str(contact).expect("a test uuid"))
        .bind(owner)
        .bind(title)
        .bind(stage)
        .bind(revision)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the opportunity");
    }

    for (request, opportunity, from_stage, to_stage, reason, actor, revision) in [
        (
            "2c3e5a10-1111-4111-8111-000000000001",
            OP_ALPHA,
            None::<&str>,
            "new",
            None::<&str>,
            "alice",
            1,
        ),
        (
            "2c3e5a10-1111-4111-8111-000000000002",
            OP_BETA,
            None::<&str>,
            "new",
            None::<&str>,
            "carol",
            1,
        ),
        (
            "2c3e5a10-1111-4111-8111-000000000003",
            OP_BETA,
            Some("new"),
            "proposal",
            Some("Demo concluded"),
            "carol",
            2,
        ),
        (
            "2c3e5a10-1111-4111-8111-000000000004",
            OP_GAMMA,
            None::<&str>,
            "new",
            None::<&str>,
            "carol",
            1,
        ),
        (
            "2c3e5a10-1111-4111-8111-000000000005",
            OP_GAMMA,
            Some("new"),
            "qualified",
            None::<&str>,
            "carol",
            2,
        ),
        (
            "2c3e5a10-1111-4111-8111-000000000006",
            OP_GAMMA,
            Some("qualified"),
            "won",
            None::<&str>,
            "carol",
            3,
        ),
    ] {
        sqlx::query(
            "INSERT INTO archava_crm.opportunity_transitions \
             (tenant_id, request_id, opportunity_id, from_stage, to_stage, reason, actor_id, revision) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(ACME)
        .bind(Uuid::parse_str(request).expect("a test uuid"))
        .bind(Uuid::parse_str(opportunity).expect("a test uuid"))
        .bind(from_stage)
        .bind(to_stage)
        .bind(reason)
        .bind(actor)
        .bind(revision)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the transition");
    }

    seeded
}

// --- HTTP helpers ------------------------------------------------------------

/// A session's credentials.
struct Session {
    token: String,
    csrf: String,
}

/// How many logins this suite may have in flight at once — the production
/// budget (RN04/RN07: never raised from inside the suite).
static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

/// Log in and return the session's bearer token and CSRF value.
async fn login(app: &Router, principal_id: &str) -> Session {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = serde_json::json!({
        "username": format!("{}@archava.test", principal_id),
        "password": PASSWORD,
    })
    .to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("build the login request"),
        )
        .await
        .expect("the login route must answer");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "login as {principal_id} must succeed"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the login response");
    let outcome: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the login response is JSON");

    Session {
        token: outcome["token"]
            .as_str()
            .expect("the login response carries a token")
            .to_string(),
        csrf: outcome["csrf"]
            .as_str()
            .expect("the login response carries a CSRF value")
            .to_string(),
    }
}

/// Send a POST with (or without) a session's credentials, always carrying the
/// CSRF header unless `with_csrf` says otherwise.
async fn post(
    app: &Router,
    uri: &str,
    body: &str,
    session: Option<&Session>,
    with_csrf: bool,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(session) = session {
        builder = builder.header("authorization", format!("Bearer {}", session.token));
        if with_csrf {
            builder = builder.header("x-archava-csrf", &session.csrf);
        }
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("build the request"),
        )
        .await
        .expect("the route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the response body");
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    (status, outcome)
}

/// One conversation turn against [`ACME`] with a caller-supplied body.
async fn ask(
    app: &Router,
    session: &Session,
    body: &serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    post(
        app,
        &format!("/api/crm/{ACME}/conversation"),
        &body.to_string(),
        Some(session),
        true,
    )
    .await
}

/// The plain-utterance body.
fn says(text: &str) -> serde_json::Value {
    serde_json::json!({ "utterance": text })
}

// --- Database counters ----------------------------------------------------------

/// How many follow-ups a contact currently carries — the read-only proof for
/// every prepare (the conversation lane writes NOTHING).
async fn followups_for(fixture: &Fixture, contact: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM archava_crm.followups \
         WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(contact).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups")
}

/// A follow-up's live status.
async fn followup_status(fixture: &Fixture, id: &str) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT status FROM archava_crm.followups WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read a followup status")
}

// --- The refusals ---------------------------------------------------------------

/// **Witness: the conversation helps on an unmapped utterance.**
///
/// A session over a real CRM answers politely and names ITS planner: source
/// `none` (this is a prompt, not a CRM read), the local deterministic planner
/// identity, and the recorded Indonesian help text the TS baseline serves.
#[tokio::test]
async fn an_unmapped_utterance_gets_the_recorded_help() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "bob").await;

    let (status, reply) = ask(&app, &session, &says("halo dunia")).await;

    assert_eq!(status, StatusCode::OK, "the body answered: {reply}");
    assert_eq!(reply["source"], "none", "help is a prompt, not a CRM read");
    assert_eq!(reply["planner"]["providerId"], "archava-crm-local");
    assert_eq!(reply["planner"]["model"], "deterministic-commands-v1");
    assert_eq!(reply["data"]["kind"], "help");
    let text = reply["text"]
        .as_str()
        .expect("help carries the describing text");
    assert!(
        text.starts_with("Coba \u{201c}ringkas kondisi CRM\u{201d}"),
        "the recorded help text, got {text}"
    );
}

/// **Witness: no session, no turn.**
///
/// The dispatcher's POST rule runs first: a bearer token the server never
/// issued (or no token at all) is refused with the identity envelope — the
/// CRM answer shape stays masked either way, because identity refusals must
/// not leak whether a tenant exists.
#[tokio::test]
async fn a_turn_without_a_session_or_with_a_forged_one_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let (absent_status, _) = post(
        &app,
        &format!("/api/crm/{ACME}/conversation"),
        &says("halo").to_string(),
        None,
        false,
    )
    .await;
    assert_eq!(absent_status, StatusCode::UNAUTHORIZED);

    let forged = Session {
        token: "totally-forged".to_string(),
        csrf: "x".to_string(),
    };
    let (forged_status, _) = post(
        &app,
        &format!("/api/crm/{ACME}/conversation"),
        &says("halo").to_string(),
        Some(&forged),
        true,
    )
    .await;
    assert_eq!(forged_status, StatusCode::UNAUTHORIZED);
}

/// **Witness: the contact-read gate answers before any tool.**
///
/// `dave` holds only `followups.write`: every CRM-data utterance — list,
/// overview, summary, worklist, pipeline — is refused with 403 and the masked
/// refusal body. An empty page or an empty overview would be a different
/// fact ("no records") than the truth ("no grant"), so nothing is served.
#[tokio::test]
async fn a_member_without_the_contact_read_grant_is_refused_before_any_tool() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "dave").await;

    for utterance in [
        "tampilkan kontak",
        "ringkas kondisi CRM",
        "ringkas Ada Lovelace",
        "tampilkan follow-up terlambat",
        "tampilkan peluang",
        "siapkan follow-up untuk Ada Lovelace",
    ] {
        let (status, refusal) = ask(&app, &session, &says(utterance)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "dave asking {utterance}");
        assert_eq!(
            refusal["error"]["message"], "CRM request refused.",
            "the masked refusal, every time"
        );
    }
}

/// **Witness: the overview counts the records the scope allows.**
///
/// Alice sees the whole workspace: five contacts, four open follow-ups of
/// which two are behind the overview's asOf instant — the database clock,
/// which the request cannot pin, so the seeds are anchored to it (see
/// `Seeded`) — and three opportunities of which two are active. Carol, scope
/// `assigned`, sees only her own two contacts and her own two opportunities.
/// Both snapshots carry the invariant-holding counts (open == overdue +
/// upcoming) — the numbers are REAL row counts, not fixtures.
#[tokio::test]
async fn an_overview_counts_the_records_the_scope_allows() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let carol = login(&app, "carol").await;

    let (status, reply) = ask(&app, &alice, &says("ringkas kondisi CRM")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["source"], "crm");
    let snapshot = &reply["data"]["snapshot"];
    assert_eq!(snapshot["scope"], "all");
    assert_eq!(snapshot["contacts"], 5);
    assert_eq!(snapshot["followups"]["open"], 4);
    assert_eq!(snapshot["followups"]["overdue"], 2, "F_LATE and F_C_LATE");
    assert_eq!(
        snapshot["followups"]["upcoming"], 2,
        "the boundary task and F_SOON"
    );
    assert_eq!(snapshot["followups"]["completed"], 1);
    assert_eq!(snapshot["pipeline"]["total"], 3);
    assert_eq!(snapshot["pipeline"]["active"], 2, "new + proposal");
    assert_eq!(snapshot["pipeline"]["won"], 1);
    assert_eq!(snapshot["pipeline"]["lost"], 0);
    assert!(snapshot["asOf"]
        .as_str()
        .expect("the overview stamps its serving instant")
        .starts_with("2026-"));

    let (carol_status, carol_reply) = ask(&app, &carol, &says("ringkas kondisi CRM")).await;
    assert_eq!(carol_status, StatusCode::OK);
    let carol_snapshot = &carol_reply["data"]["snapshot"];
    assert_eq!(carol_snapshot["scope"], "assigned");
    assert_eq!(carol_snapshot["contacts"], 2, "only Carol's own contacts");
    // Carol holds no pipeline.read: the section is absent (null), exactly
    // like the TS overview — a missing section is a missing grant, not a
    // zero count.
    assert_eq!(carol_snapshot["pipeline"], serde_json::Value::Null);
}

/// **Witness: the contact search answers items, pagination and escaping over
/// real rows.**
///
/// `cari Carol` finds both Carols; `cari A%` must find the ONE contact whose
/// name carries a literal percent — AXB Format must NOT match — proving the
/// search value stays a literal the same way the contact route's does. The
/// `after` cursor slices by id: the next page excludes the rows up to the
/// cursor.
#[tokio::test]
async fn the_contact_search_answers_items_pagination_and_escaped_search() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "bob").await;

    let (status, reply) = ask(&app, &session, &says("cari Carol")).await;
    assert_eq!(status, StatusCode::OK, "search body: {reply}");
    assert_eq!(reply["source"], "crm");
    assert_eq!(reply["data"]["kind"], "contacts");
    let ids: Vec<&str> = reply["data"]["items"]
        .as_array()
        .expect("a page of items")
        .iter()
        .map(|item| item["id"].as_str().expect("an id"))
        .collect();
    assert_eq!(ids, [ID_B, ID_C], "both Carols, in the read order");
    assert_eq!(reply["data"]["next"], serde_json::Value::Null);

    let (escaped_status, escaped_reply) = ask(&app, &session, &says("cari A%")).await;
    assert_eq!(escaped_status, StatusCode::OK);
    let escaped: Vec<&str> = escaped_reply["data"]["items"]
        .as_array()
        .expect("a page of items")
        .iter()
        .map(|item| item["id"].as_str().expect("an id"))
        .collect();
    assert_eq!(
        escaped,
        [ID_W],
        "the percent stays literal: only A%B matches"
    );

    let (page_status, page_reply) = ask(
        &app,
        &session,
        &serde_json::json!({ "utterance": "tampilkan kontak", "after": ID_A }),
    )
    .await;
    assert_eq!(page_status, StatusCode::OK);
    let rest: Vec<&str> = page_reply["data"]["items"]
        .as_array()
        .expect("a page of items")
        .iter()
        .map(|item| item["id"].as_str().expect("an id"))
        .collect();
    assert_eq!(
        rest,
        [ID_B, ID_C, ID_W, ID_X],
        "the cursor slices past ID_A"
    );
}

/// **Witness: the summary walks exact-match, choose and not-found over real
/// rows, and the contactId shortcut skips the search.**
///
/// `ringkas Ada Lovelace` resolves exactly and serves the REAL record — the
/// one activity her grant covers and her three follow-ups in due order. Two
/// Carols produce the choose prompt (`source: crm`, but a list, not a record).
/// An unknown name is a not-found data answer — still 200, still `source:
/// crm`, because the fact served IS the CRM's answer.
#[tokio::test]
async fn the_summary_walks_exact_choose_and_notfound_over_real_rows() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let bob = login(&app, "bob").await;

    let (status, reply) = ask(&app, &alice, &says("ringkas Ada Lovelace")).await;
    assert_eq!(status, StatusCode::OK, "summary body: {reply}");
    assert_eq!(reply["source"], "crm");
    let summary = &reply["data"]["contact"];
    assert_eq!(summary["id"], ID_A);
    assert_eq!(summary["fullName"], "Ada Lovelace");
    assert_eq!(summary["revision"], 1);
    let activities = summary["activities"]
        .as_array()
        .expect("the activity section");
    assert_eq!(activities.len(), 1, "the one seeded activity");
    assert_eq!(activities[0]["kind"], "note");
    assert_eq!(activities[0]["summary"], "Call booked at ten.");
    let followups = summary["followups"]
        .as_array()
        .expect("the followup section");
    let due_tasks: Vec<&str> = followups
        .iter()
        .map(|task| task["id"].as_str().expect("a task id"))
        .collect();
    assert_eq!(due_tasks, [F_LATE, F_NOW, F_SOON], "due order");
    assert_eq!(followups[0]["status"], "open");

    let (_, choose) = ask(&app, &alice, &says("ringkas Carol")).await;
    assert_eq!(choose["data"]["kind"], "choose", "two Carols named");
    assert_eq!(
        choose["data"]["items"]
            .as_array()
            .expect("candidates")
            .len(),
        2
    );

    let (_, missing) = ask(&app, &alice, &says("ringkas Hilang Sekali")).await;
    assert_eq!(
        missing["data"]["kind"], "not_found",
        "an unknown name, answered"
    );

    // The recorded contactId shortcut: no search, straight to the record.
    let (_, direct) = ask(
        &app,
        &bob,
        &serde_json::json!({ "utterance": "ringkas", "contactId": ID_A }),
    )
    .await;
    assert_eq!(direct["data"]["kind"], "summary");
    assert_eq!(direct["data"]["contact"]["id"], ID_A);

    // bob lacks activities.read: the section collapses to EMPTY, not to an
    // error — the real grant boundary the TS detail read rides.
    let (_, bob_summary) = ask(&app, &bob, &says("ringkas Ada Lovelace")).await;
    assert_eq!(
        bob_summary["data"]["contact"]["activities"]
            .as_array()
            .expect("sections")
            .len(),
        0
    );
    assert_eq!(
        bob_summary["data"]["contact"]["followups"]
            .as_array()
            .expect("sections")
            .len(),
        3,
        "bob DOES hold followups.read"
    );
}

/// **Witness: the worklist reads buckets, the boundary equality and scope.**
///
/// The overdue bucket is strictly `due < asOf`; the upcoming bucket is
/// `due >= asOf` — the boundary follow-up (due exactly AT the pinned
/// instant) belongs to upcoming, in both the service code and the SQL state
/// machine. Carol's assigned scope keeps her queue to her own contacts' rows
/// even when identical rows exist for others, and a completed follow-up is
/// in NO open bucket. The keyset cursor excludes everything up to it.
#[tokio::test]
async fn the_worklist_reads_buckets_boundaries_and_scope() {
    let fixture = provision().await;
    let seeded = seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let carol = login(&app, "carol").await;

    let mut overdue = serde_json::json!({ "utterance": "tampilkan follow-up terlambat" });
    overdue["followupAsOf"] = serde_json::json!(seeded.pin);
    let (status, reply) = ask(&app, &alice, &overdue).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["data"]["kind"], "followups");
    assert_eq!(reply["data"]["bucket"], "overdue");
    assert_eq!(reply["data"]["asOf"], seeded.pin.as_str());
    let late_tasks: Vec<&str> = reply["data"]["items"]
        .as_array()
        .expect("the bucket")
        .iter()
        .map(|task| task["id"].as_str().expect("a task id"))
        .collect();
    assert_eq!(
        late_tasks,
        [F_LATE, F_C_LATE],
        "due order, completed excluded"
    );
    assert_eq!(
        reply["data"]["items"][0]["fullName"], "Ada Lovelace",
        "the queue names each row's contact"
    );
    assert_eq!(reply["data"]["next"], serde_json::Value::Null);

    let mut upcoming = serde_json::json!({ "utterance": "tampilkan follow-up mendatang" });
    upcoming["followupAsOf"] = serde_json::json!(seeded.pin);
    let (_, upcoming_reply) = ask(&app, &alice, &upcoming).await;
    let soon_tasks: Vec<&str> = upcoming_reply["data"]["items"]
        .as_array()
        .expect("the bucket")
        .iter()
        .map(|task| task["id"].as_str().expect("a task id"))
        .collect();
    assert_eq!(
        soon_tasks,
        [F_NOW, F_SOON],
        "the boundary task lands in upcoming (>= asOf)"
    );

    let mut open = serde_json::json!({ "utterance": "tampilkan follow-ups" });
    open["followupAsOf"] = serde_json::json!(seeded.pin);
    let (_, open_reply) = ask(&app, &alice, &open).await;
    assert_eq!(open_reply["data"]["bucket"], "open", "the unbucketed ask");
    assert_eq!(
        open_reply["data"]["items"].as_array().expect("rows").len(),
        4
    );

    // Keyset pagination: a cursor excludes everything up to it.
    let mut paged = serde_json::json!({ "utterance": "tampilkan follow-ups" });
    paged["followupAsOf"] = serde_json::json!(seeded.pin);
    paged["followupAfter"] = serde_json::json!({ "dueAt": seeded.due_late, "id": F_LATE });
    let (_, paged_reply) = ask(&app, &alice, &paged).await;
    let remaining: Vec<&str> = paged_reply["data"]["items"]
        .as_array()
        .expect("the page")
        .iter()
        .map(|task| task["id"].as_str().expect("a task id"))
        .collect();
    assert_eq!(
        remaining.len(),
        3,
        "F_LATE and everything before it excluded"
    );
    assert_eq!(
        paged_reply["data"]["next"],
        serde_json::Value::Null,
        "4 rows, one page cap is 20"
    );

    // Scope: carol's queue carries only rows reachable through HER contacts.
    let mut carol_overdue = serde_json::json!({ "utterance": "tampilkan follow-up terlambat" });
    carol_overdue["followupAsOf"] = serde_json::json!(seeded.pin);
    let (_, carol_reply) = ask(&app, &carol, &carol_overdue).await;
    let carol_tasks: Vec<&str> = carol_reply["data"]["items"]
        .as_array()
        .expect("the bucket")
        .iter()
        .map(|task| task["id"].as_str().expect("a task id"))
        .collect();
    assert_eq!(carol_tasks, [F_C_LATE], "her own late task only");
}

/// **Witness: the draft prepare prompts, then envelopes a REAL draft — and
/// writes nothing.**
///
/// Without a draft the turn asks for one (`needs_draft`, the contact named).
/// With a draft the envelope pins the contact's REAL revision and the
/// recorded requires-approval flag. The `followups` table holds the same row
/// count before and after — preparation is a read there too.
#[tokio::test]
async fn preparing_a_followup_needs_then_envelops_a_real_draft() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let before = followups_for(&fixture, ID_A).await;
    let (_, prompt) = ask(&app, &alice, &says("siapkan follow-up untuk Ada Lovelace")).await;
    assert_eq!(
        prompt["data"]["kind"], "needs_draft",
        "prompt body: {prompt}"
    );
    assert_eq!(prompt["source"], "none", "the prompt carries no CRM record");
    assert_eq!(followups_for(&fixture, ID_A).await, before);

    let mut with_draft = serde_json::json!({ "utterance": "siapkan follow-up untuk Ada Lovelace" });
    with_draft["draft"] = serde_json::json!({
        "summary": "Kirim katalog",
        "dueAt": "2027-03-04T10:15:00Z",
    });
    let (_, envelope) = ask(&app, &alice, &with_draft).await;
    assert_eq!(envelope["data"]["kind"], "draft");
    assert_eq!(envelope["data"]["fullName"], "Ada Lovelace");
    let draft = &envelope["data"]["draft"];
    assert_eq!(draft["contactId"], ID_A);
    assert_eq!(draft["contactRevision"], 1, "the REAL row's revision");
    assert_eq!(draft["tenantId"], ACME);
    assert_eq!(draft["summary"], "Kirim katalog");
    assert_eq!(draft["dueAt"], "2027-03-04T10:15:00Z");
    assert_eq!(draft["status"], "draft");
    assert_eq!(draft["requiresApproval"], true);
    assert_eq!(
        followups_for(&fixture, ID_A).await,
        before,
        "prepare is a read"
    );
}

/// **Witness: completion prepare prompts, pins the REAL open task — and
/// writes nothing; a completed task is a conflict.**
///
/// `tinjau penyelesaian follow-up` asks for the task; with a real task id the
/// flow returns the completion envelope over the LIVE row (status still
/// `open` in the database after the turn). Naming the COMPLETED task is a
/// 409 with the masked body — the caller DID name a task they can see, so
/// this is a lifecycle race, not a missing row.
#[tokio::test]
async fn completing_a_followup_prepares_from_a_real_open_task() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, prompt) = ask(&app, &alice, &says("tinjau penyelesaian follow-up")).await;
    assert_eq!(prompt["data"]["kind"], "needs_followup");

    let mut named = serde_json::json!({ "utterance": "tinjau penyelesaian follow-up" });
    named["followupId"] = serde_json::json!(F_LATE);
    let (status, envelope) = ask(&app, &alice, &named).await;
    assert_eq!(status, StatusCode::OK, "complete body: {envelope}");
    assert_eq!(envelope["data"]["kind"], "completion_draft");
    let task = &envelope["data"]["task"];
    assert_eq!(task["followup"]["id"], F_LATE);
    assert_eq!(task["followup"]["status"], "open");
    assert_eq!(task["followup"]["summary"], "Send the catalog");
    assert_eq!(task["contact"]["id"], ID_A);
    assert_eq!(task["contact"]["fullName"], "Ada Lovelace");
    assert_eq!(task["contact"]["revision"], 1);
    assert_eq!(envelope["data"]["requiresApproval"], true);
    assert_eq!(
        followup_status(&fixture, F_LATE).await,
        "open",
        "prepare touched nothing"
    );

    let mut completed = serde_json::json!({ "utterance": "tinjau penyelesaian follow-up" });
    completed["followupId"] = serde_json::json!(F_DONE);
    let (conflict_status, conflict) = ask(&app, &alice, &completed).await;
    assert_eq!(conflict_status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["message"], "CRM request refused.");
    assert_eq!(
        followup_status(&fixture, F_DONE).await,
        "completed",
        "still completed"
    );
}

/// **Witness: both preparations refuse without the follow-up write grant.**
///
/// `bob` reads fine but writes nothing: the draft prepare refuses at the flow
/// gate before any contact work, and the completion prepare refuses equally
/// — each with 403 and the masked body. (The TS flow checks the write grant
/// in the resolve step, before the search; this composition mirrors that
/// order, so even the contact prompt never surfaces for a non-writer.)
#[tokio::test]
async fn without_the_followup_write_grant_both_preparations_refuse() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let bob = login(&app, "bob").await;

    let (draft_status, draft_refusal) =
        ask(&app, &bob, &says("siapkan follow-up untuk Ada Lovelace")).await;
    assert_eq!(draft_status, StatusCode::FORBIDDEN);
    assert_eq!(draft_refusal["error"]["message"], "CRM request refused.");

    let mut completion = serde_json::json!({ "utterance": "tinjau penyelesaian follow-up" });
    completion["followupId"] = serde_json::json!(F_LATE);
    let (complete_status, complete_refusal) = ask(&app, &bob, &completion).await;
    assert_eq!(complete_status, StatusCode::FORBIDDEN);
    assert_eq!(complete_refusal["error"]["message"], "CRM request refused.");
}

/// **Witness: the pipeline lane gates on `pipeline.read` and serves real
/// opportunities.**
///
/// Carol holds the contact read but not the pipeline read: her peluang ask
/// refuses with 403. Alice reads the real rows: the plain list answers the
/// three seeded opportunities in id order, the search finds by title, and
/// the summary walks a REAL detail record with its transition history —
/// revisions in descending order, reasons carried through.
#[tokio::test]
async fn the_pipeline_lane_gates_reads_and_serves_real_rows() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let carol = login(&app, "carol").await;

    let (gate_status, gate_refusal) = ask(&app, &carol, &says("tampilkan peluang")).await;

    assert_eq!(gate_status, StatusCode::FORBIDDEN);
    assert_eq!(gate_refusal["error"]["message"], "CRM request refused.");

    let (status, list_reply) = ask(&app, &alice, &says("tampilkan peluang")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list_reply["source"], "crm");
    assert_eq!(list_reply["data"]["kind"], "opportunities");
    let titles: Vec<&str> = list_reply["data"]["items"]
        .as_array()
        .expect("the pipeline page")
        .iter()
        .map(|item| item["title"].as_str().expect("a title"))
        .collect();
    assert_eq!(titles, ["Ada Renewal", "Carol Quote", "Carol Renewal"]);
    assert_eq!(list_reply["data"]["next"], serde_json::Value::Null);

    let (_, search_reply) = ask(&app, &alice, &says("cari peluang Carol Quote")).await;
    let found: Vec<&str> = search_reply["data"]["items"]
        .as_array()
        .expect("the search page")
        .iter()
        .map(|item| item["id"].as_str().expect("an id"))
        .collect();
    assert_eq!(found, [OP_BETA], "the exact title, one row");

    let (_, detail_reply) = ask(&app, &alice, &says("ringkas peluang Carol Renewal")).await;
    let detail = &detail_reply["data"]["opportunity"];
    assert_eq!(detail["id"], OP_GAMMA);
    assert_eq!(detail["stage"], "won");
    assert_eq!(detail["revision"], 3);
    let history = detail["transitions"].as_array().expect("the history");
    assert_eq!(history.len(), 3, "all three recorded moves");
    assert_eq!(history[0]["revision"], 3, "history runs newest first");
    assert_eq!(history[0]["to"], "won");
    assert_eq!(history[1]["from"], "new");
    assert_eq!(history[2]["to"], "new", "the creation move is from nothing");
}

/// **Witness: the tenant segment and the body are validated before any
/// database work.**
///
/// A tenant slug that cannot exist is a 400, and so is a body that is not
/// `ConversationInput`: the masked `Invalid request.` — the same answer the
/// input-strict parse serves — never a 500, never a partial read.
#[tokio::test]
async fn the_tenant_segment_and_body_are_validated_before_any_database_work() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (tenant_status, _tenant_refusal) = post(
        &app,
        "/api/crm/Not_a_Tenant/conversation",
        &says("halo").to_string(),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(tenant_status, StatusCode::BAD_REQUEST);

    // Malformed JSON is the same 400.
    let (bad_json_status, bad_json) = post(
        &app,
        &format!("/api/crm/{ACME}/conversation"),
        "{\"utterance\":",
        Some(&session),
        true,
    )
    .await;
    assert_eq!(bad_json_status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_json["error"]["message"], "Invalid request.");

    // An unknown key is refused the same way — the input parse is strict.
    let (_, unknown) = ask(
        &app,
        &session,
        &serde_json::json!({ "utterance": "halo", "expected": "no such key" }),
    )
    .await;
    assert_eq!(unknown["error"]["message"], "Invalid request.");

    // A same-session POST without the CSRF header is the identity refusal —
    // the house envelope (reason `csrf_required`), not the CRM-masked shape.
    let (csrf_status, csrf_refusal) = post(
        &app,
        &format!("/api/crm/{ACME}/conversation"),
        &says("halo").to_string(),
        Some(&session),
        false,
    )
    .await;
    assert_eq!(csrf_status, StatusCode::FORBIDDEN);
    assert_eq!(csrf_refusal["reason"], "csrf_required");
    assert_eq!(
        csrf_refusal["message"],
        "The CSRF header must match this session's token."
    );
}

/// **Witness: an inactive membership reads as refused; a member of another
/// workspace reads as refused.**
///
/// `revoked` authenticates (the identity user is fine) but the CRM
/// membership is inactive, so every turn is a 403 — the same fact as a
/// missing membership, and deliberately the SAME masked answer. `other` is a
/// reader in the second workspace: asking the first workspace's conversation
/// is the same refusal.
#[tokio::test]
async fn an_inactive_or_foreign_membership_is_refused_like_a_missing_one() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let revoked = login(&app, "revoked").await;
    let (revoked_status, revoked_refusal) = ask(&app, &revoked, &says("tampilkan kontak")).await;
    assert_eq!(revoked_status, StatusCode::FORBIDDEN);
    assert_eq!(revoked_refusal["error"]["message"], "CRM request refused.");

    let other = login(&app, "other").await;
    let (foreign_status, foreign_refusal) = ask(&app, &other, &says("tampilkan kontak")).await;
    assert_eq!(foreign_status, StatusCode::FORBIDDEN);
    assert_eq!(foreign_refusal["error"]["message"], "CRM request refused.");
}

#[tokio::test]
async fn disabling_the_pipeline_module_removes_the_tool_and_zeroes_the_counts() {
    let fixture = provision().await;
    seed(&fixture).await;
    sqlx::query(
        "INSERT INTO archava_owner.owner_grants \
         (tenant_id, principal_id, permissions, active) \
         VALUES ($1, 'alice', ARRAY['config.read','config.apply'], true)",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the owner grant");
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (status, _) = ask(&app, &alice, &says("tampilkan peluang")).await;
    assert_eq!(status, StatusCode::OK, "the enabled module serves the tool");

    let disable =
        serde_json::json!({ "baseRevision": 0, "enable": [], "disable": ["crm-pipeline"] });
    let (status, planned) = post(
        &app,
        "/own/plan",
        &serde_json::json!({ "tenant": ACME, "request": disable.clone() }).to_string(),
        Some(&alice),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let hash = planned["plan"]["planHash"]
        .as_str()
        .expect("a plan hash")
        .to_string();
    let (status, applied) = post(
        &app,
        "/own/apply",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": "00000000-0000-4000-8000-00000000d001",
            "request": disable,
            "confirmation": { "planHash": hash, "baseRevision": 0 },
        })
        .to_string(),
        Some(&alice),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);

    let (status, _) = ask(&app, &alice, &says("tampilkan peluang")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the disabled module removes the pipeline tool"
    );
    let (status, reply) = ask(&app, &alice, &says("ringkas kondisi CRM")).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert!(
        reply["data"]["snapshot"]["pipeline"].is_null(),
        "the disabled module removes the overview's pipeline section, not fakes zeros"
    );

    let enable =
        serde_json::json!({ "baseRevision": 1, "enable": ["crm-pipeline"], "disable": [] });
    let (status, planned) = post(
        &app,
        "/own/plan",
        &serde_json::json!({ "tenant": ACME, "request": enable.clone() }).to_string(),
        Some(&alice),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let hash = planned["plan"]["planHash"]
        .as_str()
        .expect("a plan hash")
        .to_string();
    let (status, applied) = post(
        &app,
        "/own/apply",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": "00000000-0000-4000-8000-00000000d002",
            "request": enable,
            "confirmation": { "planHash": hash, "baseRevision": 1 },
        })
        .to_string(),
        Some(&alice),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 2);

    let (status, reply) = ask(&app, &alice, &says("tampilkan peluang")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the re-enabled module serves again: {reply}"
    );
    let (status, overview) = ask(&app, &alice, &says("ringkas kondisi CRM")).await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["data"]["snapshot"]["pipeline"]["total"], 3);
}
