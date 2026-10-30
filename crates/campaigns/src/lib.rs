//! Proactive Operations & Autonomous Campaigns for Archava.

pub mod campaign;
pub mod runner;
pub mod trigger;

pub use campaign::{build_ar_debt_plan, build_reorder_po_plan, CampaignPlan};
pub use runner::CampaignRunner;
pub use trigger::TriggerKind;

#[cfg(test)]
mod tests {
    use super::*;
    use archava_cockpit::CockpitEngine;

    #[test]
    fn test_ar_debt_collection_campaign() {
        let cockpit = CockpitEngine::new();
        let runner = CampaignRunner::new(&cockpit);

        let trigger = TriggerKind::OverdueInvoice {
            invoice_id: "INV-902".into(),
            customer_name: "Budi Santoso".into(),
            customer_phone: "628123456789".into(),
            amount_cents: 500000000, // Rp 5.000.000 (< 10jt)
            days_overdue: 4,
        };

        let (plan, mission_id) = runner.process_trigger("ten_01", trigger);
        assert_eq!(plan.campaign_code, "AR-INV-902");
        assert!(!plan.requires_holding_pattern_approval);
        assert!(cockpit.get_mission(mission_id).is_some());
    }

    #[test]
    fn test_high_value_ar_enters_holding_pattern() {
        let cockpit = CockpitEngine::new();
        let runner = CampaignRunner::new(&cockpit);

        let trigger = TriggerKind::OverdueInvoice {
            invoice_id: "INV-999".into(),
            customer_name: "PT Mega Properti".into(),
            customer_phone: "628198765432".into(),
            amount_cents: 1500000000, // Rp 15.000.000 (> 10jt)
            days_overdue: 7,
        };

        let (plan, mission_id) = runner.process_trigger("ten_01", trigger);
        assert!(plan.requires_holding_pattern_approval);
        let mission = cockpit.get_mission(mission_id).unwrap();
        assert!(mission.holding_reason.is_some());
        assert!(mission.holding_signal_key.is_some());
    }
}
