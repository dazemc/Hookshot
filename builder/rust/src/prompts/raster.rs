//! Glyph rasterization for the prompt textures: the Kenney icons and the
//! game font's glyphs as L masks, the way `nx2/prompt_assets.py` draws them
//! with Pillow (a 4x supersampled scanline fill, then a box filter).
//!
//! A mask is a square, the glyph's longest side fitted to its size and
//! centred. `shape` fills its holes, `holes` is what the shape fills that the
//! glyph does not (the letter in a button or the counter of an A).

use ttf_parser::{Face, GlyphId, OutlineBuilder};

#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // 0 or 255
}

impl Mask {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height) as usize],
        }
    }

    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.data[(y * self.width + x) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, value: u8) {
        let at = (y * self.width + x) as usize;
        self.data[at] = value;
    }

    pub fn any(&self) -> bool {
        self.data.iter().any(|value| *value != 0)
    }

    /// The mask with its holes filled: everything the outside cannot reach.
    pub fn shape(&self) -> Mask {
        let w = self.width as i32;
        let h = self.height as i32;
        let mut outside = vec![false; ((w + 4) * (h + 4)) as usize];
        let index = |x: i32, y: i32| ((y + 2) * (w + 4) + x + 2) as usize;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((-2, -2));
        outside[index(-2, -2)] = true;
        while let Some((x, y)) = queue.pop_front() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < -2 || ny < -2 || nx >= w + 2 || ny >= h + 2 {
                    continue;
                }
                let at = index(nx, ny);
                if outside[at] {
                    continue;
                }
                // (the glyph's pixels block the flood; its soft edge is a
                // threshold of 127, as Pillow's masks are)
                if nx >= 0 && ny >= 0 && nx < w && ny < h && self.at(nx as u32, ny as u32) > 127 {
                    continue;
                }
                outside[at] = true;
                queue.push_back((nx, ny));
            }
        }
        let mut out = Mask::new(self.width, self.height);
        for y in 0..h {
            for x in 0..w {
                if !outside[index(x, y)] {
                    out.set(x as u32, y as u32, 255);
                }
            }
        }
        out
    }

    /// The enclosed background inside the shape: a letter in a disc, the
    /// counter of an A.
    pub fn holes(&self) -> Mask {
        let shape = self.shape();
        let mut out = Mask::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                if shape.at(x, y) != 0 && self.at(x, y) <= 127 {
                    out.set(x, y, 255);
                }
            }
        }
        out
    }

    /// The bounding box of the non-zero pixels: (left, upper, right, lower),
    /// right and lower exclusive (Pillow's `getbbox`).
    pub fn bbox(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (self.width, self.height, 0, 0);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.at(x, y) != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x1 > 0).then_some((x0, y0, x1, y1))
    }

    /// The lighter of two masks, pixel by pixel (Pillow's `ImageChops.lighter`).
    pub fn lighter(&self, other: &Mask) -> Mask {
        self.combine(other, |a, b| a.max(b))
    }

    /// The first minus the second, never below zero (`ImageChops.subtract`).
    pub fn subtract(&self, other: &Mask) -> Mask {
        self.combine(other, |a, b| a.saturating_sub(b))
    }

    /// The product over 255, truncated (`ImageChops.multiply`).
    pub fn multiply(&self, other: &Mask) -> Mask {
        self.combine(other, |a, b| ((u32::from(a) * u32::from(b)) / 255) as u8)
    }

    fn combine(&self, other: &Mask, op: impl Fn(u8, u8) -> u8) -> Mask {
        let width = self.width.min(other.width);
        let height = self.height.min(other.height);
        let mut out = Mask::new(width, height);
        for y in 0..height {
            for x in 0..width {
                out.set(x, y, op(self.at(x, y), other.at(x, y)));
            }
        }
        out
    }

    /// The largest value in a (2 * radius + 1) square around each pixel,
    /// zeros outside the mask (`ImageFilter.MaxFilter`).
    pub fn max_filter(&self, radius: u32) -> Mask {
        let horizontal = self.max_filter_axis(radius, true);
        horizontal.max_filter_axis(radius, false)
    }

    fn max_filter_axis(&self, radius: u32, horizontal: bool) -> Mask {
        let mut out = Mask::new(self.width, self.height);
        let radius = radius as usize;
        if horizontal {
            for y in 0..self.height {
                let row = sliding_max(self.width as usize, radius, |x| self.at(x as u32, y));
                for (x, value) in row.into_iter().enumerate() {
                    out.set(x as u32, y, value);
                }
            }
        } else {
            for x in 0..self.width {
                let column = sliding_max(self.height as usize, radius, |y| self.at(x, y as u32));
                for (y, value) in column.into_iter().enumerate() {
                    out.set(x, y as u32, value);
                }
            }
        }
        out
    }

    /// This glyph centred in a texture-sized mask.
    pub fn place(&self, width: u32, height: u32) -> Mask {
        let mut out = Mask::new(width, height);
        let x0 = (width.saturating_sub(self.width)) / 2;
        let y0 = (height.saturating_sub(self.height)) / 2;
        for y in 0..self.height.min(height) {
            for x in 0..self.width.min(width) {
                let value = self.at(x, y);
                if value != 0 {
                    out.set(x0 + x, y0 + y, value);
                }
            }
        }
        out
    }
}

