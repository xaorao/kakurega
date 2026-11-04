use kk_effects::{Class, Grant, Kind, Ledger, NewIntent, Remote, SendResult, State, SystemClock};
use std::fs;
use std::path::{Path, PathBuf};

pub struct CapsuleFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl Remote for CapsuleFile {
    fn send(&self, _key: Option<&str>, _marker: &str, _payload: &[u8]) -> SendResult {
        if let Ok(existing) = fs::read(&self.path) {
            return if existing == self.bytes {
                SendResult::Delivered
            } else {
                SendResult::Rejected("a different capsule already exists at this path".into())
            };
        }
        let tmp = self.path.with_extension("kkg.part");
        let write = fs::write(&tmp, &self.bytes).and_then(|_| fs::rename(&tmp, &self.path));
        match write {
            Ok(()) => SendResult::Delivered,
            Err(_) => SendResult::OutcomeUnknown,
        }
    }

    fn read_back(&self, _marker: &str) -> Result<bool, String> {
        Ok(fs::read(&self.path)
            .map(|b| b == self.bytes)
            .unwrap_or(false))
    }
}

pub fn deliver(
    ledger: &mut Ledger<SystemClock>,
    intent_id: &str,
    grant_ref: &str,
    digest: &str,
    file: &CapsuleFile,
) -> Result<State, String> {
    if ledger.get(intent_id).is_err() {
        ledger
            .record_intent(&NewIntent {
                intent_id: intent_id.to_string(),
                kind: Kind::Release,
                class: Class::E1,
                idempotency_key: Some(digest.to_string()),
                key_window_secs: Some(10 * 365 * 24 * 3600),
                cooldown_secs: 0,
            })
            .map_err(|e| e.to_string())?;
        ledger
            .authorize(intent_id, Kind::Release, &Grant::general(grant_ref))
            .map_err(|e| e.to_string())?;
    }
    ledger
        .execute(intent_id, file, &[])
        .map_err(|e| e.to_string())
}

pub fn capsule_dir(data_dir: &Path) -> std::io::Result<PathBuf> {
    let d = data_dir.join("capsules");
    fs::create_dir_all(&d)?;
    Ok(d)
}
