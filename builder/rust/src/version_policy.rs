//! Per-installation version pin and save/settings backup, from the Flutter app.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde_json::{Value, json};

const POLICY: &str = "hookshot_update_policy.txt";

fn policy_path(folder: &str) -> PathBuf {
    Path::new(folder).join("user").join(POLICY)
}

fn safe_tag(tag: &str) -> bool {
    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    tag.len() <= 128
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

pub fn pinned(folder: &str) -> Option<String> {
    let text = fs::read_to_string(policy_path(folder)).ok()?;
    let mut mode = None;
    let mut version = None;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "mode" => mode = Some(value.trim().to_owned()),
                "version" => version = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }
    version.filter(|v| mode.as_deref() == Some("pinned") && safe_tag(v))
}

pub fn pinned_json(folder: &str) -> Result<Value, String> {
    Ok(json!({"version": pinned(folder)}))
}

pub fn set(folder: &str, tag: Option<&str>) -> Result<(), String> {
    let path = policy_path(folder);
    let Some(tag) = tag else {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    };
    if !safe_tag(tag) {
        return Err("Invalid version tag.".into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("txt.tmp");
    fs::write(&temp, format!("mode=pinned\nversion={tag}\n")).map_err(|e| e.to_string())?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    fs::rename(temp, path).map_err(|e| e.to_string())
}

pub fn set_json(folder: &str, tag: Option<&str>) -> Result<Value, String> {
    set(folder, tag)?;
    Ok(json!({"version": tag}))
}

/// Copy saves and settings before a version switch. Returns null when there
/// is nothing to preserve. Symlinks are refused rather than followed outside
/// the game folder; a failed copy prevents the switch.
pub fn backup(folder: &str, root: Option<&str>) -> Result<Value, String> {
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    let user = Path::new(folder).join("user");
    let local_save = user.join("save");
    if local_save.is_dir() {
        sources.push((local_save.clone(), "game/user/save".into()));
    }
    let local_profiles = user.join("profiles.txt");
    if local_profiles.is_file() {
        sources.push((local_profiles, "game/user/profiles.txt".into()));
    }
    let local_policy = user.join(POLICY);
    if local_policy.is_file() {
        sources.push((local_policy, format!("game/user/{POLICY}")));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|p| p.join("hookshot"));
    if let Some(shared) = base {
        let save = shared.join("save");
        if save.is_dir() && save != local_save {
            sources.push((save, "shared-user/save".into()));
            let profiles = shared.join("profiles.txt");
            if profiles.is_file() {
                sources.push((profiles, "shared-user/profiles.txt".into()));
            }
        }
    }
    for name in ["hookshot_options.ini", "hookshot_env.txt"] {
        let path = Path::new(folder).join(name);
        if path.is_file() {
            sources.push((path, format!("game/{name}")));
        }
    }
    if sources.is_empty() {
        return Ok(Value::Null);
    }
    let root = root
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(default_builder_home())));
    let root = root.unwrap();
    let backups = root.join("version-backups");
    fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S-%3f");
    let name = Path::new(folder)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let mut dest = backups.join(format!("{name}-{stamp}"));
    let mut k = 2;
    while dest.exists() {
        dest = backups.join(format!("{name}-{stamp}-{k}"));
        k += 1;
    }
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    for (source, relative) in sources {
        copy_tree(&source, &dest.join(relative))?;
    }
    Ok(json!(dest.to_string_lossy()))
}

fn default_builder_home() -> String {
    if cfg!(windows) {
        return format!(
            "{}/HookshotBuilder",
            std::env::var("LOCALAPPDATA").unwrap_or_default()
        );
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_default();
    base.join("hookshot/builder").to_string_lossy().into_owned()
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Cannot back up a linked save/settings path safely: {}",
            source.display()
        ));
    }
    if meta.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(source, target).map_err(|e| e.to_string())?;
    } else if meta.is_dir() {
        fs::create_dir_all(target).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())?.flatten() {
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    }
    Ok(())
}

pub fn backup_json(folder: &str, root: Option<&str>) -> Result<Value, String> {
    Ok(json!({"path": backup(folder, root)? }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_round_trips_and_rejects_unsafe_tags() {
        let folder = std::env::temp_dir().join(format!("hookshot-rust-pin-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let folder = folder.to_string_lossy().to_string();
        assert_eq!(pinned(&folder), None);
        set(&folder, Some("v2.2.0")).unwrap();
        assert_eq!(pinned(&folder).as_deref(), Some("v2.2.0"));
        assert!(set(&folder, Some("../outside")).is_err());
        set(&folder, None).unwrap();
        assert_eq!(pinned(&folder), None);
    }
}
