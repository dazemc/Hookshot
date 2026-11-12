//! Hand-made prompt images (`nx2/prompt_faces/<face>/`): a PNG of a layer's
//! name takes that layer's place, as `nx2/prompt_faces.py`'s `override` does
//! for the Python build.
//!
//! The build writes every image it makes as a layer first (`--dump` in
//! `nx2/prompt_assets.py` names them). An image put here is read instead: the
//! note textures as RGBA, the letters and the leader lines as greyscale. Its
//! size must be the texture's, or the build stops with its name, as the
//! Python build does. A hand-made controller picture is the source of the
//! page's tinted copy and shadow, unless those are given too.

use std::fs::File;
use std::path::{Path, PathBuf};

use super::raster::Mask;

/// Where the hand-made images live, under the checkout (`nx2/prompt_faces`).
pub const ROOT: &str = "nx2/prompt_faces";

/// The hand-made images of a build: a folder holding `<face>/<layer>.png`.
#[derive(Clone, Debug)]
pub struct Overrides {
    root: PathBuf,
}

impl Default for Overrides {
    fn default() -> Self {
        Self::new(crate::kit::root().join(ROOT))
    }
}

impl Overrides {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// A build with no hand-made images (tests, and a packaged build without
    /// the checkout's `nx2/prompt_faces`).
    pub fn none() -> Self {
        Self::new(PathBuf::new())
    }

    #[allow(dead_code)] // (the picture layers and the tests read it)
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The path of a hand-made image taking the place of `name` (the layer's
    /// name as `--dump` writes it), or None.
    pub fn path(&self, face: &str, name: &str) -> Option<PathBuf> {
        if self.root.as_os_str().is_empty() {
            return None;
        }
        let path = self.root.join(face).join(name);
        path.is_file().then_some(path)
    }

    /// The hand-made RGBA image of `name`, when there is one: its size must be
    /// `width` x `height`, the texture's.
    pub fn rgba(
        &self,
        face: &str,
        name: &str,
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<u8>>, String> {
        let Some(path) = self.path(face, name) else {
            return Ok(None);
        };
        let (w, h, data) = read_rgba(&path)?;
        check_size(&path, w, h, width, height)?;
        Ok(Some(data))
    }

    /// The hand-made greyscale image of `name` as an L mask, when there is
    /// one: its size must be `width` x `height`, the texture's.
    pub fn mask(
        &self,
        face: &str,
        name: &str,
        width: u32,
        height: u32,
    ) -> Result<Option<Mask>, String> {
        let Some(path) = self.path(face, name) else {
            return Ok(None);
        };
        let (w, h, data) = read_rgba(&path)?;
        check_size(&path, w, h, width, height)?;
        let mut mask = Mask::new(w, h);
        for (at, pixel) in data.chunks_exact(4).enumerate() {
            // (Pillow's convert("L"): the rounded ITU-R 601-2 luma, alpha
            // dropped)
            let luma = (u32::from(pixel[0]) * 299
                + u32::from(pixel[1]) * 587
                + u32::from(pixel[2]) * 114
                + 500)
                / 1000;
            mask.data[at] = luma as u8;
        }
        Ok(Some(mask))
    }
}

fn check_size(path: &Path, w: u32, h: u32, width: u32, height: u32) -> Result<(), String> {
    if (w, h) != (width, height) {
        return Err(format!(
            "prompts: {} is {w}x{h}, the texture {width}x{height}",
            path.display()
        ));
    }
    Ok(())
}

/// A PNG as RGBA8, 8 bits a channel (`Image.open(path).convert("RGBA")`).
fn read_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let data = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Grayscale => data
            .iter()
            .flat_map(|value| [*value, *value, *value, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::Rgba => data.to_vec(),
        other => {
            return Err(format!(
                "prompts: {} is {other:?}, not a greyscale or colour PNG",
                path.display()
            ));
        }
    };
    Ok((info.width, info.height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hookshot-overrides-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(path: &Path, width: u32, height: u32, color: png::ColorType, data: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(path).unwrap();
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
    }

    #[test]
    fn a_hand_made_image_takes_a_layers_place_when_its_size_matches() {
        let dir = scratch("place");
        let face = dir.join("xbox");
        write_png(
            &face.join("KeyTexFontA_00.south.png"),
            2,
            2,
            png::ColorType::Grayscale,
            &[0, 64, 128, 255],
        );
        let overrides = Overrides::new(&dir);
        assert_eq!(
            overrides.path("xbox", "KeyTexFontA_00.south.png"),
            Some(face.join("KeyTexFontA_00.south.png"))
        );
        assert!(overrides.path("xbox", "KeyTexFontA_00.east.png").is_none());
        assert!(overrides.path("ps4", "KeyTexFontA_00.south.png").is_none());

        let mask = overrides
            .mask("xbox", "KeyTexFontA_00.south.png", 2, 2)
            .unwrap()
            .unwrap();
        assert_eq!(mask.data, vec![0, 64, 128, 255]);

        // (a wrong size stops the build with the image's name)
        let error = overrides
            .mask("xbox", "KeyTexFontA_00.south.png", 4, 4)
            .unwrap_err();
        assert!(error.contains("KeyTexFontA_00.south.png"), "{error}");
        assert!(error.contains("is 2x2, the texture 4x4"), "{error}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_colour_image_is_read_as_rgba_and_as_pillows_luma() {
        let dir = scratch("luma");
        let face = dir.join("switch");
        // (red, green, blue, white: Pillow's L values, the alpha dropped)
        let pixels = [
            255, 0, 0, 0, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 0,
        ];
        write_png(
            &face.join("OcarinaButtonA_00.south.png"),
            2,
            2,
            png::ColorType::Rgba,
            &pixels,
        );
        let overrides = Overrides::new(&dir);
        let rgba = overrides
            .rgba("switch", "OcarinaButtonA_00.south.png", 2, 2)
            .unwrap()
            .unwrap();
        assert_eq!(rgba, pixels);
        let mask = overrides
            .mask("switch", "OcarinaButtonA_00.south.png", 2, 2)
            .unwrap()
            .unwrap();
        assert_eq!(mask.data, vec![76, 150, 29, 255]);

        // (a greyscale PNG keeps its values; Pillow's convert("L") does too)
        let palette = dir.join("switch").join("KeyTexFontA_00.south.png");
        write_png(&palette, 2, 1, png::ColorType::Grayscale, &[12, 200]);
        let mask = overrides
            .mask("switch", "KeyTexFontA_00.south.png", 2, 1)
            .unwrap()
            .unwrap();
        assert_eq!(mask.data, vec![12, 200]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn no_folder_means_no_overrides() {
        let overrides = Overrides::none();
        assert!(overrides.path("xbox", "Controller_Pro.png").is_none());
        assert!(
            overrides
                .rgba("xbox", "Controller_Pro.png", 8, 8)
                .unwrap()
                .is_none()
        );
        assert!(
            overrides
                .mask("xbox", "KeyTexFontA_00.png", 8, 8)
                .unwrap()
                .is_none()
        );
        let missing = Overrides::new("/nonexistent/hookshot/prompt_faces");
        assert!(missing.path("xbox", "Controller_Pro.png").is_none());
    }
}
