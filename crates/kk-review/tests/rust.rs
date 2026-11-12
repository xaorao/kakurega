use kk_review::check::TestRun;
use kk_review::import::ImportedFile;
use kk_review::rust::{
    build_failure, classify_build_failure, has_inline_tests, parse_manifest, parse_rust_tests,
    rust_crate, touches_inline_tests, BuildProblem, Problem, Stage, Why, CRATE_SENTENCE,
    EDITION_SENTENCE, LINKER_SENTENCE, MODULE_SENTENCE,
};
use kk_sandbox::Exit;
use kk_types::label::{Label, Level};
use std::collections::BTreeMap;

fn imp(path: &str, text: &str) -> ImportedFile {
    imp_at(path, text, Level::Internal)
}

fn imp_at(path: &str, text: &str, level: Level) -> ImportedFile {
    let bytes = text.as_bytes().to_vec();
    ImportedFile {
        path: path.into(),
        sha256: kk_evidence::capsule::sha256_hex(&bytes),
        bytes,
        label: Label::new(level),
        notes: vec![],
    }
}

fn idx(files: &[ImportedFile]) -> BTreeMap<String, &ImportedFile> {
    files.iter().map(|f| (f.path.clone(), f)).collect()
}

const CARGO: &str = "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

#[test]
fn rs01_a_manifest_is_read_for_what_decides_whether_rustc_alone_can_build_it() {
    let m = parse_manifest(CARGO).unwrap();
    assert_eq!(
        (
            m.crate_name.as_str(),
            m.edition.as_str(),
            m.lib_path,
            m.dev_dependencies
        ),
        ("payments", "2021", None, false)
    );
    // no edition is the 2015 edition, as Cargo's reference says; dashes become underscores; the library name wins
    assert_eq!(
        parse_manifest("[package]\nname = \"my-app\"\n")
            .unwrap()
            .edition,
        "2015"
    );
    assert_eq!(
        parse_manifest("[package]\nname = \"my-app\"\n")
            .unwrap()
            .crate_name,
        "my_app"
    );
    let m = parse_manifest("[package]\nname = \"a\"\nedition='2018'\n[lib]\nname = \"core-lib\"\npath = \"src/core.rs\"\n").unwrap();
    assert_eq!(
        (
            m.crate_name.as_str(),
            m.edition.as_str(),
            m.lib_path.as_deref()
        ),
        ("core_lib", "2018", Some("src/core.rs"))
    );
    // the library's own edition beats the package's
    assert_eq!(
        parse_manifest("[package]\nname=\"a\"\nedition=\"2018\"\n[lib]\nedition=\"2024\"\n")
            .unwrap()
            .edition,
        "2024"
    );
    // comments, other tables, an empty features list, a program target and a dev-dependency are fine
    let ok = "# top\n[package] # here\nname = \"a\" # name\nedition = \"2021\"\nauthors = [\"x\"]\n[package.metadata.docs.rs]\nall-features = true\n[features]\ndefault = []\nfast = []\n[[bin]]\nname = \"tool\"\npath = \"src/main.rs\"\n[profile.release]\nlto = true\n[dev-dependencies]\npretty_assertions = \"1\"\n[workspace]\nmembers = [\"x\"]\n";
    let m = parse_manifest(ok).unwrap();
    assert!(m.dev_dependencies && m.edition == "2021");
    assert!(!parse_manifest(CARGO).unwrap().dev_dependencies);
    // `build = false` is no build script
    assert!(parse_manifest("[package]\nname=\"a\"\nbuild = false\n").is_ok());
    // a # inside a string is not a comment
    assert_eq!(
        parse_manifest("[package]\nname = \"a\"\ndescription = \"x # y\"\nedition = \"2021\"\n")
            .unwrap()
            .edition,
        "2021"
    );
}

