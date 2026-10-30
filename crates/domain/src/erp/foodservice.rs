//! The foodservice domain contracts — the Archxy 2 port of
//! `packages/foodservice/src/contracts.ts` (the frozen command grammar,
//! receipts, memberships and the pure grant ladder).
//!
//! ## Why foodservice lives beside purchasing in the SAME stock cohort
//!
//! A foodservice menu owns a `managed_by='foodservice'` commerce portion
//! product; a recipe's components are tracked commerce products; a ticket
//! queue/prepare drives commerce order reservations and consumption. One
//! physical stock pool, two packages, one writer pattern — which is what
//! makes this module part of R06 rather than its own cohort.
//!
//! ## The wire grammar
//!
//! `foodCommandSchema` is a 13-arm discriminated union whose members use
//! `.strict()` (exactly the declared fields) with `.default()` optional
//! members (`station` = kitchen, `guests` = 1, `holdSeconds` = 900). The
//! parse below reproduces both halves: field-count checks per arm, optional
//! members with their defaults, and the one cross-field rule
//! (`(mode === 'dine_in') !== (tableId !== undefined)` is malformed).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::purchasing::ValidationError;
use super::purchasing::{COST_MAX, REVISION_MAX};
use super::{bounded_title, valid_code, valid_uuid, ErpError, ErpScope};

/// The command tick the receipt and audit rows record
/// (`z.string().min(1).max(100)` — so a single `validate_reason`-traversed
/// string, not an enum, to keep stored receipts byte-stable).
pub const OPERATION_MAX: usize = 100;

/// Every table authority string row's size ceiling (`max(19)` after the
/// enforced first character).
pub const TABLE_CODE_MAX: usize = 20;

/// One typed foodservice command — `foodCommandSchema`, discriminated by
/// `operation`, serialized with the exact wire members (camelCase) for the
/// fingerprint canonicalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "operation")]
pub enum FoodCommand {
    #[serde(rename = "table.create")]
    TableCreate {
        code: String,
        title: String,
        seats: i64,
    },
    #[serde(rename = "table.set-active")]
    TableSetActive {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        active: bool,
        reason: String,
    },
    #[serde(rename = "menu.create")]
    MenuCreate {
        sku: String,
        title: String,
        price: i64,
        station: String,
        recipe: Vec<RecipeComponent>,
    },
    #[serde(rename = "menu.set-price")]
    MenuSetPrice {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        price: i64,
        reason: String,
    },
    #[serde(rename = "menu.set-active")]
    MenuSetActive {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        active: bool,
        reason: String,
    },
    #[serde(rename = "menu.replace-recipe")]
    MenuReplaceRecipe {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        recipe: Vec<RecipeComponent>,
        reason: String,
    },
    #[serde(rename = "ticket.create")]
    TicketCreate {
        #[serde(rename = "customerName")]
        customer_name: String,
        mode: TicketMode,
        #[serde(rename = "tableId", skip_serializing_if = "Option::is_none")]
        table_id: Option<String>,
        guests: i64,
        lines: Vec<TicketLineInput>,
    },
    #[serde(rename = "ticket.queue")]
    TicketQueue {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "holdSeconds")]
        hold_seconds: i64,
    },
    #[serde(rename = "ticket.prepare")]
    TicketPrepare {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "ticket.ready")]
    TicketReady {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "ticket.serve")]
    TicketServe {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "ticket.close")]
    TicketClose {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "ticket.cancel")]
    TicketCancel {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: String,
    },
}

/// One sealed-recipe component — `recipeComponentsSchema`'s members. Nested
/// in `menu.create` and `menu.replace-recipe`, serialized through the same
/// wire keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeComponent {
    #[serde(rename = "productId")]
    pub product_id: String,
    pub quantity: i64,
}

/// One menu binding — `{menuId,quantity}` on a draft. Serialized through the
/// command shape (`menuId`), so the fingerprint canonicalization sees the
/// exact submitted document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketLineInput {
    #[serde(rename = "menuId")]
    pub menu_id: String,
    pub quantity: i64,
}

/// The ticket `mode` — table binding rides separately in the command body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketMode {
    #[serde(rename = "dine_in")]
    DineIn,
    #[serde(rename = "takeaway")]
    Takeaway,
}

impl TicketMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dine_in" => Some(Self::DineIn),
            "takeaway" => Some(Self::Takeaway),
            _ => None,
        }
    }
}

