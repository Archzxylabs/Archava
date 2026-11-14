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
    #[serde(rename = "product.create_for_company")]
    ProductCreateForCompany {
        #[serde(rename = "companyId")]
        company_id: String,
        #[serde(rename = "expectedCompanyRevision")]
        expected_company_revision: i64,
        sku: String,
        title: String,
        unit: String,
        #[serde(rename = "trackStock")]
        track_stock: bool,
        price: i64,
    },
    #[serde(rename = "stock.adjust_for_company")]
    StockAdjustForCompany {
        #[serde(rename = "companyId")]
        company_id: String,
        #[serde(rename = "expectedCompanyRevision")]
        expected_company_revision: i64,
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
    #[serde(rename = "order.create_for_company")]
    OrderCreateForCompany {
        #[serde(rename = "companyId")]
        company_id: String,
        #[serde(rename = "expectedCompanyRevision")]
        expected_company_revision: i64,
        #[serde(rename = "partyId")]
        party_id: String,
        #[serde(rename = "expectedPartyRevision")]
        expected_party_revision: i64,
        lines: Vec<OrderLine>,
        /// The code of an output tax code of the company's book
        /// (`docs/architecture/TAX.md`, stage B). The company entered it;
        /// Archava verifies no rate. Stated together with `priceIncludesTax`.
        #[serde(rename = "taxCode", default, skip_serializing_if = "Option::is_none")]
        tax_code: Option<String>,
        /// Whether the product prices already contain the tax (`true`) or the
        /// tax is added to them (`false`).
        #[serde(
            rename = "priceIncludesTax",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        price_includes_tax: Option<bool>,
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
    #[serde(rename = "order.create_from_contact")]
    OrderCreateFromContact {
        #[serde(rename = "contactId")]
        contact_id: String,
        #[serde(rename = "expectedContactRevision")]
        expected_contact_revision: i64,
        lines: Vec<OrderLine>,
    },
    #[serde(rename = "order.create_from_party")]
    OrderCreateFromParty {
        #[serde(rename = "partyId")]
        party_id: String,
        #[serde(rename = "expectedPartyRevision")]
        expected_party_revision: i64,
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
fn valid_lines(lines: &[OrderLine]) -> bool {
    !lines.is_empty()
        && lines.len() <= 50
        && lines
            .iter()
            .all(|line| valid_uuid(&line.product_id) && (1..=10_000).contains(&line.quantity))
        && lines
            .iter()
            .map(|line| &line.product_id)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == lines.len()
}
impl CommerceCommand {
    pub fn company_selection(&self) -> Option<(&str, i64)> {
        match self {
            Self::ProductCreateForCompany {
                company_id,
                expected_company_revision,
                ..
            }
            | Self::StockAdjustForCompany {
                company_id,
                expected_company_revision,
                ..
            }
            | Self::OrderCreateForCompany {
                company_id,
                expected_company_revision,
                ..
            } => Some((company_id, *expected_company_revision)),
            _ => None,
        }
    }
    pub fn operation(&self) -> &'static str {
        match self {
            Self::ProductCreate { .. } => "product.create",
            Self::ProductCreateForCompany { .. } => "product.create_for_company",
            Self::StockAdjustForCompany { .. } => "stock.adjust_for_company",
            Self::OrderCreateForCompany { .. } => "order.create_for_company",
            Self::ProductSet { .. } => "product.set",
            Self::StockAdjust { .. } => "stock.adjust",
            Self::OrderCreate { .. } => "order.create",
            Self::OrderCreateFromContact { .. } => "order.create_from_contact",
            Self::OrderCreateFromParty { .. } => "order.create_from_party",
            Self::OrderReserve { .. } => "order.reserve",
            Self::OrderFulfill { .. } => "order.fulfill",
            Self::OrderCancel { .. } => "order.cancel",
            Self::OrderReturn { .. } => "order.return",
        }
    }
    pub fn permissions(&self) -> &'static [&'static str] {
        match self {
            Self::ProductCreate { .. }
            | Self::ProductCreateForCompany { .. }
            | Self::ProductSet { .. } => &["catalog.read", "catalog.write"],
            Self::StockAdjust { .. } | Self::StockAdjustForCompany { .. } => {
                &["catalog.read", "stock.read", "stock.write"]
            }
            Self::OrderCreate { .. }
            | Self::OrderCreateFromContact { .. }
            | Self::OrderCreateFromParty { .. }
            | Self::OrderCreateForCompany { .. } => {
                &["catalog.read", "orders.read", "orders.write"]
            }
            Self::OrderReserve { .. } | Self::OrderFulfill { .. } | Self::OrderCancel { .. } => {
                &["orders.read", "orders.write"]
            }
            Self::OrderReturn { .. } => &["orders.read", "returns.write"],
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .company_selection()
            .is_some_and(|(id, rev)| !valid_uuid(id) || !revision(rev))
        {
            return Err("Review a valid company and revision.");
        }
        let valid = match self {
            Self::ProductCreate {
                sku,
                title,
                unit,
                price,
                ..
            }
            | Self::ProductCreateForCompany {
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
            }
            | Self::StockAdjustForCompany {
                product_id,
                quantity,
                reason,
                unit_cost_minor,
                ..
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
            } => text(customer_name, 160) && valid_lines(lines),
            Self::OrderCreateFromParty {
                party_id,
                expected_party_revision,
                lines,
            } => valid_uuid(party_id) && revision(*expected_party_revision) && valid_lines(lines),
            // A tax code without a price basis, or a basis without a code, is
            // well formed; the adapter refuses it with its specific reason.
            Self::OrderCreateForCompany {
                party_id,
                expected_party_revision,
                lines,
                tax_code,
                ..
            } => {
                valid_uuid(party_id)
                    && revision(*expected_party_revision)
                    && valid_lines(lines)
                    && tax_code.as_deref().is_none_or(valid_code)
            }
            Self::OrderCreateFromContact {
                contact_id,
                expected_contact_revision,
                lines,
            } => {
                valid_uuid(contact_id) && revision(*expected_contact_revision) && valid_lines(lines)
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

    #[test]
    fn linked_customer_command_requires_exact_identity_revision_and_valid_lines() {
        let value = serde_json::json!({"operation":"order.create_from_contact","contactId":"11111111-1111-4111-8111-111111111111","expectedContactRevision":1,"lines":[{"productId":"22222222-2222-4222-8222-222222222222","quantity":2}]});
        let parsed = serde_json::from_value::<CommerceCommand>(value.clone()).unwrap();
        assert!(parsed.validate().is_ok());
        assert_eq!(parsed.operation(), "order.create_from_contact");
        for key in ["customerName", "customerContactId", "ownerId", "role"] {
            let mut forged = value.clone();
            forged[key] = serde_json::json!("forged");
            assert!(
                serde_json::from_value::<CommerceCommand>(forged).is_err(),
                "{key}"
            );
        }
        for revision in [0, -1, 2147483648i64] {
            let mut invalid = value.clone();
            invalid["expectedContactRevision"] = serde_json::json!(revision);
            assert!(serde_json::from_value::<CommerceCommand>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
        let mut invalid = value;
        invalid["lines"] = serde_json::json!([]);
        assert!(serde_json::from_value::<CommerceCommand>(invalid)
            .unwrap()
            .validate()
            .is_err());
    }
    #[test]
    fn a_company_order_may_name_a_tax_code_and_an_untaxed_one_serialises_as_before() {
        let id = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
        let plain = serde_json::json!({"operation":"order.create_for_company","companyId":id,"expectedCompanyRevision":1,"partyId":id,"expectedPartyRevision":1,"lines":[{"productId":id,"quantity":1}]});
        let parsed = serde_json::from_value::<CommerceCommand>(plain.clone()).unwrap();
        assert!(parsed.validate().is_ok());
        // The fingerprint of an existing command is its JSON: no new member.
        assert_eq!(serde_json::to_value(&parsed).unwrap(), plain);
        let mut taxed = plain.clone();
        taxed["taxCode"] = serde_json::json!("VAT-OUT");
        taxed["priceIncludesTax"] = serde_json::json!(false);
        let parsed = serde_json::from_value::<CommerceCommand>(taxed.clone()).unwrap();
        assert!(parsed.validate().is_ok());
        assert_eq!(serde_json::to_value(&parsed).unwrap(), taxed);
        for code in ["vat", "", " VAT", "-VAT", &"A".repeat(41)] {
            let mut body = taxed.clone();
            body["taxCode"] = serde_json::json!(code);
            assert!(
                serde_json::from_value::<CommerceCommand>(body)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{code}"
            );
        }
        // One without the other parses; the adapter names the refusal.
        let mut half = plain.clone();
        half["priceIncludesTax"] = serde_json::json!(true);
        assert!(serde_json::from_value::<CommerceCommand>(half)
            .unwrap()
            .validate()
            .is_ok());
        // No other order command takes a tax code, and a rate is never a field.
        let other = serde_json::json!({"operation":"order.create","customerName":"Walk-in","lines":[{"productId":id,"quantity":1}],"taxCode":"VAT-OUT","priceIncludesTax":false});
        assert!(serde_json::from_value::<CommerceCommand>(other).is_err());
        for key in ["taxRate", "taxMinor", "rate"] {
            let mut forged = taxed.clone();
            forged[key] = serde_json::json!(1);
            assert!(
                serde_json::from_value::<CommerceCommand>(forged).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn party_order_requires_reviewed_version_and_rejects_identity_claims() {
        let id = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
        let valid = serde_json::json!({"operation":"order.create_from_party","partyId":id,"expectedPartyRevision":1,"lines":[{"productId":id,"quantity":1}]});
        assert!(serde_json::from_value::<CommerceCommand>(valid.clone())
            .unwrap()
            .validate()
            .is_ok());
        for value in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!(2147483648i64),
        ] {
            let mut body = valid.clone();
            body["expectedPartyRevision"] = value;
            assert!(serde_json::from_value::<CommerceCommand>(body)
                .unwrap()
                .validate()
                .is_err());
        }
        for key in [
            "customerName",
            "contactId",
            "ownerId",
            "customerPartyRevision",
        ] {
            let mut body = valid.clone();
            body[key] = serde_json::json!(id);
            assert!(serde_json::from_value::<CommerceCommand>(body).is_err());
        }
    }
}
