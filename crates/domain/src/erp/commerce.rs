//! Native commerce commands. HTTP identity, SQL transactions and provider effects are outside this contract.
use super::{valid_code, valid_uuid};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrderLine {
    pub product_id: String,
    pub quantity: i64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum CommerceCommand {
    #[serde(rename = "product.create")]
    ProductCreate {
        sku: String,
        title: String,
        unit: String,
        #[serde(rename = "trackStock")]
        track_stock: bool,
        price: i64,
    },
    #[serde(rename = "product.set")]
    ProductSet {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        price: i64,
        active: bool,
    },
    #[serde(rename = "stock.adjust")]
    StockAdjust {
        #[serde(rename = "productId")]
        product_id: String,
        quantity: i64,
        reason: String,
        #[serde(
            rename = "unitCostMinor",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        unit_cost_minor: Option<i64>,
    },
    #[serde(rename = "order.create")]
    OrderCreate {
        #[serde(rename = "customerName")]
        customer_name: String,
        lines: Vec<OrderLine>,
    },
    #[serde(rename = "order.reserve")]
    OrderReserve {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "holdSeconds")]
        hold_seconds: i64,
    },
    #[serde(rename = "order.fulfill")]
    OrderFulfill {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "order.cancel")]
    OrderCancel {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: String,
    },
    #[serde(rename = "order.return")]
    OrderReturn {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "lineId")]
        line_id: String,
        quantity: i64,
        disposition: String,
        reason: String,
    },
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.trim() == value && value.chars().count() <= max
}
fn revision(value: i64) -> bool {
    (1..=i64::from(i32::MAX)).contains(&value)
}
impl CommerceCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::ProductCreate { .. } => "product.create",
            Self::ProductSet { .. } => "product.set",
            Self::StockAdjust { .. } => "stock.adjust",
            Self::OrderCreate { .. } => "order.create",
            Self::OrderReserve { .. } => "order.reserve",
            Self::OrderFulfill { .. } => "order.fulfill",
            Self::OrderCancel { .. } => "order.cancel",
            Self::OrderReturn { .. } => "order.return",
        }
    }
    pub fn permissions(&self) -> &'static [&'static str] {
        match self {
            Self::ProductCreate { .. } | Self::ProductSet { .. } => {
                &["catalog.read", "catalog.write"]
            }
            Self::StockAdjust { .. } => &["catalog.read", "stock.read", "stock.write"],
            Self::OrderCreate { .. } => &["catalog.read", "orders.read", "orders.write"],
            Self::OrderReserve { .. } | Self::OrderFulfill { .. } | Self::OrderCancel { .. } => {
                &["orders.read", "orders.write"]
            }
            Self::OrderReturn { .. } => &["orders.read", "returns.write"],
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        let valid = match self {
            Self::ProductCreate {
                sku,
                title,
                unit,
                price,
                ..
            } => {
                valid_code(sku)
                    && text(title, 160)
                    && matches!(unit.as_str(), "unit" | "gram" | "millilitre")
                    && (0..=1_000_000_000).contains(price)
            }
            Self::ProductSet {
                id,
                expected_revision,
                price,
                ..
            } => {
                valid_uuid(id)
                    && revision(*expected_revision)
                    && (0..=1_000_000_000).contains(price)
            }
            Self::StockAdjust {
                product_id,
                quantity,
                reason,
                unit_cost_minor,
            } => {
                valid_uuid(product_id)
                    && *quantity != 0
                    && (-1_000_000_000..=1_000_000_000).contains(quantity)
                    && text(reason, 2000)
                    && unit_cost_minor
                        .is_none_or(|cost| *quantity > 0 && (0..=1_000_000_000).contains(&cost))
            }
            Self::OrderCreate {
                customer_name,
                lines,
            } => {
                text(customer_name, 160)
                    && !lines.is_empty()
                    && lines.len() <= 50
                    && lines
                        .iter()
                        .all(|l| valid_uuid(&l.product_id) && (1..=10_000).contains(&l.quantity))
                    && lines
                        .iter()
                        .map(|l| &l.product_id)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == lines.len()
            }
            Self::OrderReserve {
                id,
                expected_revision,
                hold_seconds,
            } => {
                valid_uuid(id) && revision(*expected_revision) && (1..=1800).contains(hold_seconds)
            }
            Self::OrderFulfill {
                id,
                expected_revision,
            } => valid_uuid(id) && revision(*expected_revision),
            Self::OrderCancel {
                id,
                expected_revision,
                reason,
            } => valid_uuid(id) && revision(*expected_revision) && text(reason, 2000),
            Self::OrderReturn {
                id,
                expected_revision,
                line_id,
                quantity,
                disposition,
                reason,
            } => {
                valid_uuid(id)
                    && revision(*expected_revision)
                    && valid_uuid(line_id)
                    && (1..=10_000).contains(quantity)
                    && matches!(disposition.as_str(), "restock" | "discard")
                    && text(reason, 2000)
            }
        };
        if valid {
            Ok(())
        } else {
            Err("Invalid commerce command fields.")
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_financial_values_and_duplicate_lines_are_rejected() {
        let product = CommerceCommand::ProductCreate {
            sku: "SKU-1".into(),
            title: "Example".into(),
            unit: "unit".into(),
            track_stock: true,
            price: -1,
        };
        assert!(product.validate().is_err());
        let id = "11111111-1111-4111-8111-111111111111".to_string();
        let line = OrderLine {
            product_id: id,
            quantity: 1,
        };
        let order = CommerceCommand::OrderCreate {
            customer_name: "Example".into(),
            lines: vec![line.clone(), line],
        };
        assert!(order.validate().is_err());
    }
    #[test]
    fn authority_is_not_a_client_field() {
        assert!(serde_json::from_value::<CommerceCommand>(serde_json::json!({"operation":"stock.adjust","productId":"11111111-1111-4111-8111-111111111111","quantity":1,"reason":"Count","role":"owner"})).is_err());
    }
}