impl fmt::Display for TicketMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TicketMode {
    /// The column value (`dine_in`/`takeaway`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DineIn => "dine_in",
            Self::Takeaway => "takeaway",
        }
    }
}

// --- Memberships ------------------------------------------------------------

/// The foodservice ledger's permission vocabulary — the schema CHECK's
/// 8-word array (`permissions <@ ARRAY[...]`) and the TS `FOOD_PERMISSIONS`.
pub const FOOD_PERMISSIONS: &[&str] = &[
    "tables.read",
    "tables.write",
    "menu.read",
    "menu.write",
    "tickets.read",
    "tickets.write",
    "kitchen.write",
    "audit.read",
];

/// The principal envelope is shape-identical to the purchasing package's
/// (`foodPrincipalSchema` = the same human/service union over the same
/// kebab-case slug grammar), so the parse is reused, not duplicated.
pub use super::purchasing::PurchasePrincipal as FoodPrincipal;

/// A resolved foodservice-side access — the parsed membership grant plus the
/// owner id the scope narrows reads and writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl FoodAccess {
    /// Whether the access holds EVERY named permission — the `grant()` rule.
    fn grants(&self, wanted: &[&str]) -> bool {
        wanted
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// Public form of [`Self::grants`] for adapters that must reproduce the
    /// repository's `grant()`/`ownedTicket` checks.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        self.grants(wanted)
    }

    /// The repository's `global(a)` (both ledgers must be unassigned).
    pub fn is_global(&self) -> bool {
        self.scope.covers_all()
    }
}

/// One context: both ledger grants resolved in ONE transaction, after the
/// service-on-behalf intersection has been applied.
#[derive(Debug, Clone)]
pub struct FoodContext {
    pub food: FoodAccess,
    pub commerce: super::purchasing::CommerceAccess,
}

impl FoodContext {
    /// Applies the commerce scope rule the baseline closes the transaction
    /// with: an `assigned` commerce grant narrows the food-service side too.
    pub fn narrowed(self) -> Self {
        let narrowed = match self.commerce.scope {
            ErpScope::Assigned => ErpScope::Assigned,
            ErpScope::All => self.food.scope,
        };
        Self {
            food: FoodAccess {
                permissions: self.food.permissions,
                owner_id: self.food.owner_id,
                scope: narrowed,
            },
            commerce: self.commerce,
        }
    }
}

// --- Receipts and status ----------------------------------------------------

/// The receipt the mutation records and recovery replays —
/// `foodReceiptSchema`. Serialized with the wire key order kept stable
/// because a stored receipt must round-trip exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoodReceipt {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "operation")]
    pub operation: String,
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "revision")]
    pub revision: i64,
    #[serde(rename = "replayed")]
    pub replayed: bool,
}

/// The command-status union — the same two-state shape every ledger has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state")]
pub enum FoodCommandStatus {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "committed")]
    Committed { receipt: FoodReceipt },
}

/// Builds the replayed receipt: the stored one with `replayed` flipped.
pub fn replayed_receipt(receipt: &FoodReceipt) -> FoodReceipt {
    let mut replayed = receipt.clone();
    replayed.replayed = true;
    replayed
}

/// The ticket-state vocabulary — the column CHECK and the TS const list.
pub const FOOD_TICKET_STATES: &[&str] = &[
    "draft",
    "queued",
    "preparing",
    "ready",
    "served",
    "closed",
    "cancelled",
];

/// The `reason` cell one command carries — exactly the arms whose schema
/// members declare it, in the order the audit row's single nullable column
/// stores it.
pub fn command_reason(command: &FoodCommand) -> Option<&str> {
    match command {
        FoodCommand::TableSetActive { reason, .. }
        | FoodCommand::MenuSetPrice { reason, .. }
        | FoodCommand::MenuSetActive { reason, .. }
        | FoodCommand::MenuReplaceRecipe { reason, .. }
        | FoodCommand::TicketCancel { reason, .. } => Some(reason),
        _ => None,
    }
}

