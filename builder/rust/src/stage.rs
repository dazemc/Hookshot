//! The build's read stage: check that a dump is the game and build Hookshot
//! supports, extracting its exefs first when the user chose a .nup.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::nso;
use crate::nup;

/// `--check`: the answer make_game.py gives the builder before a build.
pub fn check_json(nup: Option<&str>, exefs: Option<&str>, work: &str) -> Result<Value, String> {
    let (folder, probe) = match (nup, exefs) {
        (Some(nup), _) if !nup.is_empty() => {
            let probe = Path::new(work).join("probe");
            let _ = fs::remove_dir_all(&probe);
            nup::extract_json(nup, probe.to_str().unwrap_or_default(), Some("exefs"), None)?;
            (probe.join("exefs"), true)
        }
        (_, Some(exefs)) if !exefs.is_empty() => (Path::new(exefs).to_path_buf(), false),
        _ => return Err("needs a .nup file or an exefs folder".into()),
    };
    let answer = (|| {
        nso::check_modules(&folder)?;
        let build = nso::build_id(&folder.join("main"))?;
        let id = &build[..8.min(build.len())];
        let supported = build.starts_with(nso::SUPPORTED_BUILD);
        let message = if supported {
            format!("This is the version Hookshot supports (build {id}).")
        } else {
            format!(
                "This is build {id}. Hookshot supports build {}.",
                nso::SUPPORTED_BUILD
            )
        };
        Ok(json!({
            "ok": supported,
            "build": build,
            "build_id": id,
            "supported": supported,
            "message": message,
        }))
    })();
    if probe {
        let _ = fs::remove_dir_all(Path::new(work).join("probe"));
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exefs_with(build: [u8; 8], tag: &str) -> std::path::PathBuf {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-check-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let mut main = vec![0u8; 0x60];
        main[..4].copy_from_slice(b"NSO0");
        for (i, byte) in build.iter().enumerate() {
            main[0x40 + i] = *byte;
        }
        fs::write(folder.join("main"), &main).unwrap();
        fs::write(folder.join("rtld"), b"NSO0").unwrap();
        folder
    }

    #[test]
    fn accepts_the_supported_build() {
        let folder = exefs_with([0xe0, 0xf3, 0xcc, 0xfa, 0, 0, 0, 0], "good");
        let answer = check_json(None, Some(folder.to_str().unwrap()), "/tmp").unwrap();
        assert_eq!(answer["ok"], true);
        assert_eq!(answer["build_id"], "e0f3ccfa");
        assert!(answer["message"].as_str().unwrap().contains("supports"));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn names_another_build() {
        let folder = exefs_with([1, 2, 3, 4, 0, 0, 0, 0], "other");
        let answer = check_json(None, Some(folder.to_str().unwrap()), "/tmp").unwrap();
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["build_id"], "01020304");
        assert!(answer["message"].as_str().unwrap().contains("e0f3ccfa"));
        let _ = fs::remove_dir_all(folder);
    }
}
