use kk_sandbox::{valid_unit_name, SandboxSpec};

#[test]
fn w01_the_unit_name_travels_with_the_spec_so_the_host_can_stop_the_scope_inside_the_vm() {
    let mut spec = SandboxSpec::new("/usr/bin/python3");
    spec.args = vec!["-c".into(), "print(1)".into()];
    let wire = spec.to_wire_with_unit(Some("kk-worker-123-4"));
    let (back, unit) = SandboxSpec::from_wire_with_unit(&wire).unwrap();
    assert_eq!(unit.as_deref(), Some("kk-worker-123-4"));
    assert_eq!(back.program, spec.program);
    assert_eq!(back.args, spec.args);
    // the older form still works and carries no unit
    let (_, none) = SandboxSpec::from_wire_with_unit(&spec.to_wire()).unwrap();
    assert_eq!(none, None);
    assert_eq!(
        SandboxSpec::from_wire(&spec.to_wire()).unwrap().program,
        spec.program
    );
}

#[test]
fn w02_only_a_plain_worker_unit_name_is_ever_used() {
    for good in [
        "kk-worker-1-0",
        "kk-worker-4242-17",
        "kk-worker-9999999999-9999999999",
    ] {
        assert!(valid_unit_name(good), "{good}");
    }
    for bad in [
        "",
        "kk-worker-",
        "kk-worker-1-0.scope",
        "kk-worker-1-0; reboot",
        "kk-worker-1 -0",
        "kk-worker-../../x",
        "kk-worker-1-0\n",
        "kk-worker-1-0\u{202e}",
        "sshd",
        "ssh.service",
        "kk-worker-a-b",
        "KK-WORKER-1-0",
        "-p",
        "1-2",
        "worker-1-2",
        "xkk-worker-1-0",
        "kk-worker-111111111111111111111-1",
        "kk-worker-1-111111111111111111111",
        "kk-worker--1",
        "kk-worker-1-",
        "kk-worker-1-0-1",
        &format!("kk-worker-{}", "1".repeat(80)),
    ] {
        assert!(!valid_unit_name(bad), "{bad:?}");
    }
}

#[test]
fn w_scope_limits_are_set_and_an_out_of_memory_kill_does_not_make_systemd_stop_the_whole_scope() {
    let args: Vec<String> = kk_sandbox::scope_args(&kk_sandbox::Limits::default(), "kk-worker-1-1")
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    for want in [
        "--user",
        "--scope",
        "--unit=kk-worker-1-1",
        "MemorySwapMax=0",
    ] {
        assert!(args.iter().any(|a| a == want), "missing {want}: {args:?}");
    }
    assert!(
        args.iter().any(|a| a.starts_with("TasksMax="))
            && args.iter().any(|a| a.starts_with("MemoryMax="))
    );
    // The kernel kills the process that goes over its memory. By default systemd then also sends SIGTERM to the rest of the
    // scope, which races with the sandbox seeing the real death and makes the reported result vary (SIGKILL one time, SIGTERM
    // the next). The kill is already the enforcement, so the scope is left alone.
    assert!(args.iter().any(|a| a == "OOMPolicy=continue"), "{args:?}");
}
