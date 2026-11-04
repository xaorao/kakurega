use hmac::{Hmac, Mac};
use kk_evidence::canon::{self, Value};
use kk_evidence::capsule::sha256_hex;
use kk_types::label::{Label, Level};
use sha2::Sha256;
use std::fs;
use std::path::Path;

pub const MAX_FILE_BYTES: u64 = 2 << 20;
pub const MAX_FILES: usize = 5000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub label: Label,
    pub sha256: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Imported {
    pub files: Vec<ImportedFile>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub canonical: String,
    pub digest: String,
    pub file_count: usize,
}

const SECRET_NAMES: &[&str] = &[
    ".env",
    "id_rsa",
    "id_ed25519",
    "id_dsa",
    ".npmrc",
    ".pypirc",
    ".netrc",
];
const SECRET_SUFFIXES: &[&str] = &[".pem", ".key", ".p12", ".pfx", ".env", ".keystore", ".jks"];
const INJECTION_PHRASES: &[&str] = &[
    "ignore all previous instructions",
    "ignore previous instructions",
    "disregard the above",
    "system notice",
    "for ai assistants",
    "for automated tools",
    "do not mention this",
    "send its entire contents",
];

fn classify(path: &str, text: Option<&str>, default_level: Level) -> (Label, Vec<String>) {
    let mut notes = Vec::new();
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let mut secret = SECRET_NAMES.iter().any(|n| name == *n)
        || SECRET_SUFFIXES.iter().any(|s| name.ends_with(s));
    if secret {
        notes.push("file name marks a secret".into());
    }
    if let Some(t) = text {
        if kk_secrets::looks_secret(t).is_some() {
            if !secret {
                notes.push("content looks like a secret".into());
            }
            secret = true;
        }
        let lower = t.to_ascii_lowercase();
        if INJECTION_PHRASES.iter().any(|p| lower.contains(p)) {
            notes.push("contains text addressed to AI tools (possible prompt injection)".into());
        }
    }
    let mut label = Label::new(default_level).untrusted();
    if secret {
        label = Label::new(Level::Restricted)
            .with_compartment("secrets")
            .untrusted();
    }
    (label, notes)
}

fn walk(root: &Path, dir: &Path, out: &mut Imported, default_level: Level) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        let name = e.file_name().to_string_lossy().to_string();
        if meta.file_type().is_symlink() {
            out.skipped.push(Skipped {
                path: rel,
                reason: "symbolic link (never followed)".into(),
            });
            continue;
        }
        if meta.is_dir() {
            if matches!(
                name.as_str(),
                ".git" | "node_modules" | "target" | "__pycache__" | ".venv" | "dist"
            ) {
                out.skipped.push(Skipped {
                    path: format!("{rel}/"),
                    reason: "build or vendor directory".into(),
                });
                continue;
            }
            walk(root, &p, out, default_level);
            continue;
        }
        if !meta.is_file() {
            out.skipped.push(Skipped {
                path: rel,
                reason: "not a regular file".into(),
            });
            continue;
        }
        if out.files.len() >= MAX_FILES {
            out.skipped.push(Skipped {
                path: rel,
                reason: "file limit reached".into(),
            });
            continue;
        }
        if meta.len() > MAX_FILE_BYTES {
            out.skipped.push(Skipped {
                path: rel,
                reason: "larger than the 2 MiB limit".into(),
            });
            continue;
        }
        let Ok(bytes) = fs::read(&p) else {
            out.skipped.push(Skipped {
                path: rel,
                reason: "unreadable".into(),
            });
            continue;
        };
        if bytes.contains(&0) {
            out.skipped.push(Skipped {
                path: rel,
                reason: "binary file".into(),
            });
            continue;
        }
        let text = std::str::from_utf8(&bytes).ok();
        if text.is_none() {
            out.skipped.push(Skipped {
                path: rel,
                reason: "not UTF-8 text".into(),
            });
            continue;
        }
        let (label, notes) = classify(&rel, text, default_level);
        let sha256 = sha256_hex(&bytes);
        out.files.push(ImportedFile {
            path: rel,
            bytes,
            label,
            sha256,
            notes,
        });
    }
}

pub fn scan_tree(root: &Path, default_level: Level) -> Imported {
    let mut out = Imported {
        files: Vec::new(),
        skipped: Vec::new(),
    };
    walk(root, root, &mut out, default_level);
    out
}

pub fn manifest(files: &[ImportedFile]) -> Manifest {
    let mut sorted: Vec<&ImportedFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let entries: Vec<Value> = sorted
        .iter()
        .map(|f| {
            Value::Object(vec![
                ("path".into(), Value::Str(f.path.clone())),
                ("sha256".into(), Value::Str(f.sha256.clone())),
                ("bytes".into(), Value::Int(f.bytes.len() as i64)),
                (
                    "restricted".into(),
                    Value::Bool(f.label.level == Level::Restricted),
                ),
            ])
        })
        .collect();
    let canonical = canon::serialize(&Value::Object(vec![
        ("kind".into(), Value::Str("source-manifest".into())),
        ("version".into(), Value::Int(1)),
        ("files".into(), Value::Array(entries)),
    ]));
    let digest = sha256_hex(canonical.as_bytes());
    Manifest {
        canonical,
        digest,
        file_count: files.len(),
    }
}

pub fn keyed_commitment(key: &[u8; 32], manifest: &Manifest) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(b"kakurega/source-commitment/v1\n");
    mac.update(manifest.canonical.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
