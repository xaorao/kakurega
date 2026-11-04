use kk_evidence::canon::{canonicalize, digest_hex};
use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("line");
        let bytes = line.as_bytes();
        match (canonicalize(bytes), digest_hex(bytes)) {
            (Ok(c), Ok(d)) => writeln!(out, "OK\t{d}\t{c}").expect("write"),
            (Err(e), _) | (_, Err(e)) => writeln!(out, "ERR\t{}", e.code.as_str()).expect("write"),
        }
    }
}
