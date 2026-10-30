//! Unit tests for the ERP domain contracts (no database needed on purpose:
//! these rules are portable invariants).

use serde_json::json;

use super::purchasing::{
    authorize_operation, parse_command, replayed_receipt, CommerceAccess, PurchaseAccess,
    PurchaseCommand, PurchaseContext, PurchaseMutation, ValidationError,
};
use super::{canonical_json, valid_code, valid_principal_id, valid_uuid, ErpError, ErpScope};

fn access(permissions: &[&str], owner: &str, scope: ErpScope) -> PurchaseAccess {
    PurchaseAccess {
        permissions: permissions.iter().map(|p| p.to_string()).collect(),
        owner_id: owner.to_string(),
        scope,
    }
}

fn commerce(permissions: &[&str], scope: ErpScope) -> CommerceAccess {
    CommerceAccess {
        permissions: permissions.iter().map(|p| p.to_string()).collect(),
        scope,
    }
}

fn context(purchasing: PurchaseAccess, commerce: CommerceAccess) -> PurchaseContext {
    PurchaseContext {
        purchasing,
        commerce,
    }
}

// --- Input validators ------------------------------------------------------

#[test]
fn principal_ids_refuse_everything_the_slug_schema_refuses() {
    assert!(valid_principal_id("acme-retail"));
    assert!(valid_principal_id("a-1-b"));
    assert!(!valid_principal_id("Acme"));
    assert!(!valid_principal_id(""));
    assert!(!valid_principal_id("leading-"));
    assert!(!valid_principal_id("-trailing-"));
    assert!(!valid_principal_id("has--double-dash"));
    assert!(!valid_principal_id("has space"));
    assert!(!valid_principal_id(&"a".repeat(101)));
}

#[test]
fn uuid_validation_is_the_8_4_4_4_12_hex_shape() {
    assert!(valid_uuid("01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d"));
    assert!(valid_uuid("01933DF1-8D5E-783F-9B1E-1D3F9A2B3C4D"));
    assert!(!valid_uuid(""));
    assert!(!valid_uuid("01933df1-8d5e-783f-9b1e-1d3f9a2b3c4"));
    assert!(!valid_uuid("01933df1_8d5e-783f-9b1e-1d3f9a2b3c4d"));
    assert!(!valid_uuid("zzz33df1-8d5e-783f-9b1e-1d3f9a2b3c4d"));
}

#[test]
fn codes_are_uppercase_with_the_documented_pattern() {
    assert!(valid_code("SUP-1_A"));
    assert!(valid_code("A"));
    assert!(!valid_code(""));
    assert!(!valid_code("a-sup"));
    assert!(!valid_code("-UPPER-LEAD"));
    assert!(!valid_code(&"A".repeat(41)));
}

// --- Canonical JSON --------------------------------------------------------