/// The `operation` wire string of one command — the same string the receipt
/// and the audit rows record, so the stored receipt can be re-authorized
/// from the cell alone.
pub fn command_operation(command: &FoodCommand) -> &'static str {
    match command {
        FoodCommand::TableCreate { .. } => "table.create",
        FoodCommand::TableSetActive { .. } => "table.set-active",
        FoodCommand::MenuCreate { .. } => "menu.create",
        FoodCommand::MenuSetPrice { .. } => "menu.set-price",
        FoodCommand::MenuSetActive { .. } => "menu.set-active",
        FoodCommand::MenuReplaceRecipe { .. } => "menu.replace-recipe",
        FoodCommand::TicketCreate { .. } => "ticket.create",
        FoodCommand::TicketQueue { .. } => "ticket.queue",
        FoodCommand::TicketPrepare { .. } => "ticket.prepare",
        FoodCommand::TicketReady { .. } => "ticket.ready",
        FoodCommand::TicketServe { .. } => "ticket.serve",
        FoodCommand::TicketClose { .. } => "ticket.close",
        FoodCommand::TicketCancel { .. } => "ticket.cancel",
    }
}

// --- Field parsers ----------------------------------------------------------

fn as_bool(value: &Value, field: &'static str) -> Result<bool, ValidationError> {
    value.as_bool().ok_or(ValidationError(field))
}

fn as_uuid(value: &Value, field: &'static str) -> Result<String, ValidationError> {
    let raw = value.as_str().ok_or(ValidationError(field))?;
    if !valid_uuid(raw) {
        return Err(ValidationError(field));
    }
    Ok(raw.to_string())
}

fn as_i64(value: &Value, field: &'static str, min: i64, max: i64) -> Result<i64, ValidationError> {
    value
        .as_i64()
        .filter(|number| *number >= min && *number <= max)
        .ok_or(ValidationError(field))
}

/// The configi title/reason cell: trim, then `1..=max` characters.
fn as_title(value: &Value, field: &'static str, max: usize) -> Result<String, ValidationError> {
    let raw = value.as_str().ok_or(ValidationError(field))?;
    bounded_title(raw, max).ok_or(ValidationError(field))
}

fn as_reason(value: &Value) -> Result<String, ValidationError> {
    // The schema's `.trim().min(1).max(2000)`.
    let raw = value.as_str().ok_or(ValidationError("reason"))?;
    bounded_title(raw, 2000).ok_or(ValidationError("reason"))
}

/// `{id,expectedRevision,reason}` for an existing-record command member.
fn parse_target(
    object: &serde_json::Map<String, Value>,
) -> Result<(String, i64, String), ValidationError> {
    let id = as_uuid(object.get("id").ok_or(ValidationError("id"))?, "id")?;
    let expected_revision = as_i64(
        object
            .get("expectedRevision")
            .ok_or(ValidationError("expectedRevision"))?,
        "expectedRevision",
        1,
        REVISION_MAX,
    )?;
    let reason_field = object.get("reason").ok_or(ValidationError("reason"))?;
    let reason = as_reason(reason_field)?;
    Ok((id, expected_revision, reason))
}

/// The table code cell: the shared code characters with a 20-character
/// ceiling (`^[A-Z0-9][A-Z0-9_-]{0,19}$`).
fn valid_table_code(value: &str) -> bool {
    if !valid_code(value) {
        return false;
    }
    value.len() <= TABLE_CODE_MAX
}

/// The recipe component array — strict members, 1..=20 entries, each quantity
/// `1..=1_000_000`, product ids unique.
fn parse_recipe(value: &Value) -> Result<Vec<RecipeComponent>, ValidationError> {
    let array = value.as_array().ok_or(ValidationError("recipe"))?;
    if array.is_empty() || array.len() > 20 {
        return Err(ValidationError("recipe"));
    }
    let mut parsed = Vec::with_capacity(array.len());
    for item in array {
        let object = item.as_object().ok_or(ValidationError("recipe"))?;
        if object.len() != 2 {
            return Err(ValidationError("recipe"));
        }
        let product_id = as_uuid(
            object.get("productId").ok_or(ValidationError("recipe"))?,
            "recipe",
        )?;
        let quantity = as_i64(
            object.get("quantity").ok_or(ValidationError("recipe"))?,
            "recipe",
            1,
            1_000_000,
        )?;
        parsed.push(RecipeComponent {
            product_id,
            quantity,
        });
    }
    for index in 0..parsed.len() {
        if parsed[index + 1..]
            .iter()
            .any(|other| other.product_id == parsed[index].product_id)
        {
            return Err(ValidationError("recipe"));
        }
    }
    Ok(parsed)
}

