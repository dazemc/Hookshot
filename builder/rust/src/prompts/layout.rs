//! The two layout patches: the Talk ring's A/B/X/Y places (`ring.bin`) and
//! the ocarina hint's icon spacing (`hint.bin`), found in the game's BFFH
//! layouts as `nx2/prompt_assets.py` finds them.

use std::fs;
use std::path::Path;

pub const LAYOUTS: &str = "UI/Layout2Archive/Common.Product.100.Nin_Ounce_NVN2.blarc.zs";

/// The switch buttons, by name, in HOS_SW_* order.
const SWITCH: [&str; 16] = [
    "a", "b", "x", "y", "ls", "rs", "l", "r", "zl", "zr", "plus", "minus", "left", "up", "right",
    "down",
];

fn sw(name: &str) -> Result<u8, String> {
    SWITCH
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| index as u8)
        .ok_or_else(|| format!("layout: {name} is not a Switch button"))
}

const RING_PLACES: [(&str, (f32, f32)); 4] = [
    ("east", (80.0, 0.0)),
    ("south", (0.0, -80.0)),
    ("north", (0.0, 80.0)),
    ("west", (-80.0, 0.0)),
];

/// (group, (its base pane, the others' letters, its big button))
const RING_GROUPS: [(&str, (usize, [(&str, usize); 3], usize)); 4] = [
    ("a", (5, [("b", 6), ("x", 7), ("y", 8)], 9)),
    ("b", (16, [("a", 17), ("x", 18), ("y", 19)], 20)),
    ("x", (27, [("a", 28), ("b", 29), ("y", 30)], 31)),
    ("y", (38, [("a", 39), ("b", 40), ("x", 41)], 42)),
];

const RING_AT: [(&str, &str); 4] = [("east", "a"), ("south", "b"), ("north", "x"), ("west", "y")];

