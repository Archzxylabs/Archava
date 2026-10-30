//! Outpatient Clinic Queue & Triage Paging Module (`archava_patient_queue`).
//!
//! Models clinic patient flow: emergency/urgent triage prioritization,
//! ticket numbering sequences, doctor poli room dispatch, and waiting room paging.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, valid_uuid};

pub const QUEUE_PERMISSIONS: &[&str] = &[
    "queue.read",
    "queue.issue",
    "queue.call",
    "queue.serve",
    "queue.complete",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriagePriority {
    EmergencyP1 = 1, // Highest priority: skips queue
    UrgentP2 = 2,
    RoutineP3 = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    IssuedWaiting,
    Called,
    InConsultation,
    Completed,
    NoShow,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueTicket {
    pub ticket_id: String,
    pub ticket_code: String, // e.g. "POLI-DENTAL-014"
    pub patient_id: String,
    pub poli_room_id: String,
    pub doctor_id: Option<String>,
    pub triage: TriagePriority,
    pub issue_epoch_sec: i64,
    pub called_epoch_sec: Option<i64>,
    pub completed_epoch_sec: Option<i64>,
    pub status: TicketStatus,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QueueError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid ticket transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: TicketStatus,
        to: TicketStatus,
    },
}

impl QueueTicket {
    pub fn new(
        ticket_id: String,
        ticket_code: String,
        patient_id: String,
        poli_room_id: String,
        triage: TriagePriority,
        issue_epoch_sec: i64,
    ) -> Result<Self, QueueError> {
        if !valid_uuid(&ticket_id) || !valid_uuid(&patient_id) || !valid_uuid(&poli_room_id) {
            return Err(QueueError::Validation("invalid uuid format"));
        }
        if bounded_title(&ticket_code, 32).is_none() {
            return Err(QueueError::Validation(
                "ticket code must be between 1 and 32 characters",
            ));
        }

        Ok(Self {
            ticket_id,
            ticket_code,
            patient_id,
            poli_room_id,
            doctor_id: None,
            triage,
            issue_epoch_sec,
            called_epoch_sec: None,
            completed_epoch_sec: None,
            status: TicketStatus::IssuedWaiting,
        })
    }

    pub fn call_patient(
        &mut self,
        doctor_id: String,
        call_epoch_sec: i64,
    ) -> Result<(), QueueError> {
        if self.status != TicketStatus::IssuedWaiting {
            return Err(QueueError::InvalidTransition {
                from: self.status,
                to: TicketStatus::Called,
            });
        }
        if !valid_uuid(&doctor_id) {
            return Err(QueueError::Validation("invalid doctor uuid"));
        }
        if call_epoch_sec < self.issue_epoch_sec {
            return Err(QueueError::Validation(
                "call timestamp cannot precede issue timestamp",
            ));
        }

        self.doctor_id = Some(doctor_id);
        self.called_epoch_sec = Some(call_epoch_sec);
        self.status = TicketStatus::Called;
        Ok(())
    }

    pub fn start_consultation(&mut self) -> Result<(), QueueError> {
        if self.status != TicketStatus::Called {
            return Err(QueueError::InvalidTransition {
                from: self.status,
                to: TicketStatus::InConsultation,
            });
        }
        self.status = TicketStatus::InConsultation;
        Ok(())
    }

    pub fn complete_consultation(&mut self, complete_epoch_sec: i64) -> Result<(), QueueError> {
        if self.status != TicketStatus::InConsultation {
            return Err(QueueError::InvalidTransition {
                from: self.status,
                to: TicketStatus::Completed,
            });
        }
        if let Some(called_at) = self.called_epoch_sec {
            if complete_epoch_sec < called_at {
                return Err(QueueError::Validation(
                    "complete timestamp cannot precede call timestamp",
                ));
            }
        }

        self.completed_epoch_sec = Some(complete_epoch_sec);
        self.status = TicketStatus::Completed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patient_queue_workflow() {
        let mut ticket = QueueTicket::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "POLI-A-042".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "00000000-0000-0000-0000-000000000003".into(),
            TriagePriority::UrgentP2,
            1_000,
        )
        .expect("ticket created");

        assert_eq!(ticket.status, TicketStatus::IssuedWaiting);

        ticket
            .call_patient("00000000-0000-0000-0000-000000000088".into(), 1_200)
            .expect("patient called");

        assert_eq!(ticket.status, TicketStatus::Called);
        ticket.start_consultation().expect("consultation started");
        assert_eq!(ticket.status, TicketStatus::InConsultation);

        ticket.complete_consultation(2_000).expect("completed");
        assert_eq!(ticket.status, TicketStatus::Completed);
    }
}
