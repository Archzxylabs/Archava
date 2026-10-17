//! The single physical stock vocabulary shared by commerce, purchasing and
//! foodservice (R06's *one stock pool*).
//!
//! The pool itself lives in `archava_commerce.stock_balances` and derives
//! from `archava_commerce.movements` — a rule the database triggers enforce
//! (`Stock must derive from immutable movements`). What the cohorts share is
//! the vocabulary below, and the shape of the source-bound receipt the
//! inbound cross-ledger write records:
//!
//! ```json
//! {"domain":"purchasing","id":"<order-uuid>","revision":<n>,
//!  "lines":[{"productId":"<product-uuid>","quantity":<n>}]}
//! ```
//!
//! The commerce movement guard refuses any `purchase` movement that is not
//! exactly backed by such a receipt (`Inbound quantity has no source-bound
//! receipt`), and the purchasing receipt trigger refuses the converse — which
//! is what makes "stock changed exactly once" a database property, not an
//! application habit.

use serde::{Deserialize, Serialize};

use super::{purchasing, valid_uuid, ErpError};

/// Every movement kind the three cohorts can write. Migrations 001+002
/// freeze the set; the trigger refuses anything out of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovementKind {
    Adjust,
    Consume,
    Return,
    Purchase,
}

impl MovementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Adjust => "adjust",
            Self::Consume => "consume",
            Self::Return => "return",
            Self::Purchase => "purchase",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "adjust" => Some(Self::Adjust),
            "consume" => Some(Self::Consume),
            "return" => Some(Self::Return),
            "purchase" => Some(Self::Purchase),
            _ => None,
        }
    }
}

/// Allocation state — the reservation lifecycle the `allocations` table
/// carries. `held` reserves, `consumed` fulfills, `released` refunds; no
/// forward transition exists, so a fulfilled order did not "hold" its stock
/// twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AllocationState {
    Held,
    Consumed,
    Released,
}

impl AllocationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Consumed => "consumed",
            Self::Released => "released",
        }
    }
}

/// The hard cell the stock quantity column enforces
/// (`quantity BETWEEN 0 AND 1000000000` on `stock_balances`).
pub const STOCK_QTY_MAX: i64 = 1_000_000_000;

/// The hold-window bounds `reserveCommerceOrder` parses (`holdSeconds`
/// 1..=1800): reservation locks expire fast on purpose.
pub const HOLD_SECONDS_MIN: i64 = 1;
pub const HOLD_SECONDS_MAX: i64 = 1800;

/// One inbound stock line — the exact event the purchasing receipt claims
/// against commerce stock (`{productId, quantity}`, strict).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundLine {
    #[serde(rename = "productId")]
    pub product_id: String,
    #[serde(rename = "quantity")]
    pub quantity: i64,
}

/// The source-bound receipt written to `archava_commerce.commands.receipt`
/// for a purchasing inbound, and the exact document
/// `complete_purchase_stock` reconciles against the movements written in
/// the same transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundReceipt {
    #[serde(rename = "domain")]
    pub domain: &'static str,
    #[serde(rename = "id")]
    pub purchase_id: String,
    #[serde(rename = "revision")]
    pub revision: i64,
    #[serde(rename = "lines")]
    pub lines: Vec<InboundLine>,
}

impl InboundReceipt {
    /// The canonical receipt for a purchasing receive; the domain label is
    /// always `purchasing` — this port has one inbound source.
    pub fn purchasing(purchase_id: String, revision: i64, lines: Vec<InboundLine>) -> Self {
        Self {
            domain: "purchasing",
            purchase_id,
            revision,
            lines,
        }
    }
}

/// The receipt lines a foodservice ticket consumes per its recipe — the
/// allocation payload the `ticket_complete` constraint trigger reconciles
/// (`expected` vs `actual` FULL OUTER JOIN). Kept here so the foodservice
/// cohort speaks the same vocabulary as its commerce host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecipeConsumption {
    pub product_id: String,
    pub quantity: i64,
}

// --- The commerce command receipt (surfaces products, stock and orders) ----

/// `commerceReceiptSchema` — the receipt a commerce command stores. `kind`
/// discriminates the three surfaces; `id` is the record's uuid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommerceCommandReceipt {
    #[serde(rename = "kind")]
    pub kind: String,
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "revision")]
    pub revision: i64,
}

/// `commerceCommandStatusSchema` — what a status read answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum CommerceCommandStatus {
    #[serde(rename = "not_committed")]
    NotCommitted {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "committed")]
    Committed {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "receipt")]
        receipt: CommerceCommandReceipt,
    },
}

impl CommerceCommandReceipt {
    /// The strict parse the TS side gives a stored receipt: the kind union,
    /// a uuid id, a bounded revision. A stored receipt that fails this shape
    /// is corrupt state, reported as a conflict rather than served.
    pub fn parse(raw: &serde_json::Value) -> Result<Self, ErpError> {
        let kind = raw.get("kind").and_then(serde_json::Value::as_str);
        if !matches!(kind, Some("product") | Some("stock") | Some("order")) {
            return Err(ErpError::Conflict);
        }
        let id = raw
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| valid_uuid(id))
            .ok_or(ErpError::Conflict)?;
        let revision = raw
            .get("revision")
            .and_then(serde_json::Value::as_i64)
            .filter(|rev| *rev >= 1 && *rev <= purchasing::REVISION_MAX)
            .ok_or(ErpError::Conflict)?;
        Ok(Self {
            kind: kind.expect("matched above").to_string(),
            id: id.to_string(),
            revision,
        })
    }
}