/// The ticket line array — strict `{menuId,quantity}` members, 1..=20 lines,
/// each quantity `1..=100`, menu ids unique.
fn parse_ticket_lines(value: &Value) -> Result<Vec<TicketLineInput>, ValidationError> {
    let array = value.as_array().ok_or(ValidationError("lines"))?;
    if array.is_empty() || array.len() > 20 {
        return Err(ValidationError("lines"));
    }
    let mut parsed = Vec::with_capacity(array.len());
    for item in array {
        let object = item.as_object().ok_or(ValidationError("lines"))?;
        if object.len() != 2 {
            return Err(ValidationError("lines"));
        }
        let menu_id = as_uuid(
            object.get("menuId").ok_or(ValidationError("lines"))?,
            "lines",
        )?;
        let quantity = as_i64(
            object.get("quantity").ok_or(ValidationError("lines"))?,
            "lines",
            1,
            100,
        )?;
        parsed.push(TicketLineInput { menu_id, quantity });
    }
    if parsed.iter().enumerate().any(|(index, line)| {
        parsed[index + 1..]
            .iter()
            .any(|other| other.menu_id == line.menu_id)
    }) {
        return Err(ValidationError("lines"));
    }
    Ok(parsed)
}

/// The strict parse of the command union — `foodCommandSchema` member for
/// member, including the `.default()` members the schema fills in on the
/// wire (`station`, `guests`, `holdSeconds`).
pub fn parse_command(raw: &Value) -> Result<FoodCommand, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("operation"))?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ValidationError("operation"))?;
    let rest = object
        .iter()
        .filter(|(key, _)| key.as_str() != "operation")
        .count();
    match operation {
        "table.create" => {
            if rest != 3 {
                return Err(ValidationError("operation"));
            }
            let code = object
                .get("code")
                .and_then(Value::as_str)
                .filter(|code| valid_table_code(code))
                .ok_or(ValidationError("code"))?;
            let title = as_title(
                object.get("title").ok_or(ValidationError("title"))?,
                "title",
                160,
            )?;
            let seats = as_i64(
                object.get("seats").ok_or(ValidationError("seats"))?,
                "seats",
                1,
                40,
            )?;
            Ok(FoodCommand::TableCreate {
                code: code.to_string(),
                title,
                seats,
            })
        }
        "table.set-active" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_target(object)?;
            let active = as_bool(
                object.get("active").ok_or(ValidationError("active"))?,
                "active",
            )?;
            Ok(FoodCommand::TableSetActive {
                id,
                expected_revision,
                reason,
                active,
            })
        }
        "menu.create" => {
            if rest != 4 && rest != 5 {
                return Err(ValidationError("operation"));
            }
            let sku = object
                .get("sku")
                .and_then(Value::as_str)
                .filter(|sku| valid_code(sku))
                .ok_or(ValidationError("sku"))?;
            let title = as_title(
                object.get("title").ok_or(ValidationError("title"))?,
                "title",
                160,
            )?;
            let price = as_i64(
                object.get("price").ok_or(ValidationError("price"))?,
                "price",
                0,
                COST_MAX,
            )?;
            // `.default('kitchen')`: absent → kitchen; present must be in the pair.
            let station = match object.get("station") {
                None => "kitchen".to_string(),
                Some(value) => match value.as_str() {
                    Some("kitchen") => "kitchen".to_string(),
                    Some("bar") => "bar".to_string(),
                    _ => return Err(ValidationError("station")),
                },
            };
            let recipe = parse_recipe(object.get("recipe").ok_or(ValidationError("recipe"))?)?;
            Ok(FoodCommand::MenuCreate {
                sku: sku.to_string(),
                title,
                price,
                station,
                recipe,
            })
        }
        "menu.set-price" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_target(object)?;
            let price = as_i64(
                object.get("price").ok_or(ValidationError("price"))?,
                "price",
                0,
                COST_MAX,
            )?;
            Ok(FoodCommand::MenuSetPrice {
                id,
                expected_revision,
                price,
                reason,
            })
        }
        "menu.set-active" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_target(object)?;
            let active = as_bool(
                object.get("active").ok_or(ValidationError("active"))?,
                "active",
            )?;
            Ok(FoodCommand::MenuSetActive {
                id,
                expected_revision,
                reason,
                active,
            })
        }
        "menu.replace-recipe" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_target(object)?;
            let recipe = parse_recipe(object.get("recipe").ok_or(ValidationError("recipe"))?)?;
            Ok(FoodCommand::MenuReplaceRecipe {
                id,
                expected_revision,
                recipe,
                reason,
            })
        }
        "ticket.create" => {
            if rest != 3 && rest != 4 && rest != 5 {
                return Err(ValidationError("operation"));
            }
            let customer_name = as_title(
                object
                    .get("customerName")
                    .ok_or(ValidationError("customerName"))?,
                "customerName",
                160,
            )?;
            let mode = TicketMode::parse(
                object
                    .get("mode")
                    .and_then(Value::as_str)
                    .ok_or(ValidationError("mode"))?,
            )
            .ok_or(ValidationError("mode"))?;
            let table_id = match object.get("tableId") {
                None => None,
                Some(value) => Some(as_uuid(value, "tableId")?),
            };
            // `.default(1)`: absent → 1; present must be `1..=40`.
            let guests = match object.get("guests") {
                None => 1,
                Some(value) => as_i64(value, "guests", 1, 40)?,
            };
            let lines = parse_ticket_lines(object.get("lines").ok_or(ValidationError("lines"))?)?;
            // One cross-field rule: dine-in alone carries a table.
            if (mode == TicketMode::DineIn) != table_id.is_some() {
                return Err(ValidationError("ticket.create"));
            }
            Ok(FoodCommand::TicketCreate {
                customer_name,
                mode,
                table_id,
                guests,
                lines,
            })
        }
        "ticket.queue" => {
            if rest != 2 && rest != 3 {
                return Err(ValidationError("operation"));
            }
            let id = as_uuid(object.get("id").ok_or(ValidationError("id"))?, "id")?;
            let expected_revision = as_i64(
                object
                    .get("expectedRevision")
                    .ok_or(ValidationError("expectedRevision"))?,
                "expectedRevision",
                1,
                REVISION_MAX,
            )?;
            // `.default(900)`: absent → 900; present must be `1..=1800`.
            let hold_seconds = match object.get("holdSeconds") {
                None => 900,
                Some(value) => as_i64(value, "holdSeconds", 1, 1800)?,
            };
            Ok(FoodCommand::TicketQueue {
                id,
                expected_revision,
                hold_seconds,
            })
        }
        "ticket.prepare" | "ticket.ready" | "ticket.serve" | "ticket.close" => {
            if rest != 2 {
                return Err(ValidationError("operation"));
            }
            let id = as_uuid(object.get("id").ok_or(ValidationError("id"))?, "id")?;
            let expected_revision = as_i64(
                object
                    .get("expectedRevision")
                    .ok_or(ValidationError("expectedRevision"))?,
                "expectedRevision",
                1,
                REVISION_MAX,
            )?;
            let command = match operation {
                "ticket.prepare" => FoodCommand::TicketPrepare {
                    id,
                    expected_revision,
                },
                "ticket.ready" => FoodCommand::TicketReady {
                    id,
                    expected_revision,
                },
                "ticket.serve" => FoodCommand::TicketServe {
                    id,
                    expected_revision,
                },
                _ => FoodCommand::TicketClose {
                    id,
                    expected_revision,
                },
            };
            Ok(command)
        }
        "ticket.cancel" => {
            if rest != 3 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_target(object)?;
            Ok(FoodCommand::TicketCancel {
                id,
                expected_revision,
                reason,
            })
        }
        _ => Err(ValidationError("operation")),
    }
}

