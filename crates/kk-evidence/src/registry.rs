//! The message a capsule's signing key signs to register the capsule on NEAR, exactly as the registry contract rebuilds it.
//!
//! The key signs nothing else for the chain: the text is built here from a capsule's digest, its verified model receipts, the
//! NEAR account that will send the registration and, for a paid review, the order's number. A caller never supplies the text.

use crate::capsule::{sha256_hex, ReceiptReport};

/// What the contract prefixes, so a signature made for another purpose can never be used there.
pub const BINDING_PREFIX: &str = "kakurega-register-v2";
/// The most receipts one registration carries on chain.
pub const MAX_CHAIN_RECEIPTS: usize = 16;

/// Whether `name` has the form of a NEAR account id: 2 to 64 characters, lowercase letters and digits in parts joined by
/// single `-`, `_` or `.` separators.
pub fn is_account_id(name: &str) -> bool {
    if !(2..=64).contains(&name.len()) {
        return false;
    }
    let mut last_was_separator = true;
    for c in name.bytes() {
        match c {
            b'a'..=b'z' | b'0'..=b'9' => last_was_separator = false,
            b'-' | b'_' | b'.' if !last_was_separator => last_was_separator = true,
            _ => return false,
        }
    }
    !last_was_separator
}

/// SHA-256, in hex, of the receipts as the contract reads them: one line each, `signer:model:request:response:signature`.
pub fn receipts_digest(receipts: &[ReceiptReport]) -> String {
    let mut text = String::new();
    for r in receipts {
        text.push_str(&format!(
            "{}:{}:{}:{}:{}\n",
            r.signer, r.model, r.request_sha256, r.response_sha256, r.signature
        ));
    }
    sha256_hex(text.as_bytes())
}

/// The message to sign, or `None` when a part does not have the form the contract accepts.
pub fn binding_message(
    capsule_sha256: &str,
    receipts: &[ReceiptReport],
    account: &str,
    order: Option<u64>,
) -> Option<String> {
    let hex64 =
        |s: &str| s.len() == 64 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
    if !hex64(capsule_sha256) || !is_account_id(account) || receipts.len() > MAX_CHAIN_RECEIPTS {
        return None;
    }
    let mut m = format!(
        "{BINDING_PREFIX}:{capsule_sha256}:{}:{account}",
        receipts_digest(receipts)
    );
    if let Some(id) = order {
        m.push_str(&format!(":order:{id}"));
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(n: char) -> ReceiptReport {
        ReceiptReport {
            completion_id: "c".into(),
            signer: n.to_string().repeat(64),
            model: "vendor/model".into(),
            request_sha256: "1".repeat(64),
            response_sha256: "2".repeat(64),
            signature: "3".repeat(128),
        }
    }

    // The same literals are asserted in the contract's own tests (`rg19` in `contracts/kakurega-registry`), so the two
    // sides cannot drift apart unnoticed.
    #[test]
    fn rb01_the_message_and_the_receipts_digest_match_the_contracts() {
        let d = "a".repeat(64);
        assert_eq!(
            receipts_digest(&[]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            binding_message(&d, &[], "alice.testnet", None).unwrap(),
            format!("kakurega-register-v2:{d}:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:alice.testnet")
        );
        let with = receipts_digest(&[receipt('a'), receipt('b')]);
        assert_eq!(
            with,
            "73d428482db3202c6ad45ac6748bada43579da85141e4ebdd1ed68be2a974d84"
        );
        assert_eq!(
            binding_message(&d, &[receipt('a'), receipt('b')], "rita.testnet", Some(7)).unwrap(),
            format!("kakurega-register-v2:{d}:{with}:rita.testnet:order:7")
        );
    }

    #[test]
    fn rb02_nothing_is_built_for_a_part_the_contract_would_not_accept() {
        let d = "a".repeat(64);
        assert!(binding_message("ABC", &[], "alice.testnet", None).is_none());
        assert!(binding_message(&d.to_uppercase(), &[], "alice.testnet", None).is_none());
        let many: Vec<ReceiptReport> = (0..17).map(|_| receipt('a')).collect();
        assert!(binding_message(&d, &many, "alice.testnet", None).is_none());
        assert!(binding_message(&d, &many[..16], "alice.testnet", None).is_some());
        for bad in [
            "",
            "a",
            "Alice.testnet",
            "alice..testnet",
            ".alice",
            "alice.",
            "alice testnet",
            "alice:order:1",
            "a\nb",
            "alice--b",
            "alice-.b",
            &"a".repeat(65),
            "älice",
        ] {
            assert!(binding_message(&d, &[], bad, None).is_none(), "{bad:?}");
            assert!(!is_account_id(bad), "{bad:?}");
        }
        for good in [
            "alice.testnet",
            "a1",
            "kakurega-registry-5d26f9.testnet",
            "x_y.near",
            &"a".repeat(64),
        ] {
            assert!(is_account_id(good), "{good}");
        }
    }

    #[test]
    fn rb03_an_account_cannot_smuggle_an_order_number_into_a_plain_registration() {
        let d = "a".repeat(64);
        // the only way to end in `:order:N` is to ask for the order
        let plain = binding_message(&d, &[], "alice.testnet", None).unwrap();
        let order = binding_message(&d, &[], "alice.testnet", Some(1)).unwrap();
        assert!(
            order.starts_with(&plain) && order.ends_with(":order:1") && !plain.contains(":order:")
        );
    }
}
