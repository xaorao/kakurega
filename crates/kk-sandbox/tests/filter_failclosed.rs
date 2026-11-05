#![cfg(target_os = "linux")]

use kk_sandbox::{run_unchecked, SandboxError, SandboxSpec};

/// Its own test binary, because it changes the process environment.
#[test]
fn ff01_when_the_filter_cannot_be_prepared_nothing_runs_unfiltered() {
    let missing =
        std::env::temp_dir().join(format!("kk-no-such-runtime-dir-{}", std::process::id()));
    std::env::set_var("XDG_RUNTIME_DIR", &missing);
    let mut spec = SandboxSpec::new("/usr/bin/python3");
    spec.args = vec!["-I".into(), "-c".into(), "print('RAN')".into()];
    match run_unchecked(&spec) {
        Err(SandboxError::Spawn(m)) => assert!(m.contains("system-call filter"), "{m}"),
        Err(other) => panic!("unexpected error: {other}"),
        Ok(o) => panic!(
            "the worker ran without its filter: {:?} {:?}",
            o.exit,
            String::from_utf8_lossy(&o.stdout)
        ),
    }
}
