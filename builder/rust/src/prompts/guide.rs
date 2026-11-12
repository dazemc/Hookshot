//! The pause menu's controls page: its leader lines (`ControllerGuide_Pro_00`
//! and `_01`, BC4) redrawn for each face, as `nx2/prompt_assets.py` does
//! (`guide_at` through `guide_layers_of`).
//!
//! Each texture is written as a layer of the lines that stay whatever the
//! controls (the game's on the Switch face, the bars plus the fixed stick
//! lines on the others), then one layer for each Switch button's line on
//! each pad input, drawn over it. The router is Dijkstra on the same grid and
//! costs as the reference's: fewest bends, crossing the lines that stay as
//! little as it can.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use super::bntx::{self, Texture};
use super::faces::{self, Part, Parts};
use super::raster::Mask;

/// The two leader-line textures, as the game ships them.
pub const GUIDES: [&str; 2] = ["ControllerGuide_Pro_00", "ControllerGuide_Pro_01"];

/// The picture's pixels to the lines' pixels.
const GUIDE_SCALE: f64 = 1.1136;
const GUIDE_AT: (f64, f64) = (131.3, 462.6);
/// The label bars' edges: the left ones' right edge, the right ones' left.
const BAR_LEFT: i32 = 147;
const BAR_RIGHT: i32 = 1407;

/// (texture, bar (0 left, 1 right), row (top), Switch button or pad input).
const GUIDE_LINES: [(usize, usize, u32, &str); 6] = [
    (0, 0, 333, "zl"),
    (1, 0, 561, "minus"),
    (0, 0, 667, "lstick"),
    (0, 1, 558, "plus"),
    (0, 1, 713, "a"),
    (0, 1, 1225, "rstick"),
];

/// The groups a line frames, when its pad input is one of them: the face
/// buttons, then the d-pad.
const GUIDE_GROUPS: [[&str; 4]; 2] = [
    ["north", "west", "east", "south"],
    ["up", "left", "right", "down"],
];

/// The face button whose line frames the face buttons on every pad input.
const GUIDE_FACE: &str = "a";

/// The router: a grid of `GUIDE_GRID` pixels; a bend costs `GUIDE_BEND`
/// pixels of length, crossing a line that stays `GUIDE_CROSS`, coming to a
/// part other than from above `GUIDE_SIDE`; a line keeps `GUIDE_CLEAR` pixels
/// off the other parts (the sticks half of it).
const GUIDE_GRID: i32 = 8;
const GUIDE_BEND: u64 = 64;
const GUIDE_CROSS: u64 = 40;
const GUIDE_SIDE: u64 = 48;
const GUIDE_CLEAR: f64 = 16.0;

const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

pub type Bbox = (u32, u32, u32, u32);

/// A layer of a leader-line texture, in `textures.bin`'s order: the lines
/// that stay, or one Switch button's line for one pad input over them.
pub enum Layer {
    Keep {
        texture: usize,
        mask: Mask,
        bbox: Option<Bbox>,
    },
    Over {
        texture: usize,
        button: u8,
        pad: u8,
        mask: Mask,
    },
}

fn round(value: f64) -> f64 {
    value.round_ties_even()
}

fn switch_index(name: &str) -> Option<usize> {
    faces::SWITCH_NAMES
        .iter()
        .position(|candidate| *candidate == name)
}

/// A control's place in the lines' pixels: (`guide_at`).
pub fn at(x: f64, y: f64, r: f64) -> (f64, f64, f64) {
    (
        x * GUIDE_SCALE + GUIDE_AT.0,
        y * GUIDE_SCALE + GUIDE_AT.1,
        r * GUIDE_SCALE,
    )
}

/// A group's frame: its centre and its corners' distance from it, in the
/// lines' pixels (`guide_frame_at`).
pub fn frame_at(where_: &Parts, group: &[&str; 4]) -> (f64, f64, f64) {
    let points: Vec<(f64, f64, f64)> = group
        .iter()
        .filter_map(|pad| where_.get(pad).copied())
        .map(|part| match part {
            Part::Circle { x, y, r } => at(x, y, r),
            Part::Box { x0, y0, .. } => at(x0, y0, 0.0),
        })
        .collect();
    let cx = points.iter().map(|p| p.0).sum::<f64>() / 4.0;
    let cy = points.iter().map(|p| p.1).sum::<f64>() / 4.0;
    let e = points
        .iter()
        .map(|(x, y, r)| (x - cx).abs().max((y - cy).abs()) + r)
        .fold(f64::NEG_INFINITY, f64::max)
        + 8.0;
    (cx, cy, e)
}

/// A way's points with every leg level or upright (`_square`).
pub fn square(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = vec![points[0]];
    for (x, y) in &points[1..] {
        let (px, py) = *out.last().unwrap();
        if (*x, *y) == (px, py) {
            continue;
        }
        if *x != px && *y != py {
            out.push(if out.len() > 1 && out[out.len() - 2].0 == px {
                (px, *y)
            } else {
                (*x, py)
            });
        }
        out.push((*x, *y));
    }
    out
}

