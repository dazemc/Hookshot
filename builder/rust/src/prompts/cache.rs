//! The button prompts' build cache (`nx2/prompt_assets.py`'s `--incremental`):
//! fingerprint what generation reads, and keep complete, unchanged output.
//! Reuse requires both matching inputs and every recorded output's size and
//! SHA-256.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The stamp in the prompts folder (not a generator output).
pub const KEY: &str = ".prompt_key";
/// `host/prompts.c` writes these at run time, without changing the build's
/// originals: every build clears them, even an unchanged hit, because their
/// runtime keys do not cover changes to `host/prompts.c`.
pub const RUNTIME: [&str; 6] = [
    "External.bfotf.zs",
    "Common.bntx.zs",
    "Common.key",
    "Common.blarc.zs",
    "Common.blarc.key",
    "rsizetable.mod.zs",
];

const SCHEME: u32 = 1;

/// The parts generation reads, each named for the log's reason. The source
/// modules are compiled in: a code change is a new key.
pub fn inputs(
    romfs: &Path,
    kenney: &Path,
    keyboard: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let mut parts = BTreeMap::new();
    let source = [
        ("mod.rs", include_str!("mod.rs")),
        ("cache.rs", include_str!("cache.rs")),
        ("bfotf.rs", include_str!("bfotf.rs")),
        ("cff.rs", include_str!("cff.rs")),
        ("cmap.rs", include_str!("cmap.rs")),
        ("faces.rs", include_str!("faces.rs")),
        ("font.rs", include_str!("font.rs")),
        ("keys.rs", include_str!("keys.rs")),
        ("guide.rs", include_str!("guide.rs")),
        ("layout.rs", include_str!("layout.rs")),
        ("restbl.rs", include_str!("restbl.rs")),
        ("bntx.rs", include_str!("bntx.rs")),
        ("raster.rs", include_str!("raster.rs")),
        ("overrides.rs", include_str!("overrides.rs")),
        ("textures.rs", include_str!("textures.rs")),
        ("zs.rs", include_str!("zs.rs")),
    ];
    parts.insert("source".to_owned(), hash_parts(source.iter().copied()));
    parts.insert(
        "platform".to_owned(),
        hash_parts([
            ("crate", env!("CARGO_PKG_VERSION")),
            ("os", std::env::consts::OS),
            ("arch", std::env::consts::ARCH),
            ("zstd", zstd::zstd_safe::version_string()),
        ]),
    );
    for (name, relative) in [
        ("dictionary", "Pack/ZsDic.pack.zs"),
        ("font", super::FONT),
        ("layout", super::layout::LAYOUTS),
        ("textures", super::bntx::TEXTURES),
    ] {
        parts.insert(name.to_owned(), hash_file(&romfs.join(relative))?);
    }
    parts.insert("restbl".to_owned(), hash_restbl(romfs)?);
    parts.insert("kenney".to_owned(), hash_tree(kenney)?);
    parts.insert("kenney_keyboard".to_owned(), hash_tree(keyboard)?);
    // (a hand-made prompt image changes what the build draws: its addition,
    // edit or removal must not leave the previous output stamped unchanged)
    parts.insert(
        "prompt_faces".to_owned(),
        hash_tree(&crate::kit::root().join(super::overrides::ROOT))?,
    );
    Ok(parts)
}

