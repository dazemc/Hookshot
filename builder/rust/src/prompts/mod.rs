//! The button-prompt assets of other controllers (`nx2/prompt_assets.py`):
//! the game's icon font redrawn per controller face, and the layouts and
//! textures the game's button icons live in. Nothing is called out to Python.

pub mod bfotf;
pub mod cmap;
pub mod restbl;
pub mod zs;

pub const FONT: &str = "Font2/Scalable/External.Nin_Ounce_NVN2.bfotf.zs";

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
    }
}
