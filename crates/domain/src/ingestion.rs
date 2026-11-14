//! AI Legacy Data Ingestion & Schema Reconciliation Engine (`archava-ingestion`).
//!
//! Provides automated schema reconciliation for messy customer legacy data (Excel, CSV, legacy ERP exports),
//! column fuzzy matching, data cleaning/normalization (IDR currencies, phone numbers),
//! diff preview cards for Ava consultation approval, and cryptographic SHA-256 batch integrity.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

use crate::erp::{trimmed, valid_uuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetEntityKind {
    CatalogProducts,
    CrmContacts,
    InventoryStock,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawTableInput {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMapping {
    pub source_column: String,
    pub target_field: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffPreview {
    pub entity_kind: TargetEntityKind,
    pub total_rows: usize,
    pub valid_rows: usize,
    pub invalid_rows: usize,
    pub resolved_mappings: Vec<ColumnMapping>,
    pub unmapped_columns: Vec<String>,
    pub sample_mapped_records: Vec<Value>,
    pub warnings: Vec<String>,
    pub batch_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchCommitResult {
    pub batch_id: String,
    pub entity_kind: TargetEntityKind,
    pub committed_records_count: usize,
    pub batch_sha256: String,
    pub status: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IngestionError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("missing mandatory target field: {0}")]
    MissingRequiredField(&'static str),
    #[error("empty input table")]
    EmptyTable,
}

fn normalize_key(s: &str) -> String {
    s.to_lowercase()
        .replace([' ', '-', '.'], "_")
        .trim_matches('_')
        .to_string()
}

/// Matches raw column names to canonical target fields based on Indonesian and international ERP patterns.
pub fn match_column(col: &str, target: TargetEntityKind) -> Option<&'static str> {
    let norm = normalize_key(col);

    match target {
        TargetEntityKind::CatalogProducts => {
            if [
                "sku",
                "kode",
                "kode_barang",
                "kode_brg",
                "item_code",
                "product_code",
            ]
            .contains(&norm.as_str())
            {
                Some("sku")
            } else if [
                "title",
                "nama",
                "nama_barang",
                "nama_produk",
                "product_name",
                "item_name",
                "description",
            ]
            .contains(&norm.as_str())
            {
                Some("title")
            } else if [
                "price",
                "harga",
                "harga_jual",
                "retail_price",
                "selling_price",
                "price_minor",
            ]
            .contains(&norm.as_str())
            {
                Some("price_minor")
            } else if ["category", "kategori", "golongan", "group"].contains(&norm.as_str()) {
                Some("category")
            } else {
                None
            }
        }
        TargetEntityKind::CrmContacts => {
            if [
                "name",
                "nama",
                "customer_name",
                "nama_lengkap",
                "contact_name",
                "client",
            ]
            .contains(&norm.as_str())
            {
                Some("name")
            } else if ["email", "surel", "e_mail", "email_address"].contains(&norm.as_str()) {
                Some("email")
            } else if [
                "phone",
                "telepon",
                "no_hp",
                "telp",
                "nomor_telepon",
                "whatsapp",
                "mobile",
            ]
            .contains(&norm.as_str())
            {
                Some("phone")
            } else if ["company", "perusahaan", "instansi", "organization"].contains(&norm.as_str())
            {
                Some("company")
            } else {
                None
            }
        }
        TargetEntityKind::InventoryStock => {
            if ["sku", "kode", "kode_barang", "item_code"].contains(&norm.as_str()) {
                Some("sku")
            } else if ["quantity", "qty", "stok", "stock", "jumlah", "saldo"]
                .contains(&norm.as_str())
            {
                Some("quantity")
            } else if ["unit_cost_minor", "unit_cost", "harga_modal", "cost"]
                .contains(&norm.as_str())
            {
                Some("unit_cost_minor")
            } else if ["location", "lokasi", "gudang", "warehouse"].contains(&norm.as_str()) {
                Some("location")
            } else {
                None
            }
        }
    }
}

pub fn parse_currency_minor(s: &str) -> u64 {
    let cleaned: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    let num: u64 = cleaned.parse().unwrap_or(0);
    // If entered as plain IDR (e.g. 50000), minor units (cents) is 50000 * 100
    num.saturating_mul(100)
}

pub fn reconcile_schema(
    input: &RawTableInput,
    entity_kind: TargetEntityKind,
    custom_overrides: Option<&HashMap<String, String>>,
) -> Result<DiffPreview, IngestionError> {
    if input.columns.is_empty() || input.rows.is_empty() {
        return Err(IngestionError::EmptyTable);
    }

    let mut resolved_mappings = Vec::new();
    let mut unmapped_columns = Vec::new();

    for col in &input.columns {
        if let Some(target) = custom_overrides.and_then(|m| m.get(col).cloned()) {
            resolved_mappings.push(ColumnMapping {
                source_column: col.clone(),
                target_field: target,
            });
        } else if let Some(target) = match_column(col, entity_kind) {
            resolved_mappings.push(ColumnMapping {
                source_column: col.clone(),
                target_field: target.to_string(),
            });
        } else {
            unmapped_columns.push(col.clone());
        }
    }

    // Verify mandatory fields
    match entity_kind {
        TargetEntityKind::CatalogProducts => {
            if !resolved_mappings.iter().any(|m| m.target_field == "sku") {
                return Err(IngestionError::MissingRequiredField("sku"));
            }
            if !resolved_mappings.iter().any(|m| m.target_field == "title") {
                return Err(IngestionError::MissingRequiredField("title"));
            }
        }
        TargetEntityKind::CrmContacts => {
            if !resolved_mappings.iter().any(|m| m.target_field == "name") {
                return Err(IngestionError::MissingRequiredField("name"));
            }
        }
        TargetEntityKind::InventoryStock => {
            if !resolved_mappings.iter().any(|m| m.target_field == "sku") {
                return Err(IngestionError::MissingRequiredField("sku"));
            }
            if !resolved_mappings
                .iter()
                .any(|m| m.target_field == "quantity")
            {
                return Err(IngestionError::MissingRequiredField("quantity"));
            }
        }
    }

    let total_rows = input.rows.len();
    let mut valid_rows = 0;
    let mut invalid_rows = 0;
    let mut sample_mapped_records = Vec::new();
    let mut warnings = Vec::new();

    // Map column index
    let mut col_indices: HashMap<String, usize> = HashMap::new();
    for (idx, col) in input.columns.iter().enumerate() {
        col_indices.insert(col.clone(), idx);
    }

    let mut hasher = Sha256::new();

    for (row_idx, row) in input.rows.iter().enumerate() {
        let mut record = serde_json::Map::new();
        let mut row_valid = true;

        for mapping in &resolved_mappings {
            if let Some(&idx) = col_indices.get(&mapping.source_column) {
                if let Some(val) = row.get(idx) {
                    let trimmed_val = trimmed(val);
                    if mapping.target_field == "price_minor" {
                        let cents = parse_currency_minor(trimmed_val);
                        record.insert(mapping.target_field.clone(), json!(cents));
                    } else if mapping.target_field == "quantity" {
                        let qty: u32 = trimmed_val.parse().unwrap_or(0);
                        record.insert(mapping.target_field.clone(), json!(qty));
                    } else {
                        if trimmed_val.is_empty()
                            && (mapping.target_field == "sku" || mapping.target_field == "name")
                        {
                            row_valid = false;
                        }
                        record.insert(mapping.target_field.clone(), json!(trimmed_val));
                    }
                }
            }
        }

        if row_valid {
            valid_rows += 1;
            let serialized = serde_json::to_string(&record).unwrap_or_default();
            hasher.update(serialized.as_bytes());

            if sample_mapped_records.len() < 5 {
                sample_mapped_records.push(Value::Object(record));
            }
        } else {
            invalid_rows += 1;
            if warnings.len() < 5 {
                warnings.push(format!(
                    "Row {}: Missing mandatory identifier, skipped",
                    row_idx + 1
                ));
            }
        }
    }

    let batch_fingerprint = format!("{:x}", hasher.finalize());

    Ok(DiffPreview {
        entity_kind,
        total_rows,
        valid_rows,
        invalid_rows,
        resolved_mappings,
        unmapped_columns,
        sample_mapped_records,
        warnings,
        batch_fingerprint,
    })
}

pub fn execute_batch_commit(
    batch_id: String,
    preview: &DiffPreview,
) -> Result<BatchCommitResult, IngestionError> {
    if !valid_uuid(&batch_id) {
        return Err(IngestionError::Validation("invalid batch uuid format"));
    }
    if preview.valid_rows == 0 {
        return Err(IngestionError::Validation("no valid records to commit"));
    }

    Ok(BatchCommitResult {
        batch_id,
        entity_kind: preview.entity_kind,
        committed_records_count: preview.valid_rows,
        batch_sha256: preview.batch_fingerprint.clone(),
        status: "committed_atomic".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_reconciliation_catalog_products() {
        let input = RawTableInput {
            columns: vec![
                "Kode_Brg".into(),
                "Nama_Barang".into(),
                "Harga_Jual".into(),
                "Kategori".into(),
                "Catatan_Internal".into(),
            ],
            rows: vec![
                vec![
                    "SKU-LIP-01".into(),
                    "Velvet Matte Lip Tint".into(),
                    "85.000".into(),
                    "Cosmetics".into(),
                    "Best seller".into(),
                ],
                vec![
                    "SKU-LIP-02".into(),
                    "Hydra Gloss Balm".into(),
                    "75000".into(),
                    "Cosmetics".into(),
                    "Promo".into(),
                ],
            ],
        };

        let diff = reconcile_schema(&input, TargetEntityKind::CatalogProducts, None)
            .expect("reconciliation passed");

        assert_eq!(diff.total_rows, 2);
        assert_eq!(diff.valid_rows, 2);
        assert_eq!(diff.invalid_rows, 0);
        assert_eq!(diff.unmapped_columns, vec!["Catatan_Internal"]);
        assert_eq!(diff.resolved_mappings.len(), 4);

        // Price in cents: 85,000 IDR * 100 = 8,500,000 cents
        let sample = &diff.sample_mapped_records[0];
        assert_eq!(sample["sku"], "SKU-LIP-01");
        assert_eq!(sample["price_minor"], 8_500_000);

        let commit = execute_batch_commit("00000000-0000-0000-0000-000000000001".into(), &diff)
            .expect("committed");

        assert_eq!(commit.committed_records_count, 2);
        assert_eq!(commit.status, "committed_atomic");
        assert_eq!(commit.batch_sha256.len(), 64);
    }
}
