use kk_evidence::keys::Signer;
use kk_vault::{Vault, VaultKey};
use std::fs;
use std::path::Path;

pub struct Keys {
    pub signer: Signer,
    pub commitment_key: [u8; 32],
    pub log_seed: [u8; 32],
}

fn random32() -> [u8; 32] {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("operating system randomness");
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn vault_key(data_dir: &Path) -> std::io::Result<[u8; 32]> {
    use kk_vault::durable;
    let path = data_dir.join("vault.key");
    let valid = |b: &[u8]| {
        std::str::from_utf8(b)
            .ok()
            .and_then(|t| unhex(t.trim()))
            .is_some()
    };
    let parse = |b: &[u8]| std::str::from_utf8(b).ok().and_then(|t| unhex(t.trim()));
    match durable::read_checked(&path, valid, 0o600) {
        Ok(Some((bytes, _))) => parse(&bytes).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "vault.key is damaged")
        }),
        Ok(None) => create_key(&path),
        // A key file that is unreadable and has no usable copy is only replaced when nothing was ever encrypted with it:
        // with a database beside it the data would be lost for good, so that is reported and left alone.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData && !has_database(data_dir) => {
            let k = random32();
            durable::write_atomic_with_backup(&path, hex(&k).as_bytes(), 0o600)?;
            Ok(k)
        }
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "vault.key is damaged",
        )),
        Err(e) => Err(e),
    }
}

/// The key for a new folder. Of two programs starting together, one creates it and the other reads what that one wrote.
fn create_key(path: &Path) -> std::io::Result<[u8; 32]> {
    let k = random32();
    if kk_vault::durable::create_atomic(path, hex(&k).as_bytes(), 0o600)? {
        // a second copy, so a key file cut short or emptied later can be put back
        let _ = kk_vault::durable::write_atomic(
            &kk_vault::durable::backup_path(path),
            hex(&k).as_bytes(),
            0o600,
        );
        return Ok(k);
    }
    let text = fs::read_to_string(path)?;
    unhex(text.trim())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "vault.key is damaged"))
}

fn has_database(data_dir: &Path) -> bool {
    fs::read_dir(data_dir).is_ok_and(|d| {
        d.flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".db"))
    })
}

pub fn open_vault(data_dir: &Path, file: &str) -> Result<Vault, String> {
    let key = vault_key(data_dir).map_err(|e| e.to_string())?;
    let path = data_dir.join(file);
    let k = VaultKey::from_bytes(key);
    if path.exists() {
        Vault::open(&path, &k).map_err(|e| e.to_string())
    } else {
        Vault::create(&path, &k).map_err(|e| e.to_string())
    }
}

pub fn load_keys(data_dir: &Path) -> Result<Keys, String> {
    let vault = open_vault(data_dir, "state.db")?;
    let c = vault.connection();
    c.execute_batch("CREATE TABLE IF NOT EXISTS kv(name TEXT PRIMARY KEY, value BLOB NOT NULL);")
        .map_err(|e| e.to_string())?;
    let get = |name: &str| -> Option<[u8; 32]> {
        c.query_row("SELECT value FROM kv WHERE name = ?1", [name], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .ok()
        .and_then(|v| <[u8; 32]>::try_from(v).ok())
    };
    let ensure = |name: &str| -> Result<[u8; 32], String> {
        if let Some(v) = get(name) {
            return Ok(v);
        }
        let v = random32();
        c.execute(
            "INSERT INTO kv(name, value) VALUES (?1, ?2)",
            (name, v.to_vec()),
        )
        .map_err(|e| e.to_string())?;
        Ok(v)
    };
    let seed = ensure("signing_seed")?;
    let commitment_key = ensure("commitment_key")?;
    let log_seed = ensure("log_signing_seed")?;
    Ok(Keys {
        signer: Signer::from_seed(seed),
        commitment_key,
        log_seed,
    })
}