/// One mutation submission — `foodMutationSchema` ({requestId, command}).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FoodMutation {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "command")]
    pub command: FoodCommand,
}

impl FoodMutation {
    /// The strict parse of the mutation envelope.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("requestId"))?;
        if object.len() != 2 {
            return Err(ValidationError("requestId"));
        }
        let request_id = as_uuid(
            object
                .get("requestId")
                .ok_or(ValidationError("requestId"))?,
            "requestId",
        )?;
        let command = parse_command(object.get("command").ok_or(ValidationError("command"))?)?;
        Ok(Self {
            request_id,
            command,
        })
    }
}

// --- Authorization (the pure grant rule) ------------------------------------

/// The permission one operation's write runs under — `permission(c)` +
/// `authorize(a, c)` in `packages/foodservice-pg/src/repository.ts`.
pub fn operation_permission(command: &FoodCommand) -> &'static str {
    match command {
        FoodCommand::TableCreate { .. } | FoodCommand::TableSetActive { .. } => "tables.write",
        FoodCommand::MenuCreate { .. }
        | FoodCommand::MenuSetPrice { .. }
        | FoodCommand::MenuSetActive { .. }
        | FoodCommand::MenuReplaceRecipe { .. } => "menu.write",
        FoodCommand::TicketPrepare { .. } | FoodCommand::TicketReady { .. } => "kitchen.write",
        // (all other beginsWith checks resolved below)
        _ => "tickets.write",
    }
}