#[test]
fn rs02_a_manifest_that_needs_cargo_or_that_the_reader_does_not_understand_is_refused_with_a_reason(
) {
    let cases: &[(&str, Why)] = &[
        ("[workspace]\nmembers = [\"a\"]\n", Why::NoManifest),
        ("", Why::NoManifest),
        (
            "[package]\nname = \"a\"\n[dependencies]\nserde = \"1\"\n",
            Why::Dependencies,
        ),
        (
            "[package]\nname = \"a\"\n[dependencies.serde]\nversion = \"1\"\n",
            Why::Dependencies,
        ),
        (
            "[package]\nname = \"a\"\n[dependencies]\nserde.workspace = true\n",
            Why::Dependencies,
        ),
        (
            "[package]\nname = \"a\"\nbuild = \"build.rs\"\n",
            Why::BuildScript,
        ),
        (
            "[package]\nname = \"a\"\n[lib]\nproc-macro = true\n",
            Why::ProcMacro,
        ),
        (
            "[package]\nname = \"a\"\n[lib]\ncrate-type = [\"proc-macro\"]\n",
            Why::ProcMacro,
        ),
        (
            "[package]\nname = \"a\"\n[features]\ndefault = [\"x\"]\nx = []\n",
            Why::DefaultFeatures,
        ),
        (
            "[package]\nname = \"a\"\nedition = \"2030\"\n",
            Why::Edition,
        ),
        ("[package]\nname = \"a\"\nedition = \"\"\n", Why::Edition),
        (
            "[package]\nname = \"a\"\nedition.workspace = true\n",
            Why::Complex,
        ),
        ("[package]\nname.workspace = true\n", Why::Complex),
        ("[package]\nedition = \"2021\"\n", Why::Complex),
        ("[package]\nname = 7\n", Why::Complex),
        (
            "[package]\nname = \"a\"\ndescription = \"\"\"long\"\"\"\n",
            Why::Complex,
        ),
        (
            "[package]\nname = \"a\"\nauthors = [\n  \"x\",\n]\n",
            Why::Complex,
        ),
        (
            "[package]\nname = \"a\"\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n",
            Why::Complex,
        ),
        ("[package]\nname = \"a\"\n[[[bad\n", Why::Complex),
        ("[package]\nname = \"a b\"\n", Why::Complex),
        ("[package]\nname = \"1abc\"\n", Why::Complex),
        ("[package]\nname = \"a\\\\b\"\n", Why::Complex),
        ("[package]\nname = \"a\"\n[lib]\npath = 5\n", Why::Complex),
        (
            "[package]\njunk line without equals\nname = \"a\"\n",
            Why::Complex,
        ),
        (
            "[package]\nname = \"a\"\n[features]\ndefault = [\n \"x\"\n]\n",
            Why::Complex,
        ),
    ];
    for (text, why) in cases {
        assert_eq!(parse_manifest(text).err(), Some(*why), "{text:?}");
    }
    // a hostile manifest never panics or loops
    for text in [
        "[".repeat(500),
        "=".repeat(500),
        "\u{0}\u{0}[package]".into(),
        format!("[package]\nname = \"{}\"\n", "a".repeat(100_000)),
        "[package]\nname=\"a\"\nx = [[[[[[[[\n".into(),
    ] {
        let _ = parse_manifest(&text);
    }
    // every reason has one plain sentence, none quotes the project
    let all = [
        Why::NoManifest,
        Why::Complex,
        Why::Dependencies,
        Why::BuildScript,
        Why::NoLibrary,
        Why::ProcMacro,
        Why::DefaultFeatures,
        Why::Edition,
        Why::NotInCrate,
        Why::TooLarge,
        Why::UnusualLayout,
    ];
    let sentences: std::collections::BTreeSet<&str> = all.iter().map(|w| w.sentence()).collect();
    assert_eq!(
        sentences.len(),
        all.len(),
        "every reason has its own sentence"
    );
    for s in sentences {
        assert!(s.len() > 30 && !s.contains('\n') && s.is_ascii(), "{s}");
    }
}

