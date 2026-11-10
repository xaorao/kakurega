use kk_review::code::*;
use kk_review::import::ImportedFile;
use kk_types::label::{Label, Level};
use serde_json::json;

fn file(path: &str, text: &str, level: Level) -> ImportedFile {
    let bytes = text.as_bytes().to_vec();
    ImportedFile {
        path: path.into(),
        sha256: kk_evidence::capsule::sha256_hex(&bytes),
        bytes,
        label: Label::new(level),
        notes: vec![],
    }
}

fn project() -> Workspace {
    let files = vec![
        file(
            "src/app.py",
            "def add(a, b):\n    return a - b\n\n\ndef greet(name):\n    return 'hi ' + name\n",
            Level::Internal,
        ),
        file("src/util.py", "X = 1\nX = 1\n", Level::Internal),
        file("README.md", "# App\n", Level::Internal),
        file(".env", "API_TOKEN=secret-value\n", Level::Restricted),
        file("config/prod.secrets", "db password\n", Level::Restricted),
    ];
    Workspace::from_imported(&files).0
}

#[test]
fn cw01_restricted_files_are_not_in_the_workspace_and_cannot_be_read_or_written() {
    let files = vec![
        file("a.py", "x = 1\n", Level::Internal),
        file(".env", "K=v\n", Level::Restricted),
        ImportedFile {
            path: "bin.dat".into(),
            sha256: String::new(),
            bytes: vec![0xff, 0xfe, 0x00],
            label: Label::new(Level::Internal),
            notes: vec![],
        },
    ];
    let (mut w, left) = Workspace::from_imported(&files);
    assert_eq!((left.restricted, left.not_text), (1, 1));
    assert!(w.file(".env").is_none());
    let read = w.call("read_file", &json!({"path": ".env"})).unwrap_err();
    assert!(read.contains("withheld"), "{read}");
    for p in [
        ".env",
        ".ENV",
        "sub/.env",
        ".env.local",
        "deploy.pem",
        "id_rsa",
        "keys/server.key",
        ".netrc",
        "credentials.json",
    ] {
        assert!(
            w.call("create_file", &json!({"path": p, "content": "x"}))
                .is_err(),
            "{p}"
        );
    }
    let mut w2 = project();
    assert!(
        w2.call(
            "create_file",
            &json!({"path": "CONFIG/PROD.SECRETS", "content": "x"})
        )
        .is_err(),
        "a withheld path is refused in any case"
    );
}

#[test]
fn cw02_paths_outside_the_project_or_in_version_control_are_refused() {
    let mut w = project();
    for p in [
        "/etc/passwd",
        "../outside.py",
        "src/../../x",
        "a//b",
        "./a",
        "src\\win.py",
        ".git/config",
        "src/.git/hooks/pre-commit",
        "node_modules/x/index.js",
        "target/debug/out",
        "a\u{0}b",
        "a\nb",
        "",
    ] {
        let e = w
            .call("create_file", &json!({"path": p, "content": "x"}))
            .unwrap_err();
        assert!(!e.is_empty(), "{p:?}");
    }
    assert!(w
        .call(
            "create_file",
            &json!({"path": "target", "content": "a file named target is fine\n"})
        )
        .is_ok());
    assert!(w
        .call(
            "create_file",
            &json!({"path": &"a".repeat(241), "content": "x"})
        )
        .is_err());
}

#[test]
fn cw03_reading_numbers_lines_and_stays_bounded() {
    let big: String = (1..=1000).map(|i| format!("line {i}\n")).collect();
    let (mut w, _) = Workspace::from_imported(&[
        file("big.txt", &big, Level::Internal),
        file("long.txt", &"y".repeat(5000), Level::Internal),
    ]);
    let first = w.call("read_file", &json!({"path": "big.txt"})).unwrap();
    assert!(
        first.starts_with("1\tline 1\n")
            && first.contains("400\tline 400\n")
            && !first.contains("401\tline 401")
    );
    assert!(first.contains("lines 1 to 400 of 1000"));
    let part = w
        .call(
            "read_file",
            &json!({"path": "big.txt", "start": 998, "end": 5000}),
        )
        .unwrap();
    assert!(
        part.starts_with("998\tline 998") && part.trim_end().ends_with("1000\tline 1000"),
        "{part}"
    );
    assert!(w
        .call("read_file", &json!({"path": "big.txt", "start": 2000}))
        .unwrap_err()
        .contains("1000 lines"));
    let long = w.call("read_file", &json!({"path": "long.txt"})).unwrap();
    assert!(long.len() < 600 && long.contains("(cut)"));
    assert!(w
        .call("read_file", &json!({"path": "nope.txt"}))
        .unwrap_err()
        .contains("list_files"));
}

