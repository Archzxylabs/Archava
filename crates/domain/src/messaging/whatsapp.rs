//! WhatsApp Business Cloud API Gateway & Inbound Webhook Parser.
//!
//! Complies with Meta Graph API v21.0.
//! Provides builders for transactional templates, interactive buttons,
//! and webhook parsers for delivery acknowledgements (sent, delivered, read, failed).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhatsAppRecipient {
    pub phone_number: String, // E.164 format without '+' e.g. "6281234567890"
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TemplateComponent {
    Header {
        parameters: Vec<TemplateParameter>,
    },
    Body {
        parameters: Vec<TemplateParameter>,
    },
    Button {
        sub_type: String,
        index: u8,
        parameters: Vec<TemplateParameter>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TemplateParameter {
    Text {
        text: String,
    },
    Currency {
        fallback_value: String,
        code: String,
        amount_1000: i64,
    },
    DateTime {
        fallback_value: String,
    },
    Document {
        link: String,
        filename: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplatePayload {
    pub name: String,
    pub language: TemplateLanguage,
    pub components: Vec<TemplateComponent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateLanguage {
    pub code: String, // e.g. "id" or "en"
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendTemplateRequest {
    pub messaging_product: String,
    pub recipient_type: String,
    pub to: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub template: TemplatePayload,
}

/// Helper to build an Invoice Notification WhatsApp Template payload.
pub fn build_invoice_whatsapp_message(
    to_phone: &str,
    customer_name: &str,
    invoice_no: &str,
    amount_cents: i64,
    due_date: &str,
    invoice_pdf_url: &str,
) -> SendTemplateRequest {
    let clean_phone = to_phone.trim_start_matches('+').replace(['-', ' '], "");
    let amount_idr = amount_cents / 100;

    SendTemplateRequest {
        messaging_product: "whatsapp".to_string(),
        recipient_type: "individual".to_string(),
        to: clean_phone,
        message_type: "template".to_string(),
        template: TemplatePayload {
            name: "invoice_notification".to_string(),
            language: TemplateLanguage {
                code: "id".to_string(),
            },
            components: vec![
                TemplateComponent::Header {
                    parameters: vec![TemplateParameter::Document {
                        link: invoice_pdf_url.to_string(),
                        filename: format!("{}.pdf", invoice_no),
                    }],
                },
                TemplateComponent::Body {
                    parameters: vec![
                        TemplateParameter::Text {
                            text: customer_name.to_string(),
                        },
                        TemplateParameter::Text {
                            text: invoice_no.to_string(),
                        },
                        TemplateParameter::Text {
                            text: format!("Rp {}", amount_idr),
                        },
                        TemplateParameter::Text {
                            text: due_date.to_string(),
                        },
                    ],
                },
            ],
        },
    }
}

/// Webhook status update from Meta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageDeliveryStatus {
    Sent,
    Delivered,
    Read,
    Failed {
        error_code: u32,
        error_message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhatsAppStatusEvent {
    pub message_id: String,
    pub recipient_id: String,
    pub status: MessageDeliveryStatus,
    pub timestamp: String,
}

/// Inbound Webhook Parser for WhatsApp Cloud API.
pub fn parse_whatsapp_webhook_status(
    raw_json: &serde_json::Value,
) -> Result<Vec<WhatsAppStatusEvent>, &'static str> {
    let mut events = Vec::new();

    let entries = raw_json["entry"].as_array().ok_or("Missing entry array")?;
    for entry in entries {
        let changes = entry["changes"].as_array().ok_or("Missing changes array")?;
        for change in changes {
            let value = &change["value"];
            if let Some(statuses) = value["statuses"].as_array() {
                for st in statuses {
                    let msg_id = st["id"].as_str().ok_or("Missing message id")?.to_string();
                    let recipient = st["recipient_id"]
                        .as_str()
                        .ok_or("Missing recipient")?
                        .to_string();
                    let status_str = st["status"].as_str().ok_or("Missing status")?;
                    let timestamp = st["timestamp"].as_str().unwrap_or("").to_string();

                    let status = match status_str {
                        "sent" => MessageDeliveryStatus::Sent,
                        "delivered" => MessageDeliveryStatus::Delivered,
                        "read" => MessageDeliveryStatus::Read,
                        "failed" => {
                            let err_code = st["errors"][0]["code"].as_u64().unwrap_or(0) as u32;
                            let err_msg = st["errors"][0]["title"]
                                .as_str()
                                .unwrap_or("Unknown error")
                                .to_string();
                            MessageDeliveryStatus::Failed {
                                error_code: err_code,
                                error_message: err_msg,
                            }
                        }
                        _ => continue,
                    };

                    events.push(WhatsAppStatusEvent {
                        message_id: msg_id,
                        recipient_id: recipient,
                        status,
                        timestamp,
                    });
                }
            }
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_invoice_whatsapp_message() {
        let req = build_invoice_whatsapp_message(
            "+62 812-3456-7890",
            "Pak Kevin",
            "INV-2026-0042",
            1_500_000_00,
            "2026-11-05",
            "https://cdn.archzxy.com/invoices/INV-2026-0042.pdf",
        );

        assert_eq!(req.to, "6281234567890");
        assert_eq!(req.template.name, "invoice_notification");
        assert_eq!(req.template.components.len(), 2);
    }

    #[test]
    fn test_parse_whatsapp_webhook_delivered() {
        let payload = serde_json::json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "100200300",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "metadata": { "display_phone_number": "12345", "phone_number_id": "67890" },
                        "statuses": [{
                            "id": "wamid.HBgLMjA=",
                            "status": "delivered",
                            "timestamp": "1730000000",
                            "recipient_id": "6281234567890"
                        }]
                    },
                    "field": "messages"
                }]
            }]
        });

        let events = parse_whatsapp_webhook_status(&payload).expect("Webhook parsed");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message_id, "wamid.HBgLMjA=");
        assert_eq!(events[0].status, MessageDeliveryStatus::Delivered);
    }
}