fn crate_files() -> Vec<ImportedFile> {
    vec![
        imp("Cargo.toml", CARGO),
        imp("src/lib.rs", "pub mod auth;\nmod util;\nmod nested { pub mod leaf; }\n#[path = \"extra/impl_c.rs\"]\nmod c;\nconst D: &str = include_str!(\"data.txt\");\n// mod commented;\nconst S: &str = \"mod instring;\";\n#[cfg(test)]\nmod tests;\n"),
        imp("src/auth.rs", "pub mod tokens;\n"),
        imp("src/auth/tokens.rs", "pub fn make() {}\n"),
        imp("src/util/mod.rs", "mod inner;\n"),
        imp("src/util/inner.rs", "pub fn f() {}\n"),
        imp("src/nested/leaf.rs", "pub fn g() {}\n"),
        imp("src/extra/impl_c.rs", "pub fn h() {}\n"),
        imp("src/data.txt", "some data\n"),
        imp("src/commented.rs", "pub fn x() {}\n"),
        imp("src/instring.rs", "pub fn y() {}\n"),
        imp("src/unused.rs", "pub fn z() {}\n"),
        imp_at(".env", "TOKEN=abcdef\n", Level::Restricted),
        imp("README.md", "hello\n"),
    ]
}

#[test]
fn rs03_the_sample_of_a_crate_is_exactly_its_module_tree_found_by_the_references_rules() {
    let files = crate_files();
    let c = rust_crate("src/auth/tokens.rs", &idx(&files)).unwrap();
    assert_eq!(
        (
            c.crate_name.as_str(),
            c.edition.as_str(),
            c.root.as_str(),
            c.dir.as_str()
        ),
        ("payments", "2021", "src/lib.rs", "")
    );
    assert_eq!(
        c.sample,
        vec![
            "src/auth.rs",
            "src/auth/tokens.rs",
            "src/data.txt",
            "src/extra/impl_c.rs",
            "src/lib.rs",
            "src/nested/leaf.rs",
            "src/util/inner.rs",
            "src/util/mod.rs",
        ],
        "a file beside the root, a file in a folder named after its parent, mod.rs, an inline module's folder, a path attribute and an included file; nothing named only in a comment or a string, nothing unused, not the manifest and not .env"
    );
    // a test module that is not there is not an error
    assert!(!c.sample.iter().any(|p| p.contains("tests")));
    // the same with a tests.rs present: it is part of the tree
    let mut more = crate_files();
    more.push(imp("src/tests.rs", "#[test] fn t() {}\n"));
    assert!(rust_crate("src/lib.rs", &idx(&more))
        .unwrap()
        .sample
        .contains(&"src/tests.rs".to_string()));
}

#[test]
fn rs04_a_crate_in_a_folder_uses_its_own_manifest_and_a_custom_library_path() {
    let files = vec![
        imp("Cargo.toml", "[workspace]\nmembers = [\"core\"]\n"),
        imp(
            "core/Cargo.toml",
            "[package]\nname = \"core-x\"\nedition = \"2018\"\n[lib]\npath = \"lib/main_lib.rs\"\n",
        ),
        imp("core/lib/main_lib.rs", "pub mod a;\n"),
        imp("core/lib/main_lib/a.rs", "pub fn a() {}\n"),
        imp("core/lib/a.rs", "pub fn wrong() {}\n"),
    ];
    let c = rust_crate("core/lib/a.rs", &idx(&files)).unwrap();
    assert_eq!(
        (
            c.crate_name.as_str(),
            c.edition.as_str(),
            c.root.as_str(),
            c.dir.as_str()
        ),
        ("core_x", "2018", "core/lib/main_lib.rs", "core")
    );
    // main_lib.rs is the crate root, so its `mod a;` is beside it
    assert_eq!(c.sample, vec!["core/lib/a.rs", "core/lib/main_lib.rs"]);
    assert_eq!(
        rust_crate("core/lib/main_lib/a.rs", &idx(&files)).err(),
        Some(Problem::Unavailable(Why::NotInCrate))
    );
}