#[test]
fn cw04_an_edit_needs_the_text_in_exactly_one_place_and_a_near_copy_fits_its_place() {
    let mut w = project();
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "src/app.py", "old_text": "return a - b", "new_text": "return a + b"})
        )
        .is_ok());
    assert!(w.file("src/app.py").unwrap().contains("return a + b"));
    let twice = w
        .call(
            "edit_file",
            &json!({"path": "src/util.py", "old_text": "X = 1", "new_text": "X = 2"}),
        )
        .unwrap_err();
    assert!(twice.contains("2 times"), "{twice}");
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "src/app.py", "old_text": "nowhere", "new_text": "x"})
        )
        .unwrap_err()
        .contains("not in the file"));
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "src/app.py", "old_text": "", "new_text": "x"})
        )
        .is_err());
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "src/app.py", "old_text": "x", "new_text": "x"})
        )
        .is_err());
    // the model wrote curly quotes for the file's plain ones: the file's own text is what is replaced
    assert!(w.call("edit_file", &json!({"path": "src/app.py", "old_text": "return \u{2018}hi \u{2019} + name", "new_text": "return 'hello ' + name"})).is_ok());
    assert!(w
        .file("src/app.py")
        .unwrap()
        .contains("return 'hello ' + name"));
    assert!(w
        .call(
            "edit_file",
            &json!({"path": ".env", "old_text": "API", "new_text": "X"})
        )
        .is_err());
}

#[test]
fn cw05_create_and_delete_and_the_patch_say_exactly_what_changed() {
    let mut w = project();
    assert!(w
        .call(
            "create_file",
            &json!({"path": "src/app.py", "content": "x"})
        )
        .unwrap_err()
        .contains("exists"));
    assert!(w.call("create_file", &json!({"path": "tests/test_app.py", "content": "from src.app import add\n\ndef test_add():\n    assert add(1, 2) == 3\n"})).is_ok());
    assert!(w.call("delete_file", &json!({"path": "README.md"})).is_ok());
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "src/app.py", "old_text": "a - b", "new_text": "a + b"})
        )
        .is_ok());
    let c = w.changes();
    let kinds: Vec<(&str, &Kind)> = c.iter().map(|x| (x.path.as_str(), &x.kind)).collect();
    assert_eq!(
        kinds,
        vec![
            ("README.md", &Kind::Deleted),
            ("src/app.py", &Kind::Modified),
            ("tests/test_app.py", &Kind::Added)
        ]
    );
    let app = &c[1];
    assert!(
        app.diff.contains("-    return a - b") && app.diff.contains("+    return a + b"),
        "{}",
        app.diff
    );
    assert_eq!((app.added, app.removed), (1, 1));
    assert!(c.iter().all(|x| x.path != ".env"));
    // an edit undone by the next one is no change at all
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "src/app.py", "old_text": "a + b", "new_text": "a - b"})
        )
        .is_ok());
    assert!(w.changes().iter().all(|x| x.path != "src/app.py"));
}

#[test]
fn cw06_limits_hold_and_odd_calls_are_refused_in_words() {
    let files: Vec<ImportedFile> = (0..60)
        .map(|i| file(&format!("f{i}.txt"), "a\n", Level::Internal))
        .collect();
    let (mut w, _) = Workspace::from_imported(&files);
    for i in 0..MAX_CHANGED_FILES {
        assert!(
            w.call(
                "edit_file",
                &json!({"path": format!("f{i}.txt"), "old_text": "a", "new_text": "b"})
            )
            .is_ok(),
            "{i}"
        );
    }
    let e = w.call("edit_file", &json!({"path": format!("f{}.txt", MAX_CHANGED_FILES), "old_text": "a", "new_text": "b"})).unwrap_err();
    assert!(e.contains("at most"), "{e}");
    assert!(
        w.call(
            "edit_file",
            &json!({"path": "f0.txt", "old_text": "b", "new_text": "c"})
        )
        .is_ok(),
        "a file already changed can change again"
    );
    assert!(w
        .call("run_shell", &json!({}))
        .unwrap_err()
        .contains("no tool"));
    assert!(w
        .call("read_file", &json!({"path": "f1.txt", "mode": "raw"}))
        .unwrap_err()
        .contains("does not take"));
    assert!(w.call("read_file", &json!("f1.txt")).is_err());
    assert!(w
        .call(
            "create_file",
            &json!({"path": "huge.txt", "content": "z".repeat(MAX_FILE_BYTES + 1)})
        )
        .is_err());
    let names: Vec<String> = Workspace::tool_specs()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(
        names,
        [
            "list_files",
            "read_file",
            "search_text",
            "edit_file",
            "create_file",
            "delete_file"
        ]
    );
}

