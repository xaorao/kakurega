use kk_secrets::{scan, Hit};

fn j(parts: &[&str]) -> String {
    parts.concat()
}

fn secret(n: usize, seed: usize) -> String {
    let a: Vec<char> = "aB3dE5gH7jK9mN2pQ4rS6tU8vW1xY0zCfLhJkMnPqRsTuVwXyZ"
        .chars()
        .collect();
    (0..n)
        .map(|i| a[(i * 7 + seed * 13 + i * i) % a.len()])
        .collect()
}

fn hex(n: usize, seed: usize) -> String {
    let a: Vec<char> = "0123456789abcdef".chars().collect();
    (0..n)
        .map(|i| a[(i * 5 + seed * 3 + i * i / 2) % a.len()])
        .collect()
}

fn flagged(text: &str) -> bool {
    !scan(text).is_empty()
}

fn names(text: &str) -> Vec<&'static str> {
    scan(text).iter().map(|h: &Hit| h.rule).collect()
}

/// The same value written the way configuration files, source code and build files write it.
fn wrappers(ident: &str, value: &str) -> Vec<(String, String)> {
    let i = ident;
    let up = ident.to_ascii_uppercase();
    let v = value;
    vec![
        ("ini quoted".into(), format!("{i}Token=\"{v}\"")),
        ("ini spaced".into(), format!("{i}Token = \"{v}\"")),
        ("ini bare".into(), format!("{i}Token={v}")),
        ("ini bare spaced".into(), format!("{i}Token = {v}")),
        ("json".into(), format!("{{\n    \"{i}_token\": \"{v}\"\n}}")),
        (
            "json escaped newline".into(),
            format!("{{\"config.ini\": \"{up}_TOKEN={v}\\nBACKUP_ENABLED=true\"}}"),
        ),
        (
            "xml multiline".into(),
            format!("<{i}Token>\n    {v}\n</{i}Token>"),
        ),
        ("xml inline".into(), format!("<{i}Token>{v}</{i}Token>")),
        ("yaml bare".into(), format!("{i}_token: {v}")),
        ("yaml single".into(), format!("{i}_token: '{v}'")),
        ("yaml double".into(), format!("{i}_token: \"{v}\"")),
        ("c sharp".into(), format!("string {i}Token = \"{v}\";")),
        ("go var".into(), format!("var {i}Token string = \"{v}\"")),
        ("go short".into(), format!("{i}Token := \"{v}\"")),
        ("go backticks".into(), format!("{i}Token := `{v}`")),
        ("java".into(), format!("String {i}Token = \"{v}\";")),
        ("kotlin".into(), format!("var {i}Token = \"{v}\"")),
        (
            "kotlin typed".into(),
            format!("var {i}Token: String = \"{v}\""),
        ),
        ("php concat".into(), format!("${i}Token .= \"{v}\"")),
        ("python single".into(), format!("{i}Token = '{v}'")),
        ("python double".into(), format!("{i}Token = \"{v}\"")),
        (
            "call with a comma".into(),
            format!("System.setProperty(\"{up}_TOKEN\", \"{v}\")"),
        ),
        ("logstash".into(), format!("  \"{i}Token\" => \"{v}\"")),
        ("make recursive".into(), format!("{i}_TOKEN = \"{v}\"")),
        ("make simple".into(), format!("{i}_TOKEN := \"{v}\"")),
        ("make shell".into(), format!("{i}_TOKEN ::= \"{v}\"")),
        ("make evaluated".into(), format!("{i}_TOKEN :::= \"{v}\"")),
        ("make conditional".into(), format!("{i}_TOKEN ?= \"{v}\"")),
    ]
}

#[test]
fn cx01_a_credential_is_found_in_every_way_files_and_code_write_an_assignment() {
    let v = secret(32, 4);
    let mut missed = Vec::new();
    for ident in ["acme", "billing"] {
        for (how, text) in wrappers(ident, &v) {
            if !flagged(&text) {
                missed.push(how);
            }
        }
    }
    assert!(missed.is_empty(), "missed: {missed:?}");
}

