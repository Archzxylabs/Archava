//! Healthcare & Clinic Electronic Medical Records (EMR) domain — patient registries,
//! clinical encounters, vital signs validation, ICD-10 diagnoses, and medical prescriptions.
//!
//! ## Invariants
//!
//! - Closed vocabularies: gender, blood type, encounter type, encounter status, severity.
//! - Strict physiological vitals bounds (e.g. systolic 40-300 mmHg, temp 30.0-45.0 °C).
//! - Clinical governance: an encounter must record at least one primary diagnosis before completion.
//! - Prescription accountability: medication name, dosage, frequency, and duration are mandatory.
//! - Patient privacy: this module only validates input. It is not mounted, so nothing
//!   here stores, isolates or audit-logs a medical record, and no HIPAA / PDP
//!   compliance is claimed.
//! - Pure domain logic: zero I/O, zero sqlx, wasm32-clean.

use serde::{Deserialize, Serialize};

use super::purchasing::ValidationError;
use super::valid_uuid;

pub const CLINIC_PERMISSIONS: &[&str] = &[
    "clinic.read",
    "clinic.write",
    "clinic.doctor",
    "clinic.prescribe",
    "clinic.audit",
];

// Gender
pub const GENDER_MALE: &str = "male";
pub const GENDER_FEMALE: &str = "female";
pub const GENDER_OTHER: &str = "other";

// Blood Types
pub const BLOOD_A_POS: &str = "A+";
pub const BLOOD_A_NEG: &str = "A-";
pub const BLOOD_B_POS: &str = "B+";
pub const BLOOD_B_NEG: &str = "B-";
pub const BLOOD_AB_POS: &str = "AB+";
pub const BLOOD_AB_NEG: &str = "AB-";
pub const BLOOD_O_POS: &str = "O+";
pub const BLOOD_O_NEG: &str = "O-";
pub const BLOOD_UNKNOWN: &str = "unknown";

// Encounter Types
pub const ENCOUNTER_CONSULTATION: &str = "consultation";
pub const ENCOUNTER_TREATMENT: &str = "treatment";
pub const ENCOUNTER_FOLLOWUP: &str = "followup";
pub const ENCOUNTER_EMERGENCY: &str = "emergency";

// Encounter Statuses
pub const ENCOUNTER_REGISTERED: &str = "registered";
pub const ENCOUNTER_IN_PROGRESS: &str = "in_progress";
pub const ENCOUNTER_COMPLETED: &str = "completed";
pub const ENCOUNTER_CANCELED: &str = "canceled";

// Diagnosis Severity
pub const SEVERITY_MILD: &str = "mild";
pub const SEVERITY_MODERATE: &str = "moderate";
pub const SEVERITY_SEVERE: &str = "severe";
pub const SEVERITY_CRITICAL: &str = "critical";

// Bounds
pub const MRN_MAX: usize = 40;
pub const NAME_MAX: usize = 120;
pub const COMPLAINT_MAX: usize = 1000;
pub const SUMMARY_MAX: usize = 2000;
pub const MEDICATION_MAX: usize = 120;
pub const MAX_DIAGNOSES_PER_ENCOUNTER: usize = 20;
pub const MAX_PRESCRIPTION_ITEMS: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VitalSigns {
    pub systolic_mmhg: u16,
    pub diastolic_mmhg: u16,
    pub pulse_bpm: u16,
    pub respiratory_rate: u16,
    pub temp_celsius: f32,
    pub weight_kg: f32,
    pub height_cm: f32,
}

