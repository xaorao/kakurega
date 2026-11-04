//! Memory refuses to store secrets. The detector is shared with the importer and the release check (kk-secrets), so the
//! three can no longer disagree about what a secret looks like.

pub fn looks_secret(text: &str) -> Option<&'static str> {
    kk_secrets::looks_secret(text)
}
