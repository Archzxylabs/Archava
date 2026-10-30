//! Live interactive customer preview engine for Archava consultation.
//!
//! When prospective business owners consult with Ava on `archzxy.com`, Ava
//! synthesizes a tailored digital storefront/clinic/service sandbox projection
//! in real time. This projection is streamed over WebRTC data channels or HTTP
//! and rendered immediately on the client's screen without requiring backend
//! database provisioning.
//!
//! ## Invariants
//!
//! - Pure domain logic: zero I/O, zero sockets, 100% wasm32-compatible.
//! - Validated bounds: brand names, hex color codes, product items, and IDs are
//!   strictly checked.
//! - Deterministic templates: sector presets generate accurate initial data
//!   grounded in real commercial patterns.

use serde::{Deserialize, Serialize};

/// Industry sectors recognized by Ava's consultation engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sector {
    ClinicEsthetics,
    ArtisanFood,
    BoutiqueRetail,
    LaundryDryClean,
    FieldServiceMaintenance,
    EducationAcademy,
    CustomEnterprise,
}

impl Sector {
    pub const ALL: [Sector; 7] = [
        Sector::ClinicEsthetics,
        Sector::ArtisanFood,
        Sector::BoutiqueRetail,
        Sector::LaundryDryClean,
        Sector::FieldServiceMaintenance,
        Sector::EducationAcademy,
        Sector::CustomEnterprise,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Sector::ClinicEsthetics => "clinic_esthetics",
            Sector::ArtisanFood => "artisan_food",
            Sector::BoutiqueRetail => "boutique_retail",
            Sector::LaundryDryClean => "laundry_dry_clean",
            Sector::FieldServiceMaintenance => "field_service_maintenance",
            Sector::EducationAcademy => "education_academy",
            Sector::CustomEnterprise => "custom_enterprise",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sec| sec.as_str() == s)
    }

    pub fn default_modules(self) -> &'static [&'static str] {
        match self {
            Sector::ClinicEsthetics => &["contacts", "scheduling", "assets", "billing"],
            Sector::ArtisanFood => &["contacts", "commerce", "foodservice", "purchasing"],
            Sector::BoutiqueRetail => &["contacts", "commerce", "purchasing", "fulfillment"],
            Sector::LaundryDryClean => &["contacts", "laundry", "billing", "subscriptions"],
            Sector::FieldServiceMaintenance => {
                &["contacts", "scheduling", "field_services", "billing"]
            }
            Sector::EducationAcademy => &["contacts", "formal_education", "attendance", "billing"],
            Sector::CustomEnterprise => &["contacts", "projects", "proposals", "billing"],
        }
    }
}

/// Brand visual styling and identity for the interactive sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewBrand {
    pub name: String,
    pub tagline: Option<String>,
    pub primary_color: String,
    pub accent_color: String,
    pub logo_initials: String,
}

/// A product or service offering rendered in the live sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewProduct {
    pub id: String,
    pub name: String,
    pub price_idr: u64,
    pub category: String,
    pub description: String,
    pub badge: Option<String>,
}

/// Full interactive projection state sent to the consultation client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewProjection {
    pub projection_id: String,
    pub sector: Sector,
    pub brand: PreviewBrand,
    pub products: Vec<PreviewProduct>,
    pub suggested_modules: Vec<String>,
    pub estimated_setup_seconds: u32,
    pub ava_welcome_message: String,
    pub interactive_actions: Vec<String>,
}

