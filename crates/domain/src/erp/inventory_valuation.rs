//! Exact integer allocation and acquisition-lot cost intervals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Value,
    Quantity,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub quantity: i64,
    pub unit_minor: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub id: String,
    pub quantity: i64,
    pub base_minor: i64,
    pub fee_minor: i64,
    pub total_minor: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostError {
    Invalid,
    Overflow,
    ZeroBasis,
}
pub fn allocate(lines: &[Line], fee: i64, basis: Basis) -> Result<Vec<Allocation>, CostError> {
    if lines.is_empty() || lines.len() > 50 || !(0..=500_000_000_000_000).contains(&fee) {
        return Err(CostError::Invalid);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut weights = Vec::with_capacity(lines.len());
    for line in lines {
        if line.id.is_empty()
            || !seen.insert(&line.id)
            || !(1..=1_000_000).contains(&line.quantity)
            || !(0..=1_000_000_000).contains(&line.unit_minor)
        {
            return Err(CostError::Invalid);
        }
        weights.push(match basis {
            Basis::Value => i128::from(line.quantity) * i128::from(line.unit_minor),
            Basis::Quantity => i128::from(line.quantity),
        });
    }
    let divisor: i128 = weights.iter().sum();
    if divisor == 0 && fee > 0 {
        return Err(CostError::ZeroBasis);
    }
    let mut floors = vec![0_i64; lines.len()];
    let mut remainder = Vec::new();
    let mut assigned = 0_i64;
    if divisor > 0 {
        for (index, weight) in weights.into_iter().enumerate() {
            let numerator = i128::from(fee) * weight;
            let floor = i64::try_from(numerator / divisor).map_err(|_| CostError::Overflow)?;
            floors[index] = floor;
            assigned = assigned.checked_add(floor).ok_or(CostError::Overflow)?;
            remainder.push((index, numerator % divisor));
        }
    }
    remainder.sort_by(|(a, ra), (b, rb)| rb.cmp(ra).then_with(|| lines[*a].id.cmp(&lines[*b].id)));
    let left = usize::try_from(fee.checked_sub(assigned).ok_or(CostError::Overflow)?)
        .map_err(|_| CostError::Overflow)?;
    if left > remainder.len() {
        return Err(CostError::Overflow);
    }
    for (index, _) in remainder.into_iter().take(left) {
        floors[index] += 1;
    }
    lines
        .iter()
        .zip(floors)
        .map(|(line, fee_minor)| {
            let base_minor = line
                .quantity
                .checked_mul(line.unit_minor)
                .ok_or(CostError::Overflow)?;
            Ok(Allocation {
                id: line.id.clone(),
                quantity: line.quantity,
                base_minor,
                fee_minor,
                total_minor: base_minor
                    .checked_add(fee_minor)
                    .ok_or(CostError::Overflow)?,
            })
        })
        .collect()
}
/// An exact interval of a lot's fixed total; NULL cost stays unknown.
pub fn portion(
    total: Option<i64>,
    original: i64,
    start: i64,
    quantity: i64,
) -> Result<Option<i64>, CostError> {
    if original <= 0
        || start < 0
        || quantity <= 0
        || start.checked_add(quantity).is_none_or(|end| end > original)
        || total.is_some_and(|n| n < 0)
    {
        return Err(CostError::Invalid);
    }
    let Some(total) = total else { return Ok(None) };
    let cumulative =
        |k: i64| i128::from(k) * i128::from(total / original) + i128::from(k.min(total % original));
    let value = cumulative(start + quantity) - cumulative(start);
    Ok(Some(i64::try_from(value).map_err(|_| CostError::Overflow)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn line(id: &str, q: i64, u: i64) -> Line {
        Line {
            id: id.into(),
            quantity: q,
            unit_minor: u,
        }
    }
    #[test]
    fn fees_conserve_integer_money_and_ties_ignore_input_order() {
        let lines = vec![line("b", 1, 100), line("a", 1, 100), line("c", 1, 100)];
        let a = allocate(&lines, 1, Basis::Value).unwrap();
        assert_eq!(
            a.iter().map(|x| x.fee_minor).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
        let mut flipped = lines;
        flipped.reverse();
        let b = allocate(&flipped, 1, Basis::Value).unwrap();
        for x in a {
            assert_eq!(
                b.iter().find(|b| b.id == x.id).unwrap().fee_minor,
                x.fee_minor
            );
        }
        for fee in 0..400 {
            let a = allocate(
                &[line("x", 3, 7), line("y", 5, 11), line("z", 2, 19)],
                fee,
                Basis::Value,
            )
            .unwrap();
            assert_eq!(a.iter().map(|x| x.fee_minor).sum::<i64>(), fee);
        }
    }
    #[test]
    fn zero_is_known_and_unknown_is_preserved() {
        assert_eq!(
            allocate(&[line("x", 3, 0)], 1, Basis::Value),
            Err(CostError::ZeroBasis)
        );
        assert_eq!(
            allocate(&[line("x", 3, 0)], 1, Basis::Quantity).unwrap()[0].total_minor,
            1
        );
        assert_eq!(portion(None, 3, 0, 1), Ok(None));
        assert_eq!(portion(Some(0), 3, 0, 1), Ok(Some(0)));
    }
    #[test]
    fn every_partition_of_a_lot_conserves_its_original_total() {
        for qty in 1..40 {
            for total in 0..160 {
                let sum = (0..qty)
                    .map(|start| portion(Some(total), qty, start, 1).unwrap().unwrap())
                    .sum::<i64>();
                assert_eq!(sum, total);
            }
        }
        assert_eq!(
            portion(Some(i64::MAX), i64::MAX, 0, i64::MAX),
            Ok(Some(i64::MAX))
        );
        assert_eq!(portion(Some(10), 3, 2, 2), Err(CostError::Invalid));
        assert_eq!(portion(Some(10), 3, i64::MAX, 1), Err(CostError::Invalid));
    }
    #[test]
    fn malformed_or_huge_inputs_refuse_without_partial_output() {
        assert_eq!(
            allocate(&[line("x", i64::MAX, 1)], 1, Basis::Quantity),
            Err(CostError::Invalid)
        );
        assert_eq!(
            allocate(&[line("x", 1, 1), line("x", 2, 1)], 1, Basis::Quantity),
            Err(CostError::Invalid)
        );
        assert_eq!(allocate(&[], 0, Basis::Quantity), Err(CostError::Invalid));
    }
}