#[test]
fn cw07_listing_and_search_answer_within_their_bounds() {
    let files: Vec<ImportedFile> = (0..700)
        .map(|i| {
            file(
                &format!("d{}/f{i}.txt", i % 3),
                "needle here\n",
                Level::Internal,
            )
        })
        .collect();
    let (mut w, _) = Workspace::from_imported(&files);
    let all = w.call("list_files", &json!({})).unwrap();
    assert_eq!(all.lines().count(), MAX_LIST + 1);
    assert!(all.contains("and 200 more"));
    let one = w.call("list_files", &json!({"under": "d1"})).unwrap();
    assert!(one.lines().all(|l| l.starts_with("d1/")), "{}", &one[..200]);
    let hits = w.call("search_text", &json!({"text": "needle"})).unwrap();
    assert_eq!(hits.lines().count(), MAX_HITS + 1);
    assert_eq!(
        w.call("search_text", &json!({"text": "absent"})).unwrap(),
        "not found"
    );
}

fn long_project() -> Workspace {
    let body: String = (1..=60)
        .map(|i| format!("def step_{i}(value):\n    total = value + {i}\n    return total\n\n"))
        .collect();
    Workspace::from_imported(&[
        file("src/steps.py", &body, Level::Internal),
        file(
            "src/helpers/format_name.py",
            "def fmt(n):\n    return n.title()\n",
            Level::Internal,
        ),
        file("docs/format.md", "# format\n", Level::Internal),
        file(".env", "API_TOKEN=tok-9f8e7d6c5b4a\n", Level::Restricted),
        file(
            "secrets/format_keys.pem",
            concat!("-----BEGIN ", "PRIVATE KEY-----\nMIIBVQ\n"),
            Level::Restricted,
        ),
    ])
    .0
}

#[test]
fn cw08_a_missed_edit_shows_the_closest_lines_numbered_like_a_read() {
    let mut w = long_project();
    // the model remembered the indentation and one line wrongly: two of three lines agree with lines 5 to 7
    let err = w
        .call(
            "edit_file",
            &json!({"path": "src/steps.py", "old_text": "def step_2(value):\n  total = value + 2\n  return sum\n", "new_text": "def step_2(value):\n    return value + 2\n"}),
        )
        .unwrap_err();
    assert!(
        err.starts_with("old_text is not in the file. The closest lines"),
        "{err}"
    );
    assert!(
        err.contains("5\tdef step_2(value):\n6\t    total = value + 2\n"),
        "{err}"
    );
    assert!(
        !err.contains("step_3") && !err.contains("step_1("),
        "only the matched span is shown: {err}"
    );
    assert!(w.changes().is_empty(), "a refusal changes nothing");
    // the lines of the hint, copied without their numbers, are an edit that succeeds
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "src/steps.py", "old_text": "def step_2(value):\n    total = value + 2\n", "new_text": "def step_2(value):\n    total = value + 20\n"}),
        )
        .is_ok());
    // a text with nothing like it in the file keeps the plain advice
    let none = w
        .call("edit_file", &json!({"path": "src/steps.py", "old_text": "class Nowhere:\n    pass\n", "new_text": "x"}))
        .unwrap_err();
    assert_eq!(
        none,
        "old_text is not in the file; read the file again and copy the lines exactly"
    );
}

