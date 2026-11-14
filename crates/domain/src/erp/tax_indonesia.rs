//! Unverified reference formulas for Indonesian PPN, PPh 21 and BPJS.
//!
//! No other module uses this one: it is only declared in `erp/mod.rs`, and no
//! adapter, route, schema or screen calls it. The rates, brackets and caps were
//! not reviewed by a tax professional and are not kept in step with regulation.
//! It is not an integration with Coretax or DJP and sends nothing to either.
//! The arithmetic goes through `f64`, so the results are not suitable for
//! posting to a ledger.
//!
//! Contents:
//! 1. PPN (VAT) amount from a rate in basis points, and an e-Faktur-shaped
//!    struct whose QR payload string is not validated against anything.
//! 2. PPh 21 TER (Tarif Efektif Rata-Rata) monthly rate lookup, written after
//!    PP 58/2023 & PMK 168/2023.
//! 3. PPh 21 Pasal 17 annual progressive brackets.
//! 4. BPJS Ketenagakerjaan (JHT, JP, JKK, JKM) & BPJS Kesehatan contribution splits.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PtkpStatus {
    Tk0, // Tidak Kawin, 0 Tanggungan (Rp 54.000.000)
    Tk1, // Tidak Kawin, 1 Tanggungan (Rp 58.500.000)
    Tk2, // Tidak Kawin, 2 Tanggungan (Rp 63.000.000)
    Tk3, // Tidak Kawin, 3 Tanggungan (Rp 67.500.000)
    K0,  // Kawin, 0 Tanggungan (Rp 58.500.000)
    K1,  // Kawin, 1 Tanggungan (Rp 63.000.000)
    K2,  // Kawin, 2 Tanggungan (Rp 67.500.000)
    K3,  // Kawin, 3 Tanggungan (Rp 72.000.000)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerCategory {
    CategoryA, // TK/0, TK/1, K/0
    CategoryB, // TK/2, TK/3, K/1, K/2
    CategoryC, // K/3
}

impl PtkpStatus {
    pub fn ter_category(&self) -> TerCategory {
        match self {
            PtkpStatus::Tk0 | PtkpStatus::Tk1 | PtkpStatus::K0 => TerCategory::CategoryA,
            PtkpStatus::Tk2 | PtkpStatus::Tk3 | PtkpStatus::K1 | PtkpStatus::K2 => {
                TerCategory::CategoryB
            }
            PtkpStatus::K3 => TerCategory::CategoryC,
        }
    }

    pub fn annual_ptkp_cents(&self) -> i64 {
        match self {
            PtkpStatus::Tk0 => 54_000_000_00,
            PtkpStatus::Tk1 | PtkpStatus::K0 => 58_500_000_00,
            PtkpStatus::Tk2 | PtkpStatus::K1 => 63_000_000_00,
            PtkpStatus::Tk3 | PtkpStatus::K2 => 67_500_000_00,
            PtkpStatus::K3 => 72_000_000_00,
        }
    }
}

