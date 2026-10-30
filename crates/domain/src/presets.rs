//! Industry Profile Presets & Zero-Config Feature Masks.
//! Enables Archava to simplify its 22-crate ecosystem into tailored, clean,
//! frictionless views based on the tenant's specific industry vertical.

use crate::modality::AvaModality;
use serde::{Deserialize, Serialize};

/// Supported industry verticals for zero-config tenant onboarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndustryProfile {
    /// Food & Beverage, Cafes, Coffee Shops, Cloud Kitchens, Restaurants.
    #[default]
    #[serde(rename = "fnb_cafe")]
    FnBCafe,
    /// Aesthetic Clinics, Dental Care, Specialty Healthcare, Medical Spas.
    ClinicHealthcare,
    /// Boutique Hotels, Luxury Villas, Resorts, Bed & Breakfasts.
    HospitalityHotel,
    /// Retail Boutiques, Multi-Branch Stores, Consumer Goods.
    RetailCommerce,
    /// Commercial Fleet, Trucking, Cold-Chain Delivery, Rental Fleets.
    LogisticsFleet,
    /// Tech Studios, Creative Agencies, Law Firms, Consulting Practices.
    ProfessionalServices,
}

/// Feature visibility and experience mask configured for an industry vertical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMask {
    pub profile: IndustryProfile,
    pub display_name: String,
    pub description: String,
    pub active_modules: Vec<String>,
    pub hidden_modules: Vec<String>,
    pub primary_kpis: Vec<String>,
    pub quick_prompts: Vec<String>,
    pub default_modality: AvaModality,
}

