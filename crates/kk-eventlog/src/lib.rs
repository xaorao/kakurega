use kk_evidence::b64;
use kk_evidence::canon::{self, Value};
use kk_evidence::capsule::sha256_hex;
use kk_evidence::dsse;
use kk_evidence::keys::{public_key_b64, KeyId, Signer, TrustedKeys};
use kk_vault::rusqlite::params;
use kk_vault::Vault;
use std::fmt;
use std::sync::Mutex;

pub const HEAD_TYPE: &str = "application/vnd.kakurega.log-head+json";
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
pub const MAX_ENTRIES_VERIFIED: usize = 5_000_000;

#[derive(Debug)]
pub struct LogError(pub String);

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event log error: {}", self.0)
    }
}

impl std::error::Error for LogError {}

impl From<kk_vault::rusqlite::Error> for LogError {
    fn from(e: kk_vault::rusqlite::Error) -> Self {
        LogError(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datum {
    Str(String),
    Int(i64),
}

impl From<&str> for Datum {
    fn from(s: &str) -> Self {
        Datum::Str(s.to_string())
    }
}

impl From<String> for Datum {
    fn from(s: String) -> Self {
        Datum::Str(s)
    }
}

impl From<i64> for Datum {
    fn from(n: i64) -> Self {
        Datum::Int(n)
    }
}

impl From<usize> for Datum {
    fn from(n: usize) -> Self {
        Datum::Int(n as i64)
    }
}

pub struct EventLog {
    vault: Mutex<Vault>,
    signer: Signer,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS log (
    seq INTEGER PRIMARY KEY,
    entry TEXT NOT NULL,
    hash TEXT NOT NULL,
    head TEXT NOT NULL
);";

fn bad_text(s: &str) -> bool {
    s.chars().any(|c| c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
}

fn valid_kind(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

fn entry_text(
    seq: i64,
    at: i64,
    kind: &str,
    subject: &str,
    data: &[(&str, Datum)],
    prev: &str,
) -> String {
    let d = Value::Object(
        data.iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    match v {
                        Datum::Str(s) => Value::Str(s.clone()),
                        Datum::Int(n) => Value::Int(*n),
                    },
                )
            })
            .collect(),
    );
    canon::serialize(&Value::Object(vec![
        ("at".into(), Value::Int(at)),
        ("data".into(), d),
        ("kind".into(), Value::Str(kind.into())),
        ("prev".into(), Value::Str(prev.into())),
        ("seq".into(), Value::Int(seq)),
        ("subject".into(), Value::Str(subject.into())),
        ("v".into(), Value::Int(1)),
    ]))
}

fn head_payload(count: i64, head: &str) -> String {
    canon::serialize(&Value::Object(vec![
        ("count".into(), Value::Int(count)),
        ("head".into(), Value::Str(head.into())),
        ("v".into(), Value::Int(1)),
    ]))
}

impl EventLog {
    pub fn open(vault: Vault, signer_seed: [u8; 32]) -> Result<EventLog, LogError> {
        vault.connection().execute_batch(SCHEMA)?;
        Ok(EventLog {
            vault: Mutex::new(vault),
            signer: Signer::from_seed(signer_seed),
        })
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.signer.public()
    }

    pub fn append(
        &self,
        now: i64,
        kind: &str,
        subject: &str,
        data: &[(&str, Datum)],
    ) -> Result<u64, LogError> {
        if !valid_kind(kind) {
            return Err(LogError(
                "the kind must be 1-64 characters of a-z, 0-9, '.', '_' or '-'".into(),
            ));
        }
        if subject.len() > 128 || bad_text(subject) || data.len() > 16 {
            return Err(LogError(
                "the subject or the data is too long or contains control characters".into(),
            ));
        }
        for (k, v) in data {
            if !valid_kind(k) || matches!(v, Datum::Str(s) if s.len() > 200 || bad_text(s)) {
                return Err(LogError("a data key or value is not allowed".into()));
            }
        }
        let vault = self
            .vault
            .lock()
            .map_err(|_| LogError("the log lock is poisoned".into()))?;
        let c = vault.connection();
        c.execute_batch("SAVEPOINT append")?;
        let result = (|| -> Result<u64, LogError> {
            let last: Option<(i64, String)> = c
                .query_row(
                    "SELECT seq, hash FROM log ORDER BY seq DESC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map(Some)
                .or_else(|e| {
                    if matches!(e, kk_vault::rusqlite::Error::QueryReturnedNoRows) {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                })?;
            let (seq, prev) = match last {
                Some((s, h)) => (s + 1, h),
                None => (0, GENESIS.to_string()),
            };
            let entry = entry_text(seq, now, kind, subject, data, &prev);
            let hash = sha256_hex(entry.as_bytes());
            let env = dsse::sign(
                HEAD_TYPE,
                head_payload(seq + 1, &hash).as_bytes(),
                &self.signer,
            );
            c.execute(
                "INSERT INTO log(seq, entry, hash, head) VALUES (?1, ?2, ?3, ?4)",
                params![seq, entry, hash, dsse::encode(&env)],
            )?;
            Ok(seq as u64)
        })();
        match result {
            Ok(s) => {
                c.execute_batch("RELEASE append")?;
                Ok(s)
            }
            Err(e) => {
                c.execute_batch("ROLLBACK TO append; RELEASE append")?;
                Err(e)
            }
        }
    }

    pub fn len(&self) -> Result<u64, LogError> {
        let vault = self.vault.lock().map_err(|_| LogError("lock".into()))?;
        Ok(vault
            .connection()
            .query_row("SELECT count(*) FROM log", [], |r| r.get::<_, i64>(0))? as u64)
    }

    pub fn is_empty(&self) -> Result<bool, LogError> {
        Ok(self.len()? == 0)
    }

    pub fn head(&self) -> Result<Option<(u64, String)>, LogError> {
        let vault = self.vault.lock().map_err(|_| LogError("lock".into()))?;
        match vault.connection().query_row(
            "SELECT seq, hash FROM log ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        ) {
            Ok((seq, hash)) => Ok(Some((seq as u64 + 1, hash))),
            Err(kk_vault::rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<String>, LogError> {
        let vault = self.vault.lock().map_err(|_| LogError("lock".into()))?;
        let mut st = vault
            .connection()
            .prepare("SELECT entry FROM log ORDER BY seq DESC LIMIT ?1")?;
        let rows = st.query_map(params![limit.clamp(1, 500) as i64], |r| {
            r.get::<_, String>(0)
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn export(&self) -> Result<String, LogError> {
        let vault = self.vault.lock().map_err(|_| LogError("lock".into()))?;
        let mut st = vault
            .connection()
            .prepare("SELECT entry, hash, head FROM log ORDER BY seq")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut entries = Vec::new();
        for r in rows {
            let (entry, hash, head) = r?;
            entries.push(Value::Object(vec![
                ("entry".into(), Value::Str(entry)),
                ("hash".into(), Value::Str(hash)),
                ("head".into(), Value::Str(head)),
            ]));
        }
        Ok(canon::serialize(&Value::Object(vec![
            ("entries".into(), Value::Array(entries)),
            ("format".into(), Value::Str("kakurega-event-log".into())),
            (
                "public_key".into(),
                Value::Str(public_key_b64(&self.signer.public())),
            ),
            ("version".into(), Value::Int(1)),
        ])))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub count: u64,
    pub head: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogReport {
    pub count: u64,
    pub head: String,
    pub signer: String,
    pub signer_pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    NotJson,
    BadShape,
    BadKey,
    UntrustedSigner,
    SeqGap,
    PrevMismatch,
    HashMismatch,
    EntryNotCanonical,
    HeadMismatch,
    BadSignature,
    TooMany,
    RolledBack,
    AnchorMismatch,
}

impl Failure {
    pub fn as_str(self) -> &'static str {
        match self {
            Failure::NotJson => "log_not_json",
            Failure::BadShape => "log_bad_shape",
            Failure::BadKey => "bad_key",
            Failure::UntrustedSigner => "untrusted_signer",
            Failure::SeqGap => "sequence_gap",
            Failure::PrevMismatch => "chain_broken",
            Failure::HashMismatch => "entry_changed",
            Failure::EntryNotCanonical => "entry_not_canonical",
            Failure::HeadMismatch => "signed_head_mismatch",
            Failure::BadSignature => "bad_signature",
            Failure::TooMany => "too_many_entries",
            Failure::RolledBack => "log_shortened_since_anchor",
            Failure::AnchorMismatch => "log_differs_from_anchor",
        }
    }
}

fn field<'a>(members: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    members.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

pub fn verify_export(
    text: &str,
    pinned: Option<&[u8; 32]>,
    anchor: Option<&Anchor>,
) -> Result<LogReport, Failure> {
    let root = canon::parse_strict(text.as_bytes()).map_err(|_| Failure::NotJson)?;
    let Value::Object(top) = &root else {
        return Err(Failure::BadShape);
    };
    let (
        Some(Value::Str(format)),
        Some(Value::Int(1)),
        Some(Value::Str(pk)),
        Some(Value::Array(entries)),
    ) = (
        field(top, "format"),
        field(top, "version"),
        field(top, "public_key"),
        field(top, "entries"),
    )
    else {
        return Err(Failure::BadShape);
    };
    if format != "kakurega-event-log" || top.len() != 4 {
        return Err(Failure::BadShape);
    }
    if entries.len() > MAX_ENTRIES_VERIFIED {
        return Err(Failure::TooMany);
    }
    let raw = b64::decode(pk).map_err(|_| Failure::BadKey)?;
    let public: [u8; 32] = raw.try_into().map_err(|_| Failure::BadKey)?;
    let mut trusted = TrustedKeys::new();
    let key_id: KeyId = trusted.add(&public).ok_or(Failure::BadKey)?;
    if let Some(p) = pinned {
        if p != &public {
            return Err(Failure::UntrustedSigner);
        }
    }
    let mut prev = GENESIS.to_string();
    let mut anchor_seen = anchor.is_none();
    for (i, e) in entries.iter().enumerate() {
        let Value::Object(m) = e else {
            return Err(Failure::BadShape);
        };
        let (Some(Value::Str(entry)), Some(Value::Str(hash)), Some(Value::Str(head))) =
            (field(m, "entry"), field(m, "hash"), field(m, "head"))
        else {
            return Err(Failure::BadShape);
        };
        if m.len() != 3 {
            return Err(Failure::BadShape);
        }
        match canon::canonicalize(entry.as_bytes()) {
            Ok(c) if &c == entry => {}
            _ => return Err(Failure::EntryNotCanonical),
        }
        let Ok(Value::Object(em)) = canon::parse_strict(entry.as_bytes()) else {
            return Err(Failure::BadShape);
        };
        let (Some(Value::Int(seq)), Some(Value::Str(eprev))) =
            (field(&em, "seq"), field(&em, "prev"))
        else {
            return Err(Failure::BadShape);
        };
        if *seq != i as i64 {
            return Err(Failure::SeqGap);
        }
        if eprev != &prev {
            return Err(Failure::PrevMismatch);
        }
        let computed = sha256_hex(entry.as_bytes());
        if &computed != hash {
            return Err(Failure::HashMismatch);
        }
        let env = dsse::decode(head.as_bytes()).map_err(|_| Failure::BadShape)?;
        dsse::verify(&env, &trusted, &[HEAD_TYPE]).map_err(|e| match e {
            dsse::VerifyError::UnknownKey => Failure::UntrustedSigner,
            dsse::VerifyError::BadSignature => Failure::BadSignature,
            _ => Failure::BadShape,
        })?;
        if env.signatures[0].0 != key_id {
            return Err(Failure::UntrustedSigner);
        }
        let expected = head_payload(i as i64 + 1, &computed);
        if env.payload != expected.as_bytes() {
            return Err(Failure::HeadMismatch);
        }
        if let Some(a) = anchor {
            if a.count == i as u64 + 1 {
                if a.head != computed {
                    return Err(Failure::AnchorMismatch);
                }
                anchor_seen = true;
            }
        }
        prev = computed;
    }
    if !anchor_seen {
        return Err(Failure::RolledBack);
    }
    Ok(LogReport {
        count: entries.len() as u64,
        head: prev,
        signer: key_id.as_str().to_string(),
        signer_pinned: pinned.is_some(),
    })
}