#[test]
fn canonical_json_is_key_sorted_compact_and_integers_plain() {
    assert_eq!(
        canonical_json(&json!({ "b": 1, "a": [true, null] })),
        r#"{"a":[true,null],"b":1}"#
    );
    // Nested objects sort at every depth, and integer numbers never print a
    // decimal part — the two places a JSON.stringify port can silently differ.
    assert_eq!(
        canonical_json(&json!({ "cmd": { "expectedRevision": 5, "id": "x" }, "z": 2.5 })),
        r#"{"cmd":{"expectedRevision":5,"id":"x"},"z":2.5}"#
    );
    assert_eq!(canonical_json(&json!({ "i": 7 })), r#"{"i":7}"#);
    assert_eq!(canonical_json(&json!(7)), r#"7"#);
}

// --- Command parsing -------------------------------------------------------

#[test]
fn a_fully_shaped_supplier_create_parses() {
    let raw =
        json!({ "operation": "supplier.create", "code": "SUP-1", "name": "  Fresh Produce  " });
    let command = parse_command(&raw).expect("valid supplier command");
    match command {
        PurchaseCommand::SupplierCreate { ref code, ref name } => {
            assert_eq!(code, "SUP-1");
            assert_eq!(name, "Fresh Produce");
        }
        other => panic!("wrong variant: {other:?}"),
    }
    assert_eq!(command.operation(), "supplier.create");
}

#[test]
fn supplier_create_is_strict_about_its_member_shape() {
    for bad in [
        json!({ "operation": "supplier.create", "code": "SUP-1" }),
        json!({ "operation": "supplier.create", "code": "", "name": "x" }),
        json!({ "operation": "supplier.create", "code": "ok", "name": "" }),
        json!({ "operation": "supplier.create", "code": "ok", "name": "x", "extra": 1 }),
        json!({ "operation": "supplier.create", "code": "bad!", "name": "x" }),
        json!([]),
        json!({}),
    ] {
        match parse_command(&bad) {
            Err(ValidationError(_)) => {}
            Ok(other) => panic!("a malformed creation must refuse, got {other:?}"),
        }
    }
}

#[test]
fn order_receive_requires_a_nonempty_finding_within_the_line_bound() {
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";

    let ok = json!({
        "operation": "order.receive",
        "id": uuid,
        "expectedRevision": 2,
        "reason": "partial delivery accepted",
        "lines": [ { "lineId": uuid, "accepted": 3, "rejected": 1 } ],
    });
    match parse_command(&ok) {
        Ok(PurchaseCommand::OrderReceive { lines, .. }) => assert_eq!(lines[0].accepted, 3),
        _ => panic!("a well-formed receipt finding must parse"),
    }

    // Zero accepted AND rejected: the schema's refine refuses — a finding
    // must count something.
    let zero = json!({
        "operation": "order.receive", "id": uuid, "expectedRevision": 2,
        "reason": "nothing", "lines": [ { "lineId": uuid, "accepted": 0, "rejected": 0 } ],
    });
    assert!(parse_command(&zero).is_err());

    // Duplicate line ids refuse (superRefine).
    let duplicate = json!({
        "operation": "order.receive", "id": uuid, "expectedRevision": 2,
        "reason": "dup", "lines": [
            { "lineId": uuid, "accepted": 1, "rejected": 0 },
            { "lineId": uuid, "accepted": 0, "rejected": 1 },
        ],
    });
    assert!(parse_command(&duplicate).is_err());
}

#[test]
fn a_mutation_envelope_parses_item_for_item() {
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
    let raw = json!({
        "requestId": uuid,
        "command": { "operation": "supplier.create", "code": "SUP-1", "name": "Fresh" },
    });
    let mutation = PurchaseMutation::parse(&raw).expect("valid mutation");
    assert_eq!(mutation.request_id, uuid);
    assert_eq!(mutation.command.operation(), "supplier.create");

    assert!(PurchaseMutation::parse(&json!({
        "requestId": "not-a-uuid",
        "command": { "operation": "order.cancel", "id": uuid, "expectedRevision": 1, "reason": "x" },
    })).is_err());
    assert!(PurchaseMutation::parse(&json!({
        "requestId": uuid,
        // Patching extra fields onto the envelope must refuse (strict).
        "command": { "operation": "order.cancel", "id": uuid, "expectedRevision": 1, "reason": "x" },
        "extra": true,
    })).is_err());
}

#[test]
fn the_typed_command_serializes_to_canonicalised_fingerprint_form() {
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
    let command = PurchaseCommand::SupplierSetActive {
        id: uuid.to_string(),
        expected_revision: 3,
        reason: "supplier gone dark".to_string(),
        active: false,
    };
    assert_eq!(
        command.canonical_form(),
        format!("{{\"active\":false,\"expectedRevision\":3,\"id\":\"{uuid}\",\"operation\":\"supplier.set-active\",\"reason\":\"supplier gone dark\"}}")
    );
}

#[test]
fn a_replayed_receipt_flips_only_the_replayed_flag() {
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
    let base = replayed_receipt(&super::purchasing::PurchaseReceipt {
        request_id: uuid.to_string(),
        operation: "supplier.create".to_string(),
        id: uuid.to_string(),
        revision: 1,
        replayed: false,
    });
    assert!(base.replayed);
    // Again on a replayed one: stable, never vacillating.
    let again = replayed_receipt(&base);
    assert!(again.replayed);
    assert_eq!(base.request_id, again.request_id);
    assert_eq!(base.revision, again.revision);
}

// --- Authorization ---------------------------------------------------------

#[test]
fn supplier_operations_need_their_grants_and_global_scope() {
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
    let ctx = context(
        access(&["suppliers.read", "suppliers.write"], uuid, ErpScope::All),
        commerce(&["catalog.read"], ErpScope::All),
    );
    assert!(authorize_operation(&ctx, "supplier.create").is_ok());

    let assigned = context(
        access(
            &["suppliers.read", "suppliers.write"],
            uuid,
            ErpScope::Assigned,
        ),
        commerce(&["catalog.read"], ErpScope::All),
    );
    assert_eq!(
        authorize_operation(&assigned, "supplier.create"),
        Err(ErpError::Forbidden)
    );
}

#[test]
fn order_create_adds_supplier_and_offer_reads() {
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
    let without = context(
        access(&["orders.read", "orders.write"], uuid, ErpScope::All),
        commerce(&["catalog.read"], ErpScope::All),
    );
    assert_eq!(
        authorize_operation(&without, "order.create"),
        Err(ErpError::Forbidden)
    );

    let with = context(
        access(
            &[
                "orders.read",
                "orders.write",
                "suppliers.read",
                "offers.read",
            ],
            uuid,
            ErpScope::Assigned,
        ),
        commerce(&["catalog.read"], ErpScope::Assigned),
    );
    assert!(
        authorize_operation(&with, "order.create").is_ok(),
        "order drafting needs no global scope (only supplier/offer mutation does)"
    );
}

#[test]
fn receive_is_the_cross_ledger_gate() {
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";

    // Missing purchasing receipts grants refuses even with everything else.
    let no_receipts = context(
        access(&["orders.read", "orders.write"], uuid, ErpScope::All),
        commerce(&["stock.read", "stock.write"], ErpScope::All),
    );
    assert_eq!(
        authorize_operation(&no_receipts, "order.receive"),
        Err(ErpError::Forbidden)
    );

    // The purchasing side carries no scope rule for receive (only the
    // supplier/offer mutations do), but the else-branch precondition still
    // demands orders.read AND orders.write — receive is an order operation,
    // so those grants ride along. Assignments on the purchasing ledger alone
    // do not narrow it away.
    let receipts_granted = |purchasing_scope: ErpScope| {
        context(
            access(
                &[
                    "orders.read",
                    "orders.write",
                    "receipts.read",
                    "receipts.write",
                ],
                uuid,
                purchasing_scope,
            ),
            commerce(&["stock.read", "stock.write"], ErpScope::All),
        )
    };
    assert!(authorize_operation(&receipts_granted(ErpScope::Assigned), "order.receive").is_ok());

    // The commerce scope MUST be all — an assigned commerce grant refuses the
    // physical stock write outright.
    let narrowed_commerce = context(
        access(
            &[
                "orders.read",
                "orders.write",
                "receipts.read",
                "receipts.write",
            ],
            uuid,
            ErpScope::All,
        ),
        commerce(&["stock.read", "stock.write"], ErpScope::Assigned),
    );
    assert_eq!(
        authorize_operation(&narrowed_commerce, "order.receive"),
        Err(ErpError::Forbidden)
    );

    // And missing stock grants refuse even when everything purchasing-side is
    // global.
    let no_stock = context(
        access(
            &[
                "orders.read",
                "orders.write",
                "receipts.read",
                "receipts.write",
            ],
            uuid,
            ErpScope::All,
        ),
        commerce(&["catalog.read"], ErpScope::All),
    );
    assert_eq!(
        authorize_operation(&no_stock, "order.receive"),
        Err(ErpError::Forbidden)
    );
}

#[test]
fn an_operation_runs_only_with_every_grant_its_branch_names() {
    // The union's discriminator decides the branch; the typed parse is what
    // stops unknown operations from existing at all (`z.discriminatedUnion`).
    // Here the property is: a context missing ANY branch-required permission
    // is refused for that branch, and one missing orders grants is refused
    // even for receive.
    let uuid = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
    let blind = context(
        access(&[], uuid, ErpScope::All),
        commerce(&["stock.read", "stock.write"], ErpScope::All),
    );
    for operation in [
        "supplier.create",
        "offer.set-cost",
        "order.confirm",
        "order.receive",
    ] {
        assert_eq!(
            authorize_operation(&blind, operation),
            Err(ErpError::Forbidden),
            "{operation} without grants is always forbidden"
        );
    }
    // One half of a required pair is still refused — the grant rule demands
    // ALL listed permissions, never any-of.
    let half = context(
        access(&["orders.write"], uuid, ErpScope::All),
        commerce(&["catalog.read"], ErpScope::All),
    );
    assert_eq!(
        authorize_operation(&half, "order.cancel"),
        Err(ErpError::Forbidden)
    );
}