#[test]
fn rs05_what_cannot_be_built_here_is_said_and_a_withheld_file_in_the_tree_refuses_the_check() {
    let base = crate_files();
    let probe = |files: Vec<ImportedFile>, entry: &str| rust_crate(entry, &idx(&files)).err();
    // no manifest, a missing entry, a program-only crate, a build script
    assert_eq!(
        probe(vec![imp("src/lib.rs", "")], "src/lib.rs"),
        Some(Problem::Unavailable(Why::NoManifest))
    );
    assert_eq!(
        probe(base.clone(), "src/missing.rs"),
        Some(Problem::EntryMissing)
    );
    assert_eq!(
        probe(
            vec![
                imp("Cargo.toml", CARGO),
                imp("src/main.rs", "fn main() {}\n")
            ],
            "src/main.rs"
        ),
        Some(Problem::Unavailable(Why::NoLibrary))
    );
    let mut with_build = base.clone();
    with_build.push(imp("build.rs", "fn main() {}\n"));
    assert_eq!(
        probe(with_build, "src/lib.rs"),
        Some(Problem::Unavailable(Why::BuildScript))
    );
    // dependencies in the manifest
    let mut deps = base.clone();
    deps[0] = imp(
        "Cargo.toml",
        &format!("{CARGO}[dependencies]\nserde = \"1\"\n"),
    );
    assert_eq!(
        probe(deps, "src/lib.rs"),
        Some(Problem::Unavailable(Why::Dependencies))
    );
    // a file that is not in the module tree cannot be depended on
    assert_eq!(
        probe(base.clone(), "src/unused.rs"),
        Some(Problem::Unavailable(Why::NotInCrate))
    );
    // a withheld module anywhere in the tree refuses the check and names the file; the manifest too
    let mut secret = base.clone();
    secret[4] = imp_at(
        "src/util/mod.rs",
        "mod inner;\nconst K: &str = \"k\";\n",
        Level::Restricted,
    );
    assert_eq!(
        probe(secret, "src/auth/tokens.rs"),
        Some(Problem::Restricted("src/util/mod.rs".into()))
    );
    let mut manifest = base.clone();
    manifest[0] = imp_at("Cargo.toml", CARGO, Level::Restricted);
    assert_eq!(
        probe(manifest, "src/lib.rs"),
        Some(Problem::Restricted("Cargo.toml".into()))
    );
    // an included withheld file refuses it too, but a withheld file only named in a comment does not
    let mut incl = base.clone();
    incl[8] = imp_at("src/data.txt", "secret\n", Level::Restricted);
    assert_eq!(
        probe(incl, "src/lib.rs"),
        Some(Problem::Restricted("src/data.txt".into()))
    );
    let mut named = base.clone();
    named[1] = imp("src/lib.rs", "// see .env\npub mod auth;\n");
    assert!(rust_crate("src/auth.rs", &idx(&named)).is_ok());
}

#[test]
fn rs06_layouts_the_scanner_cannot_place_and_hostile_sources_end_in_a_reason_or_a_smaller_sample() {
    let with_lib = |lib: &str| {
        vec![
            imp("Cargo.toml", CARGO),
            imp("src/lib.rs", lib),
            imp("src/a.rs", ""),
            imp("src/b/c.rs", ""),
        ]
    };
    let probe = |lib: &str| rust_crate("src/lib.rs", &idx(&with_lib(lib)));
    // a path attribute inside an inline module, and one hidden in cfg_attr, are not followed
    assert_eq!(
        probe("mod m { #[path = \"x.rs\"] mod y; }\n").err(),
        Some(Problem::Unavailable(Why::UnusualLayout))
    );
    assert_eq!(
        probe("#[path = \"x.rs\"] mod m { }\n").err(),
        Some(Problem::Unavailable(Why::UnusualLayout))
    );
    assert_eq!(
        probe("#[cfg_attr(unix, path = \"a.rs\")]\nmod m;\n").err(),
        Some(Problem::Unavailable(Why::UnusualLayout))
    );
    // a path that climbs out of the project is dropped; a module that names itself ends
    assert_eq!(probe("#[path = \"../../../../etc/passwd\"] mod x;\n#[path = \"lib.rs\"] mod again;\nmod a;\n").unwrap().sample, vec!["src/a.rs", "src/lib.rs"]);
    // raw strings, nested comments, lifetimes, characters and attributes do not fool it
    let tricky = "r#\"mod a;\"#; /* /* mod a; */ mod a; */ fn f<'a>(x: &'a str) {} const C: char = '\"'; #[derive(Debug)] struct S; mod b { pub mod c; }\n";
    assert_eq!(
        probe(tricky).unwrap().sample,
        vec!["src/b/c.rs", "src/lib.rs"]
    );
    // `mod` as part of a longer word, or without a name, is not a module
    assert_eq!(
        probe("let model = 1; let x = mod_a;\n").unwrap().sample,
        vec!["src/lib.rs"]
    );
    // junk never panics
    for junk in [
        "\"unterminated",
        "/* unterminated",
        "r###\"unterminated",
        "'",
        "#[",
        "#![",
        "mod",
        "mod a",
        "mod a {",
        "}}}}}}",
        &"{".repeat(10_000),
        &"mod a { ".repeat(2000),
    ] {
        let _ = probe(junk);
    }
}

