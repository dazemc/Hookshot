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
