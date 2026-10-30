//! Omnichannel messaging engines (WhatsApp Business Cloud API, Webhooks).

pub mod whatsapp;

pub use whatsapp::{
    build_invoice_whatsapp_message, parse_whatsapp_webhook_status, MessageDeliveryStatus,
    SendTemplateRequest, TemplateComponent, TemplateParameter, WhatsAppRecipient,
    WhatsAppStatusEvent,
};
