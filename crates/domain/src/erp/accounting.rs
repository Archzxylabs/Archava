//! Explicit book setup and exact, reviewed financial commands. No jurisdiction certification.
pub mod aging;
pub mod bank;
pub mod clearing;
pub mod openings;
pub mod payables;
pub mod purchase_sources;
pub mod sources;
pub mod stock_sources;
pub mod terms;
use super::{
    accommodation::parse_calendar_date, purchasing::ValidationError, tax, valid_code, valid_uuid,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PERMISSIONS: &[&str] = &[
    "books.read",
    "books.write",
    "accounts.write",
    "journals.write",
    "journals.post",
    "periods.close",
    "periods.reopen",
    "reports.read",
    "audit.read",
    "bank.read",
    "bank.configure",
    "bank.import",
    "bank.reconcile",
    "payables.read",
    "payables.write",
    "clearing.read",
    "clearing.write",
    "terms.read",
    "terms.write",
    "tax.read",
    "tax.manage",
];
pub const MAX_MINOR: i64 = 9_000_000_000_000_000;

pub fn currencies() -> Value {
    serde_json::from_str(include_str!("../../embedded/accounting-currencies.v1.json"))
        .expect("the embedded currency catalogue is valid")
}
pub fn minor_units(code: &str) -> Option<u8> {
    currencies()["currencies"][code]
        .as_u64()
        .and_then(|v| u8::try_from(v).ok())
}
/// Decimal string wire format avoids JavaScript rounding and never interprets major units.
pub fn minor(value: &str) -> Result<i64, ValidationError> {
    if value.is_empty()
        || value.len() > 16
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(ValidationError("minorAmount"));
    }
    value
        .parse::<i64>()
        .ok()
        .filter(|v| (0..=MAX_MINOR).contains(v))
        .ok_or(ValidationError("minorAmount"))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountClass {
    Asset,
    Liability,
    Equity,
    Revenue,
    CostOfGoodsSold,
    Expense,
}
impl AccountClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Liability => "liability",
            Self::Equity => "equity",
            Self::Revenue => "revenue",
            Self::CostOfGoodsSold => "cost_of_goods_sold",
            Self::Expense => "expense",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CashRole {
    None,
    Cash,
    Operating,
    Investing,
    Financing,
}
impl CashRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Cash => "cash",
            Self::Operating => "operating",
            Self::Investing => "investing",
            Self::Financing => "financing",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceAuthority {
    Native,
    External,
}
impl SourceAuthority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Line {
    pub account_id: String,
    pub expected_account_revision: i32,
    pub debit_minor: String,
    pub credit_minor: String,
    pub memo: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "invoice_terms.set", rename_all = "camelCase")]
    InvoiceTermsSet {
        source_id: String,
        expected_document_revision: i32,
        expected_revision: i32,
        effective_from: String,
        condition: terms::Condition,
        reason: String,
    },
    #[serde(rename = "supplier_terms.set", rename_all = "camelCase")]
    SupplierTermsSet {
        source_id: String,
        expected_document_revision: i32,
        expected_revision: i32,
        effective_from: String,
        condition: terms::Condition,
        reason: String,
    },
    #[serde(rename = "clearing_settlement.draft", rename_all = "camelCase")]
    ClearingSettlementDraft {
        source_id: String,
        bank_row_id: String,
        code: String,
        clearing_account: sources::AccountRef,
        cash_account: sources::AccountRef,
        fee_account: Option<sources::AccountRef>,
        gross_minor: String,
        fee_minor: String,
        reason: String,
    },
    #[serde(rename = "opening_balance.draft", rename_all = "camelCase")]
    OpeningBalanceDraft {
        book_id: String,
        control: openings::Control,
        code: String,
        date: String,
        control_account: sources::AccountRef,
        offset_account: sources::AccountRef,
        amount_minor: String,
        reason: String,
    },
    /// One version of a company's own tax code, stored as entered: it does not
    /// apply until `tax_code.confirm`. Archava verifies no rate.
    #[serde(rename = "tax_code.create", rename_all = "camelCase")]
    TaxCodeCreate {
        book_id: String,
        code: String,
        kind: tax::Kind,
        rate: tax::Fraction,
        #[serde(default = "tax::Fraction::one")]
        base: tax::Fraction,
        control_account: sources::AccountRef,
        effective_from: String,
        reason: String,
    },
    /// A reviewer confirms an entered version; from then on it applies.
    #[serde(rename = "tax_code.confirm", rename_all = "camelCase")]
    TaxCodeConfirm {
        id: String,
        expected_revision: i32,
        reason: String,
    },
    /// Retire a confirmed version (`effectiveTo` is its last day) or withdraw
    /// an entered one, which never applied and takes no last day.
    #[serde(rename = "tax_code.retire", rename_all = "camelCase")]
    TaxCodeRetire {
        id: String,
        expected_revision: i32,
        #[serde(default)]
        effective_to: Option<String>,
        reason: String,
    },
    #[serde(rename = "payable_payment.draft", rename_all = "camelCase")]
    PayablePaymentDraft {
        source_id: String,
        expected_document_revision: i32,
        bank_row_id: String,
        code: String,
        payable_account: sources::AccountRef,
        cash_account: sources::AccountRef,
        amount_minor: String,
        reason: String,
    },
    #[serde(rename = "payable_return.draft", rename_all = "camelCase")]
    PayableReturnDraft {
        original_payment_id: String,
        expected_journal_revision: i32,
        bank_row_id: String,
        code: String,
        payable_account: sources::AccountRef,
        cash_account: sources::AccountRef,
        amount_minor: String,
        reason: String,
    },
    #[serde(rename = "bank_policy.set", rename_all = "camelCase")]
    BankPolicySet {
        bank_account_id: String,
        expected_revision: i32,
        from_date: String,
        independent_review: bool,
        reason: String,
    },
    #[serde(rename = "bank_reconciliation.confirm", rename_all = "camelCase")]
    BankReconciliationConfirm {
        bank_policy_id: String,
        expected_policy_revision: i32,
        period_id: String,
        expected_period_revision: i32,
        expected_check_hash: String,
        reason: String,
    },
    #[serde(rename = "bank_account.create", rename_all = "camelCase")]
    BankAccountCreate {
        book_id: String,
        code: String,
        name: String,
        account: sources::AccountRef,
        reason: String,
    },
    #[serde(rename = "bank_statement.import", rename_all = "camelCase")]
    BankStatementImport {
        bank_account_id: String,
        account: sources::AccountRef,
        statement: bank::Statement,
        reason: String,
    },
    #[serde(rename = "bank_allocation.record", rename_all = "camelCase")]
    BankAllocationRecord {
        bank_row_id: String,
        journal_id: String,
        journal_line_ordinal: i32,
        expected_journal_revision: i32,
        account: sources::AccountRef,
        amount_minor: String,
        reason: String,
    },
    #[serde(rename = "bank_allocation.reverse", rename_all = "camelCase")]
    BankAllocationReverse {
        id: String,
        expected_revision: i32,
        reason: String,
    },
    #[serde(rename = "stock_policy.set", rename_all = "camelCase")]
    StockPolicySet {
        book_id: String,
        expected_revision: i32,
        currency_catalogue: String,
        source_unit: String,
        cogs_account: sources::AccountRef,
        reason: String,
    },
    #[serde(rename = "stock_source.draft", rename_all = "camelCase")]
    StockSourceDraft {
        source_id: String,
        code: String,
        accounts: Vec<sources::AccountRef>,
        reason: String,
    },
    #[serde(rename = "purchase_policy.set", rename_all = "camelCase")]
    PurchasePolicySet {
        book_id: String,
        expected_revision: i32,
        currency_catalogue: String,
        source_unit: String,
        mappings: Vec<purchase_sources::PurchaseMapping>,
        reason: String,
    },
    #[serde(rename = "purchase_source.draft", rename_all = "camelCase")]
    PurchaseSourceDraft {
        source_id: String,
        code: String,
        accounts: Vec<sources::AccountRef>,
        reason: String,
    },
    #[serde(rename = "invoice_policy.set", rename_all = "camelCase")]
    InvoicePolicySet {
        book_id: String,
        expected_revision: i32,
        currency_catalogue: String,
        source_unit: String,
        mappings: Vec<sources::InvoiceMapping>,
        reason: String,
    },
    #[serde(rename = "invoice_source.draft", rename_all = "camelCase")]
    InvoiceSourceDraft {
        source_id: String,
        code: String,
        accounts: Vec<sources::AccountRef>,
        reason: String,
    },
    #[serde(rename = "book.create", rename_all = "camelCase")]
    BookCreate {
        company_id: String,
        expected_company_revision: i32,
        code: String,
        name: String,
        currency: String,
        source_authority: SourceAuthority,
        external_system: Option<String>,
        independent_posting: bool,
        reason: String,
    },
    #[serde(rename = "account.create", rename_all = "camelCase")]
    AccountCreate {
        book_id: String,
        code: String,
        name: String,
        class: AccountClass,
        cash_role: CashRole,
        reason: String,
    },
    #[serde(rename = "account.set_active", rename_all = "camelCase")]
    AccountSetActive {
        id: String,
        expected_revision: i32,
        active: bool,
        reason: String,
    },
    #[serde(rename = "period.open", rename_all = "camelCase")]
    PeriodOpen {
        book_id: String,
        code: String,
        start_date: String,
        end_date: String,
        reason: String,
    },
    #[serde(rename = "period.close", rename_all = "camelCase")]
    PeriodClose {
        id: String,
        expected_revision: i32,
        reason: String,
    },
    #[serde(rename = "period.reopen", rename_all = "camelCase")]
    PeriodReopen {
        id: String,
        expected_revision: i32,
        reason: String,
    },
    #[serde(rename = "journal.draft", rename_all = "camelCase")]
    JournalDraft {
        book_id: String,
        code: String,
        date: String,
        source_ref: String,
        memo: String,
        lines: Vec<Line>,
        reason: String,
    },
    #[serde(rename = "journal.post", rename_all = "camelCase")]
    JournalPost {
        id: String,
        expected_revision: i32,
        reason: String,
    },
    #[serde(rename = "journal.void", rename_all = "camelCase")]
    JournalVoid {
        id: String,
        expected_revision: i32,
        reason: String,
    },
    #[serde(rename = "journal.reverse", rename_all = "camelCase")]
    JournalReverse {
        id: String,
        expected_revision: i32,
        code: String,
        date: String,
        source_ref: String,
        reason: String,
    },
}
impl Command {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::InvoiceTermsSet { .. } => "invoice_terms.set",
            Self::SupplierTermsSet { .. } => "supplier_terms.set",
            Self::ClearingSettlementDraft { .. } => "clearing_settlement.draft",
            Self::OpeningBalanceDraft { .. } => "opening_balance.draft",
            Self::TaxCodeCreate { .. } => "tax_code.create",
            Self::TaxCodeConfirm { .. } => "tax_code.confirm",
            Self::TaxCodeRetire { .. } => "tax_code.retire",
            Self::PayablePaymentDraft { .. } => "payable_payment.draft",
            Self::PayableReturnDraft { .. } => "payable_return.draft",
            Self::BankPolicySet { .. } => "bank_policy.set",
            Self::BankReconciliationConfirm { .. } => "bank_reconciliation.confirm",
            Self::BankAccountCreate { .. } => "bank_account.create",
            Self::BankStatementImport { .. } => "bank_statement.import",
            Self::BankAllocationRecord { .. } => "bank_allocation.record",
            Self::BankAllocationReverse { .. } => "bank_allocation.reverse",
            Self::StockPolicySet { .. } => "stock_policy.set",
            Self::StockSourceDraft { .. } => "stock_source.draft",
            Self::PurchasePolicySet { .. } => "purchase_policy.set",
            Self::PurchaseSourceDraft { .. } => "purchase_source.draft",
            Self::InvoicePolicySet { .. } => "invoice_policy.set",
            Self::InvoiceSourceDraft { .. } => "invoice_source.draft",
            Self::BookCreate { .. } => "book.create",
            Self::AccountCreate { .. } => "account.create",
            Self::AccountSetActive { .. } => "account.set_active",
            Self::PeriodOpen { .. } => "period.open",
            Self::PeriodClose { .. } => "period.close",
            Self::PeriodReopen { .. } => "period.reopen",
            Self::JournalDraft { .. } => "journal.draft",
            Self::JournalPost { .. } => "journal.post",
            Self::JournalVoid { .. } => "journal.void",
            Self::JournalReverse { .. } => "journal.reverse",
        }
    }
    pub fn permission(&self) -> &'static str {
        match self {
            Self::InvoiceTermsSet { .. } | Self::SupplierTermsSet { .. } => "terms.write",
            Self::ClearingSettlementDraft { .. } => "clearing.write",
            Self::TaxCodeCreate { .. }
            | Self::TaxCodeConfirm { .. }
            | Self::TaxCodeRetire { .. } => "tax.manage",
            Self::PayablePaymentDraft { .. } | Self::PayableReturnDraft { .. } => "payables.write",
            Self::BankPolicySet { .. } => "bank.configure",
            Self::BankReconciliationConfirm { .. } => "bank.reconcile",
            Self::BankAccountCreate { .. } => "bank.configure",
            Self::BankStatementImport { .. } => "bank.import",
            Self::BankAllocationRecord { .. } | Self::BankAllocationReverse { .. } => {
                "bank.reconcile"
            }
            Self::StockPolicySet { .. } => "books.write",
            Self::StockSourceDraft { .. } => "journals.write",
            Self::PurchasePolicySet { .. } => "books.write",
            Self::PurchaseSourceDraft { .. } => "journals.write",
            Self::InvoicePolicySet { .. } => "books.write",
            Self::InvoiceSourceDraft { .. } => "journals.write",
            Self::BookCreate { .. } => "books.write",
            Self::AccountCreate { .. } | Self::AccountSetActive { .. } => "accounts.write",
            Self::PeriodOpen { .. } | Self::PeriodClose { .. } => "periods.close",
            Self::PeriodReopen { .. } => "periods.reopen",
            Self::JournalPost { .. } => "journals.post",
            _ => "journals.write",
        }
    }
    pub fn reason(&self) -> &str {
        match self {
            Self::InvoiceTermsSet { reason, .. } | Self::SupplierTermsSet { reason, .. } => reason,
            Self::ClearingSettlementDraft { reason, .. } => reason,
            Self::OpeningBalanceDraft { reason, .. }
            | Self::TaxCodeCreate { reason, .. }
            | Self::TaxCodeConfirm { reason, .. }
            | Self::TaxCodeRetire { reason, .. }
            | Self::PayablePaymentDraft { reason, .. }
            | Self::PayableReturnDraft { reason, .. }
            | Self::BankPolicySet { reason, .. }
            | Self::BankReconciliationConfirm { reason, .. }
            | Self::BankAccountCreate { reason, .. }
            | Self::BankStatementImport { reason, .. }
            | Self::BankAllocationRecord { reason, .. }
            | Self::BankAllocationReverse { reason, .. }
            | Self::StockPolicySet { reason, .. }
            | Self::StockSourceDraft { reason, .. }
            | Self::PurchasePolicySet { reason, .. }
            | Self::PurchaseSourceDraft { reason, .. }
            | Self::InvoicePolicySet { reason, .. }
            | Self::InvoiceSourceDraft { reason, .. }
            | Self::BookCreate { reason, .. }
            | Self::AccountCreate { reason, .. }
            | Self::AccountSetActive { reason, .. }
            | Self::PeriodOpen { reason, .. }
            | Self::PeriodClose { reason, .. }
            | Self::PeriodReopen { reason, .. }
            | Self::JournalDraft { reason, .. }
            | Self::JournalPost { reason, .. }
            | Self::JournalVoid { reason, .. }
            | Self::JournalReverse { reason, .. } => reason,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mutation {
    pub request_id: String,
    pub command: Command,
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && value.chars().count() <= max
        && !value.chars().any(|c| c.is_control())
}
fn revision(value: i32) -> bool {
    (1..i32::MAX).contains(&value)
}
pub fn parse(raw: &Value) -> Result<Mutation, ValidationError> {
    let mut v = raw.clone();
    let top = v.as_object_mut().ok_or(ValidationError("mutation"))?;
    if let Some(id) = top.get_mut("requestId") {
        canonical_id(id)?;
    }
    let c = top
        .get_mut("command")
        .and_then(Value::as_object_mut)
        .ok_or(ValidationError("command"))?;
    for key in [
        "id",
        "bookId",
        "companyId",
        "sourceId",
        "bankAccountId",
        "bankRowId",
        "journalId",
        "bankPolicyId",
        "periodId",
        "originalPaymentId",
    ] {
        if let Some(id) = c.get_mut(key) {
            canonical_id(id)?;
        }
    }
    for key in ["lines", "mappings", "accounts"] {
        if let Some(lines) = c.get_mut(key).and_then(Value::as_array_mut) {
            for line in lines {
                if let Some(id) = line.get_mut("accountId") {
                    canonical_id(id)?;
                }
            }
        }
    }
    for field in [
        "cogsAccount",
        "account",
        "payableAccount",
        "cashAccount",
        "clearingAccount",
        "feeAccount",
        "controlAccount",
        "offsetAccount",
    ] {
        if let Some(account) = c.get_mut(field).and_then(Value::as_object_mut) {
            if let Some(id) = account.get_mut("accountId") {
                canonical_id(id)?;
            }
        }
    }
    let m: Mutation = serde_json::from_value(v).map_err(|_| ValidationError("mutation"))?;
    if !valid_uuid(&m.request_id) || !text(m.command.reason(), 2000) {
        return Err(ValidationError("reason"));
    }
    let good = match &m.command {
        Command::InvoiceTermsSet {
            source_id,
            expected_document_revision,
            expected_revision,
            effective_from,
            condition,
            ..
        }
        | Command::SupplierTermsSet {
            source_id,
            expected_document_revision,
            expected_revision,
            effective_from,
            condition,
            ..
        } => terms::valid_proposal(
            source_id,
            *expected_document_revision,
            *expected_revision,
            effective_from,
            condition,
        ),
        Command::ClearingSettlementDraft {
            source_id,
            bank_row_id,
            code,
            clearing_account,
            cash_account,
            fee_account,
            gross_minor,
            fee_minor,
            ..
        } => {
            let mut accounts = vec![clearing_account.clone(), cash_account.clone()];
            if let Some(fee) = fee_account {
                accounts.push(fee.clone())
            }
            valid_uuid(source_id) && valid_uuid(bank_row_id) && valid_code(code)
                && sources::valid_refs(&accounts,accounts.len())
                && minor(fee_minor).is_ok_and(|fee|(fee>0)==fee_account.is_some())
                // Either source direction must be representable. The actual
                // immutable event selects it again inside the transaction.
                && (clearing::legs(clearing::Effect::Collection,gross_minor,fee_minor).is_ok()
                    || clearing::legs(clearing::Effect::Refund,gross_minor,fee_minor).is_ok())
        }
        Command::OpeningBalanceDraft {
            book_id,
            control,
            code,
            date,
            control_account,
            offset_account,
            amount_minor,
            ..
        } => {
            valid_uuid(book_id)
                && valid_code(code)
                && parse_calendar_date(date).is_some()
                && sources::valid_refs(&[control_account.clone(), offset_account.clone()], 2)
                && openings::legs(*control, amount_minor).is_ok()
        }
        Command::TaxCodeCreate {
            book_id,
            code,
            rate,
            base,
            control_account,
            effective_from,
            ..
        } => {
            valid_uuid(book_id)
                && valid_code(code)
                && tax::TaxRule::new(*rate, *base).is_ok()
                && sources::valid_refs(std::slice::from_ref(control_account), 1)
                && parse_calendar_date(effective_from).is_some()
        }
        Command::TaxCodeRetire {
            id,
            expected_revision,
            effective_to,
            ..
        } => {
            valid_uuid(id)
                && revision(*expected_revision)
                && effective_to
                    .as_ref()
                    .is_none_or(|date| parse_calendar_date(date).is_some())
        }
        Command::PayablePaymentDraft {
            source_id,
            expected_document_revision,
            bank_row_id,
            code,
            payable_account,
            cash_account,
            amount_minor,
            ..
        } => {
            valid_uuid(source_id)
                && revision(*expected_document_revision)
                && valid_uuid(bank_row_id)
                && valid_code(code)
                && sources::valid_refs(&[payable_account.clone(), cash_account.clone()], 2)
                && payables::legs(payables::Effect::Payment, amount_minor).is_ok()
        }
        Command::PayableReturnDraft {
            original_payment_id,
            expected_journal_revision,
            bank_row_id,
            code,
            payable_account,
            cash_account,
            amount_minor,
            ..
        } => {
            valid_uuid(original_payment_id)
                && revision(*expected_journal_revision)
                && valid_uuid(bank_row_id)
                && valid_code(code)
                && sources::valid_refs(&[payable_account.clone(), cash_account.clone()], 2)
                && payables::legs(payables::Effect::Return, amount_minor).is_ok()
        }
        Command::BankPolicySet {
            bank_account_id,
            expected_revision,
            from_date,
            ..
        } => {
            valid_uuid(bank_account_id)
                && (0..i32::MAX).contains(expected_revision)
                && parse_calendar_date(from_date).is_some()
        }
        Command::BankReconciliationConfirm {
            bank_policy_id,
            expected_policy_revision,
            period_id,
            expected_period_revision,
            expected_check_hash,
            ..
        } => {
            valid_uuid(bank_policy_id)
                && valid_uuid(period_id)
                && revision(*expected_policy_revision)
                && revision(*expected_period_revision)
                && expected_check_hash.len() == 64
                && expected_check_hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        Command::BankAccountCreate {
            book_id,
            code,
            name,
            account,
            ..
        } => {
            valid_uuid(book_id)
                && valid_code(code)
                && text(name, 160)
                && sources::valid_refs(std::slice::from_ref(account), 1)
        }
        Command::BankStatementImport {
            bank_account_id,
            account,
            statement,
            ..
        } => {
            valid_uuid(bank_account_id)
                && sources::valid_refs(std::slice::from_ref(account), 1)
                && bank::statement_totals(statement, &statement.currency).is_ok()
        }
        Command::BankAllocationRecord {
            bank_row_id,
            journal_id,
            journal_line_ordinal,
            expected_journal_revision,
            account,
            amount_minor,
            ..
        } => {
            valid_uuid(bank_row_id)
                && valid_uuid(journal_id)
                && (1..=100).contains(journal_line_ordinal)
                && revision(*expected_journal_revision)
                && sources::valid_refs(std::slice::from_ref(account), 1)
                && minor(amount_minor).is_ok_and(|n| n > 0)
        }
        Command::BankAllocationReverse {
            id,
            expected_revision,
            ..
        } => valid_uuid(id) && *expected_revision == 1,
        Command::StockPolicySet {
            book_id,
            expected_revision,
            currency_catalogue,
            source_unit,
            cogs_account,
            ..
        } => {
            valid_uuid(book_id)
                && (0..i32::MAX).contains(expected_revision)
                && currency_catalogue == sources::CURRENCY_CATALOGUE
                && sources::source_unit_factor(source_unit).is_some()
                && sources::valid_refs(std::slice::from_ref(cogs_account), 1)
        }
        Command::StockSourceDraft {
            source_id,
            code,
            accounts,
            ..
        } => {
            valid_uuid(source_id)
                && valid_code(code)
                && (2..=100).contains(&accounts.len())
                && sources::valid_refs(accounts, accounts.len())
        }
        Command::PurchasePolicySet {
            book_id,
            expected_revision,
            currency_catalogue,
            source_unit,
            mappings,
            ..
        } => {
            valid_uuid(book_id)
                && (0..i32::MAX).contains(expected_revision)
                && currency_catalogue == sources::CURRENCY_CATALOGUE
                && sources::source_unit_factor(source_unit).is_some()
                && purchase_sources::valid_mappings(mappings)
        }
        Command::PurchaseSourceDraft {
            source_id,
            code,
            accounts,
            ..
        } => {
            valid_uuid(source_id)
                && valid_code(code)
                && (2..=3).contains(&accounts.len())
                && sources::valid_refs(accounts, accounts.len())
        }
        Command::InvoicePolicySet {
            book_id,
            expected_revision,
            currency_catalogue,
            source_unit,
            mappings,
            ..
        } => {
            valid_uuid(book_id)
                && (0..i32::MAX).contains(expected_revision)
                && currency_catalogue == sources::CURRENCY_CATALOGUE
                && sources::source_unit_factor(source_unit).is_some()
                && sources::valid_mappings(mappings)
        }
        Command::InvoiceSourceDraft {
            source_id,
            code,
            accounts,
            ..
        } => {
            // Two reviewed accounts, or three when the source carries tax:
            // which one the source needs is decided against the stored source.
            valid_uuid(source_id)
                && valid_code(code)
                && (2..=3).contains(&accounts.len())
                && sources::valid_refs(accounts, accounts.len())
        }
        Command::BookCreate {
            company_id,
            expected_company_revision,
            code,
            name,
            currency,
            source_authority,
            external_system,
            ..
        } => {
            valid_uuid(company_id)
                && revision(*expected_company_revision)
                && valid_code(code)
                && text(name, 160)
                && minor_units(currency).is_some()
                && match source_authority {
                    SourceAuthority::Native => external_system.is_none(),
                    SourceAuthority::External => {
                        external_system.as_ref().is_some_and(|s| valid_code(s))
                    }
                }
        }
        Command::AccountCreate {
            book_id,
            code,
            name,
            class,
            cash_role,
            ..
        } => {
            valid_uuid(book_id)
                && valid_code(code)
                && text(name, 160)
                && (*cash_role != CashRole::Cash || *class == AccountClass::Asset)
        }
        Command::PeriodOpen {
            book_id,
            code,
            start_date,
            end_date,
            ..
        } => {
            valid_uuid(book_id)
                && valid_code(code)
                && parse_calendar_date(start_date).is_some()
                && parse_calendar_date(end_date).is_some()
                && start_date <= end_date
        }
        Command::JournalDraft {
            book_id,
            code,
            date,
            source_ref,
            memo,
            lines,
            ..
        } => {
            valid_uuid(book_id)
                && valid_code(code)
                && parse_calendar_date(date).is_some()
                && text(source_ref, 200)
                && !source_ref.starts_with("billing:")
                && !source_ref.starts_with("stock-cost:")
                && text(memo, 500)
                && balanced(lines)
        }
        Command::JournalReverse {
            id,
            expected_revision,
            code,
            date,
            source_ref,
            ..
        } => {
            valid_uuid(id)
                && revision(*expected_revision)
                && valid_code(code)
                && parse_calendar_date(date).is_some()
                && text(source_ref, 200)
        }
        Command::AccountSetActive {
            id,
            expected_revision,
            ..
        }
        | Command::TaxCodeConfirm {
            id,
            expected_revision,
            ..
        }
        | Command::PeriodClose {
            id,
            expected_revision,
            ..
        }
        | Command::PeriodReopen {
            id,
            expected_revision,
            ..
        }
        | Command::JournalPost {
            id,
            expected_revision,
            ..
        }
        | Command::JournalVoid {
            id,
            expected_revision,
            ..
        } => valid_uuid(id) && revision(*expected_revision),
    };
    if !good {
        return Err(ValidationError("command"));
    }
    Ok(m)
}
fn canonical_id(v: &mut Value) -> Result<(), ValidationError> {
    let id = v
        .as_str()
        .filter(|id| valid_uuid(id))
        .ok_or(ValidationError("id"))?;
    *v = Value::String(id.to_ascii_lowercase());
    Ok(())
}
pub fn balanced(lines: &[Line]) -> bool {
    if !(2..=100).contains(&lines.len()) {
        return false;
    }
    let mut totals = (0_i128, 0_i128);
    for line in lines {
        let (Ok(d), Ok(c)) = (minor(&line.debit_minor), minor(&line.credit_minor)) else {
            return false;
        };
        if !valid_uuid(&line.account_id)
            || !revision(line.expected_account_revision)
            || ((d == 0) == (c == 0))
            || line.memo.chars().count() > 500
            || line.memo.chars().any(|c| c.is_control())
        {
            return false;
        }
        totals.0 += i128::from(d);
        totals.1 += i128::from(c);
    }
    totals.0 > 0 && totals.0 == totals.1
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub request_id: String,
    #[serde(rename = "type")]
    pub operation: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub book_id: String,
    pub company_id: String,
    pub revision: i32,
    pub replayed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_journal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_bank_allocation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank_check_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_payable_payment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank_row_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_terms: Option<Value>,
    /// The state a tax code command left its version in: `entered` (not
    /// confirmed, does not apply), `confirmed`, `withdrawn` or `retired`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_code_state: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn bank_receipts_keep_old_history_readable_and_round_trip_the_original_match() {
        let legacy = json!({"requestId":"request","type":"bank_allocation.reverse","resourceKind":"bank_unmatch","resourceId":"unmatch","bookId":"book","companyId":"company","revision":1,"replayed":true});
        let decoded: Receipt = serde_json::from_value(legacy.clone()).unwrap();
        assert!(decoded.original_bank_allocation_id.is_none());
        let mut current = legacy;
        current["originalBankAllocationId"] = json!("original-allocation");
        let decoded: Receipt = serde_json::from_value(current.clone()).unwrap();
        assert_eq!(
            decoded.original_bank_allocation_id.as_deref(),
            Some("original-allocation")
        );
        assert_eq!(serde_json::to_value(decoded).unwrap(), current);
    }
    #[test]
    fn explicit_currency_units_and_exact_amounts_do_not_use_float_or_provider_defaults() {
        assert_eq!(minor_units("IDR"), Some(2));
        assert_eq!(minor_units("JPY"), Some(0));
        assert_eq!(minor_units("KWD"), Some(3));
        assert_eq!(minor_units("XXX"), None);
        for bad in ["01", "+1", "-1", "1.0", "1e3", " 1", "9000000000000001"] {
            assert!(minor(bad).is_err());
        }
        assert_eq!(minor("9000000000000000").unwrap(), MAX_MINOR);
    }
    #[test]
    fn an_invoice_source_draft_names_two_reviewed_accounts_or_three_with_tax() {
        let id = |n: u8| format!("a0000000-0000-4000-8000-0000000000{n:02}");
        let account = |n: u8| json!({"accountId":id(n),"expectedAccountRevision":1});
        let draft = |accounts: Vec<Value>| {
            parse(
                &json!({"requestId":id(90),"command":{"type":"invoice_source.draft","sourceId":id(91),
            "code":"ISSUE","accounts":accounts,"reason":"Reviewed source"}}),
            )
        };
        let two = draft(vec![account(1), account(2)]).unwrap();
        assert_eq!(two.command.operation(), "invoice_source.draft");
        assert_eq!(two.command.permission(), "journals.write");
        // The third is the tax control account of a taxed source.
        assert!(draft(vec![account(1), account(2), account(3)]).is_ok());
        assert!(draft(vec![account(1)]).is_err());
        assert!(draft(vec![account(1), account(2), account(3), account(4)]).is_err());
        // Every named account is a different one, at a positive revision.
        assert!(draft(vec![account(1), account(2), account(2)]).is_err());
        assert!(draft(vec![account(1), account(1)]).is_err());
        assert!(draft(vec![
            account(1),
            account(2),
            json!({"accountId":id(3),"expectedAccountRevision":0})
        ])
        .is_err());
        // No role, amount or tax can be stated: the stored source decides them.
        assert!(draft(vec![
            account(1),
            account(2),
            json!({"accountId":id(3),"expectedAccountRevision":1,"role":"tax"})
        ])
        .is_err());
    }
    #[test]
    fn tax_code_commands_take_integer_fractions_and_no_claimed_people() {
        let id = "A0000000-0000-4000-8000-000000000001";
        let raw = json!({"requestId":id,"command":{"type":"tax_code.create","bookId":id,"code":"VAT-OUT","kind":"output",
            "rate":{"numerator":12,"denominator":100},"base":{"numerator":11,"denominator":12},
            "controlAccount":{"accountId":id,"expectedAccountRevision":1},"effectiveFrom":"2026-11-01","reason":"Entered by the company"}});
        let decoded = parse(&raw).unwrap();
        assert_eq!(decoded.command.operation(), "tax_code.create");
        assert_eq!(decoded.command.permission(), "tax.manage");
        let canonical = serde_json::to_value(&decoded).unwrap();
        assert_eq!(canonical["command"]["bookId"], id.to_ascii_lowercase());
        assert_eq!(
            canonical["command"]["controlAccount"]["accountId"],
            id.to_ascii_lowercase()
        );
        // The base fraction is 1/1 unless the company enters another.
        let mut whole = raw.clone();
        whole["command"].as_object_mut().unwrap().remove("base");
        let whole = serde_json::to_value(parse(&whole).unwrap()).unwrap();
        assert_eq!(
            whole["command"]["base"],
            json!({"numerator":1,"denominator":1})
        );
        for (field, value) in [
            ("createdBy", json!("owner")),
            ("reviewedBy", json!("owner")),
            ("reviewerId", json!("owner")),
            ("version", json!(2)),
            ("effectiveTo", json!("2026-12-31")),
            ("ratePercent", json!(11)),
        ] {
            let mut forged = raw.clone();
            forged["command"][field] = value;
            assert!(parse(&forged).is_err(), "{field}");
        }
        for (field, value) in [
            ("kind", json!("withholding")),
            ("code", json!("lower case")),
            ("effectiveFrom", json!("2026-02-30")),
            ("effectiveFrom", json!("1999-12-31")),
            ("rate", json!(0.11)),
            ("rate", json!({"numerator":101,"denominator":100})),
            ("rate", json!({"numerator":-1,"denominator":100})),
            ("rate", json!({"numerator":1,"denominator":0})),
            ("rate", json!({"numerator":1,"denominator":1_000_001})),
            ("base", json!({"numerator":0,"denominator":1})),
            ("base", json!({"numerator":13,"denominator":12})),
            (
                "controlAccount",
                json!({"accountId":id,"expectedAccountRevision":0}),
            ),
        ] {
            let mut invalid = raw.clone();
            invalid["command"][field] = value;
            assert!(parse(&invalid).is_err(), "{field}");
        }
        // A zero rate is a legitimate code.
        let mut zero = raw.clone();
        zero["command"]["rate"] = json!({"numerator":0,"denominator":100});
        assert!(parse(&zero).is_ok());

        let retire = json!({"requestId":id,"command":{"type":"tax_code.retire","id":id,"expectedRevision":1,"effectiveTo":"2026-12-31","reason":"The rate changes next year"}});
        let decoded = parse(&retire).unwrap();
        assert_eq!(decoded.command.operation(), "tax_code.retire");
        assert_eq!(decoded.command.permission(), "tax.manage");
        for (field, value) in [
            ("expectedRevision", json!(0)),
            ("effectiveTo", json!("2026-13-01")),
            ("rate", json!({"numerator":1,"denominator":100})),
            ("retiredBy", json!("owner")),
        ] {
            let mut invalid = retire.clone();
            invalid["command"][field] = value;
            assert!(parse(&invalid).is_err(), "{field}");
        }
        // Withdrawing an entered version names no last day.
        let mut withdraw = retire.clone();
        withdraw["command"]
            .as_object_mut()
            .unwrap()
            .remove("effectiveTo");
        assert!(parse(&withdraw).is_ok());

        let confirm = json!({"requestId":id,"command":{"type":"tax_code.confirm","id":id,"expectedRevision":1,"reason":"Checked against the company's tax advice"}});
        let decoded = parse(&confirm).unwrap();
        assert_eq!(decoded.command.operation(), "tax_code.confirm");
        assert_eq!(decoded.command.permission(), "tax.manage");
        for (field, value) in [
            ("expectedRevision", json!(0)),
            ("reviewedBy", json!("owner")),
            ("reviewerId", json!("owner")),
            ("rate", json!({"numerator":1,"denominator":100})),
        ] {
            let mut invalid = confirm.clone();
            invalid["command"][field] = value;
            assert!(parse(&invalid).is_err(), "{field}");
        }
    }
    #[test]
    fn journal_wire_requires_exact_balanced_nonempty_one_sided_lines_and_no_claimed_actor() {
        let id = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
        let raw = json!({"requestId":id,"command":{"type":"journal.draft","bookId":id,"code":"OPENING","date":"2028-02-29","sourceRef":"Reviewed opening","memo":"Opening cash","reason":"Review","lines":[{"accountId":id,"expectedAccountRevision":1,"debitMinor":"100","creditMinor":"0","memo":""},{"accountId":id,"expectedAccountRevision":1,"debitMinor":"0","creditMinor":"100","memo":""}]}});
        assert!(parse(&raw).is_ok());
        for (key, val) in [
            ("date", json!("2027-02-29")),
            ("actorId", json!(id)),
            ("approved", json!(true)),
        ] {
            let mut b = raw.clone();
            b["command"][key] = val;
            assert!(parse(&b).is_err());
        }
        for (key, val) in [
            ("debitMinor", json!(100)),
            ("debitMinor", json!("101")),
            ("creditMinor", json!("1")),
            ("expectedAccountRevision", json!(0)),
        ] {
            let mut b = raw.clone();
            b["command"]["lines"][0][key] = val;
            assert!(parse(&b).is_err());
        }
    }
}