/// The way of a line from a bar's row to a pad input's part, round the
/// picture's other parts, with the fewest bends, crossing the lines that stay
/// as little as it can (`guide_route`). It comes to a stick's centre, to a
/// part's edge (from above best), or to a frame's corner (`frame`: centre,
/// corners' distance).
pub fn route(
    size: (u32, u32),
    bar: usize,
    row: u32,
    where_: &Parts,
    pad: &str,
    stays: &Mask,
    frame: Option<(f64, f64, f64)>,
) -> Option<Vec<(f64, f64)>> {
    let g = GUIDE_GRID;
    let x0 = if bar == 0 { BAR_LEFT } else { BAR_RIGHT };
    let y0 = row as i32 + 3;
    let span = (BAR_RIGHT - BAR_LEFT) / g;
    let (ilo, ihi) = if bar == 0 { (0, span) } else { (-span, 0) };
    let jlo = -((y0 - 250).div_euclid(g));
    let top = i32::try_from(size.1.saturating_sub(20).min(1420)).ok()?;
    let jhi = (top - y0).div_euclid(g);
    let nx = usize::try_from(ihi - ilo + 1).ok()?;
    let ny = usize::try_from(jhi - jlo + 1).ok()?;
    if nx == 0 || ny == 0 || !(ilo <= 0 && 0 <= ihi && jlo <= 0 && 0 <= jhi) {
        return None;
    }
    let index = |i: i32, j: i32| (i - ilo) as usize * ny + (j - jlo) as usize;
    let gx = |i: i32| f64::from(x0 + g * i);
    let gy = |j: i32| f64::from(y0 + g * j);
    let y0f = f64::from(y0);

    let own: Vec<&str> = match frame {
        Some(_) => GUIDE_GROUPS
            .iter()
            .find(|group| group.contains(&pad))
            .map(|group| group.to_vec())
            .unwrap_or_default(),
        None => vec![pad],
    };
    let mut blocked = vec![false; nx * ny];
    for (name, part) in where_ {
        match *part {
            Part::Box { x0, y0, x1, y1 } => {
                let (bx0, by0, _) = at(x0, y0, 0.0);
                let (bx1, by1, _) = at(x1, y1, 0.0);
                let margin = GUIDE_CLEAR / 2.0;
                for i in ilo..=ihi {
                    for j in jlo..=jhi {
                        let (x, y) = (gx(i), gy(j));
                        if x > bx0 - margin
                            && x < bx1 + margin
                            && y > by0 - margin
                            && y < by1 + margin
                        {
                            blocked[index(i, j)] = true;
                        }
                    }
                }
            }
            Part::Circle { x, y, r } => {
                if own.contains(&*name) {
                    continue;
                }
                let (x, y, r) = at(x, y, r);
                let clear = if *name == "lstick" || *name == "rstick" {
                    GUIDE_CLEAR / 2.0
                } else {
                    GUIDE_CLEAR
                };
                let reach = (r + clear) * (r + clear);
                for i in ilo..=ihi {
                    for j in jlo..=jhi {
                        let (px, py) = (gx(i) - x, gy(j) - y);
                        if px * px + py * py < reach {
                            blocked[index(i, j)] = true;
                        }
                    }
                }
            }
        }
    }
    for group in &GUIDE_GROUPS {
        if !group.iter().all(|part| where_.contains_key(part)) {
            continue;
        }
        let (cx, cy, e) = frame_at(where_, group);
        if frame.is_some() && group.contains(&pad) {
            let reach = (e - 8.0) * (e - 8.0);
            for i in ilo..=ihi {
                for j in jlo..=jhi {
                    let (px, py) = (gx(i) - cx, gy(j) - cy);
                    if px * px + py * py < reach {
                        blocked[index(i, j)] = true;
                    }
                }
            }
        } else {
            let reach = 1.15 * e + GUIDE_CLEAR;
            for i in ilo..=ihi {
                for j in jlo..=jhi {
                    if (gx(i) - cx).abs() + (gy(j) - cy).abs() < reach {
                        blocked[index(i, j)] = true;
                    }
                }
            }
        }
    }
    for i in ilo..=ihi {
        for j in jlo..=jhi {
            let (x, y) = (gx(i), gy(j));
            if ((x - f64::from(BAR_LEFT)).abs() < 14.0 || (x - f64::from(BAR_RIGHT)).abs() < 14.0)
                && y != y0f
            {
                blocked[index(i, j)] = true;
            }
        }
    }

    let stays = stays.max_filter(4);
    let pen = |i: i32, j: i32| -> u64 {
        let x = gx(i).clamp(0.0, f64::from(size.0.saturating_sub(1))) as u32;
        let y = gy(j).clamp(0.0, f64::from(size.1.saturating_sub(1))) as u32;
        if stays.at(x, y) > 0 { GUIDE_CROSS } else { 0 }
    };

    let mut goals: BTreeMap<(i32, i32, usize), ((f64, f64), u64)> = BTreeMap::new();
    let mut goal = |ex: f64, ey: f64, d: usize, extra: u64| {
        let i = round((ex - f64::from(x0)) / f64::from(g)) as i32 - 2 * DIRS[d].0;
        let j = round((ey - f64::from(y0)) / f64::from(g)) as i32 - 2 * DIRS[d].1;
        for k in 0..2 {
            let (ci, cj) = (i + k * DIRS[d].0, j + k * DIRS[d].1);
            if ci < ilo || ci > ihi || cj < jlo || cj > jhi || blocked[index(ci, cj)] {
                return;
            }
        }
        goals.insert((i, j, d), ((ex, ey), extra));
    };

    let Part::Circle {
        x: px,
        y: py,
        r: pr,
    } = *where_.get(pad)?
    else {
        return None;
    };
    let (x, y, r) = at(px, py, pr);
    if let Some((cx, cy, e)) = frame {
        for k in 0..2 {
            let sy = f64::from(y0)
                + f64::from(g) * ((cy - f64::from(y0)) / f64::from(g)).floor()
                + f64::from(g * k)
                - cy;
            let sx = f64::from(x0)
                + f64::from(g) * ((cx - f64::from(x0)) / f64::from(g)).floor()
                + f64::from(g * k)
                - cx;
            goal(cx + e - sy.abs(), cy + sy, 1, 0);
            goal(cx + sx, cy - e + sx.abs(), 2, GUIDE_SIDE);
            goal(cx - e + sy.abs(), cy + sy, 0, GUIDE_SIDE);
            goal(cx + sx, cy + e - sx.abs(), 3, GUIDE_SIDE);
        }
    } else if pad == "lstick" || pad == "rstick" {
        for d in 0..4 {
            goal(x, y, d, 0);
        }
    } else {
        let e = (r - 2.0).max(0.0);
        let candidates = [
            ((x, y - e), 2, 0),
            ((x, y + e), 3, GUIDE_SIDE),
            ((x - e, y), 0, GUIDE_SIDE),
            ((x + e, y), 1, GUIDE_SIDE),
        ];
        for ((ex, ey), d, extra) in candidates {
            if r != 0.0 || d >= 2 {
                goal(ex, ey, d, extra);
            }
        }
    }

    let start = (0, 0, if bar == 0 { 0 } else { 1 });
    let states = nx * ny * 4;
    let mut best = vec![u64::MAX; states];
    let mut came: Vec<Option<(i32, i32, usize)>> = vec![None; states];
    let start_index = index(0, 0) * 4 + start.2;
    best[start_index] = 0;
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0u64, 0i32, 0i32, start.2 as u8)));
    while let Some(Reverse((cost, i, j, d))) = heap.pop() {
        let d = usize::from(d);
        let state_index = index(i, j) * 4 + d;
        if cost > best[state_index] {
            continue;
        }
        if let Some(((ex, ey), _)) = goals.get(&(i, j, d)) {
            let mut way = Vec::new();
            let mut at_state = Some((i, j, d));
            while let Some((ki, kj, kd)) = at_state {
                way.push((ki, kj, kd));
                at_state = came[index(ki, kj) * 4 + kd];
            }
            way.reverse();
            let mut points = vec![(gx(way[0].0), gy(way[0].1))];
            for pair in way.windows(2) {
                if pair[0].2 != pair[1].2 {
                    points.push((gx(pair[0].0), gy(pair[0].1)));
                }
            }
            points.push(if points.len() > 1 || (ey - y0f).abs() > f64::from(g) {
                (*ex, *ey)
            } else {
                (*ex, y0f)
            });
            if points.len() > 2 {
                let (cx_, cy_) = points[points.len() - 2];
                let last = if DIRS[d].0 != 0 {
                    (cx_, *ey)
                } else {
                    (*ex, cy_)
                };
                let before = points.len() - 2;
                points[before] = last;
            }
            return Some(square(&points));
        }
        for nd in 0..4 {
            if nd == (d ^ 1) || (nd != d && j == 0 && i.abs() < 5) {
                continue;
            }
            let (ni, nj) = (i + DIRS[nd].0, j + DIRS[nd].1);
            if ni < ilo || ni > ihi || nj < jlo || nj > jhi {
                continue;
            }
            let next = index(ni, nj);
            if blocked[next] {
                continue;
            }
            let mut next_cost =
                cost + GUIDE_GRID as u64 + pen(ni, nj) + if nd != d { GUIDE_BEND } else { 0 };
            if let Some((_, extra)) = goals.get(&(ni, nj, nd)) {
                next_cost += extra;
            }
            let next_index = next * 4 + nd;
            if next_cost < best[next_index] {
                best[next_index] = next_cost;
                came[next_index] = Some((i, j, d));
                heap.push(Reverse((next_cost, ni, nj, nd as u8)));
            }
        }
    }
    None
}