/// Monthly TER rate lookup based on gross income (in Rupiah minor cents).
/// Written after Lampiran PP 58/2023 & PMK 168/2023; the table is unverified.
pub fn lookup_ter_rate(category: TerCategory, gross_income_cents: i64) -> f64 {
    let gross_idr = gross_income_cents / 100;

    match category {
        TerCategory::CategoryA => {
            if gross_idr <= 5_400_000 {
                0.00
            } else if gross_idr <= 5_650_000 {
                0.0025
            } else if gross_idr <= 5_950_000 {
                0.005
            } else if gross_idr <= 6_300_000 {
                0.0075
            } else if gross_idr <= 6_750_000 {
                0.01
            } else if gross_idr <= 7_500_000 {
                0.0125
            } else if gross_idr <= 8_550_000 {
                0.015
            } else if gross_idr <= 9_650_000 {
                0.0175
            } else if gross_idr <= 10_050_000 {
                0.02
            } else if gross_idr <= 10_350_000 {
                0.0225
            } else if gross_idr <= 10_700_000 {
                0.025
            } else if gross_idr <= 11_050_000 {
                0.03
            } else if gross_idr <= 11_600_000 {
                0.035
            } else if gross_idr <= 12_500_000 {
                0.04
            } else if gross_idr <= 13_750_000 {
                0.05
            } else if gross_idr <= 15_100_000 {
                0.06
            } else if gross_idr <= 16_950_000 {
                0.07
            } else if gross_idr <= 19_750_000 {
                0.08
            } else if gross_idr <= 24_150_000 {
                0.09
            } else if gross_idr <= 26_450_000 {
                0.10
            } else if gross_idr <= 28_000_000 {
                0.11
            } else if gross_idr <= 30_050_000 {
                0.12
            } else if gross_idr <= 32_400_000 {
                0.13
            } else if gross_idr <= 35_400_000 {
                0.14
            } else if gross_idr <= 39_100_000 {
                0.15
            } else if gross_idr <= 43_850_000 {
                0.16
            } else if gross_idr <= 47_800_000 {
                0.17
            } else if gross_idr <= 51_400_000 {
                0.18
            } else if gross_idr <= 56_300_000 {
                0.19
            } else if gross_idr <= 62_200_000 {
                0.20
            } else if gross_idr <= 68_600_000 {
                0.21
            } else if gross_idr <= 77_500_000 {
                0.22
            } else if gross_idr <= 89_000_000 {
                0.23
            } else if gross_idr <= 103_000_000 {
                0.24
            } else if gross_idr <= 125_000_000 {
                0.25
            } else if gross_idr <= 157_000_000 {
                0.26
            } else if gross_idr <= 206_000_000 {
                0.27
            } else if gross_idr <= 337_000_000 {
                0.28
            } else if gross_idr <= 454_000_000 {
                0.29
            } else if gross_idr <= 550_000_000 {
                0.30
            } else if gross_idr <= 695_000_000 {
                0.31
            } else if gross_idr <= 910_000_000 {
                0.32
            } else if gross_idr <= 1_400_000_000 {
                0.33
            } else {
                0.34
            }
        }
        TerCategory::CategoryB => {
            if gross_idr <= 6_200_000 {
                0.00
            } else if gross_idr <= 6_500_000 {
                0.0025
            } else if gross_idr <= 6_850_000 {
                0.005
            } else if gross_idr <= 7_300_000 {
                0.0075
            } else if gross_idr <= 9_200_000 {
                0.01
            } else if gross_idr <= 10_750_000 {
                0.015
            } else if gross_idr <= 11_250_000 {
                0.02
            } else if gross_idr <= 11_600_000 {
                0.025
            } else if gross_idr <= 12_600_000 {
                0.03
            } else if gross_idr <= 13_600_000 {
                0.04
            } else if gross_idr <= 14_950_000 {
                0.05
            } else if gross_idr <= 16_400_000 {
                0.06
            } else if gross_idr <= 18_450_000 {
                0.07
            } else if gross_idr <= 21_850_000 {
                0.08
            } else if gross_idr <= 26_000_000 {
                0.09
            } else if gross_idr <= 27_700_000 {
                0.10
            } else if gross_idr <= 29_350_000 {
                0.11
            } else if gross_idr <= 31_450_000 {
                0.12
            } else if gross_idr <= 33_950_000 {
                0.13
            } else if gross_idr <= 37_100_000 {
                0.14
            } else if gross_idr <= 41_100_000 {
                0.15
            } else if gross_idr <= 45_800_000 {
                0.16
            } else if gross_idr <= 49_500_000 {
                0.17
            } else if gross_idr <= 53_800_000 {
                0.18
            } else if gross_idr <= 58_500_000 {
                0.19
            } else if gross_idr <= 64_000_000 {
                0.20
            } else if gross_idr <= 71_000_000 {
                0.21
            } else if gross_idr <= 80_000_000 {
                0.22
            } else if gross_idr <= 93_000_000 {
                0.23
            } else if gross_idr <= 109_000_000 {
                0.24
            } else if gross_idr <= 129_000_000 {
                0.25
            } else if gross_idr <= 163_000_000 {
                0.26
            } else if gross_idr <= 211_000_000 {
                0.27
            } else if gross_idr <= 374_000_000 {
                0.28
            } else if gross_idr <= 459_000_000 {
                0.29
            } else if gross_idr <= 555_000_000 {
                0.30
            } else if gross_idr <= 704_000_000 {
                0.31
            } else if gross_idr <= 957_000_000 {
                0.32
            } else if gross_idr <= 1_405_000_000 {
                0.33
            } else {
                0.34
            }
        }
        TerCategory::CategoryC => {
            if gross_idr <= 6_600_000 {
                0.00
            } else if gross_idr <= 6_950_000 {
                0.0025
            } else if gross_idr <= 7_350_000 {
                0.005
            } else if gross_idr <= 7_800_000 {
                0.0075
            } else if gross_idr <= 8_850_000 {
                0.01
            } else if gross_idr <= 9_800_000 {
                0.0125
            } else if gross_idr <= 10_950_000 {
                0.015
            } else if gross_idr <= 11_200_000 {
                0.0175
            } else if gross_idr <= 12_050_000 {
                0.02
            } else if gross_idr <= 12_950_000 {
                0.03
            } else if gross_idr <= 14_150_000 {
                0.04
            } else if gross_idr <= 15_550_000 {
                0.05
            } else if gross_idr <= 17_050_000 {
                0.06
            } else if gross_idr <= 19_500_000 {
                0.07
            } else if gross_idr <= 22_700_000 {
                0.08
            } else if gross_idr <= 26_600_000 {
                0.09
            } else if gross_idr <= 28_100_000 {
                0.10
            } else if gross_idr <= 30_100_000 {
                0.11
            } else if gross_idr <= 32_600_000 {
                0.12
            } else if gross_idr <= 35_400_000 {
                0.13
            } else if gross_idr <= 38_900_000 {
                0.14
            } else if gross_idr <= 43_000_000 {
                0.15
            } else if gross_idr <= 47_400_000 {
                0.16
            } else if gross_idr <= 51_200_000 {
                0.17
            } else if gross_idr <= 55_800_000 {
                0.18
            } else if gross_idr <= 60_400_000 {
                0.19
            } else if gross_idr <= 66_700_000 {
                0.20
            } else if gross_idr <= 74_500_000 {
                0.21
            } else if gross_idr <= 83_200_000 {
                0.22
            } else if gross_idr <= 95_600_000 {
                0.23
            } else if gross_idr <= 110_000_000 {
                0.24
            } else if gross_idr <= 134_000_000 {
                0.25
            } else if gross_idr <= 169_000_000 {
                0.26
            } else if gross_idr <= 221_000_000 {
                0.27
            } else if gross_idr <= 390_000_000 {
                0.28
            } else if gross_idr <= 463_000_000 {
                0.29
            } else if gross_idr <= 561_000_000 {
                0.30
            } else if gross_idr <= 709_000_000 {
                0.31
            } else if gross_idr <= 965_000_000 {
                0.32
            } else if gross_idr <= 1_419_000_000 {
                0.33
            } else {
                0.34
            }
        }
    }
}