impl VitalSigns {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.systolic_mmhg < 40 || self.systolic_mmhg > 300 {
            return Err(ValidationError(
                "systolic blood pressure out of physiological range",
            ));
        }
        if self.diastolic_mmhg < 20 || self.diastolic_mmhg > 200 {
            return Err(ValidationError(
                "diastolic blood pressure out of physiological range",
            ));
        }
        if self.systolic_mmhg <= self.diastolic_mmhg {
            return Err(ValidationError(
                "systolic must be greater than diastolic pressure",
            ));
        }
        if self.pulse_bpm < 20 || self.pulse_bpm > 260 {
            return Err(ValidationError("pulse rate out of bounds"));
        }
        if self.respiratory_rate < 4 || self.respiratory_rate > 80 {
            return Err(ValidationError("respiratory rate out of bounds"));
        }
        if self.temp_celsius < 30.0 || self.temp_celsius > 45.0 {
            return Err(ValidationError(
                "body temperature out of physiological range",
            ));
        }
        if self.weight_kg <= 0.0 || self.weight_kg > 500.0 {
            return Err(ValidationError("invalid body weight"));
        }
        if self.height_cm <= 0.0 || self.height_cm > 300.0 {
            return Err(ValidationError("invalid body height"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClinicalDiagnosis {
    pub icd10_code: String,
    pub description: String,
    pub severity: String,
    pub is_primary: bool,
}

impl ClinicalDiagnosis {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let code = self.icd10_code.trim();
        if code.len() < 3 || code.len() > 10 {
            return Err(ValidationError("invalid ICD-10 code format"));
        }
        if self.description.trim().is_empty() || self.description.len() > 300 {
            return Err(ValidationError("invalid diagnosis description length"));
        }
        match self.severity.as_str() {
            SEVERITY_MILD | SEVERITY_MODERATE | SEVERITY_SEVERE | SEVERITY_CRITICAL => {}
            _ => return Err(ValidationError("unknown diagnosis severity")),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrescriptionItem {
    pub medication_name: String,
    pub form: String, // tablet, syrup, injection, cream
    pub dosage: String,
    pub frequency: String,
    pub duration_days: u16,
    pub instructions: Option<String>,
}

impl PrescriptionItem {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.medication_name.trim().is_empty() || self.medication_name.len() > MEDICATION_MAX {
            return Err(ValidationError("invalid medication name length"));
        }
        if self.dosage.trim().is_empty() || self.dosage.len() > 60 {
            return Err(ValidationError("invalid dosage specification"));
        }
        if self.duration_days == 0 || self.duration_days > 365 {
            return Err(ValidationError("duration_days must be between 1 and 365"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartEncounterCommand {
    pub patient_id: String,
    pub doctor_id: String,
    pub encounter_type: String,
    pub chief_complaint: String,
    pub vitals: Option<VitalSigns>,
}

impl StartEncounterCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.patient_id) {
            return Err(ValidationError("invalid patient_id UUID"));
        }
        if !valid_uuid(&self.doctor_id) {
            return Err(ValidationError("invalid doctor_id UUID"));
        }
        match self.encounter_type.as_str() {
            ENCOUNTER_CONSULTATION
            | ENCOUNTER_TREATMENT
            | ENCOUNTER_FOLLOWUP
            | ENCOUNTER_EMERGENCY => {}
            _ => return Err(ValidationError("unknown encounter type")),
        }
        if self.chief_complaint.trim().is_empty() || self.chief_complaint.len() > COMPLAINT_MAX {
            return Err(ValidationError("chief_complaint invalid length"));
        }
        if let Some(ref v) = self.vitals {
            v.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteEncounterCommand {
    pub encounter_id: String,
    pub diagnoses: Vec<ClinicalDiagnosis>,
    pub prescriptions: Vec<PrescriptionItem>,
    pub clinical_summary: String,
}

impl CompleteEncounterCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.encounter_id) {
            return Err(ValidationError("invalid encounter_id UUID"));
        }
        if self.diagnoses.is_empty() || self.diagnoses.len() > MAX_DIAGNOSES_PER_ENCOUNTER {
            return Err(ValidationError(
                "encounter requires between 1 and 20 diagnoses",
            ));
        }
        let mut primary_count = 0;
        for diag in &self.diagnoses {
            diag.validate()?;
            if diag.is_primary {
                primary_count += 1;
            }
        }
        if primary_count == 0 {
            return Err(ValidationError(
                "encounter must specify at least one primary diagnosis",
            ));
        }
        if self.prescriptions.len() > MAX_PRESCRIPTION_ITEMS {
            return Err(ValidationError("too many prescription items"));
        }
        for rx in &self.prescriptions {
            rx.validate()?;
        }
        if self.clinical_summary.trim().is_empty() || self.clinical_summary.len() > SUMMARY_MAX {
            return Err(ValidationError(
                "clinical summary exceeds bounds or is empty",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vital_signs_physiological_bounds() {
        let normal_vitals = VitalSigns {
            systolic_mmhg: 120,
            diastolic_mmhg: 80,
            pulse_bpm: 72,
            respiratory_rate: 16,
            temp_celsius: 36.6,
            weight_kg: 68.5,
            height_cm: 172.0,
        };
        assert!(normal_vitals.validate().is_ok());

        // Inverted blood pressure (diastolic >= systolic)
        let mut bad_bp = normal_vitals.clone();
        bad_bp.systolic_mmhg = 80;
        bad_bp.diastolic_mmhg = 120;
        assert!(bad_bp.validate().is_err());

        // Lethal temperature
        let mut high_temp = normal_vitals.clone();
        high_temp.temp_celsius = 46.5;
        assert!(high_temp.validate().is_err());
    }

    #[test]
    fn complete_encounter_requires_primary_diagnosis() {
        let cmd = CompleteEncounterCommand {
            encounter_id: "00000000-0000-0000-0000-000000000001".to_string(),
            diagnoses: vec![ClinicalDiagnosis {
                icd10_code: "L70.0".to_string(),
                description: "Acne vulgaris".to_string(),
                severity: SEVERITY_MILD.to_string(),
                is_primary: false, // No primary
            }],
            prescriptions: vec![],
            clinical_summary: "Pasien diberikan krim tretinoin 0.05%".to_string(),
        };
        assert!(cmd.validate().is_err());

        // With primary diagnosis
        let mut valid_cmd = cmd.clone();
        valid_cmd.diagnoses[0].is_primary = true;
        assert!(valid_cmd.validate().is_ok());
    }
}
