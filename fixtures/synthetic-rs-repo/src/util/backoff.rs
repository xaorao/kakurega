pub fn retry_delay(attempt: u32) -> u64 {
    (2u64.pow(attempt) * 100).min(30_000)
}
