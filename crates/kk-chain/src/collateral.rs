//! Intel's collateral for a TDX quote, in the form NEAR's verifier contract takes.
//!
//! A quote carries the certificate of the platform that made it. The collateral is what Intel publishes about that platform
//! (its patch-level table, the quoting enclave's identity and the revocation lists), each part signed by Intel. The verifier
//! contract checks the signatures; this module only fetches the parts and lays them out. The steps follow Antibody's client
//! (MIT), which does the same for OutLayer quotes.

use std::fmt;

pub const INTEL_PCS: &str = "https://api.trustedservices.intel.com";
pub const INTEL_ROOT_CRL: &str =
    "https://certificates.trustedservices.intel.com/IntelSGXRootCA.der";

/// The most that is read of any one answer from Intel.
const MAX_PART_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollateralError {
    /// The quote does not carry a platform certificate in the form this reads.
    Quote(&'static str),
    /// Intel's service could not be reached or answered with an error.
    Fetch(String),
    /// An answer from Intel is not in the expected form.
    Answer(&'static str),
}

impl fmt::Display for CollateralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CollateralError::Quote(m) => write!(f, "the quote cannot be read: {m}"),
            CollateralError::Fetch(m) => write!(f, "Intel's certificate service: {m}"),
            CollateralError::Answer(m) => {
                write!(f, "Intel's answer is not in the expected form: {m}")
            }
        }
    }
}

impl std::error::Error for CollateralError {}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The platform a quote came from: its family-model-stepping id (FMSPC, 12 uppercase hex digits) and which Intel authority
/// issued its certificate. Read from the first certificate inside the quote.
pub fn quote_platform(quote: &[u8]) -> Result<(String, &'static str), CollateralError> {
    use base64::Engine as _;
    const BEGIN: &[u8] = b"-----BEGIN CERTIFICATE-----";
    const END: &[u8] = b"-----END CERTIFICATE-----";
    let begin = find(quote, BEGIN).ok_or(CollateralError::Quote("no certificate"))?;
    let end =
        find(&quote[begin..], END).ok_or(CollateralError::Quote("unfinished certificate"))? + begin;
    let body: Vec<u8> = quote[begin + BEGIN.len()..end]
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_| CollateralError::Quote("certificate is not base64"))?;
    // Intel's FMSPC extension: object id 1.2.840.113741.1.13.1.4, then a six-byte octet string
    const OID: [u8; 12] = [
        0x06, 0x0a, 0x2a, 0x86, 0x48, 0x86, 0xf8, 0x4d, 0x01, 0x0d, 0x01, 0x04,
    ];
    let at = find(&der, &OID).ok_or(CollateralError::Quote("no platform id"))? + OID.len();
    let value = der
        .get(at..at + 8)
        .ok_or(CollateralError::Quote("short platform id"))?;
    if value[0] != 0x04 || value[1] != 0x06 {
        return Err(CollateralError::Quote("platform id in an unknown form"));
    }
    let fmspc = value[2..].iter().map(|b| format!("{b:02X}")).collect();
    let ca = if find(&der, b"Processor CA").is_some() {
        "processor"
    } else {
        "platform"
    };
    Ok((fmspc, ca))
}

/// The exact text of one object inside a signed answer. Intel signs these bytes, so they are cut out as sent, never rebuilt.
pub fn raw_object<'a>(body: &'a str, key: &str) -> Result<&'a str, CollateralError> {
    let marker = format!("\"{key}\":");
    let start = body
        .find(&marker)
        .ok_or(CollateralError::Answer("a signed object is missing"))?
        + marker.len();
    let rest = &body[start..];
    if !rest.starts_with('{') {
        return Err(CollateralError::Answer(
            "a signed object does not start where expected",
        ));
    }
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (i, c) in rest.char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&rest[..=i]);
                }
            }
            _ => {}
        }
    }
    Err(CollateralError::Answer("a signed object does not end"))
}

fn unhex(s: &str) -> Result<Vec<u8>, CollateralError> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CollateralError::Answer("a signature is not hex"));
    }
    Ok((0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap_or(0))
        .collect())
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(bytes);
}

/// The parts of a collateral, as fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parts {
    pub pck_crl_issuer_chain: String,
    pub root_ca_crl: Vec<u8>,
    pub pck_crl: Vec<u8>,
    pub tcb_info_issuer_chain: String,
    /// Intel's answer for the platform's patch-level table, whole (the signed object and its signature).
    pub tcb_answer: String,
    pub qe_identity_issuer_chain: String,
    pub qe_answer: String,
}

