//! Glyph rasterization and Pillow-compatible image operations for the prompt
//! textures: the Kenney icons and the game font's glyphs as L masks, the
//! shapes, Gaussian blur and LANCZOS/BICUBIC/BOX resampling
//! `nx2/prompt_assets.py` and `nx2/prompt_draw.py` use.
//!
//! A mask is a square, the glyph's longest side fitted to its size and
//! centred. `shape` fills its holes, `holes` is what the shape fills that the
//! glyph does not (the letter in a button or the counter of an A).
//!
//! The shape, blur and resampling functions match Pillow 13 byte for byte on
//! synthetic inputs (their tests pin its hashes). Glyphs are filled by a
//! scanline rasterizer rather than FreeType, so antialiased edges can differ
//! by a pixel: the controller silhouette differs on about 0.08% of its
//! pixels at the size the picture uses, which is why the real pictures are
//! close but not byte-identical to the Python reference.

use std::f64::consts::PI;

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

    /// A box of the mask; outside the mask is zero (`PIL.Image.crop`).
    pub fn crop(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> Mask {
        let width = (x1 - x0).max(0) as u32;
        let height = (y1 - y0).max(0) as u32;
        let mut out = Mask::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let sx = x as i64 + x0;
                let sy = y as i64 + y0;
                if sx >= 0 && sy >= 0 && sx < self.width as i64 && sy < self.height as i64 {
                    out.set(x, y, self.at(sx as u32, sy as u32));
                }
            }
        }
        out
    }

    /// Resize with Pillow's 8-bit resampler (`PIL.Image.resize`).
    pub fn resize(&self, width: u32, height: u32, filter: Filter) -> Mask {
        Mask {
            width,
            height,
            data: resample(
                &self.data,
                1,
                self.width,
                self.height,
                width,
                height,
                filter,
            ),
        }
    }

    /// `PIL.ImageFilter.GaussianBlur`: three box-blur passes, as Pillow does.
    pub fn gaussian_blur(&self, radius: f64) -> Mask {
        if radius <= 0.0 {
            return self.clone();
        }
        let radius = gaussian_blur_radius(radius as f32, 3);
        let mut out = self.data.clone();
        if radius != 0.0 {
            for _ in 0..3 {
                out = box_blur(&out, self.width, self.height, radius, true);
            }
            for _ in 0..3 {
                out = box_blur(&out, self.width, self.height, radius, false);
            }
        }
        Mask {
            width: self.width,
            height: self.height,
            data: out,
        }
    }

    /// `PIL.Image.point` with an L-mode function.
    pub fn point(&self, map: impl Fn(u8) -> u8) -> Mask {
        Mask {
            width: self.width,
            height: self.height,
            data: self.data.iter().map(|value| map(*value)).collect(),
        }
    }

    /// `ImageChops.invert` on an L mask.
    pub fn invert(&self) -> Mask {
        self.point(|value| 255 - value)
    }

    /// The smallest value in a (2 * radius + 1) square around each pixel,
    /// with the edges extended (`ImageFilter.MinFilter`).
    pub fn min_filter(&self, radius: u32) -> Mask {
        let mut out = Mask::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                let mut best = 255u8;
                for dy in -(radius as i64)..=radius as i64 {
                    for dx in -(radius as i64)..=radius as i64 {
                        let px = (x as i64 + dx).clamp(0, self.width as i64 - 1);
                        let py = (y as i64 + dy).clamp(0, self.height as i64 - 1);
                        best = best.min(self.at(px as u32, py as u32));
                    }
                }
                out.set(x, y, best);
            }
        }
        out
    }

    /// Copy `src` into this mask at (x, y), clipped (`PIL.Image.paste` without
    /// a mask).
    pub fn paste(&mut self, src: &Mask, x: i64, y: i64) {
        for sy in 0..src.height {
            for sx in 0..src.width {
                let dx = x + sx as i64;
                let dy = y + sy as i64;
                if dx >= 0 && dy >= 0 && dx < self.width as i64 && dy < self.height as i64 {
                    self.set(dx as u32, dy as u32, src.at(sx, sy));
                }
            }
        }
    }

    /// A filled rectangle, both corners inclusive (`PIL.ImageDraw.rectangle`).
    pub fn fill_rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, value: u8) {
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i64 - 1);
        let y1 = y1.min(self.height as i64 - 1);
        if x1 < x0 || y1 < y0 {
            return;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x as u32, y as u32, value);
            }
        }
    }

    /// A filled ellipse inside the inclusive box (`ImageDraw.ellipse` with
    /// `fill`), using Pillow's ellipse rasterizer.
    pub fn fill_ellipse(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, value: u8) {
        self.ellipse(x0, y0, x1, y1, 0, true, value, None);
    }

    /// An ellipse outline of `width` pixels inside the box
    /// (`ImageDraw.ellipse` with `outline` and `width`).
    pub fn outline_ellipse(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, width: i64, value: u8) {
        self.ellipse(x0, y0, x1, y1, width, false, value, None);
    }

    fn ellipse(
        &mut self,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        width: i64,
        fill: bool,
        value: u8,
        clip: Option<(i64, i64, i64, i64)>,
    ) {
        let (a, b) = ((x1 - x0) as i32, (y1 - y0) as i32);
        if a < 0 || b < 0 {
            return;
        }
        let width = if fill { a + b } else { width as i32 };
        let mut state = EllipseState::new(a, b, width);
        while let Some((sx0, sy, sx1)) = state.next() {
            let y = y0 + ((sy as i64 + b as i64) / 2);
            let left = x0 + ((sx0 as i64 + a as i64) / 2);
            let right = x0 + ((sx1 as i64 + a as i64) / 2);
            let (left, right, y) = match clip {
                Some((cx0, cy0, cx1, cy1)) => {
                    if y < cy0 || y > cy1 {
                        continue;
                    }
                    (left.max(cx0), right.min(cx1), y)
                }
                None => (left, right, y),
            };
            if left <= right {
                self.fill_rect(left, y, right, y, value);
            }
        }
    }

    /// A filled rounded rectangle (`ImageDraw.rounded_rectangle` with `fill`).
    pub fn fill_rounded_rect(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        radius: f64,
        value: u8,
    ) {
        self.rounded_rect(x0, y0, x1, y1, radius, true, 0, value);
    }

    /// A rounded rectangle outline of `width` pixels
    /// (`ImageDraw.rounded_rectangle` with `outline` and `width`).
    pub fn outline_rounded_rect(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        radius: f64,
        width: i64,
        value: u8,
    ) {
        self.rounded_rect(x0, y0, x1, y1, radius, false, width, value);
    }

    #[allow(clippy::too_many_arguments)]
    fn rounded_rect(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        radius: f64,
        fill: bool,
        width: i64,
        ink: u8,
    ) {
        let mut d = (x1 - x0).min(y1 - y0).min(radius * 2.0);
        let x0 = py_round(x0) as i64;
        let y0 = py_round(y0) as i64;
        let x1 = py_round(x1) as i64;
        let y1 = py_round(y1) as i64;
        let full_x = d >= (x1 - x0 - 1) as f64;
        if full_x {
            d = (x1 - x0) as f64;
        }
        let full_y = d >= (y1 - y0 - 1) as f64;
        if full_y {
            d = (y1 - y0) as f64;
        }
        if full_x && full_y {
            if fill {
                self.fill_ellipse(x0, y0, x1, y1, ink);
            } else {
                self.outline_ellipse(x0, y0, x1, y1, width, ink);
            }
            return;
        }
        if d == 0.0 {
            if fill {
                self.fill_rect(x0, y0, x1, y1, ink);
            } else {
                self.outline_rect(x0, y0, x1, y1, width, ink);
            }
            return;
        }
        let r = (d / 2.0).floor() as i64;
        let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);

        let corners: [(i64, i64, i64, i64); 4] = [
            (x0, y0, x0 + d as i64, y0 + d as i64),
            (x1 - d as i64, y0, x1, y0 + d as i64),
            (x1 - d as i64, y1 - d as i64, x1, y1),
            (x0, y1 - d as i64, x0 + d as i64, y1),
        ];
        let enabled: [bool; 4] = if full_x {
            [true, false, true, false]
        } else if full_y {
            [true, true, false, false]
        } else {
            [true, true, true, true]
        };
        for (index, &(bx0, by0, bx1, by1)) in corners.iter().enumerate() {
            if !enabled[index] {
                continue;
            }
            let (qx0, qy0, qx1, qy1) = if full_x {
                if index == 0 {
                    (bx0, by0, bx1, cy)
                } else {
                    (bx0, cy, bx1, by1)
                }
            } else if full_y {
                if index == 0 {
                    (bx0, by0, cx, by1)
                } else {
                    (cx, by0, bx1, by1)
                }
            } else {
                let mx = (bx0 + bx1) / 2;
                let my = (by0 + by1) / 2;
                match index {
                    0 => (bx0, by0, mx, my),
                    1 => (mx, by0, bx1, my),
                    2 => (mx, my, bx1, by1),
                    _ => (bx0, my, mx, by1),
                }
            };
            if fill {
                self.pie(bx0, by0, bx1, by1, qx0, qy0, qx1, qy1, ink);
            } else {
                self.arc(bx0, by0, bx1, by1, qx0, qy0, qx1, qy1, width, ink);
            }
        }

        if fill {
            if full_x {
                self.fill_rect(x0, y0 + r + 1, x1, y1 - r - 1, ink);
            } else if x1 - r - 1 >= x0 + r + 1 {
                self.fill_rect(x0 + r + 1, y0, x1 - r - 1, y1, ink);
            }
            if !full_x && !full_y {
                self.fill_rect(x0, y0 + r + 1, x0 + r, y1 - r - 1, ink);
                self.fill_rect(x1 - r, y0 + r + 1, x1, y1 - r - 1, ink);
            }
        } else {
            if !full_x {
                self.fill_rect(x0 + r + 1, y0, x1 - r - 1, y0 + width - 1, ink);
                self.fill_rect(x0 + r + 1, y1 - width + 1, x1 - r - 1, y1, ink);
            }
            if !full_y {
                self.fill_rect(x0, y0 + r + 1, x0 + width - 1, y1 - r - 1, ink);
                self.fill_rect(x1 - width + 1, y0 + r + 1, x1, y1 - r - 1, ink);
            }
        }
    }

    fn outline_rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, width: i64, value: u8) {
        let width = width.max(1);
        for i in 0..width {
            self.fill_rect(x0, y0 + i, x1, y0 + i, value);
            self.fill_rect(x0, y1 - i, x1, y1 - i, value);
            self.fill_rect(x0 + i, y0, x0 + i, y1, value);
            self.fill_rect(x1 - i, y0, x1 - i, y1, value);
        }
    }

    fn pie(
        &mut self,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        qx0: i64,
        qy0: i64,
        qx1: i64,
        qy1: i64,
        value: u8,
    ) {
        let (a, b) = ((x1 - x0) as i32, (y1 - y0) as i32);
        if a < 0 || b < 0 {
            return;
        }
        let mut state = EllipseState::new(a, b, a + b + 1);
        while let Some((sx0, sy, sx1)) = state.next() {
            let y = y0 + ((sy as i64 + b as i64) / 2);
            if y < qy0 || y > qy1 {
                continue;
            }
            let left = (x0 + ((sx0 as i64 + a as i64) / 2)).max(qx0);
            let right = (x0 + ((sx1 as i64 + a as i64) / 2)).min(qx1);
            if left <= right {
                self.fill_rect(left, y, right, y, value);
            }
        }
    }

    fn arc(
        &mut self,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        qx0: i64,
        qy0: i64,
        qx1: i64,
        qy1: i64,
        width: i64,
        value: u8,
    ) {
        let (a, b) = ((x1 - x0) as i32, (y1 - y0) as i32);
        if a < 0 || b < 0 {
            return;
        }
        let mut state = EllipseState::new(a, b, width as i32);
        while let Some((sx0, sy, sx1)) = state.next() {
            let y = y0 + ((sy as i64 + b as i64) / 2);
            if y < qy0 || y > qy1 {
                continue;
            }
            let left = (x0 + ((sx0 as i64 + a as i64) / 2)).max(qx0);
            let right = (x0 + ((sx1 as i64 + a as i64) / 2)).min(qx1);
            if left <= right {
                self.fill_rect(left, y, right, y, value);
            }
        }
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

/// Python's `round`: ties to even.
pub fn py_round(value: f64) -> f64 {
    value.round_ties_even()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Box,
    Bicubic,
    Lanczos,
}

impl Filter {
    fn support(self) -> f64 {
        match self {
            Filter::Box => 0.5,
            Filter::Bicubic => 2.0,
            Filter::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f64) -> f64 {
        match self {
            Filter::Box => {
                if x > -0.5 && x <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Filter::Bicubic => {
                const A: f64 = -0.5;
                let x = x.abs();
                if x < 1.0 {
                    ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * A
                } else {
                    0.0
                }
            }
            Filter::Lanczos => {
                if (-3.0..3.0).contains(&x) {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
        }
    }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let x = x * PI;
        x.sin() / x
    }
}

const PRECISION_BITS: i32 = 32 - 8 - 2;

struct Coeffs {
    ksize: usize,
    bounds: Vec<(usize, usize)>,
    kk: Vec<i32>,
}

fn precompute(in_size: usize, out_size: usize, filter: Filter) -> Coeffs {
    let scale = in_size as f64 / out_size as f64;
    let filterscale = scale.max(1.0);
    let support = filter.support() * filterscale;
    let ksize = support.ceil() as usize * 2 + 1;
    let mut bounds = Vec::with_capacity(out_size);
    let mut kk = vec![0i32; out_size * ksize];
    let inv_filterscale = 1.0 / filterscale;
    for xx in 0..out_size {
        let center = (xx as f64 + 0.5) * scale;
        let mut xmin = (center - support + 0.5) as i64;
        if xmin < 0 {
            xmin = 0;
        }
        let mut xmax = (center + support + 0.5) as i64;
        if xmax > in_size as i64 {
            xmax = in_size as i64;
        }
        let count = (xmax - xmin).max(0) as usize;
        let k = &mut kk[xx * ksize..xx * ksize + ksize];
        let mut weights = vec![0.0; count];
        let mut sum = 0.0;
        for x in 0..count {
            weights[x] = filter.weight((x as f64 + xmin as f64 - center + 0.5) * inv_filterscale);
            sum += weights[x];
        }
        if sum != 0.0 {
            for weight in &mut weights {
                *weight /= sum;
            }
        }
        for x in 0..count {
            k[x] = fixed(weights[x]);
        }
        for slot in &mut k[count..] {
            *slot = 0;
        }
        bounds.push((xmin as usize, count));
    }
    Coeffs { ksize, bounds, kk }
}

fn fixed(weight: f64) -> i32 {
    if weight < 0.0 {
        (-0.5 + weight * f64::from(1 << PRECISION_BITS)) as i32
    } else {
        (0.5 + weight * f64::from(1 << PRECISION_BITS)) as i32
    }
}

fn clip8(value: i64) -> u8 {
    ((value >> PRECISION_BITS).clamp(0, 255)) as u8
}

fn resample(
    data: &[u8],
    channels: usize,
    in_w: u32,
    in_h: u32,
    out_w: u32,
    out_h: u32,
    filter: Filter,
) -> Vec<u8> {
    if in_w == out_w && in_h == out_h {
        return data.to_vec();
    }
    let need_horizontal = out_w != in_w;
    let need_vertical = out_h != in_h;
    let horizontal_first = !((in_h > out_h) && (in_h - out_h) > (in_w - out_w) * 2);
    let mut current = data.to_vec();
    let horizontal = |current: &[u8], width: u32, height: u32| {
        let coeffs = precompute(width as usize, out_w as usize, filter);
        let mut out = vec![0u8; out_w as usize * height as usize * channels];
        for y in 0..height as usize {
            for xx in 0..out_w as usize {
                let (xmin, count) = coeffs.bounds[xx];
                let k = &coeffs.kk[xx * coeffs.ksize..xx * coeffs.ksize + count];
                for c in 0..channels {
                    let mut sum = 1i64 << (PRECISION_BITS - 1);
                    for x in 0..count {
                        let at = (y * width as usize + xmin + x) * channels + c;
                        sum += i64::from(current[at]) * i64::from(k[x]);
                    }
                    out[(y * out_w as usize + xx) * channels + c] = clip8(sum);
                }
            }
        }
        out
    };
    let vertical = |current: &[u8], width: u32, height: u32| {
        let coeffs = precompute(height as usize, out_h as usize, filter);
        let mut out = vec![0u8; width as usize * out_h as usize * channels];
        for yy in 0..out_h as usize {
            let (ymin, count) = coeffs.bounds[yy];
            let k = &coeffs.kk[yy * coeffs.ksize..yy * coeffs.ksize + count];
            for x in 0..width as usize {
                for c in 0..channels {
                    let mut sum = 1i64 << (PRECISION_BITS - 1);
                    for y in 0..count {
                        let at = ((ymin + y) * width as usize + x) * channels + c;
                        sum += i64::from(current[at]) * i64::from(k[y]);
                    }
                    out[(yy * width as usize + x) * channels + c] = clip8(sum);
                }
            }
        }
        out
    };
    if horizontal_first {
        if need_horizontal {
            current = horizontal(&current, in_w, in_h);
        }
        if need_vertical {
            current = vertical(&current, out_w, in_h);
        }
    } else {
        if need_vertical {
            current = vertical(&current, in_w, in_h);
        }
        if need_horizontal {
            current = horizontal(&current, in_w, out_h);
        }
    }
    current
}

fn gaussian_blur_radius(radius: f32, passes: i32) -> f32 {
    let sigma2 = radius * radius / passes as f32;
    let l = ((12.0 * f64::from(sigma2) + 1.0).sqrt()) as f32;
    let l = ((l - 1.0) / 2.0).floor();
    let mut a = (2.0 * l + 1.0) * (l * (l + 1.0) - 3.0 * sigma2);
    a /= 6.0 * (sigma2 - (l + 1.0) * (l + 1.0));
    l + a
}

fn box_blur(data: &[u8], width: u32, height: u32, radius: f32, horizontal: bool) -> Vec<u8> {
    let radius_int = radius as i64;
    let ww = ((1u64 << 24) as f64 / (f64::from(radius) * 2.0 + 1.0)) as u64;
    let fw = ((1u64 << 24) - (radius_int as u64 * 2 + 1) * ww) / 2;
    let mut out = vec![0u8; data.len()];
    let (lines, len) = if horizontal {
        (height, width)
    } else {
        (width, height)
    };
    for line in 0..lines {
        let mut values = Vec::with_capacity(len as usize);
        for x in 0..len as i64 {
            let mut acc = 0u64;
            for k in -radius_int..=radius_int {
                let at = (x + k).clamp(0, len as i64 - 1);
                let index = if horizontal {
                    line as usize * width as usize + at as usize
                } else {
                    at as usize * width as usize + line as usize
                };
                acc += u64::from(data[index]);
            }
            let left = (x - radius_int - 1).clamp(0, len as i64 - 1);
            let right = (x + radius_int + 1).clamp(0, len as i64 - 1);
            let left_index = if horizontal {
                line as usize * width as usize + left as usize
            } else {
                left as usize * width as usize + line as usize
            };
            let right_index = if horizontal {
                line as usize * width as usize + right as usize
            } else {
                right as usize * width as usize + line as usize
            };
            let bulk = acc * ww + (u64::from(data[left_index]) + u64::from(data[right_index])) * fw;
            values.push(((bulk + (1 << 23)) >> 24) as u8);
        }
        for (x, value) in values.into_iter().enumerate() {
            let index = if horizontal {
                line as usize * width as usize + x
            } else {
                x * width as usize + line as usize
            };
            out[index] = value;
        }
    }
    out
}

struct Quarter {
    cx: i32,
    cy: i32,
    ex: i32,
    ey: i32,
    a2: i64,
    b2: i64,
    a2b2: i64,
    finished: bool,
}

impl Quarter {
    fn new(a: i32, b: i32) -> Self {
        if a < 0 || b < 0 {
            return Self {
                cx: 0,
                cy: 0,
                ex: 0,
                ey: 0,
                a2: 0,
                b2: 0,
                a2b2: 0,
                finished: true,
            };
        }
        Self {
            cx: a,
            cy: b % 2,
            ex: a % 2,
            ey: b,
            a2: i64::from(a) * i64::from(a),
            b2: i64::from(b) * i64::from(b),
            a2b2: i64::from(a) * i64::from(a) * i64::from(b) * i64::from(b),
            finished: false,
        }
    }

    fn delta(&self, x: i64, y: i64) -> i64 {
        (self.a2 * y * y + self.b2 * x * x - self.a2b2).abs()
    }

    fn next(&mut self) -> Option<(i32, i32)> {
        if self.finished {
            return None;
        }
        let (rx, ry) = (self.cx, self.cy);
        if self.cx == self.ex && self.cy == self.ey {
            self.finished = true;
        } else {
            let (mut nx, mut ny) = (self.cx, self.cy + 2);
            let mut ndelta = self.delta(i64::from(nx), i64::from(ny));
            if nx > 1 {
                let mut newdelta = self.delta(i64::from(self.cx - 2), i64::from(self.cy + 2));
                if ndelta > newdelta {
                    nx = self.cx - 2;
                    ny = self.cy + 2;
                    ndelta = newdelta;
                }
                newdelta = self.delta(i64::from(self.cx - 2), i64::from(self.cy));
                if ndelta > newdelta {
                    nx = self.cx - 2;
                    ny = self.cy;
                }
            }
            self.cx = nx;
            self.cy = ny;
        }
        Some((rx, ry))
    }
}

struct EllipseState {
    outer: Quarter,
    inner: Quarter,
    py: i32,
    pl: i32,
    pr: i32,
    cy: [i32; 4],
    cl: [i32; 4],
    cr: [i32; 4],
    bufcnt: i32,
    finished: bool,
    leftmost: i32,
}

impl EllipseState {
    fn new(a: i32, b: i32, width: i32) -> Self {
        let mut state = Self {
            outer: Quarter::new(a, b),
            inner: Quarter::new(a, b),
            py: 0,
            pl: 0,
            pr: 0,
            cy: [0; 4],
            cl: [0; 4],
            cr: [0; 4],
            bufcnt: 0,
            finished: false,
            leftmost: a % 2,
        };
        let mut pr = 0;
        let mut py = 0;
        if width < 1 || state.outer.next().is_none() {
            state.finished = true;
            return state;
        }
        // (the first point was just consumed to prime `pr`/`py`; redo it)
        let mut outer = Quarter::new(a, b);
        if let Some((x, y)) = outer.next() {
            pr = x;
            py = y;
        }
        state.outer = outer;
        state.pr = pr;
        state.py = py;
        state.inner = Quarter::new(a - 2 * (width - 1), b - 2 * (width - 1));
        state.pl = state.leftmost;
        state
    }

    fn next(&mut self) -> Option<(i32, i32, i32)> {
        if self.bufcnt == 0 {
            if self.finished {
                return None;
            }
            let y = self.py;
            let mut l = self.pl;
            let r = self.pr;
            let mut next_ret = self.outer.next();
            while let Some((cx, cy)) = next_ret {
                if cy <= y {
                    next_ret = self.outer.next();
                } else {
                    self.pr = cx;
                    self.py = cy;
                    break;
                }
            }
            if next_ret.is_none() {
                self.finished = true;
            }
            loop {
                match self.inner.next() {
                    Some((cx, cy)) if cy <= y => l = cx,
                    Some((cx, _)) => {
                        self.pl = cx;
                        break;
                    }
                    None => {
                        self.pl = self.leftmost;
                        break;
                    }
                }
            }
            let push = |state: &mut Self, x0: i32, y: i32, x1: i32| {
                state.cl[state.bufcnt as usize] = x0;
                state.cy[state.bufcnt as usize] = y;
                state.cr[state.bufcnt as usize] = x1;
                state.bufcnt += 1;
            };
            if (l > 0 || l < r) && y > 0 {
                push(self, if l == 0 { 2 } else { l }, y, r);
            }
            if y > 0 {
                push(self, -r, y, -l);
            }
            if l > 0 || l < r {
                push(self, if l == 0 { 2 } else { l }, -y, r);
            }
            push(self, -r, -y, -l);
        }
        self.bufcnt -= 1;
        let at = self.bufcnt as usize;
        Some((self.cl[at], self.cy[at], self.cr[at]))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Rgba {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn from_rgba(data: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data,
        }
    }

    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        [
            self.data[at],
            self.data[at + 1],
            self.data[at + 2],
            self.data[at + 3],
        ]
    }

    pub fn set(&mut self, x: u32, y: u32, value: [u8; 4]) {
        let at = ((y * self.width + x) * 4) as usize;
        self.data[at..at + 4].copy_from_slice(&value);
    }

    pub fn crop(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> Rgba {
        let width = (x1 - x0).max(0) as u32;
        let height = (y1 - y0).max(0) as u32;
        let mut out = Rgba::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let sx = x as i64 + x0;
                let sy = y as i64 + y0;
                if sx >= 0 && sy >= 0 && sx < self.width as i64 && sy < self.height as i64 {
                    out.set(x, y, self.at(sx as u32, sy as u32));
                }
            }
        }
        out
    }

    pub fn resize(&self, width: u32, height: u32, filter: Filter) -> Rgba {
        // (Pillow resizes RGBA in premultiplied alpha, `RGBa`, and converts
        // back afterwards; see `Image.resize`)
        let mut premultiplied = self.data.clone();
        for pixel in premultiplied.chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                let value = u32::from(*channel) * alpha + 128;
                *channel = (((value >> 8) + value) >> 8) as u8;
            }
        }
        let mut data = resample(
            &premultiplied,
            4,
            self.width,
            self.height,
            width,
            height,
            filter,
        );
        for pixel in data.chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            if alpha != 0 && alpha != 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((255 * u32::from(*channel)) / alpha).min(255) as u8;
                }
            }
        }
        Rgba {
            width,
            height,
            data,
        }
    }

    pub fn alpha(&self) -> Mask {
        let mut out = Mask::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                out.set(x, y, self.at(x, y)[3]);
            }
        }
        out
    }

    /// `Image.alpha_composite` with this image as the destination.
    pub fn alpha_composite(&mut self, src: &Rgba, dx: i64, dy: i64) {
        for sy in 0..src.height {
            for sx in 0..src.width {
                let x = dx + sx as i64;
                let y = dy + sy as i64;
                if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
                    continue;
                }
                let [sr, sg, sb, sa] = src.at(sx, sy);
                let [dr, dg, db, da] = self.at(x as u32, y as u32);
                let out = if sa == 0 {
                    [dr, dg, db, da]
                } else {
                    let blend = u32::from(da) * (255 - u32::from(sa));
                    let outa255 = u32::from(sa) * 255 + blend;
                    let coef1 = u32::from(sa) * 255 * 255 * (1 << 7) / outa255;
                    let coef2 = 255 * (1 << 7) - coef1;
                    let channel = |s: u8, d: u8| {
                        let tmp = u32::from(s) * coef1 + u32::from(d) * coef2;
                        let tmp = tmp + (0x80 << 7);
                        let tmp = ((tmp >> 8) + tmp) >> 8;
                        (tmp >> 7) as u8
                    };
                    let outa = outa255 + 0x80;
                    let outa = ((outa >> 8) + outa) >> 8;
                    [
                        channel(sr, dr),
                        channel(sg, dg),
                        channel(sb, db),
                        outa as u8,
                    ]
                };
                self.set(x as u32, y as u32, out);
            }
        }
    }

    /// `Image.paste` with a colour and an L mask (`fill_mask_L`).
    pub fn paste_color(&mut self, color: [u8; 4], mask: &Mask, dx: i64, dy: i64) {
        for my in 0..mask.height {
            for mx in 0..mask.width {
                let x = dx + mx as i64;
                let y = dy + my as i64;
                if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
                    continue;
                }
                let m = mask.at(mx, my);
                let dst = self.at(x as u32, y as u32);
                let mut out = [0u8; 4];
                for channel in 0..4 {
                    let mut channel_mask = m;
                    if channel != 3 && channel_mask != 0 {
                        channel_mask =
                            (255 - (255 - channel_mask) * (1 - (255 - dst[3]) / 255)) as u8;
                    }
                    out[channel] = blend(channel_mask, dst[channel], color[channel]);
                }
                self.set(x as u32, y as u32, out);
            }
        }
    }

    pub fn fill_rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, color: [u8; 4]) {
        let mut mask = Mask::new((x1 - x0 + 1).max(0) as u32, (y1 - y0 + 1).max(0) as u32);
        for y in 0..mask.height {
            for x in 0..mask.width {
                mask.set(x, y, 255);
            }
        }
        self.paste_color(color, &mask, x0, y0);
    }

    pub fn fill_ellipse(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, color: [u8; 4]) {
        let width = (x1 - x0 + 1).max(0) as u32;
        let height = (y1 - y0 + 1).max(0) as u32;
        let mut mask = Mask::new(width, height);
        mask.fill_ellipse(0, 0, i64::from(width) - 1, i64::from(height) - 1, 255);
        self.paste_color(color, &mask, x0, y0);
    }

    pub fn outline_ellipse(
        &mut self,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        width: i64,
        color: [u8; 4],
    ) {
        let mask_width = (x1 - x0 + 1).max(0) as u32;
        let mask_height = (y1 - y0 + 1).max(0) as u32;
        let mut mask = Mask::new(mask_width, mask_height);
        mask.outline_ellipse(
            0,
            0,
            i64::from(mask_width) - 1,
            i64::from(mask_height) - 1,
            width,
            255,
        );
        self.paste_color(color, &mask, x0, y0);
    }

    pub fn outline_rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, width: i64, color: [u8; 4]) {
        let mask_width = (x1 - x0 + 1).max(0) as u32;
        let mask_height = (y1 - y0 + 1).max(0) as u32;
        let mut mask = Mask::new(mask_width, mask_height);
        mask.outline_rect(
            0,
            0,
            i64::from(mask_width) - 1,
            i64::from(mask_height) - 1,
            width,
            255,
        );
        self.paste_color(color, &mask, x0, y0);
    }

    /// Multiply the alpha channel by an L mask (`ImageChops.multiply` on the
    /// alpha channel), leaving the colours as they are.
    pub fn multiply_alpha(&mut self, mask: &Mask) {
        for y in 0..self.height.min(mask.height) {
            for x in 0..self.width.min(mask.width) {
                let at = ((y * self.width + x) * 4) as usize;
                self.data[at + 3] =
                    ((u32::from(self.data[at + 3]) * u32::from(mask.at(x, y))) / 255) as u8;
            }
        }
    }

    pub fn fill_rounded_rect(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        radius: f64,
        color: [u8; 4],
    ) {
        let (mask, left, top) = rounded_rect_mask(x0, y0, x1, y1, radius, 0.0, true);
        self.paste_color(color, &mask, left, top);
    }

    pub fn outline_rounded_rect(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        radius: f64,
        width: i64,
        color: [u8; 4],
    ) {
        let (mask, left, top) = rounded_rect_mask(x0, y0, x1, y1, radius, width as f64, false);
        self.paste_color(color, &mask, left, top);
    }
}

