use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const SECRET: &str = "not-a-real-secret-for-tests";

pub fn issue(user_id: &str, counter: u64) -> String {
    let body = format!("{user_id}.{counter}");
    let mut h = DefaultHasher::new();
    SECRET.hash(&mut h);
    body.hash(&mut h);
    format!("{body}.{:016x}", h.finish())
}

pub fn is_authentic(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    match parts[1].parse::<u64>() {
        Ok(counter) => issue(parts[0], counter) == token,
        Err(_) => false,
    }
}
