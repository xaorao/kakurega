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

pub fn run(args: &[String]) -> u8 {
    let dir = crate::data_dir(args);
    let mut guard = Guard(None);
    if !responsive(&dir) {
        let _ = std::fs::create_dir_all(&dir);
        let log = std::fs::File::create(dir.join("serve.log")).ok();
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("cannot find the kaku executable: {e}");
                return 1;
            }
        };
        let mut cmd = Command::new(exe);
        cmd.arg("serve").arg("--data-dir").arg(&dir);
        for a in args {
            if a != "--data-dir" && Some(a.as_str()) != crate::flag(args, "--data-dir").as_deref() {
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
                return 1;
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
            return 1;
        }
    }
    match kk_tui::run::run(&dir) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