#[test]
fn rs07_a_crate_with_too_many_files_or_too_many_bytes_is_too_large_to_build_here() {
    let mut lib = String::new();
    let mut files = vec![imp("Cargo.toml", CARGO)];
    for i in 0..450 {
        lib.push_str(&format!("mod m{i};\n"));
        files.push(imp(&format!("src/m{i}.rs"), ""));
    }
    files.push(imp("src/lib.rs", &lib));
    assert_eq!(
        rust_crate("src/lib.rs", &idx(&files)).err(),
        Some(Problem::Unavailable(Why::TooLarge))
    );
    let big = "x".repeat(3 << 20);
    let files = vec![
        imp("Cargo.toml", CARGO),
        imp("src/lib.rs", "mod a;\nmod b;\nmod c;\n"),
        imp("src/a.rs", &big),
        imp("src/b.rs", &big),
        imp("src/c.rs", &big),
    ];
    assert_eq!(
        rust_crate("src/lib.rs", &idx(&files)).err(),
        Some(Problem::Unavailable(Why::TooLarge))
    );
}

const PASS: &str = "\nrunning 4 tests\ntest tests::expected_panic - should panic ... ok\ntest tests::passes ... ok\ntest tests::passes_two ... ok\ntest tests::skipped ... ignored\n\ntest result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n";
const FAIL: &str = "\nrunning 2 tests\ntest tests::other ... ok\ntest tests::passes ... FAILED\n\nfailures:\n\n---- tests::passes stdout ----\n\nthread 'tests::passes' panicked at b.rs:5:27:\nassertion `left == right` failed\n  left: 4\n right: 3\n\n\nfailures:\n    tests::passes\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n";
const NONE: &str = "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n";

