//! Which Hookshot a build is: the release it came from and a fingerprint of
//! its sources, so a report says what was run. Ported from nx2/version.py.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sha1::{Digest, Sha1};

const SOURCES: [&str; 3] = ["host", "gpu", "nx2"];
const SOURCE_EXT: [&str; 13] = [
    ".c", ".cc", ".cpp", ".h", ".hpp", ".inc", ".py", ".glsl", ".comp", ".vert", ".frag", ".spv",
    ".rs",
];
const DEFAULT_REPOSITORY: &str = "NotRexed/Hookshot";

pub fn root() -> PathBuf {
    crate::kit::root()
}

pub fn release(root: &Path) -> String {
    let mut name = std::env::var("HOOKSHOT_VERSION")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if name.is_empty() {
        for file in ["VERSION", "version.txt"] {
            if let Ok(text) = fs::read_to_string(root.join(file)) {
                name = text.lines().next().unwrap_or("").trim().to_owned();
            }
            if !name.is_empty() {
                break;
            }
        }
    }
    if name.is_empty() && root.join(".git").exists() {
        if let Ok(output) = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "describe", "--tags", "--always", "--dirty", "--match", "v[0-9]*",
            ])
            .output()
        {
            if output.status.success() {
                name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            }
        }
    }
    if name.is_empty() {
        "unknown".to_owned()
    } else {
        name
    }
}

/// SHA-1 of host/, gpu/ and nx2/ sources, line endings aside, so every
/// build of the same release says the same thing.
pub fn fingerprint(root: &Path) -> String {
    let mut digest = Sha1::new();
    for top in SOURCES {
        hash_sources(&root.join(top), root, &mut digest);
    }
    let mut hex = format!("{:x}", digest.finalize());
    hex.truncate(8);
    hex
}

fn hash_sources(folder: &Path, root: &Path, digest: &mut Sha1) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            if name != "__pycache__" {
                dirs.push(entry.path());
            }
        } else {
            files.push(entry.path());
        }
    }
    files.sort();
    dirs.sort();
    for path in files {
        if !SOURCE_EXT
            .iter()
            .any(|ext| path.to_string_lossy().ends_with(ext))
        {
            continue;
        }
        let Ok(data) = fs::read(&path) else { continue };
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        digest.update(relative.as_bytes());
        digest.update([0]);
        let mut at = 0;
        while at < data.len() {
            if data[at] == b'\r' && data.get(at + 1) == Some(&b'\n') {
                at += 1;
                continue;
            }
            digest.update([data[at]]);
            at += 1;
        }
        digest.update([0]);
    }
    for dir in dirs {
        hash_sources(&dir, root, digest);
    }
}

pub fn repository(root: &Path) -> String {
    let mut name = std::env::var("HOOKSHOT_REPOSITORY")
        .or_else(|_| std::env::var("GITHUB_REPOSITORY"))
        .unwrap_or_default()
        .trim()
        .to_owned();
    if name.is_empty() {
        if let Ok(text) = fs::read_to_string(root.join("repository.txt")) {
            name = text.lines().next().unwrap_or("").trim().to_owned();
        }
    }
    if valid_repository(&name) {
        name
    } else {
        DEFAULT_REPOSITORY.to_owned()
    }
}

fn valid_repository(name: &str) -> bool {
    let mut parts = name.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !owner.is_empty()
        && !repo.is_empty()
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn describe(root: &Path) -> String {
    let text = format!("{} (source {})", release(root), fingerprint(root));
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " ._+()/-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

pub fn define(root: &Path) -> String {
    format!("-DHOOKSHOT_VERSION=\"{}\"", describe(root))
}

pub fn describe_json() -> Result<Value, String> {
    let root = root();
    Ok(json!({
        "release": release(&root),
        "fingerprint": fingerprint(&root),
        "description": describe(&root),
        "repository": repository(&root),
        "define": define(&root),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_sources_without_line_ending_noise() {
        let root =
            std::env::temp_dir().join(format!("hookshot-rust-version-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("host")).unwrap();
        fs::create_dir_all(root.join("nx2/__pycache__")).unwrap();
        fs::write(root.join("host/a.c"), b"int a;\r\n").unwrap();
        fs::write(root.join("nx2/c.py"), b"x = 1\n").unwrap();
        fs::write(root.join("nx2/__pycache__/c.pyc"), b"ignored").unwrap();
        let first = fingerprint(&root);
        fs::write(root.join("host/a.c"), b"int a;\n").unwrap();
        assert_eq!(first, fingerprint(&root));
        fs::write(root.join("host/a.c"), b"int b;\n").unwrap();
        assert_ne!(first, fingerprint(&root));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_names_are_checked() {
        assert!(valid_repository("NotRexed/Hookshot"));
        assert!(valid_repository("dazemc/Hookshot"));
        assert!(!valid_repository("no-slash"));
        assert!(!valid_repository("a/b/c"));
        assert!(!valid_repository("a b/c"));
        assert_eq!(define(Path::new(".")).matches('"').count(), 2);
    }
}
