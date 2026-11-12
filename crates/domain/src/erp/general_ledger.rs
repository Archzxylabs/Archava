//! Double-Entry General Ledger (GL) & Financial Reporting Engine.
//!
//! Pure arithmetic helpers; these alone do not establish statutory IFRS / PSAK compliance.
//! Invariant: `Sum(Debits) == Sum(Credits)` down to minor integer cents.
//! Computes Trial Balance, Balance Sheet (Assets = Liabilities + Equity), and Profit & Loss.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccountClass {
    Asset,
    Liability,
    Equity,
    Revenue,
    CostOfGoodsSold,
    Expense,
}

impl AccountClass {
    /// Normal balance: Assets, COGS, and Expenses have normal DEBIT balances.
    /// Liabilities, Equity, and Revenue have normal CREDIT balances.
    pub fn is_normal_debit(&self) -> bool {
        matches!(
            self,
            AccountClass::Asset | AccountClass::CostOfGoodsSold | AccountClass::Expense
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub code: String,
    pub name: String,
    pub class: AccountClass,
    pub currency: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLine {
    pub account_code: String,
    pub debit_cents: i64,
    pub credit_cents: i64,
    pub memo: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub tenant_id: String,
    pub reference_no: String,
    pub entry_date: String,
    pub memo: String,
    pub lines: Vec<JournalLine>,
    pub is_posted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountBalance {
    pub code: String,
    pub name: String,
    pub class: AccountClass,
    pub total_debit_cents: i64,
    pub total_credit_cents: i64,
    pub net_balance_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialBalance {
    pub accounts: Vec<AccountBalance>,
    pub total_debits_cents: i64,
    pub total_credits_cents: i64,
    pub is_balanced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceSheet {
    pub total_assets_cents: i64,
    pub total_liabilities_cents: i64,
    pub total_equity_cents: i64,
    pub net_income_retained_cents: i64,
    pub total_liabilities_and_equity_cents: i64,
    pub is_balanced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfitLossStatement {
    pub total_revenue_cents: i64,
    pub total_cogs_cents: i64,
    pub gross_profit_cents: i64,
    pub total_expenses_cents: i64,
    pub net_operating_income_cents: i64,
}

/// Standard IFRS / PSAK Chart of Accounts template.
pub fn standard_chart_of_accounts() -> Vec<Account> {
    vec![
        // Assets (10000..19999)
        Account {
            code: "10100".into(),
            name: "Kas Utama (Cash on Hand)".into(),
            class: AccountClass::Asset,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "10200".into(),
            name: "Bank BCA Operasional".into(),
            class: AccountClass::Asset,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "11100".into(),
            name: "Piutang Usaha (Accounts Receivable)".into(),
            class: AccountClass::Asset,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "12100".into(),
            name: "Persediaan Barang (Inventory Asset)".into(),
            class: AccountClass::Asset,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "13100".into(),
            name: "PPN Masukan (Input VAT)".into(),
            class: AccountClass::Asset,
            currency: "IDR".into(),
            is_active: true,
        },
        // Liabilities (20000..29999)
        Account {
            code: "20100".into(),
            name: "Hutang Usaha (Accounts Payable)".into(),
            class: AccountClass::Liability,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "21100".into(),
            name: "Hutang PPN Keluaran (Output VAT Payable)".into(),
            class: AccountClass::Liability,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "21200".into(),
            name: "Hutang PPh 21 Karyawan".into(),
            class: AccountClass::Liability,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "21300".into(),
            name: "Hutang Iuran BPJS".into(),
            class: AccountClass::Liability,
            currency: "IDR".into(),
            is_active: true,
        },
        // Equity (30000..39999)
        Account {
            code: "30100".into(),
            name: "Modal Disetor (Paid-in Capital)".into(),
            class: AccountClass::Equity,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "30200".into(),
            name: "Laba Ditahan (Retained Earnings)".into(),
            class: AccountClass::Equity,
            currency: "IDR".into(),
            is_active: true,
        },
        // Revenue (40000..49999)
        Account {
            code: "40100".into(),
            name: "Pendapatan Penjualan (Sales Revenue)".into(),
            class: AccountClass::Revenue,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "40200".into(),
            name: "Pendapatan Jasa & Konsultasi".into(),
            class: AccountClass::Revenue,
            currency: "IDR".into(),
            is_active: true,
        },
        // COGS (50000..59999)
        Account {
            code: "50100".into(),
            name: "Beban Pokok Penjualan (Cost of Goods Sold)".into(),
            class: AccountClass::CostOfGoodsSold,
            currency: "IDR".into(),
            is_active: true,
        },
        // Operating Expenses (60000..69999)
        Account {
            code: "60100".into(),
            name: "Beban Gaji & Upah (Payroll Expense)".into(),
            class: AccountClass::Expense,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "60200".into(),
            name: "Beban Sewa & Gedung (Rent Expense)".into(),
            class: AccountClass::Expense,
            currency: "IDR".into(),
            is_active: true,
        },
        Account {
            code: "60300".into(),
            name: "Beban Utilitas & Internet".into(),
            class: AccountClass::Expense,
            currency: "IDR".into(),
            is_active: true,
        },
    ]
}

/// Validates that a journal entry strictly adheres to double-entry rules.
pub fn validate_journal_entry(entry: &JournalEntry) -> Result<(), &'static str> {
    if entry.lines.len() < 2 {
        return Err("A journal entry must contain at least 2 lines");
    }

    let mut total_debits: i64 = 0;
    let mut total_credits: i64 = 0;

    for line in &entry.lines {
        if line.debit_cents < 0 || line.credit_cents < 0 {
            return Err("Debit and credit amounts cannot be negative");
        }
        if line.debit_cents > 0 && line.credit_cents > 0 {
            return Err("A line cannot contain both debit and credit amounts");
        }
        if line.debit_cents == 0 && line.credit_cents == 0 {
            return Err("A line must have a non-zero debit or credit amount");
        }

        total_debits = total_debits
            .checked_add(line.debit_cents)
            .ok_or("Debit overflow")?;
        total_credits = total_credits
            .checked_add(line.credit_cents)
            .ok_or("Credit overflow")?;
    }

    if total_debits == 0 {
        return Err("Total journal entry value cannot be zero");
    }

    if total_debits != total_credits {
        return Err("Double-entry unbalanced: total debits must equal total credits");
    }

    Ok(())
}

/// Computes the Trial Balance across a Chart of Accounts and posted journal entries.
/// Enforces checked arithmetic across all accumulators to prevent silent release-mode overflow corruption (C06).
pub fn compute_trial_balance(accounts: &[Account], entries: &[JournalEntry]) -> TrialBalance {
    let mut balances = Vec::with_capacity(accounts.len());
    let mut total_debits: i64 = 0;
    let mut total_credits: i64 = 0;
    let mut overflow_detected = false;

    for acc in accounts {
        let mut d_sum: i64 = 0;
        let mut c_sum: i64 = 0;

        for entry in entries {
            if !entry.is_posted {
                continue;
            }
            for line in &entry.lines {
                if line.account_code == acc.code {
                    if let Some(next_d) = d_sum.checked_add(line.debit_cents) {
                        d_sum = next_d;
                    } else {
                        overflow_detected = true;
                    }

                    if let Some(next_c) = c_sum.checked_add(line.credit_cents) {
                        c_sum = next_c;
                    } else {
                        overflow_detected = true;
                    }
                }
            }
        }

        let net = if acc.class.is_normal_debit() {
            d_sum.checked_sub(c_sum).unwrap_or_else(|| {
                overflow_detected = true;
                0
            })
        } else {
            c_sum.checked_sub(d_sum).unwrap_or_else(|| {
                overflow_detected = true;
                0
            })
        };

        if let Some(next_total_d) = total_debits.checked_add(d_sum) {
            total_debits = next_total_d;
        } else {
            overflow_detected = true;
        }

        if let Some(next_total_c) = total_credits.checked_add(c_sum) {
            total_credits = next_total_c;
        } else {
            overflow_detected = true;
        }

        balances.push(AccountBalance {
            code: acc.code.clone(),
            name: acc.name.clone(),
            class: acc.class,
            total_debit_cents: d_sum,
            total_credit_cents: c_sum,
            net_balance_cents: net,
        });
    }

    let is_balanced = !overflow_detected && (total_debits == total_credits);

    TrialBalance {
        accounts: balances,
        total_debits_cents: if overflow_detected { -1 } else { total_debits },
        total_credits_cents: if overflow_detected { -1 } else { total_credits },
        is_balanced,
    }
}

/// Generates a Profit & Loss Statement (Income Statement).
pub fn generate_profit_loss(tb: &TrialBalance) -> Result<ProfitLossStatement, &'static str> {
    if !tb.is_balanced {
        return Err("Trial balance is not balanced or has arithmetic overflow");
    }

    let mut revenue: i64 = 0;
    let mut cogs: i64 = 0;
    let mut expenses: i64 = 0;

    for acc in &tb.accounts {
        match acc.class {
            AccountClass::Revenue => {
                revenue = revenue
                    .checked_add(acc.net_balance_cents)
                    .ok_or("Arithmetic overflow in revenue calculation")?;
            }
            AccountClass::CostOfGoodsSold => {
                cogs = cogs
                    .checked_add(acc.net_balance_cents)
                    .ok_or("Arithmetic overflow in COGS calculation")?;
            }
            AccountClass::Expense => {
                expenses = expenses
                    .checked_add(acc.net_balance_cents)
                    .ok_or("Arithmetic overflow in expenses calculation")?;
            }
            _ => {}
        }
    }

    let gross_profit = revenue
        .checked_sub(cogs)
        .ok_or("Arithmetic overflow in gross profit calculation")?;
    let net_operating_income = gross_profit
        .checked_sub(expenses)
        .ok_or("Arithmetic overflow in net operating income calculation")?;

    Ok(ProfitLossStatement {
        total_revenue_cents: revenue,
        total_cogs_cents: cogs,
        gross_profit_cents: gross_profit,
        total_expenses_cents: expenses,
        net_operating_income_cents: net_operating_income,
    })
}

/// Generates a Balance Sheet statement proving Assets = Liabilities + Equity + Net Income.
pub fn generate_balance_sheet(
    tb: &TrialBalance,
    pl: &ProfitLossStatement,
) -> Result<BalanceSheet, &'static str> {
    if !tb.is_balanced {
        return Err("Trial balance is not balanced or has arithmetic overflow");
    }

    let mut assets: i64 = 0;
    let mut liabilities: i64 = 0;
    let mut equity: i64 = 0;

    for acc in &tb.accounts {
        match acc.class {
            AccountClass::Asset => {
                assets = assets
                    .checked_add(acc.net_balance_cents)
                    .ok_or("Arithmetic overflow in assets calculation")?;
            }
            AccountClass::Liability => {
                liabilities = liabilities
                    .checked_add(acc.net_balance_cents)
                    .ok_or("Arithmetic overflow in liabilities calculation")?;
            }
            AccountClass::Equity => {
                equity = equity
                    .checked_add(acc.net_balance_cents)
                    .ok_or("Arithmetic overflow in equity calculation")?;
            }
            _ => {}
        }
    }

    let total_equity_with_income = equity
        .checked_add(pl.net_operating_income_cents)
        .ok_or("Arithmetic overflow in total equity calculation")?;
    let total_liabilities_and_equity = liabilities
        .checked_add(total_equity_with_income)
        .ok_or("Arithmetic overflow in liabilities + equity calculation")?;
    let is_balanced = assets == total_liabilities_and_equity;

    Ok(BalanceSheet {
        total_assets_cents: assets,
        total_liabilities_cents: liabilities,
        total_equity_cents: equity,
        net_income_retained_cents: pl.net_operating_income_cents,
        total_liabilities_and_equity_cents: total_liabilities_and_equity,
        is_balanced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_double_entry_balances() {
        let entry = JournalEntry {
            id: "je_001".into(),
            tenant_id: "tenant_alpha".into(),
            reference_no: "INV-2026-001".into(),
            entry_date: "2026-10-28".into(),
            memo: "Penjualan Barang Tunai".into(),
            lines: vec![
                JournalLine {
                    account_code: "10100".into(), // Cash
                    debit_cents: 1_110_000,
                    credit_cents: 0,
                    memo: Some("Kas masuk".into()),
                },
                JournalLine {
                    account_code: "40100".into(), // Sales Revenue
                    debit_cents: 0,
                    credit_cents: 1_000_000,
                    memo: Some("Penjualan".into()),
                },
                JournalLine {
                    account_code: "21100".into(), // PPN Keluaran 11%
                    debit_cents: 0,
                    credit_cents: 110_000,
                    memo: Some("PPN 11%".into()),
                },
            ],
            is_posted: true,
        };

        assert!(validate_journal_entry(&entry).is_ok());
    }

    #[test]
    fn test_unbalanced_entry_fails() {
        let entry = JournalEntry {
            id: "je_bad".into(),
            tenant_id: "tenant_alpha".into(),
            reference_no: "ERR-001".into(),
            entry_date: "2026-10-28".into(),
            memo: "Pencatatan Gagal".into(),
            lines: vec![
                JournalLine {
                    account_code: "10100".into(),
                    debit_cents: 500_000,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "40100".into(),
                    debit_cents: 0,
                    credit_cents: 400_000, // unbalanced by 100_000
                    memo: None,
                },
            ],
            is_posted: true,
        };

        assert_eq!(
            validate_journal_entry(&entry),
            Err("Double-entry unbalanced: total debits must equal total credits")
        );
    }

    #[test]
    fn test_complete_accounting_cycle_and_balance_sheet() {
        let coa = standard_chart_of_accounts();

        // 1. Owner injects Capital: Rp 50.000.000 to Bank BCA
        let e1 = JournalEntry {
            id: "je_capital".into(),
            tenant_id: "tenant_alpha".into(),
            reference_no: "CAP-001".into(),
            entry_date: "2026-10-01".into(),
            memo: "Setoran Modal Awal".into(),
            lines: vec![
                JournalLine {
                    account_code: "10200".into(),
                    debit_cents: 50_000_000,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "30100".into(),
                    debit_cents: 0,
                    credit_cents: 50_000_000,
                    memo: None,
                },
            ],
            is_posted: true,
        };

        // 2. Buy Inventory on Credit: Rp 20.000.000 (AP)
        let e2 = JournalEntry {
            id: "je_inv".into(),
            tenant_id: "tenant_alpha".into(),
            reference_no: "PO-001".into(),
            entry_date: "2026-10-05".into(),
            memo: "Pembelian Persediaan".into(),
            lines: vec![
                JournalLine {
                    account_code: "12100".into(),
                    debit_cents: 20_000_000,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "20100".into(),
                    debit_cents: 0,
                    credit_cents: 20_000_000,
                    memo: None,
                },
            ],
            is_posted: true,
        };

        // 3. Sell Inventory: Revenue Rp 15.000.000 (Cash in Bank), COGS Rp 8.000.000
        let e3 = JournalEntry {
            id: "je_sale".into(),
            tenant_id: "tenant_alpha".into(),
            reference_no: "SO-001".into(),
            entry_date: "2026-10-10".into(),
            memo: "Penjualan Produk".into(),
            lines: vec![
                JournalLine {
                    account_code: "10200".into(),
                    debit_cents: 15_000_000,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "40100".into(),
                    debit_cents: 0,
                    credit_cents: 15_000_000,
                    memo: None,
                },
                JournalLine {
                    account_code: "50100".into(),
                    debit_cents: 8_000_000,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "12100".into(),
                    debit_cents: 0,
                    credit_cents: 8_000_000,
                    memo: None,
                },
            ],
            is_posted: true,
        };

        // 4. Pay Rent Expense: Rp 3.000.000 from Bank
        let e4 = JournalEntry {
            id: "je_rent".into(),
            tenant_id: "tenant_alpha".into(),
            reference_no: "RENT-001".into(),
            entry_date: "2026-10-15".into(),
            memo: "Bayar Sewa Kantor".into(),
            lines: vec![
                JournalLine {
                    account_code: "60200".into(),
                    debit_cents: 3_000_000,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "10200".into(),
                    debit_cents: 0,
                    credit_cents: 3_000_000,
                    memo: None,
                },
            ],
            is_posted: true,
        };

        let entries = vec![e1, e2, e3, e4];
        let tb = compute_trial_balance(&coa, &entries);

        assert!(tb.is_balanced, "Trial Balance must balance");
        assert_eq!(tb.total_debits_cents, tb.total_credits_cents);

        let pl = generate_profit_loss(&tb).expect("P&L generation succeeds");
        assert_eq!(pl.total_revenue_cents, 15_000_000);
        assert_eq!(pl.total_cogs_cents, 8_000_000);
        assert_eq!(pl.gross_profit_cents, 7_000_000);
        assert_eq!(pl.total_expenses_cents, 3_000_000);
        assert_eq!(pl.net_operating_income_cents, 4_000_000);

        let bs = generate_balance_sheet(&tb, &pl).expect("Balance sheet generation succeeds");
        assert!(
            bs.is_balanced,
            "Balance sheet must satisfy Assets = Liabilities + Equity"
        );
        assert_eq!(bs.total_assets_cents, 74_000_000); // Bank 62m + Inv 12m
        assert_eq!(bs.total_liabilities_and_equity_cents, 74_000_000); // AP 20m + Capital 50m + Income 4m
    }

    #[test]
    fn test_c06_reproduction_arithmetic_overflow_cannot_balance() {
        let coa = standard_chart_of_accounts();
        // Construct two extreme entries each with i64::MAX
        let e1 = JournalEntry {
            id: "je_overflow_1".into(),
            tenant_id: "t1".into(),
            reference_no: "OVF-1".into(),
            entry_date: "2026-10-28".into(),
            memo: "Extreme 1".into(),
            lines: vec![
                JournalLine {
                    account_code: "10100".into(),
                    debit_cents: i64::MAX,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "40100".into(),
                    debit_cents: 0,
                    credit_cents: i64::MAX,
                    memo: None,
                },
            ],
            is_posted: true,
        };
        let e2 = JournalEntry {
            id: "je_overflow_2".into(),
            tenant_id: "t1".into(),
            reference_no: "OVF-2".into(),
            entry_date: "2026-10-28".into(),
            memo: "Extreme 2".into(),
            lines: vec![
                JournalLine {
                    account_code: "10100".into(),
                    debit_cents: i64::MAX,
                    credit_cents: 0,
                    memo: None,
                },
                JournalLine {
                    account_code: "40100".into(),
                    debit_cents: 0,
                    credit_cents: i64::MAX,
                    memo: None,
                },
            ],
            is_posted: true,
        };

        let tb = compute_trial_balance(&coa, &[e1, e2]);
        // C06 Assertion: Overflow must NEVER be considered balanced!
        assert!(
            !tb.is_balanced,
            "Trial balance with integer overflow must NOT report balanced"
        );
        // Financial statements MUST refuse unbalanced or overflowed trial balance
        assert!(generate_profit_loss(&tb).is_err());
        let dummy_pl = ProfitLossStatement {
            total_revenue_cents: 0,
            total_cogs_cents: 0,
            gross_profit_cents: 0,
            total_expenses_cents: 0,
            net_operating_income_cents: 0,
        };
        assert!(generate_balance_sheet(&tb, &dummy_pl).is_err());
    }
}
