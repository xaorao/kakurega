//! A grid of secret shapes with values that are new on every run, and ordinary text that looks like them (hashes, ids, image data),
//! graded by whether the detector flags each one. A miss lets a secret reach a model; a false alarm withholds ordinary text.

use kk_secrets::{looks_secret, scan};

fn fresh(alpha: &str, n: usize, seed: u64) -> String {
    let c: Vec<char> = alpha.chars().collect();
    let mut s = seed | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            c[(s % c.len() as u64) as usize]
        })
        .collect()
}

#[test]
fn sg01_every_secret_shape_is_flagged_and_no_ordinary_look_alike_is() {
    let a = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let hexs = "0123456789abcdef";
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let secrets: Vec<(&str, String)> = vec![
        ("stripe live", format!("{}{}{}", "sk_li", "ve_", fresh(a, 24, t))),
        ("stripe restricted", format!("{}{}{}", "rk_li", "ve_", fresh(a, 24, t + 1))),
        ("slack bot", format!("{}{}-{}-{}", "xox", "b", fresh("0123456789", 12, t + 2), fresh(a, 24, t + 3))),
        ("google api", format!("AIza{}", fresh(&format!("{a}_-"), 35, t + 4))),
        ("sendgrid", format!("SG.{}.{}", fresh(a, 22, t + 5), fresh(a, 43, t + 6))),
        ("npm token", format!("npm_{}", fresh(a, 36, t + 7))),
        ("pypi token", format!("pypi-AgEIcHlwaS5vcmc{}", fresh(a, 60, t + 8))),
        ("jwt", format!("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.{}", fresh(a, 43, t + 9))),
        ("postgres url", format!("postgres://app:{}@db.internal:5432/prod", fresh(a, 16, t + 10))),
        ("basic auth url", format!("https://deploy:{}@git.corp.internal/repo.git", fresh(a, 18, t + 11))),
        ("json private key", format!("{{\"private_key\": \"{}{}\\n{}\\n{}\"}}", "-----BEGIN PRIV", "ATE KEY-----", fresh(a, 64, t + 12), "-----END PRIVATE KEY-----")),
        ("bearer header", format!("curl -H 'Authorization: Bearer {}' https://api.example.com", fresh(a, 40, t + 13))),
        ("yaml password", format!("database:\n  password: {}4\n", fresh(a, 19, t + 14))),
        ("env secret", format!("SECRET_KEY={}", fresh(a, 32, t + 15))),
        ("aws secret", format!("aws_secret_access_key = {}", fresh(&format!("{a}/+"), 40, t + 16))),
        ("github fine-grained", format!("github_pat_{}", fresh(a, 59, t + 17))),
        ("gitlab", format!("glpat-{}", fresh(a, 20, t + 18))),
        ("azure conn string", format!("DefaultEndpointsProtocol=https;AccountName=acct;AccountKey={}==;EndpointSuffix=core.windows.net", fresh(a, 86, t + 19))),
        ("twilio", format!("SK{}", fresh(hexs, 32, t + 20))),
        ("openai", format!("sk-{}", fresh(a, 48, t + 21))),
        ("mailgun", format!("key-{}", fresh(hexs, 32, t + 22))),
        ("password sentence", format!("my password is {}7 please keep it", fresh(a, 13, t + 23))),
    ];
    let ordinary: Vec<(&str, String)> = vec![
        ("git sha", format!("commit {}", fresh(hexs, 40, t + 30))),
        (
            "uuid",
            "id 123e4567-e89b-12d3-a456-426614174000".to_string(),
        ),
        (
            "sha256 digest",
            format!("sha256: {}", fresh(hexs, 64, t + 31)),
        ),
        (
            "base64 image",
            format!(
                "data:image/png;base64,{}",
                fresh(&format!("{a}+/"), 120, t + 32)
            ),
        ),
        (
            "long word list",
            "the quick brown fox jumps over the lazy dog and keeps running".to_string(),
        ),
        (
            "package version",
            "serde = \"1.0.197\" tokio = { version = \"1.36\" }".to_string(),
        ),
        (
            "css colour",
            "color: #1a2b3c; background: #ffeedd;".to_string(),
        ),
        (
            "file path",
            "/Users/alice/projects/payments-service/src/session.py".to_string(),
        ),
        (
            "url without credentials",
            "https://example.com/docs/getting-started?lang=en".to_string(),
        ),
        (
            "sentence with word password",
            "The password policy requires twelve characters and a symbol.".to_string(),
        ),
    ];
    let mut missed = vec![];
    for (n, v) in &secrets {
        if scan(v).is_empty() && looks_secret(v).is_none() {
            missed.push(*n);
        }
    }
    let mut alarms = vec![];
    for (n, v) in &ordinary {
        if !scan(v).is_empty() || looks_secret(v).is_some() {
            alarms.push(*n);
        }
    }
    assert!(missed.is_empty(), "not flagged: {missed:?}");
    assert!(alarms.is_empty(), "flagged by mistake: {alarms:?}");
}

