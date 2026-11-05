//! Getting the file a signed announcement names, and nothing else.
//!
//! The announcement fixes the file's exact size and SHA-256. The download therefore streams into a private temporary file,
//! stops the moment it would pass the signed size, hashes as it goes, and is moved into its final place only when both the
//! size and the checksum match. A download that fails, is cut short, runs on, or differs by one byte leaves nothing behind
//! and never touches an earlier good file. Nothing is extracted, opened or run: the result is a file the owner can inspect.

use crate::{Offer, Refusal};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Where the verified file ends up, named from the signed version and platform (never from the download address).
pub fn final_name(offer: &Offer, platform: &str) -> String {
    format!("kakurega-{}-{platform}.update", offer.version)
}

struct Sink {
    file: std::fs::File,
    hasher: Sha256,
    written: u64,
    limit: u64,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written + buf.len() as u64 > self.limit {
            return Err(std::io::Error::other(
                "the download is larger than the signed size",
            ));
        }
        let n = self.file.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Removes the temporary file on every path out unless the download was completed.
struct Temp {
    path: PathBuf,
    keep: bool,
}

impl Drop for Temp {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Runs `source` (which writes the body into the sink it is given) and returns the path of the verified file in `dir`.
pub fn download_with(
    offer: &Offer,
    platform: &str,
    dir: &Path,
    source: impl FnOnce(&mut dyn Write) -> Result<(), String>,
) -> Result<PathBuf, Refusal> {
    let ok_name = !platform.is_empty()
        && platform.len() <= 40
        && platform
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !ok_name {
        return Err(Refusal::BadArtifact("the platform name has odd characters"));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|_| Refusal::BadArtifact("cannot make the download folder"))?;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_path = dir.join(format!(".download-{}-{n}.part", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp_path)
        .map_err(|_| Refusal::BadArtifact("cannot create the download file"))?;
    let mut temp = Temp {
        path: tmp_path.clone(),
        keep: false,
    };
    let mut sink = Sink {
        file,
        hasher: Sha256::new(),
        written: 0,
        limit: offer.artifact.size,
    };
    source(&mut sink)
        .map_err(|_| Refusal::BadArtifact("the download failed or ran past the signed size"))?;
    if sink.written != offer.artifact.size {
        return Err(Refusal::BadArtifact(
            "the download ended before the signed size",
        ));
    }
    let digest: String = sink
        .hasher
        .clone()
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != offer.artifact.sha256 {
        return Err(Refusal::BadArtifact(
            "the downloaded file does not match the signed checksum",
        ));
    }
    sink.file
        .sync_all()
        .map_err(|_| Refusal::BadArtifact("cannot save the download"))?;
    let dest = dir.join(final_name(offer, platform));
    std::fs::rename(&tmp_path, &dest)
        .map_err(|_| Refusal::BadArtifact("cannot save the download"))?;
    temp.keep = true;
    Ok(dest)
}

/// Downloads the file for `offer` through the guarded fetcher into `dir`.
pub fn download(
    offer: &Offer,
    platform: &str,
    dir: &Path,
    fetcher: &kk_fetch::Fetcher,
) -> Result<PathBuf, Refusal> {
    download_with(offer, platform, dir, |out| {
        fetcher
            .fetch_to(&offer.artifact.url, offer.artifact.size, out)
            .map(|_| ())
            .map_err(|e| format!("{e:?}"))
    })
}
