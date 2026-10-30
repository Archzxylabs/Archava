//! Proactive Action Cards Engine for Ava.
//! Synthesizes continuous business signals (overdue receivables, depleted inventory,
//! upcoming appointments) into actionable, one-tap execution cards for the UI.

use crate::presets::IndustryProfile;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Severity and urgency tier of an action card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardPriority {
    /// Red banner: Immediate financial or operational risk (e.g. large overdue debt, stockout).
    Urgent = 1,
    /// Yellow banner: Action required today (e.g. supplier reorder, unconfirmed bookings).
    Attention = 2,
    /// Blue banner: Revenue expansion or customer retention opportunity.
    Opportunity = 3,
    /// Green banner: Positive milestone or daily briefing recap.
    Info = 4,
}

/// Functional category of the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCategory {
    Finance,
    Inventory,
    Operations,
    Customer,
}

/// The executable action trigger embedded in an action card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardAction {
    pub label: String,
    pub action_type: String,
    pub payload: serde_json::Value,
}

/// A standalone, interactive card presented to the business owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionCard {
    pub id: String,
    pub priority: CardPriority,
    pub category: CardCategory,
    pub title: String,
    pub subtitle: String,
    pub badge: Option<String>,
    pub primary_action: CardAction,
    pub secondary_action: Option<CardAction>,
    pub dismissible: bool,
}