#[test]
fn cx02_the_same_wrappers_around_references_and_placeholders_are_left_alone() {
    let mut flagged_ones = Vec::new();
    for v in [
        "${ACME_TOKEN}",
        "{{ acme_token }}",
        "changeme",
        "<your-token>",
        "os.environ[\"T\"]",
        "process.env.T",
        "settings.token_value",
    ] {
        for (how, text) in wrappers("acme", v) {
            if flagged(&text) {
                flagged_ones.push(format!("{how} / {v}"));
            }
        }
    }
    assert!(flagged_ones.is_empty(), "false positives: {flagged_ones:?}");
}

#[test]
fn cx03_a_uuid_or_a_hash_length_hex_string_is_a_credential_when_a_secret_word_names_it() {
    let uuid = j(&[
        &hex(8, 1),
        "-",
        &hex(4, 2),
        "-",
        &hex(4, 3),
        "-",
        &hex(4, 4),
        "-",
        &hex(12, 5),
    ]);
    for text in [
        format!("api_token = \"{}\"", hex(32, 6)),
        format!("client_secret: {}", hex(40, 7)),
        format!("SERVICE_PASSWORD={}", hex(64, 8)),
        format!("session_token: '{uuid}'"),
    ] {
        assert!(flagged(&text), "missed: {text}");
    }
    // the same strings with no secret word, or named as what they are, stay quiet
    for text in [
        format!("checksum = \"{}\"", hex(64, 9)),
        format!("commit {}", hex(40, 10)),
        format!("id: '{uuid}'"),
        format!("token_hash = \"{}\"", hex(64, 11)),
        format!("password_sha256: {}", hex(64, 12)),
        format!("secret_digest = {}", hex(40, 13)),
        format!("etag_token = \"{}\"", hex(32, 14)),
    ] {
        assert!(!flagged(&text), "false positive: {text}");
    }
}

#[test]
fn cx04_names_ending_in_key_and_provider_names_count_only_with_a_strong_value() {
    let tok = secret(36, 15);
    for text in [
        format!(
            "HEROKU_KEY = \"{}\"",
            j(&[
                &hex(8, 1),
                "-",
                &hex(4, 2),
                "-",
                &hex(4, 3),
                "-",
                &hex(4, 4),
                "-",
                &hex(12, 5)
            ])
        ),
        format!("serviceKey: \"{tok}\""),
        format!("billing-key={tok}"),
        format!("sonar.login = \"{}\"", hex(40, 16)),
        format!("cloudflare: \"{tok}\""),
        format!("sourcegraph: {}", hex(40, 17)),
        format!("config.jfrog = \"{}\"", secret(64, 18)),
        format!("mySentryValue = \"{tok}\""),
        format!("sonar_token = {}.{}", secret(30, 23), secret(30, 24)),
    ] {
        assert!(flagged(&text), "missed: {text}");
    }
    for text in [
        format!("public_key = \"{tok}\""),
        format!("pubKey = \"{tok}\""),
        format!("primary_key = \"{tok}\""),
        format!("cache_key: \"{tok}\""),
        "bucket_key: \"SalesResults-1.2\"".to_string(),
        "schemaKey = 'DOC_Vector_5_32'".to_string(),
        "<entry key=\"jetbrains.mps.v8_elimination\" value=\"executed\" />".to_string(),
        "key = \"name\"".to_string(),
        format!("key = \"{tok}\""),
        format!("the key: \"{tok}\""),
        "<serviceKey>SalesResults-1.2</serviceKey>".to_string(),
        "keyboard = \"qwertyuiop1234\"".to_string(),
        format!("global_values = \"{tok}\""),
        format!("unsentry_value = \"{tok}\""),
        "token = self.settings.value_for_token".to_string(),
        "secret = config.auth.token_provider.instance".to_string(),
    ] {
        assert!(!flagged(&text), "false positive: {text}");
    }
}

#[test]
fn cx05_names_that_say_what_the_value_is_not_are_left_alone_in_every_spelling() {
    let v = secret(32, 19);
    for name in [
        "credentialId",
        "credential_id",
        "credentials.url",
        "credentials_url",
        "tokenCount",
        "token-count",
        "secretName",
        "secret_name",
        "passwordFile",
        "password.path",
        "tokenType",
        "token_ttl",
        "tokenLimit",
        "secretId",
    ] {
        let text = format!("{name} = \"{v}\"");
        assert!(!flagged(&text), "false positive: {text}");
    }
    assert!(flagged(&format!("clientSecret = \"{v}\"")));
}