/// Computes monthly PPh 21 TER tax in minor cents.
pub fn calculate_pph21_ter_monthly(ptkp: PtkpStatus, gross_income_cents: i64) -> i64 {
    let category = ptkp.ter_category();
    let rate = lookup_ter_rate(category, gross_income_cents);
    (gross_income_cents as f64 * rate).round() as i64
}

/// Computes annual Pasal 17 progressive tax (5%, 15%, 25%, 30%, 35%) over PKP.
pub fn calculate_pasal17_annual(pkp_cents: i64) -> i64 {
    if pkp_cents <= 0 {
        return 0;
    }

    let mut tax: i64 = 0;
    let mut remaining = pkp_cents;

    // Bracket 1: 0 - 60 Juta @ 5%
    let b1 = remaining.min(60_000_000_00);
    tax += ((b1 as f64) * 0.05).round() as i64;
    remaining -= b1;

    // Bracket 2: 60 - 250 Juta (190 Juta span) @ 15%
    if remaining > 0 {
        let b2 = remaining.min(190_000_000_00);
        tax += ((b2 as f64) * 0.15).round() as i64;
        remaining -= b2;
    }

    // Bracket 3: 250 - 500 Juta (250 Juta span) @ 25%
    if remaining > 0 {
        let b3 = remaining.min(250_000_000_00);
        tax += ((b3 as f64) * 0.25).round() as i64;
        remaining -= b3;
    }

    // Bracket 4: 500 Juta - 5 Miliar (4.5 Miliar span) @ 30%
    if remaining > 0 {
        let b4 = remaining.min(4_500_000_000_00);
        tax += ((b4 as f64) * 0.30).round() as i64;
        remaining -= b4;
    }

    // Bracket 5: > 5 Miliar @ 35%
    if remaining > 0 {
        tax += ((remaining as f64) * 0.35).round() as i64;
    }

    tax
}