fn sliding_max(len: usize, radius: usize, value: impl Fn(usize) -> u8) -> Vec<u8> {
    let mut out = vec![0u8; len];
    if len == 0 {
        return out;
    }
    let mut deque = std::collections::VecDeque::new();
    for i in 0..=radius.min(len - 1) {
        let v = value(i);
        while deque.back().is_some_and(|back| value(*back) <= v) {
            deque.pop_back();
        }
        deque.push_back(i);
    }
    for (x, slot) in out.iter_mut().enumerate() {
        if x > 0 {
            let add = x + radius;
            if add < len {
                let v = value(add);
                while deque.back().is_some_and(|back| value(*back) <= v) {
                    deque.pop_back();
                }
                deque.push_back(add);
            }
            while deque.front().is_some_and(|front| front + radius < x) {
                deque.pop_front();
            }
        }
        *slot = deque.front().map(|front| value(*front)).unwrap_or(0);
    }
    out
}

#[derive(Default)]
struct Contours {
    contours: Vec<Vec<(f64, f64)>>,
    current: Vec<(f64, f64)>,
}

impl Contours {
    fn point(&mut self, x: f32, y: f32) {
        self.current.push((x as f64, y as f64));
    }
}

impl OutlineBuilder for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.current.len() > 1 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
        self.point(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let start = *self.current.last().unwrap_or(&(x1 as f64, y1 as f64));
        let (x1, y1, x, y) = (x1 as f64, y1 as f64, x as f64, y as f64);
        for step in 1..=12 {
            let t = step as f64 / 12.0;
            let u = 1.0 - t;
            self.current.push((
                u * u * start.0 + 2.0 * u * t * x1 + t * t * x,
                u * u * start.1 + 2.0 * u * t * y1 + t * t * y,
            ));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let start = *self.current.last().unwrap_or(&(x1 as f64, y1 as f64));
        let (x1, y1, x2, y2, x, y) = (
            x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64, y as f64,
        );
        for step in 1..=16 {
            let t = step as f64 / 16.0;
            let u = 1.0 - t;
            self.current.push((
                u * u * u * start.0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x,
                u * u * u * start.1 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y,
            ));
        }
    }

    fn close(&mut self) {
        if self.current.len() > 1 {
            let first = self.current[0];
            self.current.push(first);
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
}

/// A glyph of `font` as a `size` x `size` mask, its longest side fitted.
pub fn glyph(font: &Face, glyph: GlyphId, size: u32) -> Result<Mask, String> {
    let mut contours = Contours::default();
    if font.outline_glyph(glyph, &mut contours).is_none() {
        return Err(format!("raster: glyph {} has no outline", glyph.0));
    }
    if contours.current.len() > 1 {
        contours
            .contours
            .push(std::mem::take(&mut contours.current));
    }
    let points: Vec<(f64, f64)> = contours.contours.iter().flatten().copied().collect();
    if points.is_empty() {
        return Err("raster: empty glyph".to_owned());
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in &points {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    let glyph_w = (max_x - min_x).max(1.0);
    let glyph_h = (max_y - min_y).max(1.0);
    let high = (size.max(1) * 4) as f64; // (the supersampled drawing)
    let scale = (high - 2.0) / glyph_w.max(glyph_h);
    let rw = ((glyph_w * scale).ceil() as u32 + 2).max(1);
    let rh = ((glyph_h * scale).ceil() as u32 + 2).max(1);
    let mut large = Mask::new(rw, rh);
    for y in 0..rh {
        let sy = min_y + (y as f64 + 0.5 - 1.0) / scale;
        let mut crossings = Vec::new();
        for contour in &contours.contours {
            for pair in contour.windows(2) {
                let (x0, y0) = pair[0];
                let (x1, y1) = pair[1];
                if (y0 > sy) != (y1 > sy) {
                    let t = (sy - y0) / (y1 - y0);
                    crossings.push(x0 + t * (x1 - x0));
                }
            }
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in crossings.chunks_exact(2) {
            let x0 = ((pair[0] - min_x) * scale + 1.0).round().max(0.0) as u32;
            let x1 = ((pair[1] - min_x) * scale + 1.0).round().min(rw as f64) as u32;
            for x in x0..x1.min(rw) {
                large.set(x, y, 255);
            }
        }
    }
    // (box filter 4x4 to the target; the longest side is `size`)
    let dw = ((rw as f64 * size as f64 / high).round() as u32).max(1);
    let dh = ((rh as f64 * size as f64 / high).round() as u32).max(1);
    let mut small = Mask::new(dw, dh);
    for y in 0..dh {
        for x in 0..dw {
            let mut sum = 0u32;
            let mut count = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x * 4 + sx).min(rw - 1);
                    let py = (y * 4 + sy).min(rh - 1);
                    sum += u32::from(large.at(px, py));
                    count += 1;
                }
            }
            small.set(x, y, (sum / count) as u8);
        }
    }
    Ok(small.place(size, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kenney(name: &str) -> Vec<u8> {
        let path = crate::kit::root()
            .join("third_party/kenney_input")
            .join(name);
        std::fs::read(path).unwrap()
    }

    #[test]
    fn an_icon_has_a_shape_and_a_letter_its_holes() {
        let bytes = kenney("kenney_input_xbox_series.otf");
        let font = Face::parse(&bytes, 0).unwrap();
        let gid = (0..font.number_of_glyphs())
            .find(|gid| font.glyph_name(GlyphId(*gid)) == Some("xbox_button_a"))
            .map(GlyphId)
            .unwrap();
        let icon = glyph(&font, gid, 64).unwrap();
        assert!(icon.any());
        assert!(
            icon.shape().data.iter().filter(|v| **v != 0).count()
                > icon.data.iter().filter(|v| **v != 0).count()
        );
        // (A has a counter: the hole is an enclosed part of the shape)
        assert!(icon.holes().any());
    }

    #[test]
    fn filters_and_combinations_match_their_definitions() {
        let mut mask = Mask::new(9, 7);
        for y in 0..7 {
            for x in 0..9 {
                let value = ((x * 37 + y * 53 + x * y * 11) % 256) as u8;
                mask.set(x, y, value);
            }
        }
        for radius in 0..4 {
            let filtered = mask.max_filter(radius);
            for y in 0..7 {
                for x in 0..9 {
                    let mut best = 0u8;
                    for dy in -(radius as i64)..=radius as i64 {
                        for dx in -(radius as i64)..=radius as i64 {
                            let (px, py) = (x as i64 + dx, y as i64 + dy);
                            if px >= 0 && py >= 0 && px < 9 && py < 7 {
                                best = best.max(mask.at(px as u32, py as u32));
                            }
                        }
                    }
                    assert_eq!(filtered.at(x, y), best, "radius {radius} at {x},{y}");
                }
            }
        }
        assert_eq!(mask.bbox(), Some((0, 0, 9, 7)));
        let mut other = Mask::new(9, 7);
        other.set(0, 0, 200);
        assert_eq!(mask.lighter(&other).at(0, 0), mask.at(0, 0).max(200));
        assert_eq!(
            mask.multiply(&other).at(0, 0),
            (u32::from(mask.at(0, 0)) * 200 / 255) as u8
        );
        assert_eq!(
            other.subtract(&mask).at(0, 0),
            200u8.saturating_sub(mask.at(0, 0))
        );
        assert_eq!(Mask::new(9, 7).bbox(), None);
    }

    #[test]
    fn a_hole_is_the_counter_of_an_a() {
        let mut mask = Mask::new(16, 16);
        for x in 2..14 {
            for y in 2..14 {
                if x == 2 || x == 13 || y == 2 || y == 13 {
                    mask.set(x, y, 255);
                }
            }
        }
        let shape = mask.shape();
        let holes = mask.holes();
        assert_eq!(shape.at(8, 8), 255); // (the counter is inside the shape)
        assert_eq!(holes.at(8, 8), 255); // (and is the hole)
        assert_eq!(holes.at(1, 1), 0);
    }
}