fn rectangle(mask: &mut Mask, x0: i64, y0: i64, x1: i64, y1: i64) {
    for y in y0.max(0)..=y1.min(i64::from(mask.height) - 1) {
        for x in x0.max(0)..=x1.min(i64::from(mask.width) - 1) {
            mask.set(x as u32, y as u32, 255);
        }
    }
}

fn polygon(mask: &mut Mask, points: &[(f64, f64)]) {
    if points.len() < 3 {
        return;
    }
    // (Pillow's polygon takes integer vertices: Python's float coordinates
    // are truncated by the C binding)
    let points: Vec<(i64, i64)> = points.iter().map(|p| (p.0 as i64, p.1 as i64)).collect();
    let mut min_y = i64::MAX;
    let mut max_y = i64::MIN;
    for (_, y) in &points {
        min_y = min_y.min(*y);
        max_y = max_y.max(*y);
    }
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        if a.1 == b.1 {
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            for x in x0.max(0)..=x1.min(i64::from(mask.width) - 1) {
                if a.1 >= 0 && a.1 < i64::from(mask.height) {
                    mask.set(x as u32, a.1 as u32, 255);
                }
            }
        }
    }
    for y in min_y.max(0)..=max_y.min(i64::from(mask.height) - 1) {
        let mut crossings = Vec::new();
        for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
            if a.1 == b.1 {
                continue;
            }
            let (ymin, ymax) = (a.1.min(b.1), a.1.max(b.1));
            if y >= ymin && y <= ymax {
                let dx = (b.0 - a.0) as f64 / (b.1 - a.1) as f64;
                let crossing = (y - a.1) as f64 * dx + a.0 as f64;
                crossings.push(crossing);
                if y == ymax && y < max_y {
                    crossings.push(crossing);
                }
            }
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in crossings.chunks_exact(2) {
            let round_up = |value: f64| {
                if value >= 0.0 {
                    (value + 0.5).floor() as i64
                } else {
                    -((-value + 0.5).floor() as i64)
                }
            };
            let round_down = |value: f64| {
                if value >= 0.0 {
                    (value - 0.5).ceil() as i64
                } else {
                    -((-value - 0.5).ceil() as i64)
                }
            };
            let x0 = round_up(pair[0]);
            let x1 = round_down(pair[1]);
            for x in x0.max(0)..=x1.min(i64::from(mask.width) - 1) {
                if x0 <= x1 {
                    mask.set(x as u32, y as u32, 255);
                }
            }
        }
    }
}

/// A line along its way (6 pixels thick, square corners), ending on a dot
/// (`guide_draw`).
pub fn draw(size: (u32, u32), way: &[(f64, f64)], dot: bool) -> Mask {
    let mut im = Mask::new(size.0, size.1);
    for pair in way.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let x0 = round(a.0.min(b.0));
        let x1 = round(a.0.max(b.0));
        let y0 = round(a.1.min(b.1));
        let y1 = round(a.1.max(b.1));
        rectangle(
            &mut im,
            x0 as i64 - 3,
            y0 as i64 - 3,
            x1 as i64 + 2,
            y1 as i64 + 2,
        );
    }
    if dot {
        let (last_x, last_y) = *way.last().unwrap();
        let (cx, cy) = (round(last_x), round(last_y));
        let radius = 11.0;
        let mut points = Vec::new();
        let mut angle: f64 = 270.0 - 0.5 * 72.0 + 18.0;
        for _ in 0..5 {
            let radians = (360.0 - angle).to_radians();
            let x = round((radius * radians.cos() + cx) * 100.0) / 100.0;
            let y = round((radius * radians.sin() + cy) * 100.0) / 100.0;
            points.push((x, y));
            angle += 72.0;
            if angle > 360.0 {
                angle -= 360.0;
            }
        }
        polygon(&mut im, &points);
    }
    im
}