fn rounded_rect_mask(
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    radius: f64,
    width: f64,
    fill: bool,
) -> (Mask, i64, i64) {
    let left = py_round(x0.min(x1)) as i64;
    let top = py_round(y0.min(y1)) as i64;
    let right = py_round(x0.max(x1)) as i64;
    let bottom = py_round(y0.max(y1)) as i64;
    let mask_width = (right - left + 1).max(0) as u32;
    let mask_height = (bottom - top + 1).max(0) as u32;
    let mut mask = Mask::new(mask_width, mask_height);
    if fill {
        mask.fill_rounded_rect(
            x0 - left as f64,
            y0 - top as f64,
            x1 - left as f64,
            y1 - top as f64,
            radius,
            255,
        );
    } else {
        mask.outline_rounded_rect(
            x0 - left as f64,
            y0 - top as f64,
            x1 - left as f64,
            y1 - top as f64,
            radius,
            width as i64,
            255,
        );
    }
    (mask, left, top)
}

pub fn blend(mask: u8, dst: u8, src: u8) -> u8 {
    let value = u32::from(dst) * (255 - u32::from(mask)) + u32::from(src) * u32::from(mask);
    let value = value + 128;
    (((value >> 8) + value) >> 8) as u8
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

/// A glyph rendered at `pixel_size` pixels per em, cropped to its bounding box
/// with antialiased coverage (`PIL.ImageFont.truetype(path, pixel_size)` and
/// `ImageDraw.text`). The glyph's position on the baseline does not matter:
/// the result is cropped like the reference's `getbbox`.
pub fn glyph_pixels(font: &Face, glyph: GlyphId, pixel_size: u32) -> Result<Mask, String> {
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
    let units = f64::from(font.units_per_em().max(1));
    let scale = f64::from(pixel_size.max(1)) / units;
    let left = (min_x * scale).floor();
    let top = (max_y * scale).ceil();
    let width = ((max_x * scale).ceil() - left).max(1.0) as u32;
    let height = (top - (min_y * scale).floor()).max(1.0) as u32;
    let high = 4.0;
    let rw = width * 4;
    let rh = height * 4;
    let mut large = Mask::new(rw, rh);
    for hy in 0..rh {
        let sy = max_y - (hy as f64 + 0.5) / (scale * high);
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
            let x0 = ((pair[0] * scale - left) * high - 0.5).round().max(0.0) as u32;
            let x1 = ((pair[1] * scale - left) * high - 0.5)
                .round()
                .min(rw as f64) as u32;
            for x in x0..x1.min(rw) {
                large.set(x, hy, 255);
            }
        }
    }
    let mut out = Mask::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    sum += u32::from(large.at(x * 4 + sx, y * 4 + sy));
                }
            }
            out.set(x, y, (sum / 16) as u8);
        }
    }
    Ok(out)
}

