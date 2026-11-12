//! The button-prompt assets of other controllers (`nx2/prompt_assets.py`):
//! the game's icon font redrawn per controller face, and the layouts and
//! textures the game's button icons live in. Nothing is called out to Python.

use std::path::Path;

pub mod bfotf;
pub mod cff;
pub mod cmap;
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
}

/// Write `<out>/External.bfotf` and, when the larger font no longer fits its
/// entry, `<out>/rsizetable.zs`. The caller keeps a failure non-fatal: the
/// game then shows its own button icons.
pub fn build(romfs: &Path, out: &Path) -> Result<Written, String> {
    let dictionary = zs::zs_dictionary(romfs)?;
    let game_font = zs::read_zs(romfs, FONT, &dictionary)?;
    let (key, otf) = bfotf::unwrap(&game_font)?;
    let kenney = crate::kit::root().join("third_party/kenney_input");
    let (font, widths) = font::build(&otf, &kenney)?;
    let wrapped = bfotf::wrap(key, &font);
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
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
