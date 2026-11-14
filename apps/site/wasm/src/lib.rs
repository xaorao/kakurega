//! The product's own secret detector, built for a browser. No bindings generator: three plain functions. The page hands
//! in text and gets back, as JSON, where each credential-shaped thing is and which rule found it. Nothing here can
//! reach a network or a file: a WebAssembly module has only the memory it is given.

/// Room for `len` bytes that the page fills with UTF-8 text.
#[no_mangle]
pub extern "C" fn kk_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Gives back memory that `kk_alloc` or `kk_scan` handed out, with the size it had.
///
/// # Safety
/// `ptr` and `cap` must come from one of those two functions, once.
#[no_mangle]
pub unsafe extern "C" fn kk_free(ptr: *mut u8, cap: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, cap.max(1)));
    }
}

/// Scans `len` bytes at `ptr`. The answer is a block of memory that starts with its own length (4 bytes, little
/// endian) followed by that many bytes of JSON: `[{"rule": "...", "start": N, "end": N}]`, offsets in bytes.
///
/// # Safety
/// `ptr` must point at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn kk_scan(ptr: *const u8, len: usize) -> *mut u8 {
    let bytes = std::slice::from_raw_parts(ptr, len);
    let text = String::from_utf8_lossy(bytes);
    let mut json = String::from("[");
    for (i, (rule, start, end)) in find_all(&text).iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        // a rule name is one of the detector's own fixed words: letters, digits, dashes and underscores
        let rule: String = rule.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == ' ').collect();
        json.push_str(&format!("{{\"rule\":\"{rule}\",\"start\":{start},\"end\":{end}}}"));
    }
    json.push(']');
    let body = json.into_bytes();
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    let ptr = out.as_mut_ptr();
    std::mem::forget(out);
    ptr
}

/// The detector's generation, so the page can say which one it runs.
#[no_mangle]
pub extern "C" fn kk_detector_version() -> u32 {
    kk_secrets::DETECTOR_VERSION
}

/// Everything in `text` that must not leave: what the detector's shapes find, a password said as a password, and every
/// line of a private-key block. The shape rule marks only a block's first line, and the lines under it show nothing of
/// what they are on their own, so a block is taken whole here: from its `-----BEGIN ... PRIVATE KEY-----` line through the
/// matching `-----END` line, or to the end of the text when there is none (the product does the same for files it reads).
/// Offsets are bytes.
pub fn find_all(text: &str) -> Vec<(&'static str, usize, usize)> {
    let mut found: Vec<(&'static str, usize, usize)> = kk_secrets::scan(text).iter().map(|h| (h.rule, h.start, h.end)).collect();
    for (start, end) in kk_secrets::spoken::find(text) {
        found.push(("password said as a password", start, end));
    }
    let mut inside = false;
    let mut at = 0;
    for line in text.split('\n') {
        let t = line.trim();
        let begins = !inside && t.starts_with("-----BEGIN ") && t.contains("PRIVATE KEY");
        if begins || inside {
            found.push(("private key block", at, at + line.len().max(1)));
            inside = if begins { true } else { !t.starts_with("-----END ") };
        }
        at += line.len() + 1;
    }
    found.sort_by_key(|f| (f.1, f.2));
    found
}

#[cfg(test)]
mod tests {
    use super::find_all;

    /// The 1-based lines a hit touches, as the page works them out.
    fn lines(text: &str) -> Vec<usize> {
        let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let line_of = |o: usize| starts.iter().rposition(|s| *s <= o).unwrap_or(0) + 1;
        let mut out: Vec<usize> = find_all(text).iter().flat_map(|(_, s, e)| line_of(*s)..=line_of(e.saturating_sub(1).max(*s))).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn a_private_key_block_is_withheld_whole_not_only_its_first_line() {
        let key = ["-----BEGIN ", "OPENSSH PRIVATE KEY-----"].concat();
        let text = format!("x = 1\n{key}\nb3BlbnNzaC1rZXktdjEAAAAA\nQyNTUxOQAAACD\n-----END OPENSSH PRIVATE KEY-----\ny = 2\n");
        assert_eq!(lines(&text), vec![2, 3, 4, 5]);
    }

    #[test]
    fn a_block_with_no_end_is_withheld_to_the_end_and_a_second_block_is_found_too() {
        let begin = ["-----BEGIN ", "RSA PRIVATE KEY-----"].concat();
        let two = format!("{begin}\nAAAA\n-----END RSA PRIVATE KEY-----\nkeep\n{begin}\nBBBB\nCCCC");
        assert_eq!(lines(&two), vec![1, 2, 3, 5, 6, 7]);
    }

    #[test]
    fn offsets_are_bytes_so_text_in_other_scripts_before_a_block_does_not_shift_it() {
        let begin = ["-----BEGIN ", "EC PRIVATE KEY-----"].concat();
        let text = format!("# \u{0928}\u{092e}\u{0938}\u{094d}\u{0924}\u{0947} \u{1F512}\n{begin}\nMHcCAQEE\n-----END EC PRIVATE KEY-----\nok\n");
        assert_eq!(lines(&text), vec![2, 3, 4]);
    }

    #[test]
    fn a_password_said_as_a_password_is_found_and_plain_code_is_not() {
        assert!(!lines("the database password is Tuesday-maple-42, do not share it\n").is_empty());
        assert!(lines("def add(a, b):\n    return a + b\n").is_empty());
        // a public key block is not a secret
        assert!(lines("-----BEGIN PUBLIC KEY-----\nMFkw\n-----END PUBLIC KEY-----\n").is_empty());
    }
}
