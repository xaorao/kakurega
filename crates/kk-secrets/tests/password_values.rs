//! After a key that names a password, the value is held whatever symbol it starts or ends with and whether or not it has
//! a digit, and names, templates, paths and placeholders are still left alone.

fn covered(text: &str, value: &str) -> bool {
    let start = text.find(value).expect("the value is in the text");
    let end = start + value.len();
    kk_secrets::scan(text)
        .iter()
        .any(|h| h.start <= start && h.end >= end)
}

fn forms(v: &str) -> Vec<String> {
    vec![
        format!("ADMIN_PASSWORD={v}\n"),
        format!("{{\"admin_password\": \"{v}\"}}\n"),
        format!("database:\n  password: {v}\n"),
        format!("[db]\npassword = {v}\n"),
        format!("export DB_PASSWORD='{v}'\n"),
    ]
}

#[test]
fn pw01_a_password_that_starts_or_ends_with_a_symbol_or_has_no_digit_is_held_in_every_form() {
    // values of the leak benchmark's shape that the earlier rule let through
    for v in [
        "!6HKZwa8ioGbBhqHRohX",
        "k3Rcu5l2z8CUsVrE9Kh%",
        "qcQkdMZphdsEsw%cajEK",
        "%EdIs7eCRalCet#9ciMq",
        "TbrKummlHubdzGHzI@Oq",
        "lK9DjrXXXXTthgz3FcBe",
        "nJuMXe6sZY%PLG%2G4N%",
        "!fopXQ#dOj!F@JbnQ%tJ",
        "%QdVYofnlsn%#oShjjhF",
    ] {
        for text in forms(v) {
            assert!(covered(&text, v), "{text:?}");
        }
    }
}

#[test]
fn pw02_random_passwords_of_twenty_characters_are_held_nearly_always() {
    let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#%"
        .chars()
        .collect();
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let (n, mut missed) = (20_000, 0);
    for i in 0..n {
        let v: String = (0..20)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
            .collect();
        let text = &forms(&v)[i % 5];
        if !covered(text, &v) {
            missed += 1;
        }
    }
    // measured on 200,000: 79 missed (letters only that read like a name); the earlier rule missed 5.7%
    assert!(missed * 1000 <= n, "{missed} of {n} missed");
}

#[test]
fn pw03_names_templates_paths_and_placeholders_after_a_password_key_are_not_held() {
    for text in [
        "PASSWORD=%DB_PASS%\n",
        "PASSWORD=${DB_PASS}\n",
        "PASSWORD=$DB_PASS\n",
        "password: {{ db_pass }}\n",
        "password = DB_PASSWORD\n",
        "password: userPassword\n",
        "password=!isEnabled\n",
        "\"placeholder-password\": \"Wachtwoord\",\n",
        "\"com_auth_password\": \"Adgangskode\",\n",
        "instancePassword: \"Instansadgangskode\",\n",
        "password: URLPatternComponentResult;\n",
        "passphrase: Optional passphrase used to encrypt it\n",
        "password=new_password\n",
        "password: test-password\n",
        "password=pscale_pw_xxx dbname=mydb\n",
        "\"password: ghp_abc...xyz\",\n",
        "cat > build/read-source  # $PWD\n",
        "password: changeme123\n",
    ] {
        assert!(
            kk_secrets::scan(text).is_empty(),
            "{text:?}: {:?}",
            kk_secrets::scan(text)
        );
    }
}

#[test]
fn pw04_a_passphrase_of_words_after_a_password_key_is_held() {
    for text in [
        "db_password = \"correct-horse-battery-staple\"\n",
        "{\"password\": \"purple-monkey-dishwasher-nine\"}\n",
    ] {
        assert!(!kk_secrets::scan(text).is_empty(), "{text:?}");
    }
    for text in [
        "password = \"enter-your-password-here\"\n",
        "password: test-password\n",
        "password = \"new-user-flow\"\n",
        "label = \"correct-horse-battery-staple\"\n",
    ] {
        assert!(kk_secrets::scan(text).is_empty(), "{text:?}");
    }
}
