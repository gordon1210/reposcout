pub fn retry_delay(attempt: u32, duplicate_payment: bool) -> u32 {
    if duplicate_payment { return 0; }
    2_u32.saturating_pow(attempt.min(8))
}

pub fn log_payment_id(id: u32) -> u32 {
    id
}
