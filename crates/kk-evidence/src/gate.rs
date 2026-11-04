use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// What an authenticated hardware quote says. It is produced only by a `QuoteVerifier` after the quote's own signature
/// chain has been checked; nothing in it is taken from fields the server sent next to the quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub measurement: String,
    pub report_data: [u8; 32],
    pub debug: bool,
    pub issued_at: i64,
}

pub trait QuoteVerifier {
    fn verify(&self, quote: &[u8]) -> Result<Evidence, String>;
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub allowed_measurements: BTreeSet<String>,
    pub max_age_secs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    NoPolicy,
    QuoteRejected(String),
    DebugMode,
    UnknownMeasurement,
    NotBoundToThisExchange,
    Stale,
    FromTheFuture,
}

pub const DOMAIN: &[u8] = b"kakurega-hosted-v1";

/// What the server must put in the quote's report data: it ties the quote to this client's fresh nonce and to the key
/// of the TLS connection actually in use, so a quote cannot be replayed, relayed from another machine, or paired with
/// a different endpoint.
pub fn expected_report_data(nonce: &[u8; 32], tls_key_hash: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(nonce);
    h.update(tls_key_hash);
    h.finalize().into()
}

/// The client calls this before it sends the server anything of the owner's. Any error means: send nothing.
pub fn check(
    policy: &Policy,
    verifier: &dyn QuoteVerifier,
    quote: &[u8],
    nonce: &[u8; 32],
    tls_key_hash: &[u8; 32],
    now: i64,
) -> Result<Evidence, GateError> {
    if policy.allowed_measurements.is_empty() {
        return Err(GateError::NoPolicy);
    }
    let ev = verifier.verify(quote).map_err(GateError::QuoteRejected)?;
    if ev.debug {
        return Err(GateError::DebugMode);
    }
    if !policy.allowed_measurements.contains(&ev.measurement) {
        return Err(GateError::UnknownMeasurement);
    }
    if ev.report_data != expected_report_data(nonce, tls_key_hash) {
        return Err(GateError::NotBoundToThisExchange);
    }
    if ev.issued_at > now + 60 {
        return Err(GateError::FromTheFuture);
    }
    if now - ev.issued_at > policy.max_age_secs {
        return Err(GateError::Stale);
    }
    Ok(ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for real hardware verification: it parses a fixed text format and has no cryptography. It exists to
    /// test the gate's logic only; a real verifier must authenticate the quote before returning evidence.
    struct Fake;
    impl QuoteVerifier for Fake {
        fn verify(&self, quote: &[u8]) -> Result<Evidence, String> {
            let t = std::str::from_utf8(quote).map_err(|_| "not text")?;
            let p: Vec<&str> = t.split('|').collect();
            if p.len() != 5 || p[0] != "FAKEQUOTE" {
                return Err("bad quote".into());
            }
            let mut rd = [0u8; 32];
            for (i, b) in rd.iter_mut().enumerate() {
                *b = u8::from_str_radix(p[2].get(i * 2..i * 2 + 2).ok_or("short")?, 16)
                    .map_err(|_| "hex")?;
            }
            Ok(Evidence {
                measurement: p[1].into(),
                report_data: rd,
                debug: p[3] == "debug",
                issued_at: p[4].parse().map_err(|_| "time")?,
            })
        }
    }

    fn quote(measurement: &str, rd: &[u8; 32], debug: bool, at: i64) -> Vec<u8> {
        let hex: String = rd.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "FAKEQUOTE|{measurement}|{hex}|{}|{at}",
            if debug { "debug" } else { "prod" }
        )
        .into_bytes()
    }

    fn policy() -> Policy {
        Policy {
            allowed_measurements: ["m-good".to_string()].into(),
            max_age_secs: 300,
        }
    }

    const NONCE: [u8; 32] = [7; 32];
    const TLS: [u8; 32] = [9; 32];

    #[test]
    fn g01_the_gate_admits_only_a_fresh_bound_quote_of_a_known_production_build() {
        let rd = expected_report_data(&NONCE, &TLS);
        let ok = quote("m-good", &rd, false, 1000);
        assert!(check(&policy(), &Fake, &ok, &NONCE, &TLS, 1100).is_ok());
        type Case = (&'static str, Vec<u8>, [u8; 32], [u8; 32], i64, GateError);
        let cases: Vec<Case> = vec![
            (
                "unknown build",
                quote("m-evil", &rd, false, 1000),
                NONCE,
                TLS,
                1100,
                GateError::UnknownMeasurement,
            ),
            (
                "debug enclave",
                quote("m-good", &rd, true, 1000),
                NONCE,
                TLS,
                1100,
                GateError::DebugMode,
            ),
            (
                "replayed for another nonce",
                ok.clone(),
                [8; 32],
                TLS,
                1100,
                GateError::NotBoundToThisExchange,
            ),
            (
                "relayed to another TLS endpoint",
                ok.clone(),
                NONCE,
                [1; 32],
                1100,
                GateError::NotBoundToThisExchange,
            ),
            ("old quote", ok.clone(), NONCE, TLS, 1301, GateError::Stale),
            (
                "quote from the future",
                quote("m-good", &rd, false, 5000),
                NONCE,
                TLS,
                1100,
                GateError::FromTheFuture,
            ),
        ];
        for (name, q, n, t, now, want) in cases {
            assert_eq!(
                check(&policy(), &Fake, &q, &n, &t, now).err(),
                Some(want),
                "{name}"
            );
        }
        assert!(matches!(
            check(&policy(), &Fake, b"junk", &NONCE, &TLS, 1100),
            Err(GateError::QuoteRejected(_))
        ));
        assert!(matches!(
            check(&policy(), &Fake, &[0xff, 0xfe], &NONCE, &TLS, 1100),
            Err(GateError::QuoteRejected(_))
        ));
        let empty = Policy {
            allowed_measurements: BTreeSet::new(),
            max_age_secs: 300,
        };
        assert_eq!(
            check(&empty, &Fake, &ok, &NONCE, &TLS, 1100).err(),
            Some(GateError::NoPolicy),
            "an empty allow-list admits nothing"
        );
        assert_eq!(
            check(&policy(), &Fake, &ok, &NONCE, &TLS, 1300).map(|e| e.measurement),
            Ok("m-good".into()),
            "the age limit is inclusive"
        );
    }

    #[test]
    fn g02_report_data_binds_domain_nonce_and_key_in_that_order() {
        let a = expected_report_data(&NONCE, &TLS);
        assert_ne!(
            a,
            expected_report_data(&TLS, &NONCE),
            "swapping nonce and key changes it"
        );
        assert_ne!(a, expected_report_data(&[8; 32], &TLS));
        let mut h = Sha256::new();
        h.update(b"kakurega-hosted-v1");
        h.update(NONCE);
        h.update(TLS);
        let want: [u8; 32] = h.finalize().into();
        assert_eq!(
            a, want,
            "the construction is fixed so a server implementation can reproduce it"
        );
    }
}
