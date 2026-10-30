//! Indonesian Payroll & Compensation domain — salary structure, overtime,
//! BPJS Ketenagakerjaan & Kesehatan deductions, PPh 21 tax withholding, and payslip disbursement.
//!
//! ## Invariants
//!
//! - Closed vocabularies: payslip status, employment type, tax status (TK/0, K/0, K/1, etc.).
//! - Mandatory Indonesian statutory deductions:
//!   - BPJS Ketenagakerjaan (JHT 2% employee, JP 1% employee up to statutory cap).
//!   - BPJS Kesehatan (1% employee up to statutory cap).
//! - Net wage invariant: `net_salary = gross_earnings - employee_deductions - pph21_tax`.
//! - Pure domain logic: zero I/O, zero sqlx, wasm32-clean.

use serde::{Deserialize, Serialize};

use super::purchasing::ValidationError;
use super::valid_uuid;

pub const PAYROLL_PERMISSIONS: &[&str] = &[
    "payroll.read",
    "payroll.calculate",
    "payroll.disburse",
    "payroll.audit",
];

// Payslip Statuses
pub const PAYSLIP_DRAFT: &str = "draft";
pub const PAYSLIP_REVIEWED: &str = "reviewed";
pub const PAYSLIP_APPROVED: &str = "approved";
pub const PAYSLIP_DISBURSED: &str = "disbursed";

// Statutory Caps (IDR Rupiah in Cents)
pub const BPJS_KES_MAX_WAGE_CENTS: u64 = 1_200_000_000; // Rp 12.000.000 cap
pub const BPJS_JP_MAX_WAGE_CENTS: u64 = 1_004_230_000; // Rp 10.042.300 cap

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SalaryStructure {
    pub base_salary_cents: u64,
    pub fixed_allowance_cents: u64,
    pub transport_allowance_cents: u64,
    pub overtime_hours: u32,
    pub overtime_rate_per_hour_cents: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatutoryDeductions {
    pub bpjs_tk_jht_employee_cents: u64, // 2% JHT
    pub bpjs_tk_jp_employee_cents: u64,  // 1% JP (capped)
    pub bpjs_kes_employee_cents: u64,    // 1% Kesehatan (capped)
    pub pph21_tax_cents: u64,
    pub total_deductions_cents: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalculatedPayslip {
    pub gross_earnings_cents: u64,
    pub deductions: StatutoryDeductions,
    pub net_salary_cents: u64,
}

/// Computes statutory Indonesian payroll breakdown for one employee.
pub fn calculate_indonesian_payslip(
    salary: &SalaryStructure,
    pph21_estimated_cents: u64,
) -> CalculatedPayslip {
    let regular_wage = salary
        .base_salary_cents
        .saturating_add(salary.fixed_allowance_cents);

    let overtime_pay =
        (salary.overtime_hours as u64).saturating_mul(salary.overtime_rate_per_hour_cents);

    let gross_earnings = regular_wage
        .saturating_add(salary.transport_allowance_cents)
        .saturating_add(overtime_pay);

    // BPJS TK JHT (2% of regular base + fixed allowance)
    let jht = regular_wage.saturating_mul(2) / 100;

    // BPJS TK JP (1% capped)
    let jp_basis = regular_wage.min(BPJS_JP_MAX_WAGE_CENTS);
    let jp = jp_basis.saturating_mul(1) / 100;

    // BPJS Kesehatan (1% capped)
    let kes_basis = regular_wage.min(BPJS_KES_MAX_WAGE_CENTS);
    let kes = kes_basis.saturating_mul(1) / 100;

    let total_deductions = jht
        .saturating_add(jp)
        .saturating_add(kes)
        .saturating_add(pph21_estimated_cents);

    let net_salary = gross_earnings.saturating_sub(total_deductions);

    CalculatedPayslip {
        gross_earnings_cents: gross_earnings,
        deductions: StatutoryDeductions {
            bpjs_tk_jht_employee_cents: jht,
            bpjs_tk_jp_employee_cents: jp,
            bpjs_kes_employee_cents: kes,
            pph21_tax_cents: pph21_estimated_cents,
            total_deductions_cents: total_deductions,
        },
        net_salary_cents: net_salary,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovePayslipCommand {
    pub payslip_id: String,
    pub approver_id: String,
    pub from_status: String,
}

impl ApprovePayslipCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.payslip_id) {
            return Err(ValidationError("invalid payslip_id UUID"));
        }
        if !valid_uuid(&self.approver_id) {
            return Err(ValidationError("invalid approver_id UUID"));
        }
        if self.from_status != PAYSLIP_REVIEWED {
            return Err(ValidationError("only reviewed payslips can be approved"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indonesian_statutory_payroll_calculation() {
        let structure = SalaryStructure {
            base_salary_cents: 800_000_000,        // Rp 8.000.000 in cents
            fixed_allowance_cents: 100_000_000,    // Rp 1.000.000 in cents
            transport_allowance_cents: 50_000_000, // Rp 500.000 in cents
            overtime_hours: 10,
            overtime_rate_per_hour_cents: 5_000_000, // Rp 50.000/hr = Rp 500.000 total lembur
        };

        let pph21 = 15_000_000; // Rp 150.000 in cents
        let payslip = calculate_indonesian_payslip(&structure, pph21);

        // Gross: 8jt + 1jt + 500rb + 500rb = Rp 10.000.000 (1_000_000_000 cents)
        assert_eq!(payslip.gross_earnings_cents, 1_000_000_000);

        // Regular wage: 9jt (900_000_000 cents)
        // JHT (2% of 9jt) = Rp 180.000 (18_000_000 cents)
        assert_eq!(payslip.deductions.bpjs_tk_jht_employee_cents, 18_000_000);

        // JP (1% of 9jt, below 10.04jt cap) = Rp 90.000 (9_000_000 cents)
        assert_eq!(payslip.deductions.bpjs_tk_jp_employee_cents, 9_000_000);

        // Kes (1% of 9jt, below 12jt cap) = Rp 90.000 (9_000_000 cents)
        assert_eq!(payslip.deductions.bpjs_kes_employee_cents, 9_000_000);

        // Total deductions: 180rb + 90rb + 90rb + 150rb = 510.000 (51_000_000 cents)
        assert_eq!(payslip.deductions.total_deductions_cents, 51_000_000);

        // Net: 10.000.000 - 510.000 = Rp 9.490.000 (949_000_000 cents)
        assert_eq!(payslip.net_salary_cents, 949_000_000);
    }
}