#[test]
fn rs08_a_test_run_is_read_strictly_from_the_real_output_of_the_test_runner() {
    // outputs captured from rustc 1.94.1 and 1.93.1
    assert_eq!(parse_rust_tests(&Exit::Code(0), PASS), TestRun::Passed(3));
    assert!(
        matches!(parse_rust_tests(&Exit::Code(101), FAIL), TestRun::Failed(m) if m.contains("1 test failed"))
    );
    assert_eq!(parse_rust_tests(&Exit::Code(0), NONE), TestRun::NoTests);
    // everything ignored is no evidence
    let ignored = "\nrunning 1 test\ntest t ... ignored\n\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert_eq!(parse_rust_tests(&Exit::Code(0), ignored), TestRun::NoTests);
    // the exit status must agree
    assert!(matches!(
        parse_rust_tests(&Exit::Code(1), PASS),
        TestRun::Failed(_)
    ));
    assert!(matches!(
        parse_rust_tests(&Exit::TimedOut, PASS),
        TestRun::Failed(_)
    ));
    assert!(matches!(
        parse_rust_tests(&Exit::Code(0), FAIL),
        TestRun::Failed(_)
    ));
    // nothing, junk, a summary without its announcement, a summary that does not add up
    for text in [
        "",
        "hello",
        "test result: ok.",
        "test result: ok. 3 passed",
        "test result: ok. x passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
        "test result: maybe. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
    ] {
        assert!(
            matches!(parse_rust_tests(&Exit::Code(0), text), TestRun::Failed(_)),
            "{text:?}"
        );
    }
    let no_header = PASS.replace("running 4 tests\n", "");
    assert!(matches!(
        parse_rust_tests(&Exit::Code(0), &no_header),
        TestRun::Failed(_)
    ));
    // only the announcement is wrong: the lines and the counts agree with each other but not with how many tests were said to run
    let lying_header = PASS.replace("running 4 tests", "running 9 tests");
    assert!(
        matches!(parse_rust_tests(&Exit::Code(0), &lying_header), TestRun::Failed(m) if m.contains("add up"))
    );
    let wrong_sum = PASS.replace("3 passed", "5 passed");
    assert!(matches!(
        parse_rust_tests(&Exit::Code(0), &wrong_sum),
        TestRun::Failed(_)
    ));
    // a summary that says ok but counts a failure, or FAILED with none
    assert!(matches!(
        parse_rust_tests(
            &Exit::Code(0),
            &PASS.replace("0 failed; 1 ignored", "1 failed; 0 ignored")
        ),
        TestRun::Failed(_)
    ));
    assert!(matches!(
        parse_rust_tests(
            &Exit::Code(0),
            &PASS.replace("test result: ok.", "test result: FAILED.")
        ),
        TestRun::Failed(_)
    ));
    // a failing test cannot print its way to a pass: a forged summary inside its output is followed by the real, last one
    let forged = FAIL.replace("assertion `left == right` failed", "test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s");
    assert!(matches!(
        parse_rust_tests(&Exit::Code(101), &forged),
        TestRun::Failed(_)
    ));
    // a forged line after the real summary changes the announced total, so it cannot add up
    let after = format!("{PASS}test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n");
    assert!(matches!(
        parse_rust_tests(&Exit::Code(0), &after),
        TestRun::Failed(_)
    ));
    // a report line that is not backed by the summary
    let fake_line = PASS.replace(
        "test tests::skipped ... ignored",
        "test tests::skipped ... ok",
    );
    assert!(matches!(
        parse_rust_tests(&Exit::Code(0), &fake_line),
        TestRun::Failed(_)
    ));
}

#[test]
fn rs09_a_failed_build_is_sorted_into_the_environment_cannot_and_the_script_is_wrong() {
    let marker = |stage: &str, rest: &str| format!("KK_BUILD_FAILED {stage}\n{rest}");
    assert_eq!(
        build_failure(&marker("crate", "error: x")),
        Some((Stage::Crate, "error: x"))
    );
    assert_eq!(
        build_failure(&marker("script", "")),
        Some((Stage::Script, ""))
    );
    // only the first line counts: a program that prints the words later is not a build failure
    for text in [
        "",
        "hello\nKK_BUILD_FAILED crate\n",
        "KK_BUILD_FAILED crate extra\n",
        "kk_build_failed crate\n",
        "KK_BUILD_FAILED\n",
        " KK_BUILD_FAILED crate\n",
    ] {
        assert_eq!(build_failure(text), None, "{text:?}");
    }
    let linker =
        "error: linker `cc` not found\n  |\n  = note: No such file or directory (os error 2)\n";
    for stage in [Stage::Crate, Stage::Script] {
        assert_eq!(
            classify_build_failure(stage, linker),
            Some(BuildProblem::Environment(LINKER_SENTENCE))
        );
        assert_eq!(
            classify_build_failure(
                stage,
                "error: edition 2024 is unstable and only available with -Z unstable-options\n"
            ),
            Some(BuildProblem::Environment(EDITION_SENTENCE))
        );
    }
    // a module or included file missing from the crate's sample is the sample's gap; the same words from the script are its own mistake
    let e0583 = "error[E0583]: file not found for module `x`\n";
    let unread = "error: couldn't read `/sample/src/data.txt`: No such file\n";
    assert_eq!(
        classify_build_failure(Stage::Crate, e0583),
        Some(BuildProblem::Environment(MODULE_SENTENCE))
    );
    assert_eq!(
        classify_build_failure(Stage::Crate, unread),
        Some(BuildProblem::Environment(MODULE_SENTENCE))
    );
    assert_eq!(classify_build_failure(Stage::Script, e0583), None);
    assert_eq!(classify_build_failure(Stage::Script, unread), None);
    // the project's crate failing is the environment's problem; the script failing is the script's, unless it names a missing crate
    let type_error = "error[E0308]: mismatched types\n";
    assert_eq!(
        classify_build_failure(Stage::Crate, type_error),
        Some(BuildProblem::Environment(CRATE_SENTENCE))
    );
    assert_eq!(classify_build_failure(Stage::Script, type_error), None);
    assert_eq!(
        classify_build_failure(
            Stage::Script,
            "error[E0463]: can't find crate for `serde`\n"
        ),
        Some(BuildProblem::Package("serde".into()))
    );
    // only a plain crate name is ever returned
    for evil in [
        "can't find crate for `a b`",
        "can't find crate for `../../x`",
        "can't find crate for `a\nb`",
        "can't find crate for `` ",
        "can't find crate for `1x`",
    ] {
        assert_eq!(
            classify_build_failure(Stage::Script, evil),
            None,
            "{evil:?}"
        );
    }
}

