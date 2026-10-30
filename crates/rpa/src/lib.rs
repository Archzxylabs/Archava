//! Archava RPA: Computer Use & Vision Headless RPA Bridge for Legacy Banking & Government Portals.

pub mod action;
pub mod executor;
pub mod security;

pub use action::{RpaAction, RpaStepResult};
pub use executor::{plan_bank_statement_scrape, plan_djp_efaktur_download, RpaRecipe};
pub use security::{is_domain_allowed, mask_credential};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_djp_efaktur_recipe_planning() {
        let recipe = plan_djp_efaktur_download("2026-Q1").expect("valid recipe");
        assert_eq!(recipe.task_name, "DJP_EFAKTUR_2026-Q1");
        assert_eq!(recipe.steps.len(), 5);
        assert!(
            matches!(&recipe.steps[0], RpaAction::Navigate { url } if url.contains("pajak.go.id"))
        );
    }

    #[test]
    fn test_bank_statement_recipe_planning() {
        let recipe = plan_bank_statement_scrape("bca", "1234567890").expect("valid bca recipe");
        assert_eq!(recipe.task_name, "BCA_STATEMENT_1234567890");
        assert_eq!(recipe.steps.len(), 5);
    }
}
