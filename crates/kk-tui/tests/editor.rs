use kk_tui::editor::{chosen, edit_with, MAX_EDITED};
use std::path::PathBuf;

fn script(name: &str, body: &str) -> String {
    let d: PathBuf =
        std::env::temp_dir().join(format!("kk-editor-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("ed.sh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    // run through sh so a script that was just written is never executed directly (another test thread forking at that
    // moment can make the kernel refuse with "text file busy")
    format!("sh {}", p.to_string_lossy())
}

#[test]
fn ed01_the_owners_editor_is_chosen_visual_first_and_never_empty() {
    assert_eq!(
        chosen(Some("code -w".into()), Some("nano".into())),
        "code -w"
    );
    assert_eq!(chosen(None, Some("nano".into())), "nano");
    assert_eq!(chosen(Some("  ".into()), Some("nano".into())), "nano");
    assert_eq!(chosen(None, None), "vi");
}

#[test]
fn ed02_text_goes_to_the_editor_and_what_is_saved_comes_back() {
    let ed = script("ok", "printf ' and more' >> \"$1\"");
    assert_eq!(
        edit_with(&ed, "first draft").unwrap(),
        "first draft and more"
    );
    let rewrite = script(
        "rewrite",
        "printf 'rewritten line\\nsecond line\\n' > \"$1\"",
    );
    assert_eq!(
        edit_with(&rewrite, "x").unwrap(),
        "rewritten line\nsecond line\n"
    );
}

#[test]
fn ed03_the_file_is_private_while_the_editor_runs_and_gone_afterwards() {
    let probe = std::env::temp_dir().join(format!("kk-editor-probe-{}", std::process::id()));
    let ed = script(
        "probe",
        &format!(
            "ls -ld \"$(dirname \"$1\")\" > {p}; ls -l \"$1\" >> {p}; echo \"$1\" >> {p}",
            p = probe.display()
        ),
    );
    edit_with(&ed, "secret draft").unwrap();
    let out = std::fs::read_to_string(&probe).unwrap();
    let mut lines = out.lines();
    assert!(lines.next().unwrap().starts_with("drwx------"), "{out}");
    assert!(lines.next().unwrap().starts_with("-rw-------"), "{out}");
    assert!(
        !std::path::Path::new(lines.next().unwrap()).exists(),
        "the file is removed afterwards"
    );
}

#[test]
fn ed04_failures_leave_the_text_alone_and_say_why() {
    assert!(edit_with("", "x").unwrap_err().contains("no editor"));
    assert!(edit_with("/nonexistent/editor", "x")
        .unwrap_err()
        .contains("cannot start"));
    let fails = script("fail", "exit 3");
    assert!(edit_with(&fails, "x").unwrap_err().contains("unchanged"));
    let huge = script(
        "huge",
        &format!(
            "head -c {} /dev/zero | tr '\\0' a > \"$1\"",
            MAX_EDITED + 10
        ),
    );
    assert!(edit_with(&huge, "x").unwrap_err().contains("larger"));
    let binary = script("bin", "printf '\\377\\376' > \"$1\"");
    assert!(edit_with(&binary, "x")
        .unwrap_err()
        .contains("not valid text"));
}

#[test]
fn ed05_the_command_is_never_given_to_a_shell() {
    let marker = std::env::temp_dir().join(format!("kk-editor-shell-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let r = edit_with(&format!("true; touch {}", marker.display()), "x");
    assert!(r.is_ok() || r.is_err());
    assert!(
        !marker.exists(),
        "text after a semicolon is only an argument, never a command"
    );
}

#[test]
fn ed06_the_command_is_split_like_a_shell_reads_quotes_and_nothing_is_expanded() {
    use kk_tui::editor::split_command;
    let v = |s: &str| split_command(s).unwrap();
    assert_eq!(v("code -w"), ["code", "-w"]);
    assert_eq!(v("  nano   -c  "), ["nano", "-c"]);
    assert_eq!(
        v("\"/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl\" -w"),
        [
            "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl",
            "-w"
        ]
    );
    assert_eq!(
        v("'/opt/my editor/ed' --wait"),
        ["/opt/my editor/ed", "--wait"]
    );
    assert_eq!(v("/opt/my\\ editor/ed -x"), ["/opt/my editor/ed", "-x"]);
    assert_eq!(v("ed \"a \\\" b\""), ["ed", "a \" b"]);
    assert_eq!(v("ed ''"), ["ed", ""], "an empty quoted argument is kept");
    assert_eq!(
        v("ed a'b c'd"),
        ["ed", "ab cd"],
        "quotes join into one word"
    );
    // nothing is expanded or run
    assert_eq!(
        v("ed $HOME ~/x `id` *.md"),
        ["ed", "$HOME", "~/x", "`id`", "*.md"]
    );
    assert_eq!(v("ed \"$(id)\""), ["ed", "$(id)"]);
    assert!(v("").is_empty() && v("   ").is_empty());
    assert!(split_command("ed 'open").unwrap_err().contains("unclosed"));
    assert!(split_command("ed \"open").unwrap_err().contains("unclosed"));
}

#[test]
fn ed07_an_editor_whose_path_contains_a_space_is_started() {
    let d = std::env::temp_dir().join(format!("kk editor space {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("my editor.sh");
    std::fs::write(&p, "#!/bin/sh\nprintf ' by a spaced editor' >> \"$1\"\n").unwrap();
    let command = format!("sh \"{}\"", p.to_string_lossy());
    assert_eq!(
        edit_with(&command, "draft").unwrap(),
        "draft by a spaced editor"
    );
    let escaped = format!("sh {}", p.to_string_lossy().replace(' ', "\\ "));
    assert_eq!(
        edit_with(&escaped, "draft").unwrap(),
        "draft by a spaced editor"
    );
    assert!(edit_with("sh 'unclosed", "x")
        .unwrap_err()
        .contains("unclosed"));
    let _ = std::fs::remove_dir_all(&d);
}