#[test]
fn sg02_a_random_value_with_a_digit_after_a_password_name_is_always_flagged_whatever_its_letters() {
    // Found by sweeping random values: about a third of mixed-case values with one or two digits were taken for a CamelCase
    // name and let through (`password: YqSYcZaiM9aIjsnGIS5X`).
    let a = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut missed = Vec::new();
    for seed in 1..1500u64 {
        let mut v = fresh(a, 19, seed * 7919);
        v.push(char::from(b'0' + (seed % 10) as u8));
        for text in [
            format!("database:\n  password: {v}\n"),
            format!("my password is {v} please keep it"),
            format!("DB_SECRET = \"{v}\""),
        ] {
            if scan(&text).is_empty() && looks_secret(&text).is_none() {
                missed.push(text);
            }
        }
    }
    assert!(
        missed.len() <= 3,
        "{} of 4500 not flagged, for example {:?}",
        missed.len(),
        missed.iter().take(3).collect::<Vec<_>>()
    );
}

#[test]
fn sg03_names_made_of_words_are_still_names_and_not_values() {
    for name in [
        "MyPasswordField",
        "getAccessToken",
        "getAccessToken2",
        "CredentialAccountStatus",
        "HttpClientConfiguration",
        "AtomicU64",
        "AtomicU32",
        "tokenizerFactory",
        "PasswordResetService",
        "SessionStore",
        "secretManager",
    ] {
        for text in [
            format!("password: {name}"),
            format!("token = {name}"),
            format!("let secret: {name} = make();"),
            format!("api_key: {name}"),
        ] {
            assert!(
                scan(&text).is_empty() && looks_secret(&text).is_none(),
                "a name was taken for a secret: {text}"
            );
        }
    }
}

#[test]
fn sg04_a_single_word_with_digits_or_letters_that_only_look_like_words_is_a_password_not_a_name() {
    // one capitalised word and a couple of digits is how people write passwords; a name needs two words
    for v in ["Sunshine12", "Welcome7", "Dragon99", "Monkey42x"] {
        let text = format!("password: {v}");
        assert!(
            !scan(&text).is_empty() || looks_secret(&text).is_some(),
            "a password was taken for a name: {text}"
        );
    }
    // alternating small and capital letters make "words" of one letter: not a name
    for v in [
        "aBcDeFgHiJkLmNoP7",
        "xYzAbCdEfGhIjKlM3",
        "qWeRtYuIoPaSdFgH9",
    ] {
        let text = format!("password: {v}");
        assert!(
            !scan(&text).is_empty() || looks_secret(&text).is_some(),
            "random-looking letters were taken for a name: {text}"
        );
    }
}

#[test]
fn sg05_an_aws_style_secret_is_flagged_even_when_its_slashes_make_it_look_like_a_path() {
    // the alphabet of an AWS secret access key includes `/` and `+`; a value that starts with `/` or has two slashes was taken
    // for a file path (about one key in eleven)
    let a = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/+";
    let (mut path_shaped, mut missed) = (0, Vec::new());
    for seed in 1..3000u64 {
        let v = fresh(a, 40, seed * 104729);
        let slashes = v.chars().filter(|c| *c == '/').count();
        if !(v.starts_with('/') || (slashes >= 2 && !v.contains('+'))) {
            continue;
        }
        path_shaped += 1;
        let text = format!("aws_secret_access_key = {v}");
        if scan(&text).is_empty() && looks_secret(&text).is_none() {
            missed.push(text);
        }
    }
    assert!(
        path_shaped >= 40,
        "the sweep produced only {path_shaped} path-shaped keys"
    );
    assert!(
        missed.len() <= 1,
        "{} of {path_shaped} not flagged, for example {:?}",
        missed.len(),
        missed.iter().take(2).collect::<Vec<_>>()
    );
    // a real path after the same name is still a path
    for path in [
        "/usr/local/etc/credentials",
        "/home/app/.aws/credentials",
        "~/.config/app/secret.key",
        "./secrets/db/password.txt",
    ] {
        let text = format!("aws_secret_access_key = {path}");
        assert!(
            scan(&text).is_empty() && looks_secret(&text).is_none(),
            "a path was taken for a secret: {text}"
        );
    }
}

