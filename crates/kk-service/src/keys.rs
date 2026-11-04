use kk_evidence::keys::Signer;
use kk_vault::{Vault, VaultKey};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
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
    let path = data_dir.join("vault.key");
    if let Ok(text) = fs::read_to_string(&path) {
        if let Some(k) = unhex(text.trim()) {
            return Ok(k);
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "vault.key is damaged",
        ));
    }
    let k = random32();
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    f.write_all(hex(&k).as_bytes())?;
    Ok(k)
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