/// Lays the parts out as the verifier's Borsh `Collateral` (`near/mpc`, `tee-verifier-interface`): nine length-prefixed fields
/// and an absent optional certificate chain.
pub fn encode(parts: &Parts) -> Result<Vec<u8>, CollateralError> {
    let signature = |answer: &str| -> Result<Vec<u8>, CollateralError> {
        let v: serde_json::Value =
            serde_json::from_str(answer).map_err(|_| CollateralError::Answer("not JSON"))?;
        unhex(
            v["signature"]
                .as_str()
                .ok_or(CollateralError::Answer("no signature"))?,
        )
    };
    let mut out = Vec::new();
    put(&mut out, parts.pck_crl_issuer_chain.as_bytes());
    put(&mut out, &parts.root_ca_crl);
    put(&mut out, &parts.pck_crl);
    put(&mut out, parts.tcb_info_issuer_chain.as_bytes());
    put(
        &mut out,
        raw_object(&parts.tcb_answer, "tcbInfo")?.as_bytes(),
    );
    put(&mut out, &signature(&parts.tcb_answer)?);
    put(&mut out, parts.qe_identity_issuer_chain.as_bytes());
    put(
        &mut out,
        raw_object(&parts.qe_answer, "enclaveIdentity")?.as_bytes(),
    );
    put(&mut out, &signature(&parts.qe_answer)?);
    out.push(0);
    Ok(out)
}

async fn get(http: &reqwest::Client, url: String) -> Result<reqwest::Response, CollateralError> {
    let r = http
        .get(&url)
        .send()
        .await
        .map_err(|_| CollateralError::Fetch("could not be reached".into()))?;
    if !r.status().is_success() {
        return Err(CollateralError::Fetch(format!(
            "answered {}",
            r.status().as_u16()
        )));
    }
    Ok(r)
}

fn header(r: &reqwest::Response, name: &str) -> Result<String, CollateralError> {
    let raw = r
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or(CollateralError::Answer("an issuer chain is missing"))?;
    percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map(|s| s.into_owned())
        .map_err(|_| CollateralError::Answer("an issuer chain is not text"))
}

async fn bytes(r: reqwest::Response) -> Result<Vec<u8>, CollateralError> {
    let b = r
        .bytes()
        .await
        .map_err(|_| CollateralError::Fetch("an answer was cut short".into()))?;
    if b.len() > MAX_PART_BYTES {
        return Err(CollateralError::Answer("an answer is larger than expected"));
    }
    Ok(b.to_vec())
}

async fn text(r: reqwest::Response) -> Result<String, CollateralError> {
    String::from_utf8(bytes(r).await?).map_err(|_| CollateralError::Answer("an answer is not text"))
}

/// Fetches Intel's collateral for the platform this quote came from. Only the platform id and authority leave this computer.
pub async fn fetch(http: &reqwest::Client, quote: &[u8]) -> Result<Parts, CollateralError> {
    let (fmspc, ca) = quote_platform(quote)?;
    let r = get(
        http,
        format!("{INTEL_PCS}/tdx/certification/v4/tcb?fmspc={fmspc}"),
    )
    .await?;
    let tcb_info_issuer_chain = header(&r, "TCB-Info-Issuer-Chain")?;
    let tcb_answer = text(r).await?;
    let r = get(
        http,
        format!("{INTEL_PCS}/tdx/certification/v4/qe/identity"),
    )
    .await?;
    let qe_identity_issuer_chain = header(&r, "SGX-Enclave-Identity-Issuer-Chain")?;
    let qe_answer = text(r).await?;
    let r = get(
        http,
        format!("{INTEL_PCS}/sgx/certification/v4/pckcrl?ca={ca}&encoding=der"),
    )
    .await?;
    let pck_crl_issuer_chain = header(&r, "SGX-PCK-CRL-Issuer-Chain")?;
    let pck_crl = bytes(r).await?;
    let root_ca_crl = bytes(get(http, INTEL_ROOT_CRL.to_string()).await?).await?;
    Ok(Parts {
        pck_crl_issuer_chain,
        root_ca_crl,
        pck_crl,
        tcb_info_issuer_chain,
        tcb_answer,
        qe_identity_issuer_chain,
        qe_answer,
    })
}
