use crate::runner::ReviewRunner;
use kk_companion::{start, Options};
use kk_sandbox::Transport;
use kk_service::{Routes, Service};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

fn vm_home(vm: &str) -> Result<String, String> {
    let out = Command::new("limactl")
        .args(["shell", vm, "--", "bash", "-lc", "echo $HOME"])
        .output()
        .map_err(|e| format!("limactl is not available: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .last()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .ok_or_else(|| "could not read the VM home directory".to_string())
}

pub fn run(args: &[String]) -> u8 {
    match serve(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("kaku serve: {e}");
            1
        }
    }
}

fn serve(args: &[String]) -> Result<(), String> {
    let data = crate::data_dir(args);
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let home = PathBuf::from(std::env::var("HOME").map_err(|_| "HOME is not set")?);
    let mut roots: Vec<PathBuf> = crate::flags(args, "--root")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if roots.is_empty() {
        roots.push(home.clone());
    }
    let model = crate::flag(args, "--model").unwrap_or_else(|| "qwen2.5:1.5b".into());
    let ollama = crate::flag(args, "--ollama").unwrap_or_else(|| "http://127.0.0.1:11434".into());
    let default_transport = if cfg!(target_os = "linux") {
        "local"
    } else {
        "lima"
    };
    let transport_name =
        crate::flag(args, "--transport").unwrap_or_else(|| default_transport.into());
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let (transport, worker_binary) = match transport_name.as_str() {
        "local" => (
            Transport::Local,
            crate::flag(args, "--worker-bin")
                .map(PathBuf::from)
                .unwrap_or_else(|| exe_dir.join("kk-worker")),
        ),
        "lima" => {
            let vm = crate::flag(args, "--vm").unwrap_or_else(|| "kak".into());
            let vh = vm_home(&vm).unwrap_or_else(|e| {
                eprintln!("warning: {e}; the setup check in the app will say what to fix");
                "/vm-not-available".to_string()
            });
            (
                Transport::Lima {
                    vm: vm.clone(),
                    launcher: crate::flag(args, "--launcher")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            PathBuf::from(format!("{vh}/kk-target/debug/kk-sandbox-launch"))
                        }),
                },
                crate::flag(args, "--worker-bin")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(format!("{vh}/kk-target/debug/kk-worker"))),
            )
        }
        other => return Err(format!("unknown transport `{other}` (use lima or local)")),
    };
    let scratch = data.join("scratch");
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let keys = kk_service::keys::load_keys(&data)?;
    let runner = Arc::new(ReviewRunner {
        model,
        ollama,
        transport,
        worker_binary,
        scratch,
        commitment_key: keys.commitment_key,
        max_turns: 20,
        scripted: crate::flag(args, "--scripted").map(PathBuf::from),
        context_tokens: match crate::flag(args, "--context") {
            Some(v) => v
                .parse::<u32>()
                .ok()
                .filter(|n| (2048..=1_048_576).contains(n))
                .ok_or("--context must be a number of tokens between 2048 and 1048576")?,
            None => kk_route::DEFAULT_CONTEXT_TOKENS,
        },
    });
    drop(keys);
    let test_mode = crate::flag(args, "--scripted").is_some();
    let service = Service::new(&data, roots, runner, test_mode)?;
    service.set_require_approver(args.iter().any(|a| a == "--require-approver"));
    let ui_dir = crate::flag(args, "--ui-dir")
        .map(PathBuf::from)
        .or_else(|| {
            [
                exe_dir.join("../../apps/web/dist"),
                PathBuf::from("apps/web/dist"),
            ]
            .into_iter()
            .find(|p| p.join("index.html").exists())
        });
    // The desktop shell hands over its per-launch secret on stdin, never in the environment or arguments, which the review
    // workers would inherit and other programs could list.
    let owned_by_shell = args.iter().any(|a| a == "--shell-secret-stdin");
    let shell_secret = if owned_by_shell {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| format!("cannot read the shell secret: {e}"))?;
        let secret = line.trim().to_string();
        if secret.len() < 32 {
            return Err(
                "--shell-secret-stdin needs a secret of at least 32 characters on the first line"
                    .into(),
            );
        }
        Some(secret)
    } else {
        None
    };
    let companion = start(Options {
        shell_secret,
        ui_dir: ui_dir.clone(),
        extension: Arc::new(Routes(service.clone())),
        ..Options::default()
    })
    .map_err(|e| e.to_string())?;
    let admin = serde_json::json!({"port": companion.addr.port(), "token": companion.security.admin_token()});
    let admin_path = data.join("admin.json");
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::remove_file(&admin_path);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&admin_path)
            .map_err(|e| e.to_string())?;
        f.write_all(admin.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let code = companion.security.issue_pairing_code();
    println!("Kakurega is running.");
    println!(
        "  Open (valid 5 minutes, works once):  http://127.0.0.1:{}/#pair={}",
        companion.addr.port(),
        code
    );
    println!("  New link later:                      kaku pair");
    println!("  Approve a release in this terminal:  kaku approve");
    match &ui_dir {
        Some(d) => println!("  Web interface from:                  {}", d.display()),
        None => println!("  No web interface build found (apps/web/dist); only the API is served."),
    }
    println!("Press Ctrl-C to stop.");
    let owner =
        std::env::var_os("KAKU_EXIT_WITH_PARENT").map(|_| std::os::unix::process::parent_id());
    // A shell that handed over its secret keeps the other end of the pipe open for as long as it lives. When the pipe
    // closes the shell is gone, however it went (a crash or a forced quit leave no time to say so), and nothing is left to
    // serve: the service leaves too instead of running on with nobody holding its window.
    let shell_gone = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if owned_by_shell {
        let flag = shell_gone.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut sink = [0u8; 256];
            let mut stdin = std::io::stdin().lock();
            while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if shell_gone.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = std::fs::remove_file(data.join("admin.json"));
            return Ok(());
        }
        if let Some(p) = owner {
            if std::os::unix::process::parent_id() != p {
                let _ = std::fs::remove_file(data.join("admin.json"));
                return Ok(());
            }
        }
    }
}