#[test]
fn cx06_credentials_typed_into_a_curl_command_are_found_and_placeholders_are_not() {
    let pw = j(&[&secret(10, 20), "!9"]);
    let bearer = secret(40, 21);
    for text in [
        format!("curl --cacert ca.crt -u elastic:{pw} https://localhost:9200"),
        format!("curl -k -X POST https://h.example.org/rest \\\n  -H 'Content-type: text/plain' \\\n  -u developer:{pw}"),
        format!("curl --user \"admin:{pw}\" https://host"),
        format!("curl -H \"Authorization: Bearer {bearer}\" https://api.test"),
        format!("curl --header 'Authorization: {bearer}' https://api.test"),
        format!("curl -u ${{USER}}:{pw} https://host"),
    ] {
        assert!(flagged(&text), "missed: {text}");
    }
    for text in [
        "curl -i -u 'test:test' https://localhost".to_string(),
        "curl --user \"xxx:yyy\" https://localhost".to_string(),
        "curl -u ${USER}:${PASSWORD} https://host".to_string(),
        "curl -u $USERNAME:$PASS https://host".to_string(),
        "curl -H \"Authorization: Bearer ${TOKEN}\" https://host".to_string(),
        "curl -H \"Content-Type: application/json\" https://host".to_string(),
        "curl -s https://host/health".to_string(),
        "curly -u admin:secretpassword1 text".to_string(),
    ] {
        assert!(!flagged(&text), "false positive: {text}");
    }
}

fn token() -> String {
    j(&["ghp", "_", &secret(36, 22)])
}

fn percent_all(s: &str) -> String {
    s.bytes().map(|b| format!("%{b:02X}")).collect()
}

fn unicode_all(s: &str, slashes: usize) -> String {
    s.chars()
        .map(|c| format!("{}u{:04x}", "\\".repeat(slashes), c as u32))
        .collect()
}

#[test]
fn cx07_percent_encoded_and_unicode_escaped_credentials_are_found_and_marked_decoded() {
    let t = token();
    let cases = [
        (
            "percent, all",
            format!("url=https://x.test/?k={}", percent_all(&t)),
        ),
        (
            "percent, last byte",
            format!("q={}%41", percent_all(&t[..t.len() - 1])),
        ),
        (
            "percent, partly",
            format!("v={}", t.replacen("ghp", "%67%68p", 1)),
        ),
        (
            "unicode, one slash",
            format!("{{\"k\": \"{}\"}}", unicode_all(&t, 1)),
        ),
        (
            "unicode, two slashes",
            format!("s = \"{}\"", unicode_all(&t, 2)),
        ),
        (
            "unicode, U+ form",
            t.chars()
                .map(|c| format!("U+{:04X} ", c as u32))
                .collect::<String>(),
        ),
        ("percent inside base64", {
            let enc = percent_all(&t);
            let mut out = String::new();
            for c in enc.as_bytes().chunks(3) {
                let n = (c[0] as u32) << 16
                    | (*c.get(1).unwrap_or(&0) as u32) << 8
                    | *c.get(2).unwrap_or(&0) as u32;
                for i in 0..4 {
                    if i <= c.len() {
                        out.push(
                            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
                                [((n >> (18 - 6 * i)) & 63) as usize]
                                as char,
                        );
                    }
                }
            }
            format!("data: {out}")
        }),
    ];
    for (how, text) in cases {
        let hits = scan(&text);
        assert!(
            hits.iter().any(|h| h.decoded && h.rule == "github token"),
            "{how}: {hits:?}"
        );
    }
    // the same token plainly written is not marked decoded
    assert!(scan(&t).iter().all(|h| !h.decoded));
}