#[test]
fn rs10_tests_that_live_in_the_file_being_fixed_are_recognised() {
    assert!(has_inline_tests("fn a() {}\n#[test]\nfn t() {}\n"));
    assert!(!has_inline_tests(
        "fn a() {}\n// #[test]\nconst S: &str = \"#[test]\";\n"
    ));
    for yes in [
        "#[test]\nfn t() {}",
        "#[cfg(test)]\nmod x {}",
        "#[cfg(all(test, unix))]\nfn t() {}",
        "mod tests { }",
        "mod test;",
        "#[bench]\nfn b() {}",
        "#![cfg(test)]",
    ] {
        assert!(touches_inline_tests(yes), "{yes:?}");
    }
    for no in [
        "fn fix() { let tests = 1; }",
        "// #[test]\nfn a() {}",
        "const S: &str = \"#[cfg(test)]\";",
        "#[derive(Debug)]\nstruct S;",
        "#[cfg(unix)]\nfn a() {}",
        "mod testing { }",
        "pub fn add(a: i32, b: i32) -> i32 { a + b }",
    ] {
        assert!(!touches_inline_tests(no), "{no:?}");
    }
}

use kk_review::rust::{change_touches_tests, test_regions};

const WITH_TESTS: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn t() {\n        assert_eq!(add(1, 2), 3);\n    }\n}\npub fn after() {}\n";

#[test]
fn rs11_the_parts_of_a_file_that_are_tests_are_found_and_a_change_that_overlaps_one_is_caught() {
    let r = test_regions(WITH_TESTS);
    assert_eq!(r.len(), 1, "{r:?}");
    let (a, z) = r[0];
    assert!(
        WITH_TESTS[a..z].starts_with("#[cfg(test)]") && WITH_TESTS[a..z].ends_with("}"),
        "{:?}",
        &WITH_TESTS[a..z]
    );
    assert!(!WITH_TESTS[a..z].contains("after") && !WITH_TESTS[a..z].contains("a + b"));
    // production code is open to a fix; the tests are not, however the change is aimed at them
    assert!(!change_touches_tests(WITH_TESTS, "a + b", "a + b + 0"));
    assert!(!change_touches_tests(
        WITH_TESTS,
        "pub fn after() {}",
        "pub fn after() { }"
    ));
    assert!(change_touches_tests(
        WITH_TESTS,
        "assert_eq!(add(1, 2), 3);",
        "assert!(true);"
    ));
    assert!(change_touches_tests(WITH_TESTS, "add(1, 2)", "add(1, 1)"));
    assert!(
        change_touches_tests(
            WITH_TESTS,
            "    }\n}\npub fn after",
            "    }\n}\npub fn after2"
        ),
        "a change that reaches into the end of a test module overlaps it"
    );
    assert!(
        change_touches_tests(WITH_TESTS, "a + b", "a + b\n#[test]\nfn extra() {}"),
        "a change that brings in a test"
    );
    assert!(change_touches_tests(
        WITH_TESTS,
        "pub fn after() {}",
        "pub fn after() {}\nmod tests {}"
    ));
    // text that is in the file twice is not one change; it is judged as written
    assert!(!change_touches_tests(
        "fn a() {}\nfn a() {}\n",
        "fn a() {}",
        "fn b() {}"
    ));
    assert!(change_touches_tests(
        "#[test]\nfn a() {}\n#[test]\nfn a() {}\n",
        "#[test]\nfn a() {}",
        "fn b() {}"
    ));
}