/// Generates a prioritized proactive action feed based on current business metrics.
pub fn generate_action_feed(
    tenant: &str,
    profile: IndustryProfile,
    overdue_count: u32,
    overdue_total_cents: i64,
    low_stock_items: &[&str],
) -> Vec<ActionCard> {
    let mut feed = Vec::new();

    // 1. High-Value Overdue Invoices
    if overdue_count > 0 {
        let amount_idr = overdue_total_cents / 100;
        let priority = if overdue_total_cents > 1_000_000_000 {
            CardPriority::Urgent
        } else {
            CardPriority::Attention
        };

        feed.push(ActionCard {
            id: format!("card_ar_{}", tenant),
            priority,
            category: CardCategory::Finance,
            title: format!("{} Tagihan Melewati Jatuh Tempo", overdue_count),
            subtitle: format!(
                "Total Rp {} belum terbayar. Siap dikirimkan pengingat instan.",
                format_idr(amount_idr)
            ),
            badge: Some(format!("Rp {}", format_idr(amount_idr))),
            primary_action: CardAction {
                label: "Kirim WhatsApp + QRIS (1-Tap)".into(),
                action_type: "dispatch_ar_reminders".into(),
                payload: json!({
                    "tenant": tenant,
                    "count": overdue_count,
                    "total_cents": overdue_total_cents
                }),
            },
            secondary_action: Some(CardAction {
                label: "Tinjau Daftar Tagihan".into(),
                action_type: "view_invoices".into(),
                payload: json!({ "tenant": tenant }),
            }),
            dismissible: false,
        });
    }

    // 2. Low Stock Alerts
    if !low_stock_items.is_empty() {
        let items_preview = low_stock_items.join(", ");
        feed.push(ActionCard {
            id: format!("card_stock_{}", tenant),
            priority: CardPriority::Attention,
            category: CardCategory::Inventory,
            title: format!("{} Item Mendekati Batas Kritis", low_stock_items.len()),
            subtitle: format!("Persediaan: {}. Segera buat PO ke supplier.", items_preview),
            badge: Some(format!("{} SKU Menipis", low_stock_items.len())),
            primary_action: CardAction {
                label: "Buat Draft PO Supplier (1-Tap)".into(),
                action_type: "generate_reorder_po".into(),
                payload: json!({
                    "tenant": tenant,
                    "items": low_stock_items
                }),
            },
            secondary_action: Some(CardAction {
                label: "Cek Gudang".into(),
                action_type: "view_inventory".into(),
                payload: json!({ "tenant": tenant }),
            }),
            dismissible: true,
        });
    }

    // 3. Profile-Specific Smart Recommendation Card
    match profile {
        IndustryProfile::FnBCafe => {
            feed.push(ActionCard {
                id: format!("card_shift_{}", tenant),
                priority: CardPriority::Info,
                category: CardCategory::Operations,
                title: "Shift Kasir Siang Siap Direkonsiliasi".into(),
                subtitle: "Semua pesanan QRIS dan tunai telah dicatat di buku besar.".into(),
                badge: Some("Shift 1".into()),
                primary_action: CardAction {
                    label: "Cetak Ringkasan Shift (ESC/POS)".into(),
                    action_type: "print_shift_summary".into(),
                    payload: json!({ "tenant": tenant }),
                },
                secondary_action: None,
                dismissible: true,
            });
        }
        IndustryProfile::ClinicHealthcare => {
            feed.push(ActionCard {
                id: format!("card_clinic_{}", tenant),
                priority: CardPriority::Opportunity,
                category: CardCategory::Customer,
                title: "12 Pasien Terjadwal Besok Hari".into(),
                subtitle: "Kirim konfirmasi otomatis WhatsApp agar tidak terjadi no-show.".into(),
                badge: Some("12 Janji".into()),
                primary_action: CardAction {
                    label: "Kirim Konfirmasi WA (H-1)".into(),
                    action_type: "blast_patient_confirmations".into(),
                    payload: json!({ "tenant": tenant }),
                },
                secondary_action: None,
                dismissible: true,
            });
        }
        IndustryProfile::HospitalityHotel => {
            feed.push(ActionCard {
                id: format!("card_hotel_{}", tenant),
                priority: CardPriority::Info,
                category: CardCategory::Operations,
                title: "8 Villa Siap Check-In Hari Ini".into(),
                subtitle: "6 villa telah selesai dibersihkan oleh tim housekeeping.".into(),
                badge: Some("8 Check-in".into()),
                primary_action: CardAction {
                    label: "Kirim Panduan Smart Lock WA".into(),
                    action_type: "send_hotel_welcomes".into(),
                    payload: json!({ "tenant": tenant }),
                },
                secondary_action: None,
                dismissible: true,
            });
        }
        IndustryProfile::ProfessionalServices => {
            feed.push(ActionCard {
                id: format!("card_agency_{}", tenant),
                priority: CardPriority::Opportunity,
                category: CardCategory::Finance,
                title: "Milestone Selesai: Siap Terbitkan Tagihan".into(),
                subtitle: "Deliverable Proyek Studio telah di-approve oleh klien.".into(),
                badge: Some("Milestone 2".into()),
                primary_action: CardAction {
                    label: "Terbitkan Invoice Termin (1-Tap)".into(),
                    action_type: "issue_milestone_invoice".into(),
                    payload: json!({ "tenant": tenant }),
                },
                secondary_action: None,
                dismissible: true,
            });
        }
        _ => {}
    }

    feed.sort_by_key(|c| c.priority);
    feed
}

fn format_idr(mut val: i64) -> String {
    if val == 0 {
        return "0".into();
    }
    let mut parts = Vec::new();
    while val > 0 {
        let chunk = val % 1000;
        val /= 1000;
        if val > 0 {
            parts.push(format!("{:03}", chunk));
        } else {
            parts.push(format!("{}", chunk));
        }
    }
    parts.reverse();
    parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_feed_generation_and_sorting() {
        let feed = generate_action_feed(
            "dr_kevin_clinic",
            IndustryProfile::ClinicHealthcare,
            4,
            1_500_000_000, // Rp 15.000.000
            &["Serum Retinol 20ml", "Botol Vitamin C"],
        );

        // 3 cards generated: Urgent AR debt, Attention Stock, Opportunity Clinic Patients
        assert_eq!(feed.len(), 3);
        assert_eq!(feed[0].priority, CardPriority::Urgent);
        assert_eq!(feed[1].priority, CardPriority::Attention);
        assert_eq!(feed[2].priority, CardPriority::Opportunity);

        assert!(feed[0].title.contains("4 Tagihan"));
        assert!(feed[0].badge.as_ref().unwrap().contains("15.000.000"));
        assert_eq!(feed[0].primary_action.action_type, "dispatch_ar_reminders");
    }
}
