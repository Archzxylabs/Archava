//! Exact invoice-payment observations. Transport authenticity belongs to the host/provider boundary.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentState {
    Pending,
    Failed,
    Settled,
    Refunded,
    Review,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentObservation {
    pub order_id: String,
    pub transaction_id: String,
    pub amount_minor: i64,
    pub refunded_minor: i64,
    pub state: PaymentState,
    pub provider_status: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid or inconsistent payment amount")]
pub struct PaymentAmountError;

/// Midtrans IDR amounts are whole Rupiah despite the decimal string wire format.
pub fn whole_idr(value: &str) -> Result<i64, PaymentAmountError> {
    if value.is_empty() || value.len() > 32 {
        return Err(PaymentAmountError);
    }
    let (whole, decimal) = value
        .split_once('.')
        .map_or((value, None), |(w, d)| (w, Some(d)));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || decimal.is_some_and(|d| d.is_empty() || d.len() > 2 || !d.bytes().all(|b| b == b'0'))
    {
        return Err(PaymentAmountError);
    }
    let amount = whole
        .bytes()
        .try_fold(0_i64, |total, b| {
            total
                .checked_mul(10)
                .and_then(|t| t.checked_add(i64::from(b - b'0')))
        })
        .ok_or(PaymentAmountError)?;
    if amount > super::billing::SAFE_INTEGER_MAX {
        return Err(PaymentAmountError);
    }
    Ok(amount)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaymentDelta {
    pub settlement_minor: i64,
    pub refund_minor: i64,
}

/// External money and invoice allocation are separate ledgers. A changed/void
/// invoice can leave real received money unapplied; it must remain visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationDelta {
    pub settlement_minor: i64,
    pub refund_minor: i64,
    pub unapplied_minor: i64,
}
pub fn allocation(
    received: i64,
    refunded: i64,
    allocated: i64,
    allocation_refunded: i64,
    invoice_settled: i64,
    invoice_outstanding: i64,
    invoice_issued: bool,
) -> Result<AllocationDelta, PaymentAmountError> {
    let safe = super::billing::SAFE_INTEGER_MAX;
    if [
        received,
        refunded,
        allocated,
        allocation_refunded,
        invoice_settled,
    ]
    .iter()
    .any(|n| !(0..=safe).contains(n))
        || refunded > received
        || allocation_refunded > allocated
        || !(-safe..=safe).contains(&invoice_outstanding)
    {
        return Err(PaymentAmountError);
    }
    let external_net = received - refunded;
    let allocated_net = allocated - allocation_refunded;
    let refund = (allocated_net - external_net).max(0);
    if refund > invoice_settled {
        return Err(PaymentAmountError);
    }
    let credit = (external_net - allocated_net).max(0);
    let settlement = if invoice_issued {
        credit.min(invoice_outstanding.max(0))
    } else {
        0
    };
    if allocated.checked_add(settlement).is_none_or(|n| n > safe) {
        return Err(PaymentAmountError);
    }
    Ok(AllocationDelta {
        settlement_minor: settlement,
        refund_minor: refund,
        unapplied_minor: external_net - (allocated_net + settlement - refund),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentLink {
    pub id: String,
    pub invoice_id: String,
    pub integration_id: String,
    pub order_id: String,
    pub amount_minor: i64,
    pub state: String,
    pub redirect_url: Option<String>,
    pub received_minor: i64,
    pub refunded_minor: i64,
    pub allocated_minor: i64,
    pub allocation_refunded_minor: i64,
    pub unapplied_minor: i64,
    pub requires_review: bool,
}
/// Monotonic cumulative observations cannot apply the same settlement/refund twice.
/// A refund observed before the settlement notification still carries both effects.
pub fn delta(
    expected: i64,
    applied: i64,
    refunded: i64,
    observed: &PaymentObservation,
) -> Result<PaymentDelta, PaymentAmountError> {
    if expected <= 0
        || observed.amount_minor != expected
        || !(0..=expected).contains(&applied)
        || (applied != 0 && applied != expected)
        || !(0..=applied).contains(&refunded)
        || !(0..=expected).contains(&observed.refunded_minor)
        || (observed.state != PaymentState::Refunded && observed.refunded_minor != 0)
        || (observed.state == PaymentState::Refunded && observed.refunded_minor == 0)
    {
        return Err(PaymentAmountError);
    }
    if !matches!(
        observed.state,
        PaymentState::Settled | PaymentState::Refunded
    ) {
        return Ok(PaymentDelta {
            settlement_minor: 0,
            refund_minor: 0,
        });
    }
    Ok(PaymentDelta {
        settlement_minor: if applied == 0 { expected } else { 0 },
        refund_minor: observed.refunded_minor.saturating_sub(refunded).max(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_conserves_external_money_even_for_overpayment_void_and_refund_first() {
        let full = allocation(100, 0, 0, 0, 0, 100, true).unwrap();
        assert_eq!((full.settlement_minor, full.unapplied_minor), (100, 0));
        let changed = allocation(100, 0, 0, 0, 20, 80, true).unwrap();
        assert_eq!(
            (changed.settlement_minor, changed.unapplied_minor),
            (80, 20)
        );
        let void = allocation(100, 0, 0, 0, 0, 100, false).unwrap();
        assert_eq!((void.settlement_minor, void.unapplied_minor), (0, 100));
        let refund_first = allocation(100, 40, 0, 0, 0, 100, true).unwrap();
        assert_eq!(refund_first.settlement_minor, 60);
        let refund = allocation(100, 70, 60, 0, 60, 40, true).unwrap();
        assert_eq!((refund.refund_minor, refund.unapplied_minor), (30, 0));
        assert!(allocation(100, 70, 60, 0, 20, 80, true).is_err());
        let replay = allocation(100, 70, 60, 30, 30, 70, true).unwrap();
        assert_eq!((replay.settlement_minor, replay.refund_minor), (0, 0));
    }
    #[test]
    fn wire_money_is_exact_and_rejects_fractional_exponential_signed_and_oversized_values() {
        for (wire, n) in [
            ("30000.00", 30000),
            ("00030.0", 30),
            ("0", 0),
            ("9007199254740991.00", 9_007_199_254_740_991),
        ] {
            assert_eq!(whole_idr(wire), Ok(n));
        }
        for wire in [
            "",
            ".00",
            "30.",
            "30.01",
            "30.000",
            "-1",
            "+1",
            " 30",
            "30\n",
            "3e4",
            "9007199254740992.00",
            "99999999999999999999999999999999",
        ] {
            assert!(whole_idr(wire).is_err(), "{wire:?}");
        }
    }
    #[test]
    fn cumulative_refunds_and_out_of_order_notifications_never_double_apply() {
        let mut observed = PaymentObservation {
            order_id: "order".into(),
            transaction_id: "transaction".into(),
            amount_minor: 30000,
            refunded_minor: 12000,
            state: PaymentState::Refunded,
            provider_status: "partial_refund".into(),
        };
        assert_eq!(
            delta(30000, 0, 0, &observed),
            Ok(PaymentDelta {
                settlement_minor: 30000,
                refund_minor: 12000
            })
        );
        assert_eq!(
            delta(30000, 30000, 5000, &observed),
            Ok(PaymentDelta {
                settlement_minor: 0,
                refund_minor: 7000
            })
        );
        assert_eq!(
            delta(30000, 30000, 12000, &observed),
            Ok(PaymentDelta {
                settlement_minor: 0,
                refund_minor: 0
            })
        );
        observed.refunded_minor = 0;
        observed.state = PaymentState::Settled;
        assert_eq!(
            delta(30000, 30000, 12000, &observed),
            Ok(PaymentDelta {
                settlement_minor: 0,
                refund_minor: 0
            })
        );
        observed.state = PaymentState::Pending;
        assert_eq!(
            delta(30000, 30000, 12000, &observed),
            Ok(PaymentDelta {
                settlement_minor: 0,
                refund_minor: 0
            })
        );
        assert!(delta(10, 0, 0, &observed).is_err());
        assert!(delta(30000, 0, 1, &observed).is_err());
    }
}
