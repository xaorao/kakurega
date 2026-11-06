//! The key that opens every encrypted store: created once and whole, never replaced while it could still matter, and put
//! back from its copy when the file is cut short.

use kk_service::keys::vault_key;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-keys-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn mode(p: &std::path::Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn ky01_a_new_folder_gets_one_key_file_with_a_copy_and_the_same_key_every_time() {
    let d = dir("new");
    let k = vault_key(&d).unwrap();
    assert_ne!(k, [0u8; 32]);
    let main = d.join("vault.key");
    let bak = d.join("vault.key.bak");
    assert_eq!(fs::read(&main).unwrap().len(), 64, "hex of 32 bytes");
    assert_eq!(fs::read(&main).unwrap(), fs::read(&bak).unwrap());
    assert_eq!((mode(&main), mode(&bak)), (0o600, 0o600));
    assert_eq!(vault_key(&d).unwrap(), k);
    assert_eq!(vault_key(&d).unwrap(), k);
    let tmp: Vec<_> = fs::read_dir(&d)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(tmp.is_empty());
}

#[test]
fn ky02_programs_starting_together_end_up_with_one_key() {
    for round in 0..20 {
        let d = dir(&format!("race{round}"));
        let handles: Vec<_> = (0..12)
            .map(|_| {
                let d = d.clone();
                std::thread::spawn(move || vault_key(&d).unwrap())
            })
            .collect();
        let keys: Vec<[u8; 32]> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(
            keys.iter().all(|k| *k == keys[0]),
            "round {round}: two keys were handed out"
        );
        assert_eq!(vault_key(&d).unwrap(), keys[0]);
    }
}

#[test]
fn ky03_a_key_file_cut_short_or_emptied_is_put_back_from_its_copy() {
    let d = dir("restore");
    let k = vault_key(&d).unwrap();
    let main = d.join("vault.key");
    for damage in [
        &b""[..],
        &b"abc"[..],
        b"zz".repeat(32).as_slice(),
        &b"0123456789abcdef"[..],
    ] {
        fs::write(&main, damage).unwrap();
        assert_eq!(vault_key(&d).unwrap(), k, "{damage:?}");
        assert_eq!(
            fs::read(&main).unwrap().len(),
            64,
            "the file is whole again"
        );
        assert_eq!(mode(&main), 0o600);
    }
    fs::remove_file(&main).unwrap();
    assert_eq!(vault_key(&d).unwrap(), k, "even when the file is gone");
}

#[test]
fn ky04_a_damaged_key_with_nothing_encrypted_yet_is_replaced_so_a_first_start_that_crashed_is_not_a_dead_end(
) {
    let d = dir("firststart");
    fs::write(d.join("vault.key"), b"").unwrap();
    let k = vault_key(&d).unwrap();
    assert_eq!(fs::read(d.join("vault.key")).unwrap().len(), 64);
    assert_eq!(vault_key(&d).unwrap(), k);
    // both copies bad
    fs::write(d.join("vault.key"), b"junk").unwrap();
    fs::write(d.join("vault.key.bak"), b"junk").unwrap();
    let k2 = vault_key(&d).unwrap();
    assert_eq!(vault_key(&d).unwrap(), k2);
}

#[test]
fn ky05_a_damaged_key_beside_a_database_is_reported_and_nothing_is_overwritten() {
    let d = dir("withdb");
    let k = vault_key(&d).unwrap();
    fs::write(d.join("state.db"), b"encrypted bytes").unwrap();
    fs::write(d.join("vault.key"), b"junk").unwrap();
    fs::write(d.join("vault.key.bak"), b"junk too").unwrap();
    let e = vault_key(&d).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
    assert!(e.to_string().contains("damaged"), "{e}");
    assert_eq!(
        fs::read(d.join("vault.key")).unwrap(),
        b"junk",
        "left as it was"
    );
    assert_eq!(fs::read(d.join("vault.key.bak")).unwrap(), b"junk too");
    // but with a good copy the key comes back even beside a database
    fs::write(
        d.join("vault.key.bak"),
        k.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    )
    .unwrap();
    assert_eq!(vault_key(&d).unwrap(), k);
}