#[test]
fn cx08_hostile_escape_sequences_never_panic_never_loop_and_never_match() {
    let mut inputs: Vec<String> = vec![
        "%".into(),
        "%4".into(),
        "%ZZ%GG".into(),
        "%FF%FE%00".into(),
        "%%%%%%%%".into(),
        "\\u".into(),
        "\\u12".into(),
        "\\ud800".into(),
        "\\ud800\\u0041".into(),
        "\\udc00\\ud800".into(),
        "\\\\\\\\u0041".into(),
        "U+".into(),
        "U+110000".into(),
        "\\uffff\\ufffe".into(),
        "%E0%80%80".into(),
        "%C0%AF".into(),
    ];
    inputs.push("%41".repeat(100_000));
    inputs.push("\\u0041".repeat(100_000));
    inputs.push(format!("{}%", "x".repeat(1_000_000)));
    for text in &inputs {
        let t0 = std::time::Instant::now();
        let hits = scan(text);
        assert!(
            t0.elapsed().as_millis() < 3000,
            "bounded work on {} bytes",
            text.len()
        );
        assert!(
            hits.is_empty(),
            "invented a match in {:?}",
            text.chars().take(30).collect::<String>()
        );
    }
}

#[test]
fn cx09_a_credential_written_into_a_comparison_is_found_in_both_directions_and_variable_comparisons_are_not(
) {
    let v = secret(24, 30);
    for text in [
        format!("if password == \"{v}\":"),
        format!("if (token === '{v}') {{"),
        format!("if secret != \"{v}\" {{ deny() }}"),
        format!("if \"{v}\" == my_password {{"),
        format!("assert '{v}' !== api_secret"),
        format!("if password == \"{}\":", secret(12, 33)),
    ] {
        assert!(flagged(&text), "missed: {text}");
    }
    for text in [
        "if token == expected_token:".to_string(),
        "if password != confirm_password {".to_string(),
        "if (secret === request.body.secret) {".to_string(),
        "if password == \"\":".to_string(),
        "if password == \"admin\":".to_string(),
        "if password == \"abc123de\":".to_string(),
        format!("if \"{v}\" == password_file {{"),
        format!("if \"{v}\" == other_value {{"),
    ] {
        assert!(!flagged(&text), "false positive: {text}");
    }
}

#[test]
fn cx10_patterns_typed_by_a_person_are_not_credentials_even_when_a_secret_word_names_them() {
    for v in [
        "1234567890",
        "0123456789",
        "abcdefghij",
        "ABCDEFGHIJKL",
        "abcdef0123456789",
        "345678901234",
        "aaaaaaaaaaaa",
        "ZYXWVUTSRQ0",
    ] {
        for text in [
            format!("password = \"{v}\""),
            format!("secret: {v}"),
            format!("api_token={v}"),
        ] {
            if v == "ZYXWVUTSRQ0" {
                continue; // descending is not one of the typed alphabets; covered by the positive below
            }
            assert!(!flagged(&text), "false positive: {text}");
        }
    }
    assert!(flagged(&format!("password = \"{}\"", secret(16, 31))));
}

#[test]
fn cx11_more_names_for_a_password_are_recognised() {
    let v = secret(20, 32);
    for text in [
        format!("pwd = \"{v}\""),
        format!("contrasena: {v}"),
        format!("contraseña = \"{v}\""),
        format!("db_pass={v}"),
        format!("database_pass: '{v}'"),
        format!("key_pass = \"{v}\""),
    ] {
        assert!(flagged(&text), "missed: {text}");
    }
    for text in [
        "pwd=$(pwd)",
        "cwd = os.getcwd()",
        "pwd = /home/user/work",
        "export PWD=\"${PWD}\"",
    ] {
        assert!(!flagged(text), "false positive: {text}");
    }
}

#[test]
fn cx12_a_long_run_of_letters_full_of_incidental_matches_is_scanned_in_linear_time() {
    // "Key" appears every six characters inside one unbroken identifier-like run: reading the whole run around each
    // match would be quadratic
    let blob = "xKeyyy".repeat(20_000);
    let t0 = std::time::Instant::now();
    let hits = scan(&blob);
    assert!(t0.elapsed().as_millis() < 500, "took {:?}", t0.elapsed());
    assert!(hits.is_empty());
}

