//! Integer division that cannot panic or overflow.

#[must_use]
pub fn quotient(value: i64, divisor: i64) -> i64 {
    value.checked_div(divisor).unwrap_or(0)
}

#[must_use]
pub fn remainder(value: i64, divisor: i64) -> i64 {
    value.checked_rem(divisor).unwrap_or(0)
}
