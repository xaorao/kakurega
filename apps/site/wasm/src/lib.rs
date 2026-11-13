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
    for (i, hit) in kk_secrets::scan(&text).iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        // a rule name is one of the detector's own fixed words: letters, digits, dashes and underscores
        let rule: String = hit.rule.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == ' ').collect();
        json.push_str(&format!("{{\"rule\":\"{rule}\",\"start\":{},\"end\":{}}}", hit.start, hit.end));
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
