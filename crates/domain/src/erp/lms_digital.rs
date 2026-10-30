//! Digital Academy & Learning Management System (LMS) Module (`archava_lms_digital`).
//!
//! Models digital courses, syllabus module completion, quiz scoring, and
//! tamper-evident cryptographic SHA-256 certificate issuance.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::erp::valid_uuid;

pub const LMS_PERMISSIONS: &[&str] = &[
    "lms.read",
    "lms.enroll",
    "lms.progress",
    "lms.certify",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentStatus {
    Enrolled,
    InProgress,
    Passed,
    Failed,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseModuleItem {
    pub module_id: String,
    pub title: String,
    pub min_watch_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseEnrollment {
    pub enrollment_id: String,
    pub course_id: String,
    pub student_id: String,
    pub completed_modules: Vec<String>,
    pub quiz_score_percent: Option<u8>,
    pub passing_threshold_percent: u8,
    pub certificate_hash: Option<String>,
    pub status: EnrollmentStatus,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LmsError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: EnrollmentStatus,
        to: EnrollmentStatus,
    },
    #[error("quiz score {score}% does not meet passing threshold {threshold}%")]
    ScoreBelowThreshold { score: u8, threshold: u8 },
    #[error("incomplete syllabus modules: {completed}/{total}")]
    IncompleteModules { completed: usize, total: usize },
}

impl CourseEnrollment {
    pub fn new(
        enrollment_id: String,
        course_id: String,
        student_id: String,
        passing_threshold_percent: u8,
    ) -> Result<Self, LmsError> {
        if !valid_uuid(&enrollment_id) || !valid_uuid(&course_id) || !valid_uuid(&student_id) {
            return Err(LmsError::Validation("invalid uuid format"));
        }
        if passing_threshold_percent == 0 || passing_threshold_percent > 100 {
            return Err(LmsError::Validation(
                "passing threshold must be between 1 and 100 percent",
            ));
        }

        Ok(Self {
            enrollment_id,
            course_id,
            student_id,
            completed_modules: Vec::new(),
            quiz_score_percent: None,
            passing_threshold_percent,
            certificate_hash: None,
            status: EnrollmentStatus::Enrolled,
        })
    }

    pub fn complete_module(&mut self, module_id: String) -> Result<(), LmsError> {
        if self.status != EnrollmentStatus::Enrolled && self.status != EnrollmentStatus::InProgress
        {
            return Err(LmsError::Validation(
                "cannot update progress on completed or dropped enrollment",
            ));
        }
        if !self.completed_modules.contains(&module_id) {
            self.completed_modules.push(module_id);
        }
        self.status = EnrollmentStatus::InProgress;
        Ok(())
    }

    pub fn record_quiz_and_certify(
        &mut self,
        score_percent: u8,
        total_modules_count: usize,
        issue_timestamp_sec: i64,
    ) -> Result<String, LmsError> {
        if self.completed_modules.len() < total_modules_count {
            return Err(LmsError::IncompleteModules {
                completed: self.completed_modules.len(),
                total: total_modules_count,
            });
        }
        if score_percent > 100 {
            return Err(LmsError::Validation("score cannot exceed 100%"));
        }
        if score_percent < self.passing_threshold_percent {
            self.quiz_score_percent = Some(score_percent);
            self.status = EnrollmentStatus::Failed;
            return Err(LmsError::ScoreBelowThreshold {
                score: score_percent,
                threshold: self.passing_threshold_percent,
            });
        }

        // Generate deterministic SHA-256 certificate proof
        let payload = format!(
            "ARCHAVA-CERT|{}|{}|{}|{}|{}",
            self.enrollment_id, self.course_id, self.student_id, score_percent, issue_timestamp_sec
        );
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        self.quiz_score_percent = Some(score_percent);
        self.certificate_hash = Some(hash.clone());
        self.status = EnrollmentStatus::Passed;

        Ok(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lms_course_progress_and_certification() {
        let mut enr = CourseEnrollment::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "00000000-0000-0000-0000-000000000003".into(),
            80, // 80% passing grade
        )
        .expect("enrollment created");

        enr.complete_module("MOD-01".into()).expect("mod 1 done");
        enr.complete_module("MOD-02".into()).expect("mod 2 done");

        // Attempting quiz without passing grade fails
        let err = enr.record_quiz_and_certify(75, 2, 1_700_000_000);
        assert!(matches!(err, Err(LmsError::ScoreBelowThreshold { .. })));
        assert_eq!(enr.status, EnrollmentStatus::Failed);

        // Retake with 95%
        let cert_hash = enr
            .record_quiz_and_certify(95, 2, 1_700_001_000)
            .expect("passed and certified");

        assert_eq!(enr.status, EnrollmentStatus::Passed);
        assert_eq!(cert_hash.len(), 64);
        assert_eq!(enr.certificate_hash, Some(cert_hash));
    }
}