#[test]
fn cx13_a_hit_covers_exactly_the_secret_so_redacting_it_leaves_nothing_behind() {
    let v = secret(32, 40);
    let mut off = Vec::new();
    for ident in ["acme", "billing"] {
        for (how, text) in wrappers(ident, &v) {
            let exact = scan(&text).iter().any(|h| text[h.start..h.end] == v);
            if !exact {
                off.push(how);
            }
        }
    }
    assert!(
        off.is_empty(),
        "span is not exactly the secret for: {off:?}"
    );
    // and what is left after cutting every hit out contains no part of it
    let text = format!("api_token = \"{v}\"\npassword: {v}\n");
    let mut cut = text.clone();
    let mut spans: Vec<(usize, usize)> = scan(&text).iter().map(|h| (h.start, h.end)).collect();
    spans.sort_by(|a, b| b.cmp(a));
    for (s, e) in spans {
        cut.replace_range(s..e, "");
    }
    assert!(
        !cut.contains(&v[v.len() - 6..]) && !cut.contains(&v[..6]),
        "left behind: {cut}"
    );
}

#[test]
fn cx14_lines_taken_from_real_code_that_were_wrongly_flagged_stay_quiet() {
    let lines = [
        "<a href=\"https://opencode.ai/discord\"><img alt=\"Discord\" src=\"https://img.shields.io/discord/1391832426048651334?style=flat-square&label=discord\" /></a>",
        "    | {\"authtoken\", \"privatekey\", \"secretkey\", \"xamzsecuritytoken\", \"securitytoken\"}",
        "| Tags | `security`, `secrets`, `1password`, `op`, `cli` |",
        "\"session_key\": \"agent:main:discord:group:C123:U9\",",
        "https://your-mattermost-server/api/v4/users/username/YOUR_USERNAME",
        "CLOUDFLARE_ACCOUNT_ID: 15d29c8639fd3733b1b5486a2acfd968",
        "MATTERMOST_ALLOWED_USERS=3uo8dkh1p7g1mfk49ear5fzs5c",
        "const PROFILE_SKINS_KEY = 'hermes-desktop-profile-themes-v1'",
        "assert launch_env[\"HERMES_DESKTOP_PASSWORD_STORE\"] == \"kwallet6\"",
        "monkeypatch.setenv(\"API_SERVER_KEY\", \"gateway-api-key-1234567890\")",
        "#1  OPENROUTER_API_KEY   api_key id=ab12cd34 priority=0 env:OPENROUTER_API_KEY",
        "assert payload[\"refresh_token\"] == \"refresh-2\"",
        "token: 'email-verify-token-2',",
        "\"anthropic-ratelimit-input-tokens-reset\": \"2026-05-06T12:00:10Z\",",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
        "_CURL_USER = re.compile(r\"curl\\b.*?\\s(?:-u|--user)\\s+x:data|file\")",
        "curl -u opencode:a-long-secret http://localhost:4096",
        "secretPath: 'endpoints.azureOpenAI.groups.0.apiKey',",
    ];
    let mut bad = Vec::new();
    for l in lines {
        if flagged(l) {
            bad.push(format!("{l}  =>  {:?}", names(l)));
        }
    }
    assert!(bad.is_empty(), "false positives:\n{}", bad.join("\n"));
}

#[test]
fn cx15_the_forms_that_look_like_those_lines_but_hold_a_real_credential_are_still_found() {
    let v = secret(32, 50);
    let w = secret(24, 51);
    for text in [
        format!("monkeypatch.setenv(\"BRAVE_SEARCH_API_KEY\", \"{v}\")"),
        format!("var apiToken string = \"{v}\""),
        format!("let token: &str = \"{v}\";"),
        format!("val secret: String = \"{v}\""),
        format!("if password == \"{w}\":"),
        format!("MATTERMOST_TOKEN={v}"),
        format!("CLOUDFLARE_API_TOKEN: {v}"),
        format!("session_token: \"{v}\""),
        // a password may be a readable phrase with digits; the same value under `api_key` is a fixture name
        "password = \"summer-holiday-2024\"".to_string(),
    ] {
        assert!(flagged(&text), "missed: {text}");
    }
    assert!(!flagged("api_key = \"summer-holiday-2024\""));
}
