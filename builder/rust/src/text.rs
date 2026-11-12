//! Lifted-C text transforms. Ported from nx2/lift.py/a64_lift.py, starting
//! with the frame-pointer store rewrite.

use regex::Regex;
use serde_json::{Value, json};

fn fp_write(text: &str) -> bool {
    if text.contains("&c->x[29]") {
        return true;
    }
    let mut rest = text;
    while let Some(at) = rest.find("c->x[29]") {
        let after = &rest[at + 8..];
        let spaces = after.len() - after.trim_start_matches(' ').len();
        if after.as_bytes().get(spaces) == Some(&b'=')
            && after.as_bytes().get(spaces + 1) != Some(&b'=')
        {
            return true;
        }
        rest = after;
    }
    false
}
fn fp_setup() -> Regex {
    Regex::new(r"\{ uint64_t _a=c->x\[31\], _b=\d+ULL; uint64_t _r=_a\+_b; c->x\[29\]=_r; \}")
        .unwrap()
}
fn fp_restore() -> Regex {
    Regex::new(r"\{ uint64_t _b=c->x\[31\]; int64_t _o=-?\d+; \{ uint64_t _p0,_p1; recomp_ldp64\(c,[^;]*; c->x\[29\]=_p0; c->x\[30\]=_p1; \}").unwrap()
}
fn fp_store_pre() -> Regex {
    Regex::new(r"(\{ uint64_t _b=c->x\[29\]; int64_t _o=-?\d+; )recomp_(store8|store16|store32|store64|stp32|stp64)\(").unwrap()
}
fn fp_store() -> Regex {
    Regex::new(r"recomp_(store8|store16|store32|store64|stp32|stp64)\(c,(\(?)c->x\[29\]").unwrap()
}

/// An instruction's C writes x29 only as frame setup or restore.
pub fn fp_is_frame(text: &str) -> bool {
    if !fp_write(text) {
        return true;
    }
    let rest = fp_restore()
        .replace_all(&fp_setup().replace_all(text, ""), "")
        .into_owned();
    !fp_write(&rest)
}

fn sp_store_pre() -> Regex {
    Regex::new(r"(\{ uint64_t _b=c->x\[31\]; int64_t _o=-?\d+; )recomp_(store8|store16|store32|store64|stp32|stp64)\(").unwrap()
}
fn sp_store() -> Regex {
    Regex::new(r"recomp_(store8|store16|store32|store64|stp32|stp64)\(c,(\(?)c->x\[31\]").unwrap()
}

/// Give SP-based stores the stack variants.
pub fn stack_stores(text: &str) -> String {
    if !text.contains("c->x[31]") {
        return text.to_owned();
    }
    let first = sp_store_pre()
        .replace_all(text, "${1}recomp_${2}_sp(")
        .into_owned();
    sp_store()
        .replace_all(&first, "recomp_${1}_sp(c,${2}c->x[31]")
        .into_owned()
}

/// Guest registers an instruction's C may write (bit n for x[n]).
pub fn reg_writes(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let mut mask = 0u32;
    let mut at = 0usize;
    while let Some(found) = text[at..].find("c->x[") {
        let start = at + found;
        let number_at = start + 5;
        let mut end = number_at;
        while bytes.get(end).is_some_and(|b| b.is_ascii_digit()) {
            end += 1;
        }
        let Ok(number) = text[number_at..end].parse::<u32>() else {
            at = end;
            continue;
        };
        if number >= 32 {
            at = end;
            continue;
        }
        let before = text[..start].trim_end();
        let mut cursor = end;
        if bytes.get(cursor) == Some(&b']') {
            cursor += 1;
        }
        while bytes.get(cursor) == Some(&b' ') {
            cursor += 1;
        }
        let after = &text[cursor..];
        let write = before.ends_with('&')
            || before.ends_with("++")
            || before.ends_with("--")
            || after.starts_with("++")
            || after.starts_with("--")
            || (after.starts_with('=') && !after.starts_with("=="))
            || ["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="]
                .iter()
                .any(|op| after.starts_with(op));
        if write {
            mask |= 1 << number;
        }
        at = end;
    }
    mask
}

/// Remove the context synchronization markers when registers stay in the
/// context (the default build).
pub fn strip_sync(text: &str) -> String {
    let marker = Regex::new("\x01[CBRALT][0-9]+\x01").unwrap();
    marker.replace_all(text, "").into_owned()
}

/// Give x29-based stores the stack variants.
pub fn frame_stores(text: &str) -> String {
    let first = fp_store_pre()
        .replace_all(text, "${1}recomp_${2}_sp(")
        .into_owned();
    fp_store()
        .replace_all(&first, "recomp_${1}_sp(c,${2}c->x[29]")
        .into_owned()
}