/// `files` as one SHA-256, each name and content length-prefixed.
fn hash_parts<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut digest = Sha256::new();
    for (name, text) in files {
        absorb(&mut digest, name, text.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn absorb(digest: &mut Sha256, name: &str, data: &[u8]) {
    digest.update((name.len() as u64).to_le_bytes());
    digest.update(name.as_bytes());
    digest.update((data.len() as u64).to_le_bytes());
    digest.update(data);
}

/// Size and content, not just mtime: an input restored with its old time can
/// still have changed.
fn hash_file(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut digest = Sha256::new();
    digest.update(&data);
    Ok(format!("{:x}", digest.finalize()))
}

/// The matching resource size tables, names and contents: generation reads the
/// first, and a new name changes which one that is.
fn hash_restbl(romfs: &Path) -> Result<String, String> {
    let folder = romfs.join("System").join("Resource");
    let mut names: Vec<String> = fs::read_dir(&folder)
        .map_err(|e| format!("prompts: no {}: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ResourceSizeTable.") && name.ends_with(".rsizetable.zs"))
        .collect();
    names.sort();
    let mut digest = Sha256::new();
    for name in names {
        let data = fs::read(folder.join(&name)).map_err(|e| format!("{name}: {e}"))?;
        absorb(&mut digest, &name, &data);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Files and directories under `folder`: an added, changed or removed input
/// changes the key.
fn hash_tree(folder: &Path) -> Result<String, String> {
    let mut digest = Sha256::new();
    if !folder.exists() {
        return Ok(format!("{:x}", digest.finalize()));
    }
    let mut entries: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| path != folder)
        .collect();
    entries.sort();
    for path in entries {
        let relative = path
            .strip_prefix(folder)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            absorb(&mut digest, &relative, b"");
        } else if path.is_file() {
            let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            absorb(&mut digest, &relative, &data);
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// The generated files and directories, except the runtime's own copies and
/// the stamp. A directory is recorded too: the keyboard's is a runtime marker
/// even when empty.
pub fn outputs(out: &Path) -> Result<BTreeMap<String, Option<(u64, String)>>, String> {
    let mut found = BTreeMap::new();
    if !out.is_dir() {
        return Ok(found);
    }
    for entry in walkdir::WalkDir::new(out) {
        let entry = entry.map_err(|e| e.to_string())?;
        let relative = entry
            .path()
            .strip_prefix(out)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.is_empty() || RUNTIME.contains(&relative.as_str()) || relative == KEY {
            continue;
        }
        if entry.file_type().is_dir() {
            found.insert(relative, None);
        } else if entry.file_type().is_file() {
            let data =
                fs::read(entry.path()).map_err(|e| format!("{}: {e}", entry.path().display()))?;
            found.insert(
                relative,
                Some((data.len() as u64, format!("{:x}", Sha256::digest(&data)))),
            );
        }
    }
    Ok(found)
}

fn outputs_json(outputs: &BTreeMap<String, Option<(u64, String)>>) -> Value {
    let map: serde_json::Map<String, Value> = outputs
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                match value {
                    Some((size, digest)) => json!([size, digest]),
                    None => Value::Null,
                },
            )
        })
        .collect();
    Value::Object(map)
}

/// `None` means the complete outputs still match; otherwise the reason they
/// need making again.
pub fn unchanged(out: &Path, inputs: &BTreeMap<String, String>) -> Option<String> {
    let text = match fs::read_to_string(out.join(KEY)) {
        Ok(text) => text,
        Err(_) => return Some("invalid stamp".to_owned()),
    };
    let note: Value = match serde_json::from_str(&text) {
        Ok(note) => note,
        Err(_) => return Some("invalid stamp".to_owned()),
    };
    if note.get("scheme").and_then(Value::as_u64) != Some(u64::from(SCHEME)) {
        return Some("invalid stamp".to_owned());
    }
    let Some(recorded) = note.get("inputs").and_then(Value::as_object) else {
        return Some("invalid stamp".to_owned());
    };
    for (kind, value) in inputs {
        if recorded.get(kind).and_then(Value::as_str) != Some(value.as_str()) {
            return Some(format!("{kind} changed"));
        }
    }
    if recorded.len() != inputs.len() {
        return Some("inputs changed".to_owned());
    }
    let found = match outputs(out) {
        Ok(found) => found,
        Err(error) => return Some(format!("outputs unreadable: {error}")),
    };
    let Some(wanted) = note.get("outputs") else {
        return Some("invalid stamp".to_owned());
    };
    if !found.contains_key("External.bfotf") || outputs_json(&found) != *wanted {
        return Some("output missing or changed".to_owned());
    }
    None
}

/// The successful outputs, with what chose them, for the next build. The
/// caller passes the digests it just read: hashing the 244 MB `Common.bntx` a
/// second time only for the stamp costs as much as drawing several textures.
pub fn write_outputs(
    out: &Path,
    inputs: &BTreeMap<String, String>,
    written: &super::Written,
    outputs: &BTreeMap<String, Option<(u64, String)>>,
) -> Result<(), String> {
    if !outputs.contains_key("External.bfotf") {
        return Err("prompts: incomplete generated output".to_owned());
    }
    let note = json!({
        "scheme": SCHEME,
        "inputs": inputs,
        "outputs": outputs_json(outputs),
        "written": {
            "font_bytes": written.font_bytes,
            "size_table": written.size_table,
            "faces": written.faces,
            "hint": written.hint,
            "ring": written.ring,
            "textures": written.textures,
        },
    });
    fs::write(
        out.join(KEY),
        serde_json::to_vec(&note).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", out.join(KEY).display()))
}

/// What a kept folder says it wrote.
pub fn written(out: &Path) -> Option<super::Written> {
    let text = fs::read_to_string(out.join(KEY)).ok()?;
    let note: Value = serde_json::from_str(&text).ok()?;
    let written = note.get("written")?;
    Some(super::Written {
        font_bytes: written.get("font_bytes")?.as_u64()? as usize,
        size_table: written.get("size_table")?.as_bool()?,
        faces: written
            .get("faces")?
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        hint: written.get("hint")?.as_bool()?,
        ring: written.get("ring")?.as_bool()?,
        textures: written.get("textures")?.as_u64()? as usize,
        kept: true,
        reason: String::new(),
    })
}

/// Remove the files `host/prompts.c` derives; a failure means the build makes
/// the prompts again rather than trusting them.
pub fn clear_runtime(out: &Path) -> Result<(), String> {
    for name in RUNTIME {
        let path = out.join(name);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hookshot-rust-prompts-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn generated(out: &Path) {
        fs::create_dir_all(out.join("switch")).unwrap();
        fs::write(out.join("External.bfotf"), b"the font").unwrap();
        fs::write(out.join("switch/hint.bin"), b"the hint").unwrap();
        fs::write(out.join("ring.bin"), b"the ring").unwrap();
        // (the game's own run-time copies, not generation's)
        for name in RUNTIME {
            fs::write(out.join(name), b"derived at run time").unwrap();
        }
    }

    fn summary() -> super::super::Written {
        super::super::Written {
            font_bytes: 8,
            size_table: true,
            faces: vec!["switch".to_owned()],
            hint: true,
            ring: true,
            textures: 0,
            kept: false,
            reason: "a table changed".to_owned(),
        }
    }

    #[test]
    fn outputs_leave_the_runtime_files_and_the_stamp_out() {
        let base = temp("outputs");
        let out = base.join("prompts");
        generated(&out);
        fs::write(out.join(KEY), b"{}").unwrap();
        let outputs = outputs(&out).unwrap();
        assert!(outputs.contains_key("External.bfotf"));
        assert!(outputs.contains_key("switch"));
        assert!(outputs.contains_key("switch/hint.bin"));
        assert_eq!(outputs.get("ring.bin").unwrap().as_ref().unwrap().0, 8);
        for name in RUNTIME {
            assert!(!outputs.contains_key(name), "{name} is not an output");
        }
        assert!(!outputs.contains_key(KEY));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn unchanged_requires_every_input_and_output_to_match() {
        let base = temp("unchanged");
        let out = base.join("prompts");
        generated(&out);
        let mut inputs = BTreeMap::new();
        inputs.insert("font".to_owned(), "a".to_owned());
        assert!(unchanged(&out, &inputs).is_some(), "no stamp yet");
        let outputs = outputs(&out).unwrap();
        write_outputs(&out, &inputs, &summary(), &outputs).unwrap();
        assert_eq!(unchanged(&out, &inputs), None);
        let kept = written(&out).unwrap();
        assert!(kept.kept);
        assert_eq!(kept.faces, ["switch"]);
        // (the runtime's copies are not outputs, so they do not invalidate it)
        fs::write(out.join("Common.key"), b"changed at run time").unwrap();
        assert_eq!(unchanged(&out, &inputs), None);
        // (a changed input says which)
        let mut changed = inputs.clone();
        changed.insert("kenney".to_owned(), "b".to_owned());
        assert_eq!(unchanged(&out, &changed).as_deref(), Some("kenney changed"));
        // (a missing, truncated or same-size-corrupted output makes it again)
        fs::write(out.join("External.bfotf"), b"the fant").unwrap();
        assert_eq!(
            unchanged(&out, &inputs).as_deref(),
            Some("output missing or changed")
        );
        fs::write(out.join("External.bfotf"), b"the font").unwrap();
        assert_eq!(unchanged(&out, &inputs), None);
        fs::remove_file(out.join("switch/hint.bin")).unwrap();
        assert_eq!(
            unchanged(&out, &inputs).as_deref(),
            Some("output missing or changed")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn clearing_the_runtime_files_keeps_the_generated_ones() {
        let base = temp("clear");
        let out = base.join("prompts");
        generated(&out);
        clear_runtime(&out).unwrap();
        for name in RUNTIME {
            assert!(!out.join(name).exists(), "{name} was cleared");
        }
        for name in ["External.bfotf", "switch/hint.bin", "ring.bin"] {
            assert!(out.join(name).is_file(), "{name} was kept");
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn an_input_tree_is_its_names_and_contents() {
        let base = temp("tree");
        let tree = base.join("kenney");
        fs::create_dir_all(tree.join("empty")).unwrap();
        fs::write(tree.join("a.otf"), b"font a").unwrap();
        let first = hash_tree(&tree).unwrap();
        assert_eq!(first, hash_tree(&tree).unwrap());
        fs::write(tree.join("a.otf"), b"font b").unwrap();
        assert_ne!(first, hash_tree(&tree).unwrap());
        fs::write(tree.join("a.otf"), b"font a").unwrap();
        fs::write(tree.join("added.otf"), b"another").unwrap();
        assert_ne!(first, hash_tree(&tree).unwrap());
        fs::remove_file(tree.join("added.otf")).unwrap();
        fs::remove_dir(tree.join("empty")).unwrap();
        assert_ne!(first, hash_tree(&tree).unwrap());
        assert_eq!(
            hash_tree(&base.join("missing")).unwrap(),
            hash_tree(&base.join("missing2")).unwrap()
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_create_outside_the_build_removes_the_prompts() {
        let base = temp("failed");
        let romfs = base.join("romfs");
        let out = base.join("out/prompts");
        generated(&out);
        let error = super::super::build(&romfs, &out).unwrap_err();
        assert!(!error.is_empty());
        assert!(!out.exists(), "a failed generation leaves no prompts");
        let _ = fs::remove_dir_all(base);
    }
}