/// The pure authorize rule — port of `authorize(a, c)`. Table and menu
/// mutations additionally demand the unassigned scope on BOTH ledgers
/// (`global(a)`); menu mutations add commerce `catalog.write` + `stock.read`;
/// ticket mutations add commerce `orders.write` + `stock.read`, with
/// `ticket.create` overcoming `menu.read` (plus `tables.read` for dine-in).
pub fn authorize_operation(context: &FoodContext, command: &FoodCommand) -> Result<(), ErpError> {
    let food = &context.food;
    let commerce = &context.commerce;
    let permission = operation_permission(command);
    if !food.grants(&[permission]) {
        return Err(ErpError::Forbidden);
    }
    match command {
        FoodCommand::TableCreate { .. } | FoodCommand::TableSetActive { .. } => {
            if !food.grants(&["tables.read"]) {
                return Err(ErpError::Forbidden);
            }
            if !food.is_global() || !commerce.scope.covers_all() {
                return Err(ErpError::Forbidden);
            }
        }
        FoodCommand::MenuCreate { .. }
        | FoodCommand::MenuSetPrice { .. }
        | FoodCommand::MenuSetActive { .. }
        | FoodCommand::MenuReplaceRecipe { .. } => {
            if !food.grants(&["menu.read"]) {
                return Err(ErpError::Forbidden);
            }
            if !food.is_global() || !commerce.scope.covers_all() {
                return Err(ErpError::Forbidden);
            }
            if !commerce.holds(&["catalog.write", "stock.read"]) {
                return Err(ErpError::Forbidden);
            }
        }
        FoodCommand::TicketCreate { mode, .. } => {
            if !food.grants(&["tickets.read"]) {
                return Err(ErpError::Forbidden);
            }
            if !commerce.holds(&["orders.write", "stock.read"]) {
                return Err(ErpError::Forbidden);
            }
            if !food.grants(&["menu.read"]) {
                return Err(ErpError::Forbidden);
            }
            if *mode == TicketMode::DineIn && !food.grants(&["tables.read"]) {
                return Err(ErpError::Forbidden);
            }
        }
        _ => {
            if !food.grants(&["tickets.read"]) {
                return Err(ErpError::Forbidden);
            }
            if !commerce.holds(&["orders.write", "stock.read"]) {
                return Err(ErpError::Forbidden);
            }
        }
    }
    Ok(())
}

/// The recovery authorization — `recovery()` re-runs the CURRENT grant rule
/// over the stored receipt's operation string, with the ticket-state-dependent
/// extras the TS recovery reads. An old receipt is not a permanent grant.
pub fn authorize_recovery(context: &FoodContext, command: &FoodCommand) -> Result<(), ErpError> {
    authorize_operation(context, command)
}

