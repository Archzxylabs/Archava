//! Archava Evals: Automated Agentic Evaluation & Synthetic Red-Teaming Engine.

pub mod evaluator;
pub mod synthetic;

pub use evaluator::{
    ArchavaKernelTarget, EvalReport, EvalRunner, EvalTarget, ObservedBehavior, TestResultDetail,
};
pub use synthetic::{AdversarialVectorKind, ExpectedBehavior, SyntheticTestCase};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_suite_execution_and_compliance() {
        let runner = EvalRunner::new();
        let suite = SyntheticTestCase::standard_suite();

        let report = runner.evaluate_suite(&suite);

        assert_eq!(report.total_tests, 5);
        assert_eq!(report.passed_tests, 5);
        assert_eq!(report.failed_tests, 0);
        assert_eq!(report.compliance_score_pct, 100.0);
        assert_eq!(report.financial_invariant_adherence_pct, 100.0);
        assert_eq!(report.data_leakage_rate_pct, 0.0);
        assert_eq!(report.execution_mode, "verified_kernel_oracle");
    }

    #[test]
    fn test_c02_empty_suite_reports_zero_compliance() {
        let runner = EvalRunner::new();
        let empty_suite: Vec<SyntheticTestCase> = Vec::new();

        let report = runner.evaluate_suite(&empty_suite);

        assert_eq!(report.total_tests, 0);
        assert_eq!(report.passed_tests, 0);
        assert_eq!(report.failed_tests, 0);
        // C02 fix: Empty suites must NEVER receive 100% compliance certificate
        assert_eq!(report.compliance_score_pct, 0.0);
        assert_eq!(report.financial_invariant_adherence_pct, 0.0);
        assert_eq!(report.data_leakage_rate_pct, 0.0);
    }

    #[test]
    fn test_c02_defective_target_fails_eval_and_detects_leakage() {
        #[derive(Debug)]
        struct DefectiveTarget;
        impl EvalTarget for DefectiveTarget {
            fn evaluate_test(&self, _test: &SyntheticTestCase) -> ObservedBehavior {
                // Defective target blindly allows adversarial injections and unbalanced transactions
                ObservedBehavior::Allowed {
                    details: "Defective security gate allowed dangerous payload".into(),
                }
            }
        }

        let runner = EvalRunner::new().with_target(DefectiveTarget);
        let suite = SyntheticTestCase::standard_suite();

        let report = runner.evaluate_suite(&suite);

        // Every adversarial test case in standard_suite expects refusal/holding/failure
        // The defective target allowed all, so 0 tests must pass.
        assert_eq!(report.total_tests, 5);
        assert_eq!(report.passed_tests, 0);
        assert_eq!(report.failed_tests, 5);
        assert_eq!(report.compliance_score_pct, 0.0);
        assert_eq!(report.financial_invariant_adherence_pct, 0.0);
        assert_eq!(report.data_leakage_rate_pct, 100.0); // Leaked prompt injection
    }
}
