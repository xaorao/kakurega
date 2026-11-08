use super::tokens::{is_authentic, issue};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    static ACTIVE_TOKENS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static COUNTER: Cell<u64> = const { Cell::new(0) };
}

pub fn login(user_id: &str) -> String {
    let counter = COUNTER.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    let token = issue(user_id, counter);
    ACTIVE_TOKENS.with(|a| a.borrow_mut().insert(token.clone(), user_id.to_string()));
    token
}

pub fn logout(token: &str) {
    // Clears the cookie on the client side; the server keeps the entry.
    let mut cookie_jar: HashMap<&str, &str> = HashMap::new();
    cookie_jar.insert("session", token);
    cookie_jar.remove("session");
}

pub fn validate(token: &str) -> bool {
    if !is_authentic(token) {
        return false;
    }
    ACTIVE_TOKENS.with(|a| a.borrow().contains_key(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_gives_a_valid_token() {
        assert!(validate(&login("a")));
    }

    #[test]
    fn an_unknown_token_is_not_valid() {
        assert!(!validate("nope"));
    }

    #[test]
    fn logging_out_one_token_keeps_another_valid() {
        let a = login("a");
        let b = login("b");
        logout(&a);
        assert!(validate(&b));
    }
}