pub fn frame_json(text: &str) -> Value {
    json!({"frame": fp_is_frame(text), "stores": frame_stores(text)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_stores_rewrite_both_forms() {
        assert_eq!(
            stack_stores("{ uint64_t _b=c->x[31]; int64_t _o=-8; recomp_store64(c,c->x[31],v); }"),
            "{ uint64_t _b=c->x[31]; int64_t _o=-8; recomp_store64_sp(c,c->x[31],v); }"
        );
        assert_eq!(
            stack_stores("recomp_store32(c,c->x[31],v);"),
            "recomp_store32_sp(c,c->x[31],v);"
        );
        assert_eq!(
            stack_stores("recomp_store32(c,c->x[0],v);"),
            "recomp_store32(c,c->x[0],v);"
        );
    }

    #[test]
    fn sync_markers_are_removed() {
        assert_eq!(strip_sync("a\x01C3\x01b\x01L3\x01c"), "abc");
        assert_eq!(strip_sync("plain"), "plain");
    }

    #[test]
    fn guest_register_writes() {
        assert_eq!(
            reg_writes("c->x[0]=1; c->x[1]+=2; ++c->x[2]; &c->x[3];"),
            0b1111
        );
        assert_eq!(reg_writes("c->x[0]==1; x=c->x[4];"), 0);
    }

    #[test]
    fn frame_writes_and_stores() {
        assert!(fp_is_frame("c->x[0]=1;"));
        assert!(!fp_is_frame("c->x[29]=c->x[0];"));
        let text = "{ uint64_t _b=c->x[29]; int64_t _o=-8; recomp_store64(c,c->x[29],v); }";
        assert_eq!(
            frame_stores(text),
            "{ uint64_t _b=c->x[29]; int64_t _o=-8; recomp_store64_sp(c,c->x[29],v); }"
        );
        assert_eq!(
            frame_stores("recomp_store32(c,c->x[29],v);"),
            "recomp_store32_sp(c,c->x[29],v);"
        );
        assert!(fp_is_frame(
            "{ uint64_t _a=c->x[31], _b=16ULL; uint64_t _r=_a+_b; c->x[29]=_r; }"
        ));
    }
}

const FLAG_BITS: [u8; 4] = [8, 4, 2, 1];

fn flag_index(name: &str) -> usize {
    match name {
        "n" => 0,
        "z" => 1,
        "c" => 2,
        _ => 3,
    }
}

/// (flags read, flags written) by one instruction's C text. Control flow
/// inside the text reads all four and writes none.
pub fn flag_masks(text: &str) -> (u8, u8) {
    if text.contains("goto") || text.contains("return") || text.contains("TAILCALL") {
        return (15, 0);
    }
    let mut written = 0u8;
    let mut read = 0u8;
    let bytes = text.as_bytes();
    let mut at = 0usize;
    while let Some(found) = text[at..].find("c->") {
        let name_at = at + found + 3;
        let Some(name) = bytes.get(name_at).copied() else {
            break;
        };
        if !matches!(name, b'n' | b'z' | b'c' | b'v') {
            at = name_at;
            continue;
        }
        let bit = FLAG_BITS[flag_index(&(name as char).to_string())];
        let mut cursor = name_at + 1;
        while bytes.get(cursor) == Some(&b' ') {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'=') && bytes.get(cursor + 1) != Some(&b'=') {
            written |= bit;
        } else {
            read |= bit;
        }
        at = name_at + 1;
    }
    (read, written)
}

/// Remove the stores of flags in `dead` (FLAG_BITS mask).
pub fn drop_flag_stores(text: &str, dead: u8) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    while let Some(found) = text[at..].find("c->") {
        let start = at + found;
        out.push_str(&text[at..start]);
        let name_at = start + 3;
        let Some(name) = text.as_bytes().get(name_at).copied() else {
            out.push_str(&text[start..]);
            return out;
        };
        if !matches!(name, b'n' | b'z' | b'c' | b'v') {
            out.push_str(&text[start..name_at + 1]);
            at = name_at + 1;
            continue;
        }
        let mut cursor = name_at + 1;
        while text.as_bytes().get(cursor) == Some(&b' ') {
            cursor += 1;
        }
        if text.as_bytes().get(cursor) == Some(&b'=')
            && text.as_bytes().get(cursor + 1) != Some(&b'=')
        {
            if let Some(end) = text[cursor..].find(';') {
                let stop = cursor + end + 1;
                if FLAG_BITS[flag_index(&(name as char).to_string())] & dead == 0 {
                    out.push_str(&text[start..stop]);
                }
                at = stop;
                continue;
            }
        }
        out.push_str(&text[start..name_at + 1]);
        at = name_at + 1;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod flag_tests {
    use super::*;

    #[test]
    fn masks_and_drops_dead_flag_stores() {
        let text = "c->n=(x); c->z=1; c->c=c->v;";
        assert_eq!(flag_masks(text), (1, 14));
        assert_eq!(drop_flag_stores(text, 8), " c->z=1; c->c=c->v;");
        assert_eq!(drop_flag_stores(text, 15), "  ");
        assert_eq!(flag_masks("if (c->z) { goto L; }"), (15, 0));
    }
}
