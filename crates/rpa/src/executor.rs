//! Pre-Packaged Enterprise RPA Automation Recipes for Banking and Government Portals.

use crate::action::RpaAction;
use crate::security::is_domain_allowed;

/// A structured multi-step RPA recipe.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RpaRecipe {
    pub task_name: String,
    pub description: String,
    pub steps: Vec<RpaAction>,
}

/// Generates an e-Faktur Coretax XML download recipe.
pub fn plan_djp_efaktur_download(period: &str) -> Result<RpaRecipe, &'static str> {
    let url = "https://efaktur.pajak.go.id/login";
    if !is_domain_allowed(url) {
        return Err("Security Refusal: Target domain is not allowlisted.");
    }

    Ok(RpaRecipe {
        task_name: format!("DJP_EFAKTUR_{}", period),
        description: "Autonomous download of monthly Coretax VAT statement XML".into(),
        steps: vec![
            RpaAction::Navigate { url: url.into() },
            RpaAction::WaitForSelector {
                selector: "#npwpInput".into(),
                timeout_millis: 10000,
            },
            RpaAction::CaptureScreenshot {
                label: "login_page".into(),
            },
            RpaAction::Click {
                selector: "#menuEfakSPT".into(),
            },
            RpaAction::ExtractTable {
                table_selector: "#vatInvoicesTable".into(),
            },
        ],
    })
}

/// Generates a Corporate Bank Statement scrape recipe (KlikBCA Bisnis / Mandiri MCM).
pub fn plan_bank_statement_scrape(
    bank_name: &str,
    account_no: &str,
) -> Result<RpaRecipe, &'static str> {
    let url = match bank_name.to_ascii_lowercase().as_str() {
        "bca" => "https://vpn.klikbca.com",
        "mandiri" => "https://mcm2.bankmandiri.co.id",
        _ => return Err("Unsupported bank portal"),
    };

    if !is_domain_allowed(url) {
        return Err("Security Refusal: Target bank domain is not allowlisted.");
    }

    Ok(RpaRecipe {
        task_name: format!("{}_STATEMENT_{}", bank_name.to_uppercase(), account_no),
        description: format!(
            "Autonomous transaction mutation scrape for account {}",
            account_no
        ),
        steps: vec![
            RpaAction::Navigate { url: url.into() },
            RpaAction::WaitForSelector {
                selector: "#corpIdInput".into(),
                timeout_millis: 8000,
            },
            RpaAction::CaptureScreenshot {
                label: "bank_login".into(),
            },
            RpaAction::Click {
                selector: "#menuAccountStatement".into(),
            },
            RpaAction::ExtractTable {
                table_selector: "#transactionHistoryTable".into(),
            },
        ],
    })
}
