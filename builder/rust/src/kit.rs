//! Build paths and switches: the vendored a64-kit, the work folder and the
//! user's dump. Ported from nx2/kit.py.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("../.."))
}

pub fn kit() -> PathBuf {
    root().join("third_party/a64-kit")
}

pub fn work() -> PathBuf {
    std::env::var_os("OOT_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("work"))
}

/// An on/off switch, the game's rule: unset, empty, 0, false, off or no
/// (any case) is off; anything else is on.
pub fn env_on(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(
            value.trim().to_lowercase().as_str(),
            "" | "0" | "false" | "off" | "no"
        ),
        Err(_) => false,
    }
}

fn dump_root() -> Option<PathBuf> {
    if let Some(dump) = std::env::var_os("OOT_DUMP") {
        return Some(PathBuf::from(dump));
    }
    let config = work().join("dump.txt");
    let text = fs::read_to_string(config).ok()?;
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(PathBuf::from)
}

pub fn exefs() -> Option<PathBuf> {
    if let Some(exefs) = std::env::var_os("OOT_EXEFS") {
        return Some(PathBuf::from(exefs));
    }
    dump_root().map(|dump| dump.join("exefs"))
}

pub fn romfs() -> Option<PathBuf> {
    if let Some(romfs) = std::env::var_os("OOT_ROMFS") {
        return Some(PathBuf::from(romfs));
    }
    dump_root().map(|dump| dump.join("romfs"))
}

pub fn need_exefs() -> Result<PathBuf, String> {
    let exefs = exefs().ok_or_else(|| {
        format!(
            "no exefs: set OOT_DUMP (the folder with exefs/ and romfs/) or OOT_EXEFS, or write the dump folder into {}",
            work().join("dump.txt").display()
        )
    })?;
    if !exefs.is_dir() {
        return Err(format!(
            "the exefs folder {} does not exist",
            exefs.display()
        ));
    }
    Ok(exefs)
}

/// The exefs's code modules in rtld's load order (rtld, main, subsdk0..9,
/// sdk) that exist, without rtld.
pub fn game_modules(exefs: &Path) -> Vec<String> {
    let mut order = vec!["main".to_owned()];
    order.extend((0..10).map(|i| format!("subsdk{i}")));
    order.push("sdk".to_owned());
    order
        .into_iter()
        .filter(|name| exefs.join(name).is_file())
        .collect()
}

pub fn json() -> Result<Value, String> {
    let exefs = exefs();
    Ok(json!({
        "root": root().to_string_lossy(),
        "kit": kit().to_string_lossy(),
        "work": work().to_string_lossy(),
        "dump": dump_root().map(|path| path.to_string_lossy().into_owned()),
        "exefs": exefs.as_ref().map(|path| path.to_string_lossy().into_owned()),
        "romfs": romfs().map(|path| path.to_string_lossy().into_owned()),
        "modules": exefs.as_deref().map(game_modules).unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_follow_the_game_rule() {
        let name = "HOOKSHOT_RUST_TEST_SWITCH";
        unsafe { std::env::remove_var(name) };
        assert!(!env_on(name));
        for off in ["", "0", "false", "off", "NO"] {
            unsafe { std::env::set_var(name, off) };
            assert!(!env_on(name), "{off:?} should be off");
        }
        for on in ["1", "true", "yes", "2"] {
            unsafe { std::env::set_var(name, on) };
            assert!(env_on(name), "{on:?} should be on");
        }
        unsafe { std::env::remove_var(name) };
    }

    #[test]
    fn lists_the_modules_in_load_order() {
        let folder = std::env::temp_dir().join(format!("hookshot-rust-kit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        for name in ["main", "sdk", "rtld", "subsdk2"] {
            fs::write(folder.join(name), b"NSO0").unwrap();
        }
        assert_eq!(game_modules(&folder), vec!["main", "subsdk2", "sdk"]);
        let _ = fs::remove_dir_all(folder);
    }
}
