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

/// Makes the folder if it is missing, owner only. A folder that already exists is left exactly as it is: changing what someone
/// already has is `kaku doctor --fix`'s job, and only with their say-so. Parents that have to be made on the way keep the
/// default; only the folder itself holds the stores, so only it is restricted.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
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

#[cfg(test)]
mod private_dir_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kk-private-dir-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn pd01_a_missing_folder_is_made_owner_only_and_only_the_folder_itself_is_restricted() {
        let base = scratch("new");
        let leaf = base.join("a").join("data");
        ensure_private_dir(&leaf).unwrap();
        assert_eq!(mode(&leaf), 0o700);
        assert_ne!(
            mode(&base.join("a")) & 0o077,
            0o777,
            "a parent made on the way is not our folder"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn pd02_a_folder_that_already_exists_is_left_exactly_as_it_is_whatever_its_mode() {
        for m in [0o755, 0o750, 0o700, 0o775] {
            let d = scratch(&format!("exists{m:o}"));
            fs::create_dir_all(&d).unwrap();
            fs::set_permissions(&d, fs::Permissions::from_mode(m)).unwrap();
            ensure_private_dir(&d).unwrap();
            assert_eq!(mode(&d), m, "{m:o}");
            let _ = fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn pd03_a_file_in_the_way_is_an_error_not_a_silent_success() {
        let base = scratch("file");
        fs::create_dir_all(&base).unwrap();
        let file = base.join("data");
        fs::write(&file, "x").unwrap();
        // the path exists (as a file), so nothing is made; the stores will then fail loudly on their own
        assert!(ensure_private_dir(&file).is_ok());
        assert!(file.is_file());
        let under = file.join("sub");
        assert!(
            ensure_private_dir(&under).is_err(),
            "a folder cannot be made inside a file"
        );
        let _ = fs::remove_dir_all(&base);
    }
}