impl IndustryProfile {
    /// Parses an industry identifier into an `IndustryProfile`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fnb" | "cafe" | "restaurant" | "fnb_cafe" => Some(Self::FnBCafe),
            "clinic" | "healthcare" | "spa" | "medical" | "clinic_healthcare" => {
                Some(Self::ClinicHealthcare)
            }
            "hospitality" | "hotel" | "villa" | "resort" | "hospitality_hotel" => {
                Some(Self::HospitalityHotel)
            }
            "retail" | "shop" | "store" | "retail_commerce" => Some(Self::RetailCommerce),
            "logistics" | "fleet" | "transport" | "logistics_fleet" => Some(Self::LogisticsFleet),
            "agency" | "services" | "consulting" | "professional_services" => {
                Some(Self::ProfessionalServices)
            }
            _ => None,
        }
    }

    /// String slug of the profile.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FnBCafe => "fnb_cafe",
            Self::ClinicHealthcare => "clinic_healthcare",
            Self::HospitalityHotel => "hospitality_hotel",
            Self::RetailCommerce => "retail_commerce",
            Self::LogisticsFleet => "logistics_fleet",
            Self::ProfessionalServices => "professional_services",
        }
    }

    /// Human-readable display label.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::FnBCafe => "F&B, Cafe & Resto",
            Self::ClinicHealthcare => "Klinik Estetika & Kesehatan",
            Self::HospitalityHotel => "Hotel Butik & Villa",
            Self::RetailCommerce => "Retail & Consumer Store",
            Self::LogisticsFleet => "Logistik & Armada Truk",
            Self::ProfessionalServices => "Studio Agensi & Konsultan",
        }
    }

    /// Generates the tailored `ProfileMask` for this industry profile.
    pub fn mask(&self) -> ProfileMask {
        match self {
            Self::FnBCafe => ProfileMask {
                profile: *self,
                display_name: self.display_name().into(),
                description: "POS kasir cepat, antrean pesanan dapur, kontrol stok bahan baku FIFO, dan pembayaran instan QRIS.".into(),
                active_modules: vec![
                    "pos_cashier".into(),
                    "kitchen_display".into(),
                    "inventory_fifo".into(),
                    "instant_qris".into(),
                    "escpos_receipt".into(),
                    "daily_omzet".into(),
                ],
                hidden_modules: vec![
                    "fleet_telematics".into(),
                    "room_nights".into(),
                    "emr_records".into(),
                    "customs_landed_cost".into(),
                ],
                primary_kpis: vec![
                    "Omzet Hari Ini".into(),
                    "Total Pesanan Selesai".into(),
                    "Stok Kritis Bahan Baku".into(),
                    "Rata-rata Durasi Meja".into(),
                ],
                quick_prompts: vec![
                    "Berapa total omzet kafe hari ini?".into(),
                    "Bahan apa saja yang stoknya mau habis?".into(),
                    "Tutup kasir dan cetak ringkasan shift".into(),
                ],
                default_modality: AvaModality::Full,
            },
            Self::ClinicHealthcare => ProfileMask {
                profile: *self,
                display_name: self.display_name().into(),
                description: "Manajemen reservasi pasien, rekam medis ringkas, reminder WhatsApp otomatis, dan pelacakan batch obat FEFO.".into(),
                active_modules: vec![
                    "patient_appointments".into(),
                    "medical_records".into(),
                    "whatsapp_reminders".into(),
                    "inventory_fefo".into(),
                    "doctor_commissions".into(),
                    "billing_qris".into(),
                ],
                hidden_modules: vec![
                    "fleet_telematics".into(),
                    "kitchen_display".into(),
                    "room_nights".into(),
                ],
                primary_kpis: vec![
                    "Jadwal Pasien Hari Ini".into(),
                    "Konfirmasi WhatsApp (H-1)".into(),
                    "Obat Mendekati Kadaluarsa".into(),
                    "Revenue Treatment".into(),
                ],
                quick_prompts: vec![
                    "Lihat jadwal reservasi treatment hari ini".into(),
                    "Kirim pengingat WhatsApp ke pasien besok".into(),
                    "Cek stok serum botol yang mendekati expired".into(),
                ],
                default_modality: AvaModality::AvatarVoice,
            },
            Self::HospitalityHotel => ProfileMask {
                profile: *self,
                display_name: self.display_name().into(),
                description: "Pemesanan kamar & villa, status housekeeping kamar, deposit tamu, dan Ava Concierge 24/7.".into(),
                active_modules: vec![
                    "room_reservations".into(),
                    "housekeeping_tracker".into(),
                    "guest_concierge".into(),
                    "folio_billing".into(),
                    "whatsapp_checkin".into(),
                ],
                hidden_modules: vec![
                    "fleet_telematics".into(),
                    "kitchen_display".into(),
                    "emr_records".into(),
                ],
                primary_kpis: vec![
                    "Tingkat Okupansi (Occupancy %)".into(),
                    "Check-In Hari Ini".into(),
                    "Kamar Perlu Housekeeping".into(),
                    "Average Daily Rate (ADR)".into(),
                ],
                quick_prompts: vec![
                    "Berapa villa yang terisi malam ini?".into(),
                    "Kamar mana saja yang belum selesai dibersihkan?".into(),
                    "Kirim panduan check-in via WhatsApp ke tamu".into(),
                ],
                default_modality: AvaModality::AvatarVoice,
            },
            Self::RetailCommerce => ProfileMask {
                profile: *self,
                display_name: self.display_name().into(),
                description: "Kasir barcode retail, printer thermal struk, katalog multi-varian, dan kartu loyalitas pelanggan.".into(),
                active_modules: vec![
                    "barcode_pos".into(),
                    "product_variants".into(),
                    "thermal_escpos".into(),
                    "inventory_reorder".into(),
                    "loyalty_crm".into(),
                ],
                hidden_modules: vec![
                    "fleet_telematics".into(),
                    "room_nights".into(),
                    "emr_records".into(),
                ],
                primary_kpis: vec![
                    "Penjualan Kasir Hari Ini".into(),
                    "Barang Terlaris (Top 5)".into(),
                    "Peringatan Reorder Supplier".into(),
                    "Margin Kotor Rata-rata".into(),
                ],
                quick_prompts: vec![
                    "Cari sisa stok barang berdasarkan barcode".into(),
                    "Buat draft order pembelian ke distributor".into(),
                    "Cetak ulang struk transaksi terakhir".into(),
                ],
                default_modality: AvaModality::ChatOnly,
            },
            Self::LogisticsFleet => ProfileMask {
                profile: *self,
                display_name: self.display_name().into(),
                description: "Pelacakan GPS real-time Teltonika, status pengemudi, pemeliharaan armada truk, dan surat jalan digital.".into(),
                active_modules: vec![
                    "telematics_gps".into(),
                    "vehicle_status".into(),
                    "delivery_manifest".into(),
                    "fuel_monitoring".into(),
                    "fleet_maintenance".into(),
                ],
                hidden_modules: vec![
                    "pos_cashier".into(),
                    "room_nights".into(),
                    "emr_records".into(),
                ],
                primary_kpis: vec![
                    "Armada Aktif Bergerak".into(),
                    "Peringatan Bahan Bakar Kritis".into(),
                    "Surat Jalan Selesai".into(),
                    "Jadwal Servis Berkala".into(),
                ],
                quick_prompts: vec![
                    "Dimana posisi truk B-9120-XYZ saat ini?".into(),
                    "Truk mana yang perlu ganti oli minggu ini?".into(),
                    "Kirim update ETA pengiriman ke pelanggan".into(),
                ],
                default_modality: AvaModality::ChatOnly,
            },
            Self::ProfessionalServices => ProfileMask {
                profile: *self,
                display_name: self.display_name().into(),
                description: "Pelacakan kontrak SOW, milestone proyek, penagihan invoice termin, dan otomatisasi PPh 21 & PPh 23.".into(),
                active_modules: vec![
                    "project_milestones".into(),
                    "contract_sow".into(),
                    "invoice_milestones".into(),
                    "tax_pph21_pph23".into(),
                    "client_portal".into(),
                ],
                hidden_modules: vec![
                    "fleet_telematics".into(),
                    "kitchen_display".into(),
                    "room_nights".into(),
                ],
                primary_kpis: vec![
                    "Piutang Invoice Jatuh Tempo".into(),
                    "Milestone Siap Billed".into(),
                    "Utilisasi Tim (Billable %)".into(),
                    "Pipeline Nilai Proyek".into(),
                ],
                quick_prompts: vec![
                    "Invoice mana saja yang sudah jatuh tempo?".into(),
                    "Buat draf tagihan termin 2 untuk klien".into(),
                    "Kirim ringkasan progres milestone ke klien".into(),
                ],
                default_modality: AvaModality::Full,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_industry_profile_parsing() {
        assert_eq!(
            IndustryProfile::parse("fnb"),
            Some(IndustryProfile::FnBCafe)
        );
        assert_eq!(
            IndustryProfile::parse("cafe"),
            Some(IndustryProfile::FnBCafe)
        );
        assert_eq!(
            IndustryProfile::parse("clinic"),
            Some(IndustryProfile::ClinicHealthcare)
        );
        assert_eq!(
            IndustryProfile::parse("villa"),
            Some(IndustryProfile::HospitalityHotel)
        );
        assert_eq!(
            IndustryProfile::parse("logistics"),
            Some(IndustryProfile::LogisticsFleet)
        );
        assert_eq!(
            IndustryProfile::parse("agency"),
            Some(IndustryProfile::ProfessionalServices)
        );
        assert_eq!(IndustryProfile::parse("unknown_space"), None);
    }

    #[test]
    fn test_profile_mask_isolation() {
        let fnb = IndustryProfile::FnBCafe.mask();
        assert!(fnb.active_modules.contains(&"pos_cashier".to_string()));
        assert!(fnb.hidden_modules.contains(&"fleet_telematics".to_string()));
        assert_eq!(fnb.primary_kpis.len(), 4);
        assert_eq!(fnb.quick_prompts.len(), 3);

        let logistics = IndustryProfile::LogisticsFleet.mask();
        assert!(logistics
            .active_modules
            .contains(&"telematics_gps".to_string()));
        assert!(logistics
            .hidden_modules
            .contains(&"pos_cashier".to_string()));
    }
}
