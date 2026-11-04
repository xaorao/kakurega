#![allow(dead_code)]
use kk_effects::{Class, Grant, Kind, NewIntent, Remote, SendResult};
use kk_vault::VaultKey;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("os randomness");
    b
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex32(s: &str) -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, byte) in k.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("hex");
    }
    k
}

pub fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kk-effects-{tag}-{}", hex(&random_bytes::<6>())));
    fs::create_dir_all(&dir).expect("dir");
    dir
}

pub struct FileRemote {
    pub log: PathBuf,
    pub idempotent: bool,
    pub readback: bool,
    pub unknown_outcome_on_send: bool,
}

impl FileRemote {
    pub fn new(dir: &Path, idempotent: bool, readback: bool) -> Self {
        FileRemote {
            log: dir.join("remote.log"),
            idempotent,
            readback,
            unknown_outcome_on_send: false,
        }
    }

    pub fn lines(&self) -> Vec<(String, String)> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                l.split_once('\t')
                    .map(|(k, m)| (k.to_string(), m.to_string()))
            })
            .collect()
    }

    pub fn applied(&self, marker: &str) -> usize {
        self.lines().iter().filter(|(_, m)| m == marker).count()
    }
}

impl Remote for FileRemote {
    fn send(&self, key: Option<&str>, marker: &str, _payload: &[u8]) -> SendResult {
        let key_s = key.unwrap_or("-");
        if self.idempotent && key.is_some() && self.lines().iter().any(|(k, _)| k == key_s) {
            return SendResult::Delivered;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .expect("remote log");
        writeln!(f, "{key_s}\t{marker}").expect("write");
        f.sync_all().expect("sync");
        if self.unknown_outcome_on_send {
            return SendResult::OutcomeUnknown;
        }
        SendResult::Delivered
    }

    fn read_back(&self, marker: &str) -> Result<bool, String> {
        if !self.readback {
            return Err("this remote cannot be read back".into());
        }
        Ok(self.applied(marker) > 0)
    }
}

pub fn intent_for(class: Class, id: &str) -> (NewIntent, Kind) {
    let kind = match class {
        Class::E1 => Kind::RemoteApi,
        Class::E2 => Kind::Issue,
        Class::E3 => Kind::Email,
        Class::E4 => Kind::Funds,
    };
    (
        NewIntent {
            intent_id: id.to_string(),
            kind,
            class,
            idempotency_key: if class == Class::E1 {
                Some(format!("key-{id}"))
            } else {
                None
            },
            key_window_secs: if class == Class::E1 { Some(3600) } else { None },
            cooldown_secs: if class == Class::E4 { 600 } else { 0 },
        },
        kind,
    )
}

pub fn grant_for(class: Class, kind: Kind) -> Grant {
    let mut g = if matches!(kind, Kind::Funds | Kind::PermissionChange) {
        Grant::specific("grant-1", kind)
    } else {
        Grant::general("grant-1")
    };
    if class == Class::E4 {
        g.exact_amount = Some("12.50 USD".into());
        g.recipient = Some("acme".into());
        g.purpose = Some("invoice 7".into());
    }
    g
}

pub fn vault_key(bytes: [u8; 32]) -> VaultKey {
    VaultKey::from_bytes(bytes)
}