/// A leader line from a bar's row to the pad input's part, ending on a dot
/// (`guide_line`).
pub fn line(
    size: (u32, u32),
    bar: usize,
    row: u32,
    where_: &Parts,
    pad: &str,
    stays: &Mask,
) -> Option<Mask> {
    if let Some(way) = route(size, bar, row, where_, pad, stays, None) {
        return Some(draw(size, &way, true));
    }
    let Part::Circle { x, y, r } = *where_.get(pad)? else {
        return None;
    };
    let (x, y, r) = at(x, y, r);
    let edge = if pad == "lstick" || pad == "rstick" {
        0.0
    } else {
        (r - 2.0).max(0.0)
    };
    let y = if f64::from(row) < y {
        y - edge
    } else {
        y + edge
    };
    let way = [
        (
            if bar == 0 {
                f64::from(BAR_LEFT)
            } else {
                f64::from(BAR_RIGHT)
            },
            f64::from(row) + 3.0,
        ),
        (x, f64::from(row) + 3.0),
        (x, y),
    ];
    Some(draw(size, &way, true))
}

/// A group's frame shape alone: a rounded square on its corner, its tips at
/// `e` from the centre.
fn frame_shape(size: (u32, u32), cx: f64, cy: f64, e: f64) -> Mask {
    let round_ = 0.36 * e;
    let a = (e + (std::f64::consts::SQRT_2 - 1.0) * round_) / std::f64::consts::SQRT_2;
    let mut im = Mask::new(size.0, size.1);
    let steps = 4u32;
    let x0 = (cx - e - 2.0).floor().max(0.0) as u32;
    let y0 = (cy - e - 2.0).floor().max(0.0) as u32;
    let x1 = ((cx + e + 2.0).ceil() as u32).min(size.0.saturating_sub(1));
    let y1 = ((cy + e + 2.0).ceil() as u32).min(size.1.saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let mut inside = 0u32;
            for sy in 0..steps {
                for sx in 0..steps {
                    let px = f64::from(x) + (f64::from(sx) + 0.5) / f64::from(steps);
                    let py = f64::from(y) + (f64::from(sy) + 0.5) / f64::from(steps);
                    let (u, v) = (px - cx, py - cy);
                    let u = (u + v) / std::f64::consts::SQRT_2;
                    let v = (cx - px + py - cy) / std::f64::consts::SQRT_2;
                    let (qx, qy) = (u.abs() - (a - round_), v.abs() - (a - round_));
                    let distance = if qx <= 0.0 && qy <= 0.0 {
                        qx.max(qy)
                    } else {
                        (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt()
                    };
                    if distance >= round_ - 6.0 && distance <= round_ {
                        inside += 1;
                    }
                }
            }
            if inside > 0 {
                im.set(x, y, (inside * 255 / (steps * steps)) as u8);
            }
        }
    }
    im
}

/// A group's frame (a rounded square on its corner, round its parts) and its
/// line from a bar's row (`guide_frame`). The frame is drawn from its
/// geometry directly (4x4 supersampled) rather than Pillow's rotate and
/// downscale; its centreline is the same rounded diamond.
pub fn frame(
    size: (u32, u32),
    bar: usize,
    row: u32,
    where_: &Parts,
    group: &[&str; 4],
    stays: &Mask,
) -> Mask {
    let (cx, cy, e) = frame_at(where_, group);
    let im = frame_shape(size, cx, cy, e);
    let way = route(
        size,
        bar,
        row,
        where_,
        group[0],
        stays,
        Some((cx, cy, e - 3.0)),
    )
    .unwrap_or_else(|| {
        vec![
            (
                if bar == 0 {
                    f64::from(BAR_LEFT)
                } else {
                    f64::from(BAR_RIGHT)
                },
                f64::from(row) + 3.0,
            ),
            (
                if bar != 0 { cx + e - 3.0 } else { cx - e + 3.0 },
                f64::from(row) + 3.0,
            ),
        ]
    });
    im.lighter(&draw(size, &way, false))
}

/// Where the game's line from `bar` at `row` is drawn in a guide texture: the
/// shape it makes between the bars, a little wider for its soft edge
/// (`game_line`).
pub fn game_line(im: &Mask, bar: usize, row: u32) -> Mask {
    let (w, h) = (im.width, im.height);
    let x0 = u32::try_from(BAR_LEFT + 3).unwrap_or(0);
    let x1 = u32::try_from(BAR_RIGHT - 3).unwrap_or(0);
    let mut inner = Mask::new(w, h);
    for y in 0..h {
        for x in x0..x1.min(w) {
            if im.at(x, y) > 24 {
                inner.set(x, y, 255);
            }
        }
    }
    let x = if bar == 0 {
        x0 + 2
    } else {
        x1.saturating_sub(3)
    };
    let start = row.saturating_sub(12);
    let end = (row + 12).min(h.saturating_sub(1));
    let mut seed = None;
    let mut best = u32::MAX;
    for y in start..=end {
        if inner.at(x.min(w.saturating_sub(1)), y) != 0 {
            let distance = y.abs_diff(row);
            if distance < best {
                best = distance;
                seed = Some((x.min(w.saturating_sub(1)), y));
            }
        }
    }
    let Some((sx, sy)) = seed else {
        return Mask::new(w, h);
    };
    let background = inner.at(sx, sy);
    let mut queue = std::collections::VecDeque::new();
    inner.set(sx, sy, 128);
    queue.push_back((sx, sy));
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            let (nx, ny) = (nx as u32, ny as u32);
            if inner.at(nx, ny) == background {
                inner.set(nx, ny, 128);
                queue.push_back((nx, ny));
            }
        }
    }
    let mut lit = Mask::new(w, h);
    for y in 0..h {
        for x in 0..w {
            if inner.at(x, y) == 128 {
                lit.set(x, y, 255);
            }
        }
    }
    let filtered = lit.max_filter(3);
    let mut out = Mask::new(w, h);
    let left = u32::try_from(BAR_LEFT + 1).unwrap_or(0);
    let right = u32::try_from(BAR_RIGHT).unwrap_or(w).min(w);
    for y in 0..h {
        for x in left..right {
            out.set(x, y, filtered.at(x, y));
        }
    }
    out
}

/// The page's leader lines for a face's picture: each texture's lines that
/// stay, then each Switch button's line for each pad input, in
/// `textures.bin`'s order (`guide_layers_of`).
pub fn layers(game: [&Mask; 2], face: &str) -> Vec<Layer> {
    let Some(where_) = faces::parts(face) else {
        return Vec::new();
    };
    layers_with(game, face, &where_)
}

