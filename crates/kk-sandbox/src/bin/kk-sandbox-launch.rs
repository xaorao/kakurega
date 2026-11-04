fn main() {
    let Some(wire) = std::env::args().nth(1) else {
        eprintln!("usage: kk-sandbox-launch <spec-json>");
        std::process::exit(2);
    };
    let err = kk_sandbox::exec_from_wire(&wire);
    eprintln!("{err}");
    std::process::exit(3);
}