// --- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Strictness decided positively: a well-formed `table.create` parses
    /// with every bound held.
    #[test]
    fn table_create_parses() {
        let command = parse_command(&json!({
            "operation": "table.create",
            "code": "T-01",
            "title": "Window two",
            "seats": 4,
        }))
        .expect("a valid table.create parses");
        assert_eq!(
            command,
            FoodCommand::TableCreate {
                code: "T-01".to_string(),
                title: "Window two".to_string(),
                seats: 4,
            }
        );
    }

    /// Seat and code boundaries refuse.
    #[test]
    fn table_create_bounds() {
        let too_many = parse_command(&json!({
            "operation": "table.create", "code": "T40", "title": "Wide", "seats": 41,
        }));
        assert!(too_many.is_err(), "41 seats exceed the 40-seat cell");
        let bad_code = parse_command(&json!({
            "operation": "table.create", "code": "t01", "title": "Small", "seats": 2,
        }));
        assert!(
            bad_code.is_err(),
            "lowercase is outside the table code rule"
        );
    }

    /// The dine-in cross-field rule: dine-in without a table is malformed,
    /// takeaway WITH a table too.
    #[test]
    fn ticket_create_table_binding() {
        let base = json!({
            "operation": "ticket.create",
            "customerName": "Guest",
            "mode": "dine_in",
            "lines": [{"menuId": uuid(), "quantity": 1}],
        });
        assert!(parse_command(&base).is_err(), "dine-in requires tableId");
        let with_table = json!({
            "operation": "ticket.create",
            "customerName": "Guest",
            "mode": "takeaway",
            "tableId": uuid(),
            "lines": [{"menuId": uuid(), "quantity": 1}],
        });
        assert!(
            parse_command(&with_table).is_err(),
            "takeaway carries no table"
        );
    }

    /// Menu create fills the `station` default and refuses unknown stations.
    #[test]
    fn menu_create_station_default() {
        let command = parse_command(&json!({
            "operation": "menu.create",
            "sku": "MENU-01",
            "title": "Coffee",
            "price": 18000,
            "recipe": [{"productId": uuid(), "quantity": 2}],
        }))
        .expect("menu.create fills station default");
        match &command {
            FoodCommand::MenuCreate { station, .. } => assert_eq!(station, "kitchen"),
            _ => panic!("wrong arm"),
        }
        let bad = parse_command(&json!({
            "operation": "menu.create", "sku": "MENU-01", "title": "Coffee",
            "price": 18000, "station": "grill",
            "recipe": [{"productId": uuid(), "quantity": 2}],
        }));
        assert!(bad.is_err(), "unknown station is a malformed submission");
    }

    /// Unique product/menu ids inside recipe and ticket arrays.
    #[test]
    fn arrays_demand_unique_members() {
        let product = uuid();
        let duplicated = json!({
            "operation": "menu.create", "sku": "MENU-01", "title": "Coffee",
            "price": 18000,
            "recipe": [{"productId": product, "quantity": 1}, {"productId": product, "quantity": 2}],
        });
        assert!(
            parse_command(&duplicated).is_err(),
            "recipe duplicates refuse"
        );
    }

    /// The authorize ladder: a `tickets.write`-only access cannot prepare;
    /// a kitchen grant cannot create tables without the unassigned scope.
    #[test]
    fn authorize_ladder() {
        let access = |permissions: &[&str], scope: ErpScope| FoodAccess {
            permissions: permissions.iter().map(|p| p.to_string()).collect(),
            owner_id: "human-owner".to_string(),
            scope,
        };
        let commerce =
            |permissions: &[&str], scope: ErpScope| super::super::purchasing::CommerceAccess {
                permissions: permissions.iter().map(|p| p.to_string()).collect(),
                scope,
            };
        let tickets_access = FoodContext {
            food: access(
                &["tickets.write", "tickets.read", "menu.read"],
                ErpScope::Assigned,
            ),
            commerce: commerce(&["orders.write", "stock.read"], ErpScope::All),
        };
        let ticket_create = FoodCommand::TicketCreate {
            customer_name: "Guest".to_string(),
            mode: TicketMode::Takeaway,
            table_id: None,
            guests: 1,
            lines: vec![TicketLineInput {
                menu_id: uuid(),
                quantity: 1,
            }],
        };
        assert!(authorize_operation(&tickets_access, &ticket_create).is_ok());
        let kitchen_access = FoodContext {
            food: access(&["kitchen.write", "tickets.read"], ErpScope::All),
            commerce: commerce(&["orders.write", "stock.read"], ErpScope::All),
        };
        let prepare = FoodCommand::TicketPrepare {
            id: uuid(),
            expected_revision: 1,
        };
        assert!(authorize_operation(&kitchen_access, &prepare).is_ok());
        let tables_work = FoodCommand::TableCreate {
            code: "T02".to_string(),
            title: "Terrace".to_string(),
            seats: 2,
        };
        assert!(
            authorize_operation(&kitchen_access, &tables_work).is_err(),
            "no tables.write means refusal"
        );
        let assigned_tables = FoodContext {
            food: access(&["tables.write", "tables.read"], ErpScope::Assigned),
            commerce: commerce(&["catalog.write", "stock.read"], ErpScope::All),
        };
        assert!(
            authorize_operation(&assigned_tables, &tables_work).is_err(),
            "table writes demand the unassigned scope"
        );
    }

    fn uuid() -> String {
        // A well-formed uuid hex for the parse witnesses.
        "7c3ff402-9b57-4a35-b2ee-6dcee95b68a3".to_string()
    }
}
