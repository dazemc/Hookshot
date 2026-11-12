//! The button-prompt assets of other controllers (`nx2/prompt_assets.py`):
//! the game's icon font redrawn per controller face, and the layouts and
//! textures the game's button icons live in. Nothing is called out to Python.

use std::path::Path;

pub mod bfotf;
pub mod cff;
pub mod cmap;
pub mod cache;
pub mod faces;
pub mod font;
pub mod layout;
pub mod restbl;
pub mod zs;

pub const FONT: &str = "Font2/Scalable/External.Nin_Ounce_NVN2.bfotf.zs";

/// What a build wrote, for the build log and the report.
#[derive(Debug, Default)]
pub struct Written {
    pub font_bytes: usize,
    pub size_table: bool,
    pub faces: Vec<String>,
    pub hint: bool,
    pub ring: bool,
    /// The previous complete set was unchanged and kept (nothing drawn).
    pub kept: bool,
    /// Why the prompts were kept or drawn again, for the build log.
    pub reason: String,
}

/// Write `<out>/External.bfotf` and, when the larger font no longer fits its
/// entry, `<out>/rsizetable.zs`. The caller keeps a failure non-fatal: the
/// game then shows its own button icons.
///
/// Complete, unchanged output is kept: generation runs in a fresh sibling
/// folder and only complete output can replace the previous set.
/// `HOOKSHOT_FORCE_PROMPTS` draws it again. A failed generation removes the
/// prompts, as before.
pub fn build(romfs: &Path, out: &Path) -> Result<Written, String> {
    let kenney = crate::kit::root().join("third_party/kenney_input");
    let inputs = match cache::inputs(romfs, &kenney) {
        Ok(inputs) => inputs,
        Err(error) => {
            remove(out);
            return Err(error);
        }
    };
    let mut reason = if crate::kit::env_on("HOOKSHOT_FORCE_PROMPTS") {
        Some("forced".to_owned())
    } else {
        cache::unchanged(out, &inputs)
    };
    if reason.is_none() {
        match cache::clear_runtime(out) {
            Ok(()) => {
                if let Some(mut written) = cache::written(out) {
                    written.reason = "unchanged, kept".to_owned();
                    return Ok(written);
                }
                reason = Some("stamp has no summary".to_owned());
            }
            Err(error) => reason = Some(format!("runtime files could not be cleared: {error}")),
        }
    }
    let mut reason = reason.unwrap_or_else(|| "packaged or direct build".to_owned());
    // (an interrupted retry must not leave even the previous stamp behind)
    let _ = std::fs::remove_file(out.join(cache::KEY));
    let parent = out
        .parent()
        .ok_or_else(|| format!("prompts: {} has no folder", out.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let temporary = parent.join(format!(".prompts-{}", std::process::id()));
    remove(&temporary);
    let new = temporary.join("new");
    std::fs::create_dir_all(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    let written = match generate(romfs, &new) {
        Ok(written) => written,
        Err(error) => {
            remove(&temporary);
            remove(out);
            return Err(error);
        }
    };
    let complete = cache::outputs(&new)
        .map(|outputs| {
            outputs.contains_key("External.bfotf")
                && written.faces.iter().all(|face| outputs.contains_key(face))
        })
        .unwrap_or(false);
    if !complete {
        remove(&temporary);
        remove(out);
        return Err("prompts: incomplete generated output".to_owned());
    }
    // (certify only output made from the inputs it was started with)
    match cache::inputs(romfs, &kenney) {
        Ok(again) if again == inputs => {
            if let Err(error) = cache::write(&new, &inputs, &written) {
                reason = format!("{reason}; not stamped: {error}");
            }
        }
        _ => {
            reason = format!("{reason}; inputs changed or unreadable during generation, not stamped");
        }
    }
    let previous = temporary.join("previous");
    if out.exists() {
        std::fs::rename(out, &previous).map_err(|e| {
            remove(&temporary);
            format!("{}: {e}", out.display())
        })?;
    }
    if let Err(error) = std::fs::rename(&new, out) {
        // (never throw away the only complete set: keep the temporary folder
        // if the previous one cannot be put back)
        if previous.exists() {
            if std::fs::rename(&previous, out).is_ok() {
                remove(&temporary);
            }
        } else {
            remove(&temporary);
        }
        return Err(format!("{}: {error}", out.display()));
    }
    remove(&temporary);
    let mut written = written;
    written.reason = reason;
    Ok(written)
}

fn remove(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

fn generate(romfs: &Path, out: &Path) -> Result<Written, String> {
    let dictionary = zs::zs_dictionary(romfs)?;
    let game_font = zs::read_zs(romfs, FONT, &dictionary)?;
    let (key, otf) = bfotf::unwrap(&game_font)?;
    let kenney = crate::kit::root().join("third_party/kenney_input");
    let (font, widths) = font::build(&otf, &kenney)?;
    let wrapped = bfotf::wrap(key, &font);
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let font_path = out.join("External.bfotf");
    std::fs::write(&font_path, &wrapped).map_err(|e| format!("{}: {e}", font_path.display()))?;
    let blarc = zs::read_zs(romfs, layout::LAYOUTS, &dictionary)?;
    let hint = layout::hint_panes(&blarc);
    let ring = layout::ring_entries(&blarc);
    for (face, advances) in &widths {
        let dir = out.join(face);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if let Some(panes) = &hint {
            layout::write_hint(&dir.join("hint.bin"), advances, panes)?;
        }
    }
    let blarc_path = out.join("Common.blarc");
    if hint.is_some() || ring.is_some() {
        std::fs::write(&blarc_path, &blarc)
            .map_err(|e| format!("{}: {e}", blarc_path.display()))?;
    } else if blarc_path.exists() {
        std::fs::remove_file(&blarc_path).map_err(|e| format!("{}: {e}", blarc_path.display()))?;
    }
    let ring_path = out.join("ring.bin");
    match &ring {
        Some(entries) => layout::write_ring(&ring_path, entries)?,
        None if ring_path.exists() => {
            std::fs::remove_file(&ring_path)
                .map_err(|e| format!("{}: {e}", ring_path.display()))?;
        }
        None => {}
    }
    let size_table = match restbl::size_table(romfs, &dictionary, game_font.len(), wrapped.len())? {
        Some(table) => {
            let path = out.join("rsizetable.zs");
            std::fs::write(&path, restbl::compress(&table)?)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            true
        }
        None => {
            let path = out.join("rsizetable.zs");
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            false
        }
    };
    Ok(Written {
        font_bytes: wrapped.len(),
        size_table,
        faces: widths.iter().map(|(face, _)| face.clone()).collect(),
        hint: hint.is_some(),
        ring: ring.is_some(),
        kept: false,
        reason: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A local check with the game's own files, for a development machine:
    /// nothing here runs in CI, where `HOOKSHOT_TEST_ROMFS` is not set.
    #[test]
    fn the_game_files_read_when_a_romfs_is_given() {
        let Some(romfs) = std::env::var_os("HOOKSHOT_TEST_ROMFS").map(PathBuf::from) else {
            return;
        };
        let dictionary = zs::zs_dictionary(&romfs).unwrap();
        assert!(dictionary.len() > 1000);
        let font = zs::read_zs(&romfs, FONT, &dictionary).unwrap();
        let (key, otf) = bfotf::unwrap(&font).unwrap();
        assert_eq!(&otf[..4], b"OTTO");
        assert_ne!(key, 0);
        let table = restbl::size_table(&romfs, &dictionary, font.len(), font.len() + 4096).unwrap();
        assert!(table.is_some());
        if let Some(out) = std::env::var_os("HOOKSHOT_TEST_PROMPTS_OUT") {
            let written = build(&romfs, Path::new(&out)).unwrap();
            assert!(written.size_table);
            assert_eq!(written.faces, ["switch", "xbox", "ps4", "ps5", "steamdeck"]);
            assert!(Path::new(&out).join("External.bfotf").is_file());
            assert!(Path::new(&out).join("rsizetable.zs").is_file());
        }
    }
}
