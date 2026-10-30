//! Autonomous Campaign Runner.
//! Bridges triggers into Ava Cockpit Flight Missions with Holding Pattern governance.

use archava_cockpit::{CockpitEngine, FlightStep, Mission};
use serde_json::json;
use uuid::Uuid;

use crate::campaign::{build_ar_debt_plan, build_reorder_po_plan, CampaignPlan};
use crate::trigger::TriggerKind;

/// Campaign runner coordinating autonomous execution.
pub struct CampaignRunner<'a> {
    cockpit: &'a CockpitEngine,
}

impl<'a> CampaignRunner<'a> {
    pub fn new(cockpit: &'a CockpitEngine) -> Self {
        Self { cockpit }
    }

    /// Processes an operational trigger and registers an autonomous mission into Cockpit.
    pub fn process_trigger(&self, tenant_id: &str, trigger: TriggerKind) -> (CampaignPlan, Uuid) {
        let plan = match trigger {
            TriggerKind::OverdueInvoice {
                invoice_id,
                customer_name,
                customer_phone,
                amount_cents,
                days_overdue,
            } => build_ar_debt_plan(
                &invoice_id,
                &customer_name,
                &customer_phone,
                amount_cents,
                days_overdue,
            ),
            TriggerKind::LowStockAlert {
                sku,
                item_name,
                recommended_order_qty,
                supplier_id,
                ..
            } => build_reorder_po_plan(&sku, &item_name, recommended_order_qty, &supplier_id),
            TriggerKind::DormantCustomer {
                customer_name,
                customer_phone,
                days_inactive,
                ..
            } => CampaignPlan {
                campaign_code: format!("WINBACK-{}", customer_name),
                title: format!("VIP Winback: {}", customer_name),
                channel: "whatsapp".into(),
                target_recipient: customer_phone,
                message_body: format!(
                    "Halo Kak {}, sudah {} hari tidak berkunjung. Spesial untuk Kakak, nikmati voucher diskon 20% untuk treatment berikutnya!",
                    customer_name, days_inactive
                ),
                requires_holding_pattern_approval: false,
                holding_reason: None,
            },
        };

        // Create Flight Step for Cockpit
        let step1 = FlightStep::new(
            1,
            plan.title.clone(),
            format!("{}.dispatch", plan.channel),
            json!({
                "recipient": plan.target_recipient,
                "message": plan.message_body,
            }),
            vec![],
        );

        let mut mission = Mission::new(
            tenant_id,
            plan.campaign_code.clone(),
            plan.title.clone(),
            "Proactive Autonomous Operational Campaign",
            vec![step1],
        );

        if plan.requires_holding_pattern_approval {
            mission.holding_reason = plan.holding_reason.clone();
            mission.holding_signal_key = Some(format!("SIG-{}", Uuid::new_v4()));
        }

        let mission_id = self.cockpit.register_mission(mission);
        if !plan.requires_holding_pattern_approval {
            let _ = self.cockpit.launch_flight(mission_id);
        }

        (plan, mission_id)
    }
}