/// The three masks `Icons.masks` makes: the antialiased icon, its silhouette
/// with holes filled, and the holes (`nx2/prompt_assets.py`).
pub fn icon_masks(font: &Face, glyph: GlyphId, size: u32) -> Result<(Mask, Mask, Mask), String> {
    let size = size.max(1);
    let glyph = glyph_pixels(font, glyph, size * 4)?;
    let bbox = glyph
        .bbox()
        .ok_or_else(|| "raster: empty glyph".to_owned())?;
    let glyph = glyph.crop(
        i64::from(bbox.0),
        i64::from(bbox.1),
        i64::from(bbox.2),
        i64::from(bbox.3),
    );
    let scale = f64::from(size) / f64::from(glyph.width.max(glyph.height).max(1));
    let width = ((f64::from(glyph.width) * scale).round() as u32).max(1);
    let height = ((f64::from(glyph.height) * scale).round() as u32).max(1);
    let glyph = glyph.resize(width, height, Filter::Lanczos);
    let mut icon = Mask::new(size, size);
    icon.paste(
        &glyph,
        i64::from((size - glyph.width) / 2),
        i64::from((size - glyph.height) / 2),
    );
    let shape = icon.shape();
    let inner = shape.min_filter(2);
    let holes = inner.multiply(&icon.invert());
    Ok((icon, shape, holes))
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

        let mut ranked = Mask::new(11, 9);
        for y in 0..9 {
            for x in 0..11 {
                ranked.set(x, y, ((x * 29 + y * 17 + x * y * 7) % 256) as u8);
            }
        }
        // (Pillow's MinFilter expands the edges before ranking)
        assert_eq!(fnv(&ranked.min_filter(2).data), 0x7b8c_6d33_540d_c899);
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

    fn fnv(data: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in data {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        hash
    }

    /// The shapes and filters above match Pillow 13.0.0 byte for byte: the
    /// hashes are its output for the same inputs.
    #[test]
    fn pillow_shapes_and_filters_match_the_reference() {
        let mut m = Mask::new(48, 40);
        m.fill_ellipse(3, 5, 37, 25, 255);
        assert_eq!(fnv(&m.data), 0xe02e_7088_bcd8_113e);
        let mut m = Mask::new(48, 40);
        m.outline_ellipse(3, 5, 37, 25, 7, 255);
        assert_eq!(fnv(&m.data), 0x5f7e_7fc2_59b9_b895);
        let mut m = Mask::new(80, 60);
        m.fill_rounded_rect(4.0, 6.0, 75.0, 55.0, 12.0, 255);
        assert_eq!(fnv(&m.data), 0x0071_35f5_01bd_15cd);
        let mut m = Mask::new(80, 60);
        m.outline_rounded_rect(4.0, 6.0, 75.0, 55.0, 12.0, 5, 255);
        assert_eq!(fnv(&m.data), 0xe70e_ab6c_e73b_d3f5);

        let mut m = Mask::new(64, 48);
        for y in 0..48 {
            for x in 0..64 {
                m.set(x, y, ((x * 37 + y * 53 + x * y * 11) % 256) as u8);
            }
        }
        assert_eq!(fnv(&m.gaussian_blur(6.0).data), 0xb56d_a2ec_bc16_8e19);
        assert_eq!(fnv(&m.gaussian_blur(24.0).data), 0xd339_96eb_20a2_7b76);
        assert_eq!(
            fnv(&m.resize(29, 17, Filter::Lanczos).data),
            0x32f2_3c1e_3177_999b
        );
        assert_eq!(
            fnv(&m.resize(64, 40, Filter::Bicubic).data),
            0x12d6_b79b_9534_97ac
        );
        assert_eq!(
            fnv(&m.resize(16, 12, Filter::Box).data),
            0x3fd2_fa36_9f57_b7c5
        );

        let mut rgba = Rgba::new(12, 9);
        for y in 0..9 {
            for x in 0..12 {
                rgba.set(
                    x,
                    y,
                    [
                        (x * 19) as u8,
                        (y * 27) as u8,
                        (x * y * 7) as u8,
                        (x * 11 + y * 3) as u8,
                    ],
                );
            }
        }
        let mut src = Rgba::new(5, 4);
        for y in 0..4 {
            for x in 0..5 {
                src.set(
                    x,
                    y,
                    [
                        (x * 41 + 7) as u8,
                        (y * 51 + 9) as u8,
                        (x * y * 13) as u8,
                        (x * 30 + y * 20) as u8,
                    ],
                );
            }
        }
        rgba.alpha_composite(&src, 3, 2);
        assert_eq!(fnv(&rgba.data), 0xf409_cf61_7821_40eb);

        let mut large = Rgba::new(149, 149);
        for y in 0..149 {
            for x in 0..149 {
                large.set(
                    x,
                    y,
                    [
                        ((x * 7 + y * 13) & 0xff) as u8,
                        ((x * 17 + y * 3) & 0xff) as u8,
                        ((x * 5 + y * 29) & 0xff) as u8,
                        ((x * 11 + y * 23) & 0xff) as u8,
                    ],
                );
            }
        }
        // (Pillow resizes RGBA premultiplied: the alpha edges match too)
        assert_eq!(
            fnv(&large.resize(73, 73, Filter::Lanczos).data),
            0xb5c3_6119_f63e_d417
        );
    }

    #[test]
    fn a_glyph_renders_at_its_pixel_size_and_keeps_its_holes() {
        let bytes = kenney("kenney_input_xbox_series.otf");
        let font = Face::parse(&bytes, 0).unwrap();
        let gid = (0..font.number_of_glyphs())
            .find(|gid| font.glyph_name(GlyphId(*gid)) == Some("xbox_button_a"))
            .map(GlyphId)
            .unwrap();
        let (icon, shape, holes) = icon_masks(&font, gid, 64).unwrap();
        assert_eq!((icon.width, icon.height), (64, 64));
        assert!(icon.any());
        assert!(shape.any());
        // (A's counter is a hole; the soft rim is not)
        assert!(holes.any());
        let glyph = glyph_pixels(&font, gid, 128).unwrap();
        assert!(glyph.width > 0 && glyph.height > 0);
        assert!(glyph.any());
    }
}
