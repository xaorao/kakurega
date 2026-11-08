use crate::auth::session::{login, logout, validate};
use crate::util::backoff::retry_delay;

pub fn handle_login(user: Option<&str>) -> (u16, Option<String>) {
    match user {
        Some(u) if !u.is_empty() => (200, Some(login(u))),
        _ => (400, None),
    }
}

pub fn handle_logout(token: &str) -> u16 {
    logout(token);
    204
}

pub fn handle_orders(token: &str) -> (u16, Option<u64>) {
    if !validate(token) {
        return (401, None);
    }
    (200, Some(retry_delay(1)))
}