#[test]
fn rs12_every_form_of_test_marking_is_found_and_production_only_code_is_not() {
    let regions = |s: &str| test_regions(s).len();
    // a test function by itself, with attributes stacked on it
    assert_eq!(
        regions("#[test]\n#[should_panic]\nfn t() { panic!(); }\nfn real() {}\n"),
        1
    );
    // a module in another file, a bench, a cfg that needs test among other things
    assert_eq!(regions("#[cfg(test)]\nmod tests;\nfn real() {}\n"), 1);
    assert_eq!(regions("#[bench]\nfn b() {}\n"), 1);
    assert_eq!(regions("#[cfg(all(test, unix))]\nfn t() {}\n"), 1);
    // an inner attribute makes the whole file a test
    assert_eq!(
        test_regions("#![cfg(test)]\nfn a() {}\n"),
        vec![(0, "#![cfg(test)]\nfn a() {}\n".len())]
    );
    // production-only code, other attributes, and the word in a string or comment are not tests
    assert_eq!(regions("#[cfg(not(test))]\nfn real() {}\n"), 0);
    assert_eq!(
        regions("#[derive(Debug)]\nstruct S;\n#[cfg(unix)]\nfn u() {}\n"),
        0
    );
    assert_eq!(
        regions("// #[test]\nconst S: &str = \"#[cfg(test)]\";\n"),
        0
    );
    assert!(!touches_inline_tests("#[cfg(not(test))]\nfn real() {}"));
    // several tests are several regions; nested braces do not end one early
    let two = "#[test]\nfn a() { if true { let _ = 1; } }\nfn mid() {}\n#[test]\nfn b() {}\n";
    let r = test_regions(two);
    assert_eq!(r.len(), 2);
    assert!(two[r[0].0..r[0].1].ends_with("}") && !two[r[0].0..r[0].1].contains("mid"));
    // junk never panics and never reports a range outside the text
    for junk in [
        "#[",
        "#[test",
        "#[test]",
        "#[test] fn",
        "#[test] fn a() {",
        "#![",
        "#[cfg(test)] mod x { { { ",
        "é#[test]é fn a() {}é",
        "",
    ] {
        for (a, z) in test_regions(junk) {
            assert!(a <= z && z <= junk.len(), "{junk:?}");
        }
    }
}

#[test]
fn rs13_test_files_by_name_and_the_compiler_version_answer_are_read_strictly() {
    use kk_review::check::is_test_path;
    use kk_review::rust::rustc_minor;
    for yes in [
        "tests/it.rs",
        "src/tests.rs",
        "src/test.rs",
        "src/auth/session_test.rs",
        "src/auth/session_tests.rs",
        "crate/tests/helpers/mod.rs",
    ] {
        assert!(is_test_path(yes), "{yes}");
    }
    for no in [
        "src/lib.rs",
        "src/auth/session.rs",
        "src/latest.rs",
        "src/contest.rs",
        "src/testing.rs",
        "src/attest.rs",
    ] {
        assert!(!is_test_path(no), "{no}");
    }
    assert_eq!(
        rustc_minor("rustc 1.93.1 (01f6ddf75 2026-02-11) (built from a source tarball)\n"),
        Some(93)
    );
    assert_eq!(rustc_minor("rustc 1.94.1 (e408947bf 2026-03-25)"), Some(94));
    assert_eq!(
        rustc_minor("rustc 1.95.0-nightly (abc 2026-01-01)\n"),
        Some(95)
    );
    for bad in [
        "",
        "rustc",
        "rustc 2.0.0",
        "rustc 1.x.0",
        "rustc 1.93",
        "rustc 1.93.1\nrustc 1.94.1",
        "cargo 1.93.1",
        " rustc 1.93.1",
        "rustc 1.9999.1",
        "rustc 1..1",
        &format!("rustc 1.93.1 {}", "a".repeat(200)),
    ] {
        assert_eq!(rustc_minor(bad), None, "{bad:?}");
    }
}
