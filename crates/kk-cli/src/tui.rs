use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn responsive(dir: &std::path::Path) -> bool {
    kk_tui::http::Http::connect(dir).is_ok()
}

struct Guard(Option<Child>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(c) = &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Runs `f` against a responsive local service, starting one for the duration if none answers, the way the terminal workspace does.
/// Every argument that is not for this program's own flags is passed on to the service it starts.
pub fn with_service(args: &[String], f: impl FnOnce(&std::path::Path) -> u8) -> u8 {
    let dir = crate::data_dir(args);
    let mut guard = Guard(None);
    if let Err(code) = ensure_service(&dir, args, &mut guard) {
        return code;
    }
    let code = f(&dir);
    drop(guard);
    code
}

pub fn run(args: &[String]) -> u8 {
    let dir = crate::data_dir(args);
    let mut guard = Guard(None);
    if let Err(code) = ensure_service(&dir, args, &mut guard) {
        return code;
    }
    run_client(&dir, args)
}

fn ensure_service(dir: &std::path::Path, args: &[String], guard: &mut Guard) -> Result<(), u8> {
    let dir = dir.to_path_buf();
    if !responsive(&dir) {
        let _ = kk_service::keys::ensure_private_dir(&dir);
        let log = std::fs::File::create(dir.join("serve.log")).ok();
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("cannot find the kaku executable: {e}");
                return Err(1);
            }
        };
        let mut cmd = Command::new(exe);
        cmd.arg("serve").arg("--data-dir").arg(&dir);
        let resume_id = resume_request(args).and_then(|r| r.id);
        for a in args {
            if a != "--data-dir"
                && a != "--resume"
                && Some(a) != resume_id.as_ref()
                && a != "--linear"
                && a != "--fork"
                && Some(a.as_str()) != crate::flag(args, "--data-dir").as_deref()
            {
                cmd.arg(a);
            }
        }
        // the service is told which process owns it, so it leaves when that one is gone even if it died before the service
        // finished starting
        cmd.stdin(Stdio::null())
            .env("KAKU_EXIT_WITH_PARENT", std::process::id().to_string());
        match log {
            Some(f) => {
                let f2 = f.try_clone().ok();
                cmd.stdout(Stdio::from(f));
                if let Some(f2) = f2 {
                    cmd.stderr(Stdio::from(f2));
                }
            }
            None => {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        let _ = std::fs::remove_file(dir.join("admin.json"));
        match cmd.spawn() {
            Ok(c) => guard.0 = Some(c),
            Err(e) => {
                eprintln!("could not start the service: {e}");
                return Err(1);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && !responsive(&dir) {
            std::thread::sleep(Duration::from_millis(150));
        }
        if !responsive(&dir) {
            eprintln!(
                "the service did not start; see {}",
                dir.join("serve.log").display()
            );
            return Err(1);
        }
    }
    Ok(())
}

/// `--resume` opens the newest kept conversation; `--resume ID` opens that one. The id is the word right after the flag, if it is not another flag.
pub fn resume_request(args: &[String]) -> Option<kk_tui::app::Resume> {
    let i = args.iter().position(|a| a == "--resume")?;
    Some(kk_tui::app::Resume {
        id: args.get(i + 1).filter(|a| !a.starts_with('-')).cloned(),
        fork: args.iter().any(|a| a == "--fork"),
    })
}

fn run_client(dir: &std::path::Path, args: &[String]) -> u8 {
    let dir = dir.to_path_buf();
    let resume = resume_request(args);
    // the display settings: a command-line option, then the environment, then config.toml, then the defaults; a file with a mistake
    // is not applied at all and the workspace says so at the top
    let (file, notice) = match crate::config::load(&dir) {
        crate::config::Loaded::Absent => (None, None),
        crate::config::Loaded::Valid(f) => (Some(f), None),
        crate::config::Loaded::Invalid(problems) => {
            (None, Some(crate::config::notice_for(&problems)))
        }
    };
    let effective = crate::config::resolve(file.as_ref(), &|k| std::env::var_os(k), args);
    let linear = matches!(
        crate::config::value_of(&effective, "linear"),
        crate::config::Value::Bool(true)
    );
    let settings = crate::config::tui_settings(&effective, notice);
    let result = if linear {
        kk_tui::linear::run_with(&dir, resume, settings)
    } else {
        kk_tui::run::run_with(&dir, resume, settings)
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod resume_args {
    use super::resume_request;
    use kk_tui::app::Resume;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn r(id: Option<&str>, fork: bool) -> Option<Resume> {
        Some(Resume {
            id: id.map(str::to_string),
            fork,
        })
    }

    #[test]
    fn rs01_resume_alone_means_the_newest_and_resume_with_a_word_means_that_conversation() {
        assert_eq!(resume_request(&a(&["tui"])), None);
        assert_eq!(resume_request(&a(&["tui", "--resume"])), r(None, false));
        assert_eq!(
            resume_request(&a(&["--resume", "--linear"])),
            r(None, false)
        );
        assert_eq!(
            resume_request(&a(&["tui", "--resume", "--data-dir", "/x"])),
            r(None, false)
        );
        assert_eq!(
            resume_request(&a(&["tui", "--resume", "abc123"])),
            r(Some("abc123"), false)
        );
        assert_eq!(
            resume_request(&a(&["--data-dir", "/x", "--resume", "abc123", "--linear"])),
            r(Some("abc123"), false)
        );
    }

    #[test]
    fn rs02_fork_asks_for_a_branch_and_means_nothing_without_resume() {
        assert_eq!(resume_request(&a(&["--resume", "--fork"])), r(None, true));
        assert_eq!(
            resume_request(&a(&["--fork", "--resume", "abc123"])),
            r(Some("abc123"), true)
        );
        assert_eq!(
            resume_request(&a(&["--resume", "abc123", "--fork"])),
            r(Some("abc123"), true)
        );
        assert_eq!(resume_request(&a(&["tui", "--fork"])), None);
    }
}