/// BPJS monthly contribution splits (unverified rates and caps).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BpjsContribution {
    pub bpjs_kesehatan_employer_cents: i64,
    pub bpjs_kesehatan_employee_cents: i64,
    pub jht_employer_cents: i64,
    pub jht_employee_cents: i64,
    pub jp_employer_cents: i64,
    pub jp_employee_cents: i64,
    pub jkk_employer_cents: i64,
    pub jkm_employer_cents: i64,
    pub total_employer_cents: i64,
    pub total_employee_cents: i64,
}

pub fn calculate_bpjs(gross_wage_cents: i64) -> BpjsContribution {
    // Wage caps (in cents); unverified, not kept in step with regulation
    const KESEHATAN_WAGE_CAP: i64 = 12_000_000_00; // Rp 12 Juta
    const JP_WAGE_CAP: i64 = 10_042_300_00; // Rp 10.042.300

    let base_kes = gross_wage_cents.min(KESEHATAN_WAGE_CAP);
    let base_jp = gross_wage_cents.min(JP_WAGE_CAP);

    let kes_er = ((base_kes as f64) * 0.04).round() as i64; // 4% Employer
    let kes_ee = ((base_kes as f64) * 0.01).round() as i64; // 1% Employee

    let jht_er = ((gross_wage_cents as f64) * 0.037).round() as i64; // 3.7% Employer
    let jht_ee = ((gross_wage_cents as f64) * 0.020).round() as i64; // 2.0% Employee

    let jp_er = ((base_jp as f64) * 0.020).round() as i64; // 2.0% Employer
    let jp_ee = ((base_jp as f64) * 0.010).round() as i64; // 1.0% Employee

    let jkk_er = ((gross_wage_cents as f64) * 0.0024).round() as i64; // 0.24% Employer
    let jkm_er = ((gross_wage_cents as f64) * 0.0030).round() as i64; // 0.30% Employer

    let total_er = kes_er + jht_er + jp_er + jkk_er + jkm_er;
    let total_ee = kes_ee + jht_ee + jp_ee;

    BpjsContribution {
        bpjs_kesehatan_employer_cents: kes_er,
        bpjs_kesehatan_employee_cents: kes_ee,
        jht_employer_cents: jht_er,
        jht_employee_cents: jht_ee,
        jp_employer_cents: jp_er,
        jp_employee_cents: jp_ee,
        jkk_employer_cents: jkk_er,
        jkm_employer_cents: jkm_er,
        total_employer_cents: total_er,
        total_employee_cents: total_ee,
    }
}

/// An e-Faktur-shaped VAT record. Not a Coretax document: nothing validates
/// its fields or its QR payload, and nothing submits it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EFakturDocument {
    pub transaction_code: String, // e.g. "01" (Kepada Pihak Bukan Pemungut PPN)
    pub nsfp: String,             // Nomor Seri Faktur Pajak e.g. "010.001-26.00000001"
    pub invoice_date: String,
    pub seller_npwp: String, // 16-digit NPWP
    pub buyer_npwp_or_nik: String,
    pub dpp_cents: i64,    // Dasar Pengenaan Pajak
    pub vat_rate_bps: u32, // 1100 = 11%, 1200 = 12%
    pub vat_cents: i64,    // Jumlah PPN
    pub qrcode_payload: String,
}

