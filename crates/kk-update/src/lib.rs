use kk_evidence::dsse;
use serde::Deserialize;

pub mod download;
pub mod root;
use root::Root;

pub const MANIFEST_TYPE: &str = "application/vnd.kakurega.update.v1+json";
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 600 * 1024 * 1024;
const MAX_NOTES: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `major.minor.patch`, digits only, no leading zeros, nothing else (no pre-release tags, no build metadata).
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.split('.');
        let mut n = [0u32; 3];
        for slot in &mut n {
            let p = parts.next()?;
            let ok = !p.is_empty()
                && p.len() <= 4
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p == "0" || !p.starts_with('0'));
            if !ok {
                return None;
            }
            *slot = p.parse().ok()?;
        }
        parts.next().is_none().then_some(Version(n[0], n[1], n[2]))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    channel: String,
    version: String,
    /// The manifest stops being accepted after this Unix time, so an attacker who holds back newer ones cannot keep an
    /// old, vulnerable announcement alive for ever.
    not_after: u64,
    notes: String,
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: Version,
    pub notes: String,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The announcement is not signed by a key this installation trusts, or has been altered.
    NotSigned(String),
    Malformed(&'static str),
    WrongChannel,
    Expired,
    /// The announced version is lower than one already seen from the publisher (a replay of an old announcement).
    Rollback {
        seen: Version,
    },
    NoArtifactForThisComputer,
    BadArtifact(&'static str),
    /// The root this installation trusts has expired: no announcement is accepted until a newer root is applied.
    RootExpired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    UpToDate,
    Available(Offer),
}

pub struct Context<'a> {
    /// Who may announce: a list of keys and how many of them must sign.
    pub root: &'a Root,
    pub channel: &'a str,
    pub platform: &'a str,
    pub current: Version,
    /// The highest version any valid announcement has shown on this computer UNDER THE CURRENT ROOT (`root::Seen::effective`).
    pub highest_seen: Option<Version>,
    pub now: u64,
}

fn clean_notes(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n'))
        .filter(
            |c| !matches!(*c as u32, 0x200b..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069 | 0xfeff),
        )
        .take(MAX_NOTES)
        .collect()
}

fn check_artifact(a: &Artifact) -> Result<(), Refusal> {
    let platform_ok = !a.platform.is_empty()
        && a.platform.len() <= 40
        && a.platform
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !platform_ok {
        return Err(Refusal::BadArtifact("the platform name has odd characters"));
    }
    let hex = a.sha256.len() == 64
        && a.sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex {
        return Err(Refusal::BadArtifact(
            "the checksum is not 64 lowercase hex digits",
        ));
    }
    if a.size == 0 || a.size > MAX_ARTIFACT_BYTES {
        return Err(Refusal::BadArtifact("the size is zero or too large"));
    }
    if kk_fetch::validate_url(&a.url).is_err() {
        return Err(Refusal::BadArtifact(
            "the download address must be a plain https address with no credentials",
        ));
    }
    Ok(())
}

/// Decides what a signed announcement means for this computer. Nothing is downloaded or installed here.
pub fn check(envelope: &[u8], cx: &Context) -> Result<Verdict, Refusal> {
    if cx.root.is_expired(cx.now) {
        return Err(Refusal::RootExpired);
    }
    if envelope.len() > MAX_MANIFEST_BYTES {
        return Err(Refusal::Malformed("the announcement is too large"));
    }
    let env = dsse::decode(envelope).map_err(|e| Refusal::NotSigned(format!("{e:?}")))?;
    dsse::verify_threshold(&env, cx.root.keys(), cx.root.threshold, &[MANIFEST_TYPE])
        .map_err(|e| Refusal::NotSigned(format!("{e:?}")))?;
    let m: Manifest = serde_json::from_slice(&env.payload)
        .map_err(|_| Refusal::Malformed("the announcement has the wrong shape"))?;
    if m.channel != cx.channel {
        return Err(Refusal::WrongChannel);
    }
    if m.not_after <= cx.now {
        return Err(Refusal::Expired);
    }
    let version = Version::parse(&m.version)
        .ok_or(Refusal::Malformed("the version is not major.minor.patch"))?;
    if let Some(seen) = cx.highest_seen {
        if version < seen {
            return Err(Refusal::Rollback { seen });
        }
    }
    if version <= cx.current {
        return Ok(Verdict::UpToDate);
    }
    let mut mine = m.artifacts.iter().filter(|a| a.platform == cx.platform);
    let artifact = mine
        .next()
        .ok_or(Refusal::NoArtifactForThisComputer)?
        .clone();
    if mine.next().is_some() {
        return Err(Refusal::Malformed(
            "two downloads are listed for this computer",
        ));
    }
    check_artifact(&artifact)?;
    Ok(Verdict::Available(Offer {
        version,
        notes: clean_notes(&m.notes),
        artifact,
    }))
}

/// The version a valid announcement shows, for remembering the highest one seen. Only call this after `check` accepted it.
pub fn announced_version(envelope: &[u8]) -> Option<Version> {
    let env = dsse::decode(envelope).ok()?;
    let m: Manifest = serde_json::from_slice(&env.payload).ok()?;
    Version::parse(&m.version)
}

/// A downloaded file must be exactly what the signed announcement named.
pub fn verify_download(offer: &Offer, bytes: &[u8]) -> Result<(), Refusal> {
    if bytes.len() as u64 != offer.artifact.size {
        return Err(Refusal::BadArtifact(
            "the downloaded size differs from the signed size",
        ));
    }
    if kk_evidence::capsule::sha256_hex(bytes) != offer.artifact.sha256 {
        return Err(Refusal::BadArtifact(
            "the downloaded file does not match the signed checksum",
        ));
    }
    Ok(())
}

/// Publisher side: turns plain JSON into the canonical, signed announcement.
pub fn sign_manifest(json: &[u8], signer: &kk_evidence::keys::Signer) -> Result<Vec<u8>, String> {
    sign_manifest_multi(json, &[signer])
}

/// The same, signed by several publishers, for a root that asks for more than one.
pub fn sign_manifest_multi(
    json: &[u8],
    signers: &[&kk_evidence::keys::Signer],
) -> Result<Vec<u8>, String> {
    if signers.is_empty() || signers.len() > dsse::MAX_SIGNATURES {
        return Err("sign with 1 to 16 keys".into());
    }
    let canon =
        kk_evidence::canon::canonicalize(json).map_err(|e| format!("not canonical JSON: {e:?}"))?;
    let m: Manifest =
        serde_json::from_slice(canon.as_bytes()).map_err(|e| format!("wrong shape: {e}"))?;
    Version::parse(&m.version).ok_or("the version is not major.minor.patch")?;
    for a in &m.artifacts {
        check_artifact(a).map_err(|e| format!("{e:?}"))?;
    }
    let mut env = dsse::sign(MANIFEST_TYPE, canon.as_bytes(), signers[0]);
    for s in &signers[1..] {
        env.signatures.push((
            s.key_id(),
            s.sign(&dsse::pae(MANIFEST_TYPE, canon.as_bytes())),
        ));
    }
    Ok(dsse::encode(&env).into_bytes())
}