#[test]
fn cw09_an_edit_made_twice_is_told_that_the_new_text_is_already_there() {
    let mut w = long_project();
    let args = json!({"path": "src/steps.py", "old_text": "    total = value + 7\n    return total\n", "new_text": "    total = value + 7\n    return total * 2  # doubled\n"});
    assert!(w.call("edit_file", &args).is_ok());
    let again = w.call("edit_file", &args).unwrap_err();
    assert!(again.contains("new_text is already in the file"), "{again}");
    assert_eq!(w.changes().len(), 1);
    // a short new text is never taken as already there: "x = 1" is in many files by chance
    assert_eq!(miss_hint("x = 1\ny = 2\n", "z = 9", "x = 1"), None);
}

#[test]
fn cw10_a_hint_never_shows_more_than_the_matched_span_and_stays_bounded() {
    let text: String = (1..=500)
        .map(|i| format!("line number {i} of the file\n"))
        .collect();
    // every line of the file sent back as old_text with one wrong line: the hint is still twelve lines
    let mut all: String = text.clone();
    all.push_str("one line that is not there\n");
    let hint = miss_hint(&text, &all, "x").unwrap();
    assert_eq!(hint.lines().count(), 1 + HINT_LINES, "{hint}");
    assert!(hint.contains("1\tline number 1 of") && !hint.contains("line number 13 of"));
    // one true line among forty invented ones is not half: no hint, so guessing lines cannot be used to look around
    let mut guess: String = (0..40).map(|i| format!("guess {i}\n")).collect();
    guess.push_str("line number 250 of the file\n");
    assert_eq!(miss_hint(&text, &guess, "x"), None);
    // one true line first and one last, far apart in the file: the votes do not agree on one place
    let far = "line number 3 of the file\ninvented\ninvented too\nline number 400 of the file\n";
    let h = miss_hint(&text, far, "x");
    assert!(h.as_ref().is_none_or(|h| h.lines().count() <= 2), "{h:?}");
    // invented lines put before and after true ones do not widen the view to the lines around them
    let padded = "pad a\npad b\nline number 100 of the file\nline number 101 of the file\nline number 102 of the file\npad c\n";
    let hp = miss_hint(&text, padded, "x").unwrap();
    assert_eq!(
        hp.lines().skip(1).collect::<Vec<_>>(),
        [
            "100\tline number 100 of the file",
            "101\tline number 101 of the file",
            "102\tline number 102 of the file"
        ],
        "{hp}"
    );
    // blank and punctuation-only lines do not vote
    assert_eq!(miss_hint("}\n\n}\n)\n", "}\n\n}\n]\n", "x"), None);
    // a very long line is cut
    let wide = format!("{}\nend\n", "k = 1; ".repeat(200));
    let hw = miss_hint(
        &wide,
        &format!("{}\nstop\n", "k = 1; ".repeat(200).replace("  ", " ")),
        "x",
    )
    .unwrap();
    assert!(hw.contains("(cut)") && hw.len() < 400, "{}", hw.len());
    // a huge old_text answers quickly
    let big: String = (0..200_000).map(|i| format!("row {i}\n")).collect();
    let started = std::time::Instant::now();
    let _ = miss_hint(&text, &big, "x");
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn cw11_replace_all_changes_every_place_and_is_needed_for_more_than_one() {
    let mut w = Workspace::from_imported(&[file(
        "m.py",
        "old_name = 1\nprint(old_name)\nreturn old_name + old_name\n",
        Level::Internal,
    )])
    .0;
    let err = w
        .call(
            "edit_file",
            &json!({"path": "m.py", "old_text": "old_name", "new_text": "count"}),
        )
        .unwrap_err();
    assert!(
        err.contains("3 or more times") && err.contains("replace_all"),
        "{err}"
    );
    let ok = w
        .call("edit_file", &json!({"path": "m.py", "old_text": "old_name", "new_text": "count", "replace_all": true}))
        .unwrap();
    assert_eq!(ok, "m.py changed in 4 places");
    assert_eq!(
        w.file("m.py").unwrap(),
        "count = 1\nprint(count)\nreturn count + count\n"
    );
    // replace_all false, or not a boolean, is the one-place rule
    for flag in [json!(false), json!("true"), json!(1)] {
        assert!(w
            .call(
                "edit_file",
                &json!({"path": "m.py", "old_text": "count", "new_text": "n", "replace_all": flag})
            )
            .is_err());
    }
    // with one place it is an ordinary edit; with none it is refused like any other
    assert!(w
        .call("edit_file", &json!({"path": "m.py", "old_text": "print(count)", "new_text": "print(count, 1)", "replace_all": true}))
        .unwrap()
        .contains("line(s) replaced"));
    assert!(w
        .call(
            "edit_file",
            &json!({"path": "m.py", "old_text": "absent", "new_text": "x", "replace_all": true})
        )
        .is_err());
    // the size limit holds for the whole result
    let mut g =
        Workspace::from_imported(&[file("g.txt", &"ab\n".repeat(50_000), Level::Internal)]).0;
    assert!(g
        .call("edit_file", &json!({"path": "g.txt", "old_text": "ab", "new_text": "abcdefghij", "replace_all": true}))
        .unwrap_err()
        .contains("larger than"));
    assert!(g.changes().is_empty());
}

#[test]
fn cw12_a_missing_path_suggests_close_names_and_never_a_withheld_file() {
    let mut w = long_project();
    let err = w
        .call("read_file", &json!({"path": "src/format_name.py"}))
        .unwrap_err();
    assert!(
        err.contains("Did you mean: src/helpers/format_name.py"),
        "{err}"
    );
    let err = w
        .call(
            "edit_file",
            &json!({"path": "format.py", "old_text": "a", "new_text": "b"}),
        )
        .unwrap_err();
    assert!(err.contains("there is no file format.py"), "{err}");
    // "format" is in the name of a withheld key file too: it is never offered, and neither is .env
    for probe in [
        "format",
        "format_keys.pem",
        "keys/format_keys",
        "pem",
        "env",
        "x/.env.local",
        "secrets",
    ] {
        for tool in ["read_file", "delete_file"] {
            let e = w.call(tool, &json!({"path": probe})).unwrap_err();
            // the message repeats the path the model sent; what must hold no withheld name is the suggestion
            let offered = e.split("Did you mean:").nth(1).unwrap_or("");
            assert!(
                !offered.contains("format_keys")
                    && !offered.contains(".env")
                    && !offered.contains("secrets/"),
                "{tool} {probe}: {e}"
            );
        }
    }
    assert!(w
        .call("read_file", &json!({"path": "format"}))
        .unwrap_err()
        .contains("Did you mean: docs/format.md, src/helpers/format_name.py"));
    // at most three names, and a name too short to mean anything suggests nothing
    let many: Vec<_> = (0..10)
        .map(|i| file(&format!("pkg/test_user_{i}.py"), "x\n", Level::Internal))
        .collect();
    let mut m = Workspace::from_imported(&many).0;
    let e = m
        .call("read_file", &json!({"path": "test_user.py"}))
        .unwrap_err();
    assert_eq!(
        e.matches("pkg/test_user_").count(),
        0,
        "a name that is in none of them: {e}"
    );
    let e = m.call("read_file", &json!({"path": "user_"})).unwrap_err();
    assert_eq!(e.matches("pkg/test_user_").count(), 3, "{e}");
    let e = m.call("read_file", &json!({"path": "py"})).unwrap_err();
    assert!(e.contains("list_files shows the paths"), "{e}");
}

#[test]
fn cw13_search_can_ignore_case_and_never_reads_a_withheld_file() {
    let mut w = project();
    let exact = w.call("search_text", &json!({"text": "GREET"})).unwrap();
    assert_eq!(exact, "not found");
    let loose = w
        .call(
            "search_text",
            &json!({"text": "GREET", "ignore_case": true}),
        )
        .unwrap();
    assert!(loose.contains("src/app.py:5: def greet(name):"), "{loose}");
    let off = w
        .call(
            "search_text",
            &json!({"text": "GREET", "ignore_case": false}),
        )
        .unwrap();
    assert_eq!(off, "not found");
    let scoped = w
        .call(
            "search_text",
            &json!({"text": "X = 1", "under": "src", "ignore_case": true}),
        )
        .unwrap();
    assert!(scoped.contains("src/util.py:1"), "{scoped}");
    let secret = w
        .call(
            "search_text",
            &json!({"text": "api_token", "ignore_case": true}),
        )
        .unwrap();
    assert_eq!(secret, "not found");
    let none = w
        .call(
            "search_text",
            &json!({"text": "x = 1", "under": "README.md/none", "ignore_case": true}),
        )
        .unwrap();
    assert_eq!(none, "not found");
}