const HINTS: [&str; 3] = [
    "PaGuideKeyOcarinaInput_00",
    "PaGuideKeyOcarinaInput_01",
    "PaGuideKeyCapture_00",
];
const HINT_PANES: [&str; 6] = [
    "T_Icon_00",
    "T_Icon_01",
    "T_Icon_02",
    "T_Icon_03",
    "T_Icon_04",
    "T_Guide_00",
];
const HINT_FITS: f64 = 720.0;
const HINT_SCALE: f64 = 0.1;

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}
fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}
fn f32_at(data: &[u8], at: usize) -> Option<f32> {
    data.get(at..at + 4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
}

/// The (start, end) of each BFFH layout in the archive that is `name`.
pub fn layout_copies(blarc: &[u8], name: &str) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut at = 0;
    while let Some(found) = find(&blarc[at..], b"BFFH") {
        let start = at + found;
        starts.push(start);
        at = start + 4;
    }
    let needle = format!("\0{name}\0RootPane\0");
    let mut out = Vec::new();
    for (i, start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(blarc.len());
        if start + 1 < end && find(&blarc[*start + 1..end], needle.as_bytes()).is_some() {
            out.push((*start, end));
        }
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn pane(k: usize) -> usize {
    0x968 + k * 0x44 - 0x28
}

/// `ring.bin`'s entries: (group, Switch button placed, place, runs).
pub fn ring_entries(blarc: &[u8]) -> Option<Vec<(u8, u8, u8, Vec<(u32, Vec<u8>)>)>> {
    let copies = layout_copies(blarc, "DoCommand_00")
        .into_iter()
        .map(|(start, _)| start)
        .collect::<Vec<_>>();
    if copies.is_empty() {
        return None;
    }
    let start = copies[0];
    let lay = blarc.get(start..start + 0x2000)?;
    for (_, (base, _, _)) in RING_GROUPS {
        let scales = [pane(base) + 0x2c, pane(base) + 0x30];
        if f32_at(lay, scales[0]) != Some(320.0) || f32_at(lay, scales[1]) != Some(320.0) {
            return None;
        }
    }
    let mut out = Vec::new();
    for (group, (base, small, button)) in RING_GROUPS {
        for (place_index, (place, xy)) in RING_PLACES.iter().enumerate() {
            let target_group = RING_AT
                .iter()
                .find(|(name, _)| name == place)
                .map(|(_, group)| *group)?;
            let (_, (target_base, _, target_button)) =
                RING_GROUPS.iter().find(|(name, _)| *name == target_group)?;
            let mut parts: Vec<(usize, &[u8])> = vec![
                (
                    pane(base) + 0x10,
                    lay.get(pane(*target_base) + 0x10..pane(*target_base) + 0x18)?,
                ),
                (
                    pane(base) + 0x3c,
                    lay.get(pane(*target_base) + 0x3c..pane(*target_base) + 0x40)?,
                ),
                (
                    pane(button) + 0x1c,
                    lay.get(pane(*target_button) + 0x1c..pane(*target_button) + 0x24)?,
                ),
            ];
            for child in 1..=5 {
                let from = pane(*target_button + child);
                parts.push((pane(button + child), lay.get(from..from + 0x24)?));
            }
            let runs = copies
                .iter()
                .flat_map(|copy| {
                    parts
                        .iter()
                        .map(move |(offset, data)| ((*copy + *offset) as u32, data.to_vec()))
                })
                .collect();
            out.push((sw(group).ok()?, sw(group).ok()?, place_index as u8, runs));
            for (other, letter) in small {
                let runs = copies
                    .iter()
                    .map(|copy| {
                        (
                            (*copy + pane(letter) + 0x1c) as u32,
                            [xy.0.to_le_bytes(), xy.1.to_le_bytes()].concat(),
                        )
                    })
                    .collect();
                out.push((sw(group).ok()?, sw(other).ok()?, place_index as u8, runs));
            }
        }
    }
    Some(out)
}

/// The hints' panes in every copy: (offset of an x, slot, x).
pub fn hint_panes(blarc: &[u8]) -> Option<Vec<(u32, u8, f32)>> {
    let mut out = Vec::new();
    for name in HINTS {
        for (a, z) in layout_copies(blarc, name) {
            let lay = &blarc[a..z];
            let panes = layout_panes(lay)?;
            if HINT_PANES.iter().any(|pane| !panes.contains_key(*pane)) {
                return None;
            }
            let mut xs: Vec<(f32, u8)> = Vec::new();
            for (slot, pane) in HINT_PANES.iter().enumerate() {
                let record = panes[*pane];
                let x = f32_at(lay, record + 0x1c)?;
                if !xs.iter().any(|(old, _)| *old == x) {
                    xs.push((x, slot as u8));
                }
                out.push((a as u32 + record as u32 + 0x1c, slot as u8, x));
            }
            let mut at = 0usize;
            while at + 16 < lay.len() {
                let Some(offset) = u64_at(lay, at) else { break };
                let Some(count) = u32_at(lay, at + 8) else {
                    break;
                };
                let Some(size) = u32_at(lay, at + 12) else {
                    break;
                };
                if offset as usize == at + 16
                    && size == 4
                    && (2..=64).contains(&count)
                    && count % 2 == 0
                    && at + 16 + 4 * count as usize <= lay.len()
                {
                    let values = (0..count as usize)
                        .map(|i| f32_at(lay, offset as usize + 4 * i))
                        .collect::<Option<Vec<_>>>();
                    if let Some(values) = values
                        && let Some((_, slot)) = xs.iter().find(|(x, _)| *x == values[0]).copied()
                        && values.iter().step_by(2).all(|v| *v == values[0])
                        && values.iter().skip(1).step_by(2).all(|v| *v == 0.0)
                    {
                        for k in 0..count as usize / 2 {
                            out.push((a as u32 + offset as u32 + 8 * k as u32, slot, values[0]));
                        }
                    }
                }
                at += 8;
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn layout_panes(lay: &[u8]) -> Option<std::collections::BTreeMap<String, usize>> {
    let names = u64_at(lay, 0x118)? as usize;
    let records = u64_at(lay, 0x120)? as usize;
    let count = u32_at(lay, 0x130)? as usize;
    let mut out = std::collections::BTreeMap::new();
    for k in 0..count {
        let name_at = u64_at(lay, names + 8 * k)? as usize;
        let rest = lay.get(name_at..)?;
        let length = rest.iter().position(|b| *b == 0)?;
        let name = std::str::from_utf8(&rest[..length]).ok()?.to_owned();
        let record = u64_at(lay, records + 8 * k)? as usize;
        out.insert(name, record);
    }
    Some(out)
}

/// Write `ring.bin`: "HPRG", the count, then each entry's Switch buttons,
/// place and runs (offset, length, bytes).
pub fn write_ring(
    path: &Path,
    entries: &[(u8, u8, u8, Vec<(u32, Vec<u8>)>)],
) -> Result<(), String> {
    let mut out = Vec::new();
    out.extend_from_slice(b"HPRG");
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (group, placed, place, runs) in entries {
        out.extend_from_slice(&[*group, *placed, *place, 0]);
        out.extend_from_slice(&(runs.len() as u32).to_le_bytes());
        for (offset, data) in runs {
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(data);
        }
    }
    fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write `hint.bin`: "HPHN", each pad input's extra width, then the panes
/// whose x moves.
pub fn write_hint(path: &Path, widths: &[u16], panes: &[(u32, u8, f32)]) -> Result<(), String> {
    let mut out = Vec::new();
    out.extend_from_slice(b"HPHN");
    out.extend_from_slice(&(widths.len() as u32).to_le_bytes());
    for width in widths {
        let value = ((*width as f64 - HINT_FITS).max(0.0) * HINT_SCALE).round_ties_even() as f32;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&(panes.len() as u32).to_le_bytes());
    for (offset, slot, x) in panes {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&[*slot, 0, 0, 0]);
        out.extend_from_slice(&x.to_le_bytes());
    }
    fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_place_letters_are_the_switch_buttons() {
        assert_eq!(sw("a"), Ok(0));
        assert_eq!(sw("minus"), Ok(11));
        assert!(sw("nope").is_err());
    }

    #[test]
    fn a_hint_is_written_with_its_widths_and_panes() {
        let path =
            std::env::temp_dir().join(format!("hookshot-prompts-hint-{}.bin", std::process::id()));
        write_hint(&path, &[720u16; 17], &[(16, 0, 1.5)]).unwrap();
        let data = fs::read(&path).unwrap();
        assert_eq!(&data[..4], b"HPHN");
        assert_eq!(u32_at(&data, 4), Some(17));
        assert_eq!(u32_at(&data, 8), Some(0)); // (720 fits: no extra)
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_ring_is_written_with_its_runs() {
        let path =
            std::env::temp_dir().join(format!("hookshot-prompts-ring-{}.bin", std::process::id()));
        write_ring(&path, &[(0, 1, 2, vec![(3, b"abc".to_vec())])]).unwrap();
        let data = fs::read(&path).unwrap();
        assert_eq!(&data[..4], b"HPRG");
        assert_eq!(u32_at(&data, 4), Some(1));
        assert_eq!(&data[8..12], &[0, 1, 2, 0]);
        let _ = fs::remove_file(path);
    }
}
