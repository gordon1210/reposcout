//! Explicit conversions from exact repository counters into approximate
//! floating-point metrics.

#[expect(
    clippy::cast_precision_loss,
    reason = "floating-point scores and ratios intentionally approximate counters above f64's exact integer range"
)]
pub(crate) fn usize_to_f64(value: usize) -> f64 {
    value as f64
}

#[expect(
    clippy::cast_precision_loss,
    reason = "floating-point scores and ratios intentionally approximate counters above f64's exact integer range"
)]
pub(crate) fn u64_to_f64(value: u64) -> f64 {
    value as f64
}

pub(crate) fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

pub(crate) fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn u64_to_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
