use std::path::Path;

/// The project a terminal started in `start` is about: the folder name of the nearest repository above it (a folder with
/// `.git` or `.hg`), as OpenCode binds a session to the repository it was started in. Outside any repository there is no
/// project, so a terminal opened in the home folder does not claim one. The name must be a valid project name.
pub fn detect(start: &Path) -> Option<String> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(".git").exists() || d.join(".hg").exists() {
            let name = d.file_name()?.to_string_lossy().into_owned();
            return valid_name(&name).then_some(name);
        }
        dir = d.parent();
    }
    None
}

/// The same rule the service applies (a test keeps the two in step): 1 to 80 plain characters, no surrounding spaces, no
/// path separators, no control or direction-changing characters.
pub fn valid_name(p: &str) -> bool {
    !(p.trim() != p
        || p.is_empty()
        || p.chars().count() > 80
        || p.chars().any(|c| {
            c.is_control()
                || matches!(c as u32, 0x200b..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069 | 0xfeff)
        })
        || p.contains('/')
        || p.contains('\\'))
}