#[test]
fn sg06_a_marker_word_inside_a_random_token_is_chance_but_example_is_never_chance() {
    let a = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    for (n, word) in ["Test", "mock", "FAKE", "Demo"].into_iter().enumerate() {
        let v = format!(
            "{}{word}{}7",
            fresh(a, 13, 31 + n as u64),
            fresh(a, 14, 77 + n as u64)
        );
        let text = format!("SECRET_KEY={v}");
        assert!(
            !scan(&text).is_empty() || looks_secret(&text).is_some(),
            "a random token that spells {word} by chance was let through: {text}"
        );
    }
    // a value that says what it is stays a sample: the sample key from the AWS documentation, built from pieces
    let sample = ["wJalrXUtnFEMI/K7MDENG/bPxRfi", "CY", "EXAMPLE", "KEY"].concat();
    let text = format!("aws_secret_access_key = {sample}");
    assert!(
        scan(&text).is_empty() && looks_secret(&text).is_none(),
        "a documented sample was flagged: {text}"
    );
    // and a short fixture is still a fixture
    for fixture in [
        "test-api-key-12345",
        "fake_token_for_tests",
        "demo-secret-value",
        "mock-password-1",
    ] {
        let text = format!("api_key = \"{fixture}\"");
        assert!(
            scan(&text).is_empty() && looks_secret(&text).is_none(),
            "a fixture was flagged: {text}"
        );
    }
}

#[test]
fn sg07_what_does_not_look_like_a_random_token_keeps_its_old_treatment() {
    // paths with capitals and digits, and a path whose letters vary a lot, are still paths
    for path in [
        "/home/deploy/Releases2024/Shared7/Config",
        "/srv/AppData/Backups2024/NightlyDump7",
        "/home/DeployUser/Releases/SharedConfigBackupDirectory",
        "/opt/Company/ProductSuite/WorkerPool/Credentials",
    ] {
        let text = format!("aws_secret_access_key = {path}");
        assert!(
            scan(&text).is_empty() && looks_secret(&text).is_none(),
            "a path was taken for a secret: {text}"
        );
    }
    // a twenty-character value that spells a marker word is still treated as a fixture; only a value of 24 or more is judged as a token
    let twenty = format!(
        "{}Test{}",
        fresh(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            8,
            5
        ),
        fresh(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            7,
            9
        )
    );
    assert_eq!(twenty.len(), 19);
    let text = format!("SECRET_KEY={twenty}7");
    assert!(
        scan(&text).is_empty() && looks_secret(&text).is_none(),
        "the 20-character boundary moved: {text}"
    );
    // a long value with a marker word and no digit at all, or low variety, is not shaped like a random token
    for v in [
        "TestingTheDeploymentPipelineStage",
        "mockServerResponseHandlerFactoryImpl",
        "DemoAccountSetupHelperClassName",
    ] {
        let text = format!("SECRET_KEY={v}");
        assert!(
            scan(&text).is_empty() && looks_secret(&text).is_none(),
            "a name with a marker word was flagged: {text}"
        );
    }
}

#[test]
fn sg08_the_edges_of_what_counts_as_a_random_token_are_pinned() {
    let flagged = |text: &str| !scan(text).is_empty() || looks_secret(text).is_some();
    // a path is judged by how it begins: real directory names first, whatever an id or a timestamp further on looks like
    assert!(!flagged(
        "aws_secret_access_key = /home/deploy/Releases2024/Shared7/Backup20240131/Config"
    ));
    assert!(!flagged(
        "aws_secret_access_key = /home/deploy/Releases/Run8473920165384/Shared/Config"
    ));
    assert!(!flagged("aws_secret_access_key = /var/lib/company/Production/Snapshot847392016538421/Warehouse/Exports"));
    // a root-level name is a path too
    assert!(!flagged(
        "aws_secret_access_key = /ReleaseManagementProductionPipeline2024"
    ));
    // an identifier-like segment among the first two is not a name
    assert!(flagged(
        "aws_secret_access_key = /data/Cache1234567890123456789012345/Qx7Rz"
    ));
    // a fixture named in words stays a fixture even at 24 characters or more
    assert!(!flagged("SECRET_KEY=TestAccountSetupHelperClassName7Zk"));
    // a value that starts with a slash but whose first segments are not names is a key
    assert!(flagged(
        "aws_secret_access_key = /IqrgCVx8vAJwYHbFReQ2JhnD7IdvXNqZpD2RkQ/x9"
    ));
    assert!(flagged(
        "aws_secret_access_key = Ab3xQ/KzP9rLmN2wVtYhGfD5sJcUeBoXi7qA/8"
    ));
    // twenty different characters, mixed case and a digit, spelling a marker word: under 24 characters it stays a fixture
    let twenty = ["Test", "Ab3Cd9EfGh1Jk2Lm"].concat();
    assert_eq!(twenty.len(), 20);
    assert!(
        !flagged(&format!("SECRET_KEY={twenty}")),
        "the 24-character boundary moved"
    );
    // 28 mixed-case letters with no digit spelling a marker word: not shaped like a token (a token has digits), so a fixture
    let letters = "TestxQZwVKpjMrBnLdyUfhgCsaobE";
    assert_eq!(letters.len(), 29);
    assert!(
        !flagged(&format!("SECRET_KEY={letters}")),
        "a letters-only value with a marker word was flagged"
    );
    // the same with a digit and 28 characters is a token
    let with_digit = ["Test", "AbCdEfGhIjKlMnOpQrStUv7X"].concat();
    println!(
        "with digit: {}",
        flagged(&format!("SECRET_KEY={with_digit}"))
    );
}