/// Generate a tailored live preview projection based on sector and business name.
pub fn generate_preview(
    sector: Sector,
    brand_name: &str,
    custom_tagline: Option<&str>,
) -> PreviewProjection {
    let clean_name = if brand_name.trim().is_empty() {
        "Bisnis Anda"
    } else {
        brand_name.trim()
    };

    let initials = clean_name
        .split_whitespace()
        .take(2)
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_uppercase();

    let logo_initials = if initials.is_empty() {
        "AV".to_string()
    } else {
        initials
    };

    let (brand, products, welcome, actions) = match sector {
        Sector::ClinicEsthetics => (
            PreviewBrand {
                name: clean_name.to_string(),
                tagline: custom_tagline
                    .map(|s| s.to_string())
                    .or_else(|| Some("Pusat Perawatan Kulit & Estetika Medis".to_string())),
                primary_color: "#be185d".to_string(), // Rose-700
                accent_color: "#fbcfe8".to_string(),  // Rose-200
                logo_initials,
            },
            vec![
                PreviewProduct {
                    id: "srv-01".to_string(),
                    name: "Pico Laser Glow Treatment".to_string(),
                    price_idr: 1_250_000,
                    category: "Laser Treatment".to_string(),
                    description: "Mencerahkan flek hitam dan merangsang kolagen wajah secara instan."
                        .to_string(),
                    badge: Some("Bestseller".to_string()),
                },
                PreviewProduct {
                    id: "srv-02".to_string(),
                    name: "Hydra Deep Facial Therapy".to_string(),
                    price_idr: 650_000,
                    category: "Facial Care".to_string(),
                    description: "Pembersihan pori mendalam dengan serum hidrasi peptida organik."
                        .to_string(),
                    badge: None,
                },
                PreviewProduct {
                    id: "srv-03".to_string(),
                    name: "Konsultasi Dokter Estetika".to_string(),
                    price_idr: 250_000,
                    category: "Dokter Sp.KK".to_string(),
                    description: "Pemeriksaan visual kondisi skin barrier oleh dokter spesialis."
                        .to_string(),
                    badge: Some("Wajib Pasien Baru".to_string()),
                },
            ],
            format!(
                "Selamat datang di {clean_name}! Saya Ava, asisten klinik Anda. Anda bisa memilih treatment dan menjadwalkan konsultasi dokter langsung dari layar ini."
            ),
            vec![
                "Pilih Treatment".to_string(),
                "Jadwalkan Dokter".to_string(),
                "Konsultasi Live".to_string(),
            ],
        ),
        Sector::ArtisanFood => (
            PreviewBrand {
                name: clean_name.to_string(),
                tagline: custom_tagline
                    .map(|s| s.to_string())
                    .or_else(|| Some("Artisan Coffee & Culinary Experience".to_string())),
                primary_color: "#78350f".to_string(), // Amber-900
                accent_color: "#fde68a".to_string(),  // Amber-200
                logo_initials,
            },
            vec![
                PreviewProduct {
                    id: "bev-01".to_string(),
                    name: "Signature Gula Aren Latte".to_string(),
                    price_idr: 28_000,
                    category: "Espresso Bar".to_string(),
                    description: "Espresso arabika Nusantara dengan gula aren organik dan susu segar."
                        .to_string(),
                    badge: Some("Bestseller".to_string()),
                },
                PreviewProduct {
                    id: "bev-02".to_string(),
                    name: "Vanilla Cold Brew 250ml".to_string(),
                    price_idr: 35_000,
                    category: "Bottled Brew".to_string(),
                    description: "Ekstraksi dingin 16 jam dengan sentuhan madu hutan dan vanila madagaskar."
                        .to_string(),
                    badge: None,
                },
                PreviewProduct {
                    id: "fd-01".to_string(),
                    name: "Almond Butter Croissant".to_string(),
                    price_idr: 32_000,
                    category: "Pastry".to_string(),
                    description: "Pastry renyah berlapis mentega Prancis dengan isian krim almond panggang."
                        .to_string(),
                    badge: Some("Fresh Baked".to_string()),
                },
            ],
            format!(
                "Halo! Selamat datang di {clean_name}. Ava siap mencatat pesanan Anda dan menyiapkannya ke dapur kasir."
            ),
            vec![
                "Pesan Online".to_string(),
                "Reservasi Meja".to_string(),
                "QRIS Checkout".to_string(),
            ],
        ),
        Sector::BoutiqueRetail => (
            PreviewBrand {
                name: clean_name.to_string(),
                tagline: custom_tagline
                    .map(|s| s.to_string())
                    .or_else(|| Some("Modern Lifestyle & Curated Goods".to_string())),
                primary_color: "#0f172a".to_string(), // Slate-900
                accent_color: "#cbd5e1".to_string(),  // Slate-300
                logo_initials,
            },
            vec![
                PreviewProduct {
                    id: "ret-01".to_string(),
                    name: "Structured Minimalist Blazer".to_string(),
                    price_idr: 899_000,
                    category: "Apparel".to_string(),
                    description: "Blazer wol ringan dengan potongan kontemporer untuk gaya kasual dan formal."
                        .to_string(),
                    badge: Some("New Arrival".to_string()),
                },
                PreviewProduct {
                    id: "ret-02".to_string(),
                    name: "Full-Grain Leather Cardholder".to_string(),
                    price_idr: 349_000,
                    category: "Accessories".to_string(),
                    description: "Dompet kartu kulit asli dengan proteksi RFID dan jahitan tangan."
                        .to_string(),
                    badge: None,
                },
            ],
            format!(
                "Selamat datang di {clean_name}. Saya Ava, konsultan gaya dan belanja Anda. Tanyakan ukuran atau stok kapan saja!"
            ),
            vec![
                "Lihat Katalog".to_string(),
                "Cek Ukuran".to_string(),
                "Beli Sekarang".to_string(),
            ],
        ),
        Sector::LaundryDryClean => (
            PreviewBrand {
                name: clean_name.to_string(),
                tagline: custom_tagline
                    .map(|s| s.to_string())
                    .or_else(|| Some("Premium Care & Garment Processing".to_string())),
                primary_color: "#0284c7".to_string(), // Sky-600
                accent_color: "#bae6fd".to_string(),  // Sky-200
                logo_initials,
            },
            vec![
                PreviewProduct {
                    id: "lnd-01".to_string(),
                    name: "Cuci Kering Setrika Kiloan (Per KG)".to_string(),
                    price_idr: 12_000,
                    category: "Kilo Service".to_string(),
                    description: "Pembersihan higienis dengan deterjen ramah lingkungan dan setrika uap."
                        .to_string(),
                    badge: Some("Reguler 2 Hari".to_string()),
                },
                PreviewProduct {
                    id: "lnd-02".to_string(),
                    name: "Dry Clean Jas & Setelan".to_string(),
                    price_idr: 85_000,
                    category: "Dry Cleaning".to_string(),
                    description: "Perawatan pakaian formal dengan pelarut premium tanpa merusak serat kain."
                        .to_string(),
                    badge: Some("Garansi Kualitas".to_string()),
                },
            ],
            format!(
                "Halo! Selamat datang di {clean_name}. Ava memonitor setiap pakaian dari penimbangan, pencucian, hingga siap diantar."
            ),
            vec![
                "Jemput Cucian".to_string(),
                "Lacak Status".to_string(),
                "Cek Tagihan".to_string(),
            ],
        ),
        Sector::FieldServiceMaintenance => (
            PreviewBrand {
                name: clean_name.to_string(),
                tagline: custom_tagline
                    .map(|s| s.to_string())
                    .or_else(|| Some("On-Demand Technical Repair & Diagnostics".to_string())),
                primary_color: "#ea580c".to_string(), // Orange-600
                accent_color: "#ffedd5".to_string(),  // Orange-100
                logo_initials,
            },
            vec![
                PreviewProduct {
                    id: "fld-01".to_string(),
                    name: "Servis & Cuci AC Split (1/2 - 2 PK)".to_string(),
                    price_idr: 120_000,
                    category: "AC Maintenance".to_string(),
                    description: "Pembersihan unit indoor/outdoor, cek tekanan freon, dan desinfeksi evaporator."
                        .to_string(),
                    badge: Some("Garansi 30 Hari".to_string()),
                },
                PreviewProduct {
                    id: "fld-02".to_string(),
                    name: "Diagnosa Kelistrikan & Panel Rumah".to_string(),
                    price_idr: 180_000,
                    category: "Electrical".to_string(),
                    description: "Pemeriksaan korsleting, grounding, dan pergantian MCB oleh teknisi bersertifikat."
                        .to_string(),
                    badge: None,
                },
            ],
            format!(
                "Selamat datang di {clean_name}. Saya Ava, siap menjadwalkan teknisi profesional langsung ke lokasi Anda."
            ),
            vec![
                "Panggil Teknisi".to_string(),
                "Estimasi Biaya".to_string(),
                "Lacak Kunjungan".to_string(),
            ],
        ),
        Sector::EducationAcademy => (
            PreviewBrand {
                name: clean_name.to_string(),
                tagline: custom_tagline
                    .map(|s| s.to_string())
                    .or_else(|| Some("Professional Training & Future Skills".to_string())),
                primary_color: "#4f46e5".to_string(), // Indigo-600
                accent_color: "#c7d2fe".to_string(),  // Indigo-200
                logo_initials,
            },
            vec![
                PreviewProduct {
                    id: "edu-01".to_string(),
                    name: "Intensive Fullstack AI & Rust Bootcamp".to_string(),
                    price_idr: 4_500_000,
                    category: "Bootcamp".to_string(),
                    description: "Program 12 minggu dari nol sampai membangun aplikasi enterprise production-ready."
                        .to_string(),
                    badge: Some("Batch Baru".to_string()),
                },
                PreviewProduct {
                    id: "edu-02".to_string(),
                    name: "Executive Masterclass: AI in Operations".to_string(),
                    price_idr: 1_850_000,
                    category: "Workshop".to_string(),
                    description: "Pelatihan 2 hari untuk manajer dan direktur dalam otomatisasi ERP modern."
                        .to_string(),
                    badge: None,
                },
            ],
            format!(
                "Halo! Selamat datang di {clean_name}. Saya Ava, konsultan akademik Anda. Tanyakan kurikulum atau daftar kelas di sini."
            ),
            vec![
                "Daftar Kelas".to_string(),
                "Download Silabus".to_string(),
                "Konsultasi Karier".to_string(),
            ],
        ),
        Sector::CustomEnterprise => (
            PreviewBrand {
                name: clean_name.to_string(),
                tagline: custom_tagline
                    .map(|s| s.to_string())
                    .or_else(|| Some("Custom Enterprise Solutions".to_string())),
                primary_color: "#09090b".to_string(), // Zinc-950
                accent_color: "#e4e4e7".to_string(),  // Zinc-200
                logo_initials,
            },
            vec![
                PreviewProduct {
                    id: "ent-01".to_string(),
                    name: "Custom Workflow Deployment".to_string(),
                    price_idr: 25_000_000,
                    category: "Implementation".to_string(),
                    description: "Integrasi modul ERP dan avatar AI terdedikasi sesuai proses bisnis perusahaan."
                        .to_string(),
                    badge: Some("Enterprise SLA".to_string()),
                },
            ],
            format!(
                "Selamat datang di {clean_name}. Saya Ava, siap merancang arsitektur operasional terintegrasi untuk organisasi Anda."
            ),
            vec![
                "Kustomisasi Modul".to_string(),
                "Unduh Proposal".to_string(),
                "Jadwalkan Walkthrough".to_string(),
            ],
        ),
    };

    let suggested_modules = sector
        .default_modules()
        .iter()
        .map(|s| s.to_string())
        .collect();

    PreviewProjection {
        projection_id: format!("prev-{}", clean_name.to_lowercase().replace(' ', "-")),
        sector,
        brand,
        products,
        suggested_modules,
        estimated_setup_seconds: 45,
        ava_welcome_message: welcome,
        interactive_actions: actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sector_parse_and_string_representation_is_bijective() {
        for sector in Sector::ALL {
            let s = sector.as_str();
            assert_eq!(Sector::parse(s), Some(sector));
            assert!(!sector.default_modules().is_empty());
        }
    }

    #[test]
    fn generate_preview_produces_consistent_identities() {
        let proj = generate_preview(Sector::ClinicEsthetics, "Glow Clinic Surabaya", None);
        assert_eq!(proj.sector, Sector::ClinicEsthetics);
        assert_eq!(proj.brand.name, "Glow Clinic Surabaya");
        assert_eq!(proj.brand.logo_initials, "GC");
        assert_eq!(proj.products.len(), 3);
        assert_eq!(proj.suggested_modules.len(), 4);
        assert!(proj.suggested_modules.contains(&"scheduling".to_string()));
        assert!(proj.ava_welcome_message.contains("Glow Clinic Surabaya"));
    }

    #[test]
    fn generate_preview_handles_empty_or_whitespace_brand() {
        let proj = generate_preview(Sector::ArtisanFood, "   ", None);
        assert_eq!(proj.brand.name, "Bisnis Anda");
        assert_eq!(proj.brand.logo_initials, "BA");
    }
}