fn layers_with(game: [&Mask; 2], face: &str, where_: &Parts) -> Vec<Layer> {
    let (w, h) = (game[0].width, game[0].height);
    let size = (w, h);
    let mut keep = [Mask::new(w, h), Mask::new(w, h)];
    if face == "switch" {
        keep[0] = game[0].clone();
        keep[1] = game[1].clone();
    } else {
        for (tx, mask) in keep.iter_mut().enumerate() {
            for y in 0..h {
                for x in 0..w {
                    if (x as i32) < BAR_LEFT || (x as i32) >= BAR_RIGHT {
                        mask.set(x, y, game[tx].at(x, y));
                    }
                }
            }
        }
    }
    for &(tx, bar, row, end) in &GUIDE_LINES {
        if switch_index(end).is_none() && face != "switch" {
            if let Some(line) = line(size, bar, row, &where_, end, &keep[tx]) {
                keep[tx] = keep[tx].lighter(&line);
            }
        }
    }

    let mut moving: [Vec<(&'static str, Vec<(usize, Mask, usize)>)>; 2] = [Vec::new(), Vec::new()];
    for &(tx, bar, row, end) in &GUIDE_LINES {
        if switch_index(end).is_none() {
            continue;
        }
        let mut frames: Vec<(&'static str, Mask, usize)> = Vec::new();
        let mut next_id = 0usize;
        for group in &GUIDE_GROUPS {
            if !group.iter().all(|part| where_.contains_key(part)) {
                continue;
            }
            let im = frame(size, bar, row, &where_, group, &keep[tx]);
            for part in group {
                frames.push((part, im.clone(), next_id));
            }
            next_id += 1;
        }
        let mut lines: Vec<(usize, Mask, usize)> = Vec::new();
        for (pad_index, pad) in faces::PADS.iter().enumerate() {
            if !where_.contains_key(pad) {
                continue;
            }
            if let Some((_, im, id)) = frames.iter().find(|(part, _, _)| part == pad) {
                lines.push((pad_index, im.clone(), *id));
            } else if end == GUIDE_FACE {
                if let Some((_, im, id)) = frames.iter().find(|(part, _, _)| *part == "south") {
                    lines.push((pad_index, im.clone(), *id));
                } else if let Some(im) = line(size, bar, row, &where_, pad, &keep[tx]) {
                    lines.push((pad_index, im, next_id));
                    next_id += 1;
                }
            } else if let Some(im) = line(size, bar, row, &where_, pad, &keep[tx]) {
                lines.push((pad_index, im, next_id));
                next_id += 1;
            }
        }
        if face == "switch" {
            let index = switch_index(end).unwrap_or(0);
            let default = faces::DEFAULT[index];
            if let Some(own) = lines
                .iter()
                .find(|(pad_index, _, _)| faces::PADS[*pad_index] == default)
                .map(|(_, _, id)| *id)
            {
                let around = game_line(game[tx], bar, row);
                let game_own = game[tx].multiply(&around);
                for (_, im, id) in &mut lines {
                    if *id == own {
                        *im = game_own.clone();
                    }
                }
                keep[tx] = keep[tx].subtract(&around);
            }
        }
        if !lines.is_empty() {
            moving[tx].push((end, lines));
        }
    }

    let mut out = Vec::new();
    for (tx, entries) in moving.iter().enumerate() {
        let mut boxed = Mask::new(w, h);
        boxed = boxed.lighter(game[tx]);
        boxed = boxed.lighter(&keep[tx]);
        for (_, lines) in entries {
            for (_, im, _) in lines {
                boxed = boxed.lighter(im);
            }
        }
        out.push(Layer::Keep {
            texture: tx,
            mask: keep[tx].clone(),
            bbox: boxed.bbox(),
        });
        for (end, lines) in entries {
            for (pad_index, im, _) in lines {
                out.push(Layer::Over {
                    texture: tx,
                    button: switch_index(end).unwrap_or(0) as u8,
                    pad: *pad_index as u8,
                    mask: im.clone(),
                });
            }
        }
    }
    out
}

/// A BC4 texture as an L mask (Pillow's `"bcn", 4`, the standard UNORM
/// levels).
pub fn decode_bc4(bytes: &[u8], texture: &Texture) -> Mask {
    let blocks = bntx::blocks_linear(bytes, texture);
    let (bw, bh) = texture.blocks();
    let mut out = Mask::new(texture.width, texture.height);
    for by in 0..bh {
        for bx in 0..bw {
            let at = ((by * bw + bx) as usize) * 8;
            let Some(block) = blocks.get(at..at + 8) else {
                continue;
            };
            let (r0, r1) = (block[0], block[1]);
            let mut levels = [0u8; 8];
            levels[0] = r0;
            levels[1] = r1;
            if r0 > r1 {
                for (i, level) in levels.iter_mut().enumerate().take(8).skip(2) {
                    let a = 8 - i as u32;
                    let b = i as u32 - 1;
                    *level = ((a * u32::from(r0) + b * u32::from(r1)) / 7) as u8;
                }
            } else {
                for (i, level) in levels.iter_mut().enumerate().take(6).skip(2) {
                    let a = 6 - i as u32;
                    let b = i as u32 - 1;
                    *level = ((a * u32::from(r0) + b * u32::from(r1)) / 5) as u8;
                }
                levels[6] = 0;
                levels[7] = 255;
            }
            let mut index = 0u64;
            for (i, byte) in block[2..8].iter().enumerate() {
                index |= u64::from(*byte) << (8 * i);
            }
            for y in 0..4u32 {
                for x in 0..4u32 {
                    let px = bx * 4 + x;
                    let py = by * 4 + y;
                    if px < texture.width && py < texture.height {
                        let code = ((index >> (3 * (y * 4 + x))) & 7) as usize;
                        out.set(px, py, levels[code]);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(x: f64, y: f64, r: f64) -> Part {
        Part::Circle { x, y, r }
    }

    fn nonzero(mask: &Mask) -> usize {
        mask.data.iter().filter(|value| **value != 0).count()
    }

    fn fnv(data: &[u8]) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in data {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    fn rect(mask: &mut Mask, x0: u32, y0: u32, x1: u32, y1: u32, value: u8) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x < mask.width && y < mask.height {
                    mask.set(x, y, value);
                }
            }
        }
    }

    fn assert_way(actual: &[(f64, f64)], expected: &[(f64, f64)]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} != {expected:?}");
        for (a, b) in actual.iter().zip(expected) {
            assert!(
                (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9,
                "{actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn a_part_moves_into_the_lines_pixels() {
        assert_eq!(
            at(348.0, 227.0, 48.0),
            (
                348.0 * 1.1136 + 131.3,
                227.0 * 1.1136 + 462.6,
                48.0 * 1.1136
            )
        );
        let parts: Parts = [
            ("north", circle(800.0, 168.0, 32.0)),
            ("west", circle(731.0, 227.0, 32.0)),
            ("east", circle(869.0, 227.0, 32.0)),
            ("south", circle(800.0, 286.0, 32.0)),
        ]
        .into_iter()
        .collect();
        let (cx, cy, e) = frame_at(&parts, &GUIDE_GROUPS[0]);
        assert!((cx - (800.0 * 1.1136 + 131.3)).abs() < 1e-9);
        assert!((cy - (227.0 * 1.1136 + 462.6)).abs() < 1e-9);
        // (the farthest corner plus its radius, then the game's 8 pixels)
        let farthest = (69.0_f64 * 1.1136).max(59.0 * 1.1136) + 32.0 * 1.1136;
        assert!((e - (farthest + 8.0)).abs() < 1e-9);

        let squared = square(&[(10.0, 10.0), (50.0, 50.0), (90.0, 50.0)]);
        assert_eq!(
            squared,
            vec![(10.0, 10.0), (50.0, 10.0), (50.0, 50.0), (90.0, 50.0)]
        );
    }

    #[test]
    fn every_face_has_every_pad_input_and_sane_parts() {
        let boxes = [
            ("switch", 0),
            ("xbox", 1),
            ("ps4", 1),
            ("ps5", 2),
            ("steamdeck", 4),
        ];
        for (face, boxes) in boxes {
            let parts = faces::parts(face).unwrap();
            assert_eq!(parts.len(), faces::PADS.len() + boxes, "{face}");
            for pad in faces::PADS {
                let part = parts.get(pad).unwrap_or_else(|| panic!("{face}: {pad}"));
                match part {
                    Part::Circle { x, y, r } => {
                        assert!(
                            x.is_finite() && y.is_finite() && r.is_finite(),
                            "{face}: {pad}"
                        );
                        assert!(*r >= 0.0, "{face}: {pad}");
                    }
                    Part::Box { .. } => panic!("{face}: {pad} is not a control"),
                }
            }
            for (name, part) in &parts {
                if let Part::Box { x0, y0, x1, y1 } = part {
                    assert!(x0 < x1 && y0 < y1, "{face}: {name}");
                }
            }
            let (cx, cy, e) = frame_at(&parts, &GUIDE_GROUPS[0]);
            assert!(cx.is_finite() && cy.is_finite() && e > 0.0, "{face}");
        }
        let switch = faces::parts("switch").unwrap();
        assert_eq!(switch["lstick"], circle(348.0, 227.0, 48.0));
        assert_eq!(switch["east"], circle(869.0, 227.0, 32.0));
        assert_eq!(switch["ltrigger"], circle(375.0, 39.0, 0.0));
        assert_eq!(
            faces::parts("xbox").unwrap()["share"],
            Part::Box {
                x0: 558.0,
                y0: 243.0,
                x1: 598.0,
                y1: 283.0
            }
        );
        assert_eq!(
            faces::parts("ps4").unwrap()["touchpad"],
            Part::Box {
                x0: 446.0,
                y0: 117.0,
                x1: 712.0,
                y1: 263.0
            }
        );
        assert!(matches!(
            faces::parts("ps5").unwrap()["mute"],
            Part::Box { .. }
        ));
        assert!(matches!(
            faces::parts("steamdeck").unwrap()["screen"],
            Part::Box { .. }
        ));
    }

    #[test]
    fn the_router_and_drawer_match_the_reference_fixture() {
        let where_ = faces::parts("switch").unwrap();
        let size = (1600, 1000);
        let mut stays = Mask::new(size.0, size.1);
        rect(&mut stays, 600, 500, 900, 506, 255);
        let way = route(size, 0, 333, &where_, "lstick", &stays, None).unwrap();
        assert_way(
            &way,
            &[
                (147.0, 336.0),
                (518.8327999999999, 336.0),
                (518.8327999999999, 715.3872),
            ],
        );
        assert!(
            way.windows(2)
                .all(|pair| { pair[0].0 == pair[1].0 || pair[0].1 == pair[1].1 })
        );
        let im = draw(size, &way, true);
        assert_eq!(im.bbox(), Some((144, 333, 528, 726)));
        assert_eq!(nonzero(&im), 4766);

        let simple = draw(
            (160, 120),
            &[(10.0, 20.0), (110.0, 20.0), (110.0, 80.0)],
            true,
        );
        assert_eq!(simple.bbox(), Some((7, 17, 119, 91)));
        assert_eq!(nonzero(&simple), 1220);

        let (cx, cy, e) = frame_at(&where_, &GUIDE_GROUPS[0]);
        assert!((cx - 1022.18).abs() < 1e-9);
        assert!((cy - 715.3872).abs() < 1e-9);
        assert!((e - 120.4736).abs() < 1e-9);
        let framed = route(
            size,
            1,
            713,
            &where_,
            "north",
            &stays,
            Some((cx, cy, e - 3.0)),
        )
        .unwrap();
        assert_way(&framed, &[(1407.0, 716.0), (1139.0408, 716.0)]);
    }

    #[test]
    fn the_games_line_is_taken_out_where_it_runs() {
        let size = (1600, 1000);
        let mut game = Mask::new(size.0, size.1);
        rect(&mut game, 147, 330, 700, 336, 255);
        rect(&mut game, 694, 330, 700, 600, 255);
        rect(&mut game, 1407, 330, 1450, 336, 255);
        let line = game_line(&game, 0, 333);
        assert_eq!(line.bbox(), Some((148, 327, 704, 604)));
        assert_eq!(nonzero(&line), 10660);
    }

    fn synthetic() -> Parts {
        [
            ("lstick", circle(348.0, 227.0, 48.0)),
            ("rstick", circle(683.0, 348.0, 48.0)),
            ("back", circle(477.0, 161.0, 19.0)),
            ("start", circle(682.0, 161.0, 19.0)),
            ("east", circle(869.0, 227.0, 32.0)),
            ("ltrigger", circle(375.0, 39.0, 0.0)),
        ]
        .into_iter()
        .collect()
    }

    fn synthetic_game(which: usize) -> Mask {
        let mut im = Mask::new(1600, 800);
        rect(&mut im, 0, 100, 146, 700, 180);
        rect(&mut im, 700, 100, 1599, 700, 90);
        if which == 0 {
            rect(&mut im, 147, 330, 700, 336, 255);
            rect(&mut im, 694, 330, 700, 600, 255);
        } else {
            rect(&mut im, 147, 710, 500, 716, 255);
            rect(&mut im, 494, 710, 500, 799, 255);
            rect(&mut im, 1000, 710, 1407, 716, 255);
        }
        im
    }

    type Expected = (usize, u8, u8, Option<Bbox>, usize, u64);

    /// The reference build's layers for the synthetic picture, hashed: the
    /// game's files are not needed and none are read.
    const XBOX: &[Expected] = &[
        (
            0,
            0xff,
            0,
            Some((0, 100, 1600, 800)),
            207084,
            0xffeced0e67a2df91,
        ),
        (
            0,
            8,
            1,
            Some((144, 333, 1108, 693)),
            8048,
            0x17a7634e0e551d89,
        ),
        (
            0,
            8,
            4,
            Some((144, 333, 671, 634)),
            5072,
            0x47643341675c897b,
        ),
        (
            0,
            8,
            6,
            Some((144, 333, 900, 634)),
            6446,
            0x895e6c762033a1f3,
        ),
        (
            0,
            8,
            7,
            Some((144, 333, 528, 726)),
            4763,
            0x68cb05dc6f1d34c8,
        ),
        (
            0,
            8,
            8,
            Some((144, 333, 895, 800)),
            7272,
            0xb6f8d3bc192dc423,
        ),
        (
            0,
            8,
            15,
            Some((144, 333, 558, 517)),
            3692,
            0x1ee53339b2b7839d,
        ),
        (
            0,
            10,
            1,
            Some((1088, 558, 1410, 693)),
            2834,
            0xb3b4288b9b4e125f,
        ),
        (
            0,
            10,
            4,
            Some((651, 558, 1410, 634)),
            5102,
            0x00a691872027c375,
        ),
        (
            0,
            10,
            6,
            Some((880, 558, 1410, 634)),
            3728,
            0xe5dfa4df17964189,
        ),
        (
            0,
            10,
            7,
            Some((508, 558, 1410, 726)),
            6515,
            0x2b50fe8114db2b6c,
        ),
        (
            0,
            10,
            8,
            Some((889, 558, 1410, 800)),
            4542,
            0x5dda7b791e20dced,
        ),
        (
            0,
            10,
            15,
            Some((538, 495, 1410, 564)),
            5738,
            0xc052c7803160dd27,
        ),
        (
            0,
            0,
            1,
            Some((1122, 705, 1410, 727)),
            1907,
            0x2530c8b0b27052e4,
        ),
        (
            0,
            0,
            4,
            Some((651, 593, 1410, 719)),
            5612,
            0x3c245da97be7a15f,
        ),
        (
            0,
            0,
            6,
            Some((899, 631, 1410, 719)),
            3689,
            0x5d12e2b186c31cba,
        ),
        (
            0,
            0,
            7,
            Some((508, 704, 1410, 775)),
            6266,
            0x07b75f43d5e3bba7,
        ),
        (
            0,
            0,
            8,
            Some((889, 713, 1410, 800)),
            3612,
            0xc45e65fe329376fb,
        ),
        (
            0,
            0,
            15,
            Some((538, 481, 1410, 719)),
            6932,
            0x6b479f4fff3015e1,
        ),
        (
            1,
            0xff,
            0,
            Some((0, 100, 1600, 800)),
            204347,
            0x862b2473c445f772,
        ),
        (
            1,
            11,
            1,
            Some((144, 561, 1108, 693)),
            6680,
            0x27d6c8214790b9c1,
        ),
        (
            1,
            11,
            4,
            Some((144, 561, 671, 634)),
            3704,
            0x80253690ca15ef23,
        ),
        (
            1,
            11,
            6,
            Some((144, 561, 900, 634)),
            5078,
            0xb84db3031f56842b,
        ),
        (
            1,
            11,
            7,
            Some((144, 561, 528, 726)),
            3398,
            0xc96d7194b1741cbb,
        ),
        (
            1,
            11,
            8,
            Some((144, 561, 895, 800)),
            5904,
            0x8f3f716885f4870b,
        ),
        (
            1,
            11,
            15,
            Some((144, 495, 558, 567)),
            3020,
            0xa0944e4e2d756cad,
        ),
    ];

    const SWITCH: &[Expected] = &[
        (
            0,
            0xff,
            0,
            Some((0, 100, 1600, 800)),
            204347,
            0x50ed110c7eebb7b2,
        ),
        (
            0,
            8,
            1,
            Some((144, 333, 1074, 726)),
            8039,
            0x0ce52d01cbae696c,
        ),
        (
            0,
            8,
            4,
            Some((144, 333, 652, 653)),
            5069,
            0x2ecabfa1de68c932,
        ),
        (
            0,
            8,
            6,
            Some((144, 333, 900, 715)),
            7286,
            0xec560544bda81d53,
        ),
        (
            0,
            8,
            7,
            Some((144, 333, 528, 726)),
            4763,
            0xe390924d43682848,
        ),
        (
            0,
            8,
            8,
            Some((144, 333, 895, 800)),
            7272,
            0xb6f8d3bc192dc423,
        ),
        (
            0,
            8,
            15,
            Some((148, 100, 1407, 701)),
            430355,
            0xecf7209846aaed0a,
        ),
        (
            0,
            10,
            1,
            Some((1088, 558, 1410, 693)),
            2834,
            0xb3b4288b9b4e125f,
        ),
        (
            0,
            10,
            4,
            Some((651, 558, 1410, 634)),
            5102,
            0x00a691872027c375,
        ),
        (
            0,
            10,
            6,
            Some((148, 100, 1407, 701)),
            430355,
            0xecf7209846aaed0a,
        ),
        (
            0,
            10,
            7,
            Some((508, 558, 1410, 726)),
            6512,
            0x5c15fa093b1d9749,
        ),
        (
            0,
            10,
            8,
            Some((889, 558, 1410, 800)),
            4542,
            0x5dda7b791e20dced,
        ),
        (
            0,
            10,
            15,
            Some((538, 495, 1410, 564)),
            5738,
            0xc052c7803160dd27,
        ),
        (0, 0, 1, None, 0, 0x382065c52df4c325),
        (
            0,
            0,
            4,
            Some((651, 593, 1410, 719)),
            5612,
            0x3c245da97be7a15f,
        ),
        (
            0,
            0,
            6,
            Some((899, 631, 1410, 719)),
            3689,
            0x5d12e2b186c31cba,
        ),
        (
            0,
            0,
            7,
            Some((508, 704, 1410, 775)),
            6266,
            0x07b75f43d5e3bba7,
        ),
        (
            0,
            0,
            8,
            Some((889, 713, 1410, 800)),
            3612,
            0xc45e65fe329376fb,
        ),
        (
            0,
            0,
            15,
            Some((538, 481, 1410, 719)),
            6932,
            0x6b479f4fff3015e1,
        ),
        (
            1,
            0xff,
            0,
            Some((0, 100, 1600, 800)),
            635162,
            0xcb5ff7016e4f1a9e,
        ),
        (
            1,
            11,
            1,
            Some((144, 561, 1108, 775)),
            7358,
            0xd5b0638363ed096b,
        ),
        (1, 11, 4, None, 0, 0x382065c52df4c325),
        (
            1,
            11,
            6,
            Some((144, 561, 900, 711)),
            5870,
            0x1447bd5dc2733f7b,
        ),
        (
            1,
            11,
            7,
            Some((144, 561, 528, 726)),
            3398,
            0xc96d7194b1741cbb,
        ),
        (
            1,
            11,
            8,
            Some((144, 561, 895, 800)),
            5904,
            0x8f3f716885f4870b,
        ),
        (
            1,
            11,
            15,
            Some((144, 495, 558, 567)),
            3020,
            0xa0944e4e2d756cad,
        ),
    ];

    fn check_layers(face: &str, expected: &[Expected]) {
        let game = [synthetic_game(0), synthetic_game(1)];
        let layers = layers_with([&game[0], &game[1]], face, &synthetic());
        assert_eq!(layers.len(), expected.len(), "{face}");
        for (layer, wanted) in layers.iter().zip(expected) {
            let got = match layer {
                Layer::Keep {
                    texture,
                    mask,
                    bbox,
                } => (*texture, 0xff, 0, *bbox, nonzero(mask), fnv(&mask.data)),
                Layer::Over {
                    texture,
                    button,
                    pad,
                    mask,
                } => (
                    *texture,
                    *button,
                    *pad,
                    mask.bbox(),
                    nonzero(mask),
                    fnv(&mask.data),
                ),
            };
            assert_eq!(got, *wanted, "{face}: {got:?}");
        }
    }

    #[test]
    fn layers_keep_the_lines_first_then_draw_each_moving_line_over_them() {
        check_layers("xbox", XBOX);
        check_layers("switch", SWITCH);
    }

    #[test]
    fn the_switch_faces_default_lines_are_the_games_own() {
        let game = [synthetic_game(0), synthetic_game(1)];
        let layers = layers_with([&game[0], &game[1]], "switch", &synthetic());
        let Layer::Keep { mask: keep, .. } = &layers[0] else {
            panic!("the first layer is the lines that stay");
        };
        let around = game_line(&game[0], 0, 333);
        assert_eq!(keep, &game[0].subtract(&around));
        let own = game[0].multiply(&around);
        let Layer::Over {
            button, pad, mask, ..
        } = layers
            .iter()
            .find(|layer| {
                matches!(
                    layer,
                    Layer::Over {
                        button: 8,
                        pad: 15,
                        ..
                    }
                )
            })
            .unwrap()
        else {
            panic!("the ltrigger line of zl");
        };
        assert_eq!((*button, *pad), (8, 15));
        assert_eq!(mask, &own);
    }

    #[test]
    fn a_bc4_texture_decodes_to_its_levels() {
        let texture = Texture {
            name: "ControllerGuide_Pro_00".to_owned(),
            record: 0,
            format: 0,
            width: 4,
            height: 4,
            gobs: 1,
            data: 0,
        };
        let mut bytes = [0u8; 8];
        bytes[0] = 200;
        bytes[1] = 50;
        let mut index = 0u64;
        for i in 0..16 {
            index |= ((i % 8) as u64) << (3 * i);
        }
        bytes[2..].copy_from_slice(&index.to_le_bytes()[..6]);
        let mask = decode_bc4(&bytes, &texture);
        let levels = [200, 50, 178, 157, 135, 114, 92, 71];
        let wanted: Vec<u8> = levels.iter().copied().cycle().take(16).collect();
        assert_eq!(mask.data, wanted);

        bytes[0] = 50;
        bytes[1] = 200;
        let mask = decode_bc4(&bytes, &texture);
        let levels = [50, 200, 80, 110, 140, 170, 0, 255];
        let wanted: Vec<u8> = levels.iter().copied().cycle().take(16).collect();
        assert_eq!(mask.data, wanted);
    }

    #[test]
    fn every_faces_leader_lines_end_on_their_parts_edge() {
        // (as the game's own lines do: 2 pixels into a control's part; the shoulders' point has no size, so its centre)
        let stays = Mask::new(1600, 1000);
        let mut ends = 0;
        let faces_with_guides = std::iter::once("switch").chain(faces::FACES.iter().map(|face| face.name));
        for face in faces_with_guides {
            let where_ = faces::parts(face).unwrap_or_else(|| panic!("{face}"));
            for (bar, row, pads) in [
                (0, 333, ["ltrigger", "lshoulder", "back"]),
                (1, 558, ["rtrigger", "rshoulder", "start"]),
            ] {
                for pad in pads {
                    let way = route((1600, 1000), bar, row, &where_, pad, &stays, None)
                        .unwrap_or_else(|| panic!("{face}: no line from bar {bar} to {pad}"));
                    let part = *where_.get(pad).unwrap();
                    let (x, y, r) = match part {
                        Part::Circle { x, y, r } => at(x, y, r),
                        Part::Box { .. } => panic!("{face}: {pad} is not a control"),
                    };
                    let (ex, ey) = *way.last().unwrap();
                    let off = ((ex - x).powi(2) + (ey - y).powi(2)).sqrt();
                    let wanted = (r - 2.0).max(0.0);
                    assert!(
                        (off - wanted).abs() < 1.0,
                        "{face}: the line to {pad} ends {off:.1} pixels from its centre, its edge is {r:.1} (want {wanted:.1})"
                    );
                    ends += 1;
                }
            }
        }
        assert_eq!(ends, (1 + faces::FACES.len()) * 6);
    }
}
