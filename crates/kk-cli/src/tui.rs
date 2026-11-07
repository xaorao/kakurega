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
        let _ = std::fs::create_dir_all(&dir);
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
        let resume_id = resume_request(args).flatten();
        for a in args {
            if a != "--data-dir"
                && a != "--resume"
                && Some(a) != resume_id.as_ref()
                && a != "--linear"
                && Some(a.as_str()) != crate::flag(args, "--data-dir").as_deref()
            {
                cmd.arg(a);
            }
        }
        cmd.stdin(Stdio::null()).env("KAKU_EXIT_WITH_PARENT", "1");
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
pub fn resume_request(args: &[String]) -> Option<Option<String>> {
    let i = args.iter().position(|a| a == "--resume")?;
    Some(args.get(i + 1).filter(|a| !a.starts_with('-')).cloned())
}

fn run_client(dir: &std::path::Path, args: &[String]) -> u8 {
    let dir = dir.to_path_buf();
    let resume = resume_request(args);
    let linear = args.iter().any(|a| a == "--linear")
        || std::env::var("KAKU_LINEAR")
            .is_ok_and(|v| !matches!(v.trim(), "" | "0" | "off" | "false" | "no"));
    let result = if linear {
        kk_tui::linear::run(&dir, resume)
    } else {
        kk_tui::run::run(&dir, resume)
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

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rs01_resume_alone_means_the_newest_and_resume_with_a_word_means_that_conversation() {
        assert_eq!(resume_request(&a(&["tui"])), None);
        assert_eq!(resume_request(&a(&["tui", "--resume"])), Some(None));
        assert_eq!(resume_request(&a(&["--resume", "--linear"])), Some(None));
        assert_eq!(
            resume_request(&a(&["tui", "--resume", "--data-dir", "/x"])),
            Some(None)
        );
        assert_eq!(
            resume_request(&a(&["tui", "--resume", "abc123"])),
            Some(Some("abc123".into()))
        );
        assert_eq!(
            resume_request(&a(&["--data-dir", "/x", "--resume", "abc123", "--linear"])),
            Some(Some("abc123".into()))
        );
    }
}