pub fn generate_efaktur(
    transaction_code: &str,
    nsfp: &str,
    date: &str,
    seller_npwp: &str,
    buyer_id: &str,
    dpp_cents: i64,
    vat_rate_bps: u32,
) -> EFakturDocument {
    let vat_cents = ((dpp_cents as f64) * (vat_rate_bps as f64 / 10_000.0)).round() as i64;
    let qrcode_payload = format!(
        "https://coretax.pajak.go.id/faktur?kd={}&nsfp={}&npwp={}&dpp={}&ppn={}",
        transaction_code,
        nsfp,
        seller_npwp,
        dpp_cents / 100,
        vat_cents / 100
    );

    EFakturDocument {
        transaction_code: transaction_code.to_string(),
        nsfp: nsfp.to_string(),
        invoice_date: date.to_string(),
        seller_npwp: seller_npwp.to_string(),
        buyer_npwp_or_nik: buyer_id.to_string(),
        dpp_cents,
        vat_rate_bps,
        vat_cents,
        qrcode_payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ter_rate_and_monthly_withholding() {
        // Employee with TK/0 (Category A) earning Rp 10.000.000 / month (falls in 2.0% bracket)
        let gross_cents = 10_000_000_00;
        let rate = lookup_ter_rate(TerCategory::CategoryA, gross_cents);
        assert_eq!(rate, 0.02); // 2.0% for 10.000.000

        let tax = calculate_pph21_ter_monthly(PtkpStatus::Tk0, gross_cents);
        assert_eq!(tax, 200_000_00); // Rp 200.000
    }

    #[test]
    fn test_ter_category_b_and_c() {
        // Employee with K/2 (Category B) earning Rp 15.000.000 / month
        let gross_cents = 15_000_000_00;
        let rate = lookup_ter_rate(TerCategory::CategoryB, gross_cents);
        assert_eq!(rate, 0.06); // 6.0% for 15.000.000

        let tax = calculate_pph21_ter_monthly(PtkpStatus::K2, gross_cents);
        assert_eq!(tax, 900_000_00); // Rp 900.000
    }

    #[test]
    fn test_pasal17_progressive_brackets() {
        // Annual PKP = Rp 100.000.000
        // Bracket 1: 60M @ 5% = 3M
        // Bracket 2: 40M @ 15% = 6M
        // Total = 9M
        let pkp_cents = 100_000_000_00;
        let annual_tax = calculate_pasal17_annual(pkp_cents);
        assert_eq!(annual_tax, 9_000_000_00);
    }

    #[test]
    fn test_bpjs_contributions_and_caps() {
        // High earner: Rp 20.000.000 / month (exceeds both BPJS Kesehatan 12M cap and JP 10.042.300 cap)
        let wage_cents = 20_000_000_00;
        let bpjs = calculate_bpjs(wage_cents);

        // BPJS Kesehatan: capped at 12M -> Employer 4% of 12M = Rp 480.000, Employee 1% of 12M = Rp 120.000
        assert_eq!(bpjs.bpjs_kesehatan_employer_cents, 480_000_00);
        assert_eq!(bpjs.bpjs_kesehatan_employee_cents, 120_000_00);

        // JHT: uncapped on 20M -> Employer 3.7% = 740.000, Employee 2% = 400.000
        assert_eq!(bpjs.jht_employer_cents, 740_000_00);
        assert_eq!(bpjs.jht_employee_cents, 400_000_00);

        assert!(bpjs.total_employer_cents > 0);
        assert!(bpjs.total_employee_cents > 0);
    }

    #[test]
    fn test_efaktur_generation_and_vat_11_percent() {
        let doc = generate_efaktur(
            "01",
            "010.001-26.00000001",
            "2026-10-28",
            "0123456789012345",
            "3171012345670001",
            50_000_000_00, // DPP Rp 50 Juta
            1100,          // 11%
        );

        assert_eq!(doc.vat_cents, 5_500_000_00); // PPN Rp 5.5 Juta
        assert!(doc.qrcode_payload.contains("coretax.pajak.go.id"));
    }
}
