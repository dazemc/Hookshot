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
/// Replace exact per-lane vector float compares with one a64_vfcmp guard.
pub fn vectorize_fcmp(text: &str) -> String {
    if !text.contains("_quiet=") || std::env::var_os("A64_NO_VFPX").is_some() {
        return text.to_owned();
    }
    let op_index = |op: &str| match op {
        "_eq||_gt" => 0,
        "_gt" => 1,
        "_eq" => 2,
        _ => 0,
    };
    // Scalar FCMP/FCMPE: one helper call guards the exact integer block.
    let sfcmp = Regex::new(r"\{ uint64_t _a=0,_b=0; memcpy\(&_a,([^;]+?),(4|8)\);(?: memcpy\(&_b,([^;]+?),(4|8)\);)? [^\n]*? c->n=\(uint8_t\)_lt; c->z=\(uint8_t\)_eq; c->c=\(uint8_t\)!_lt; c->v=0; \} \}").unwrap();
    let text = sfcmp
        .replace_all(&text, |c: &regex::Captures| {
            if let (Some(b), Some(size2)) = (c.get(3), c.get(4)) {
                let _ = b;
                if size2.as_str() != &c[2] {
                    return c[0].to_owned();
                }
            }
            let bits = if &c[2] == "4" { 32 } else { 64 };
            let other = c.get(3).map(|m| m.as_str()).unwrap_or("a64_vzero");
            format!(
                "{{ if (!a64_fcmp{bits}(c,{},{})) {} }}",
                &c[1], other, &c[0]
            )
        })
        .into_owned();
    let vfcmp = Regex::new(r"(?s)\{ uint32_t _n\[4\],_m\[4\],_r\[4\]=\{0\}; memcpy\(_n,c->vreg\[(\d+)\],16\); memcpy\(_m,c->vreg\[(\d+)\],16\); for\(int _i=0;_i<([24]);_i\+\+\) \{ .*?if\((_eq\|\|_gt|_gt|_eq)\) _v=~\(uint64_t\)0; \} _r\[_i\]=\(uint32_t\)_v; \} memcpy\(c->vreg\[(\d+)\],_r,16\); \}").unwrap();
    let text = vfcmp
        .replace_all(&text, |c: &regex::Captures| {
            format!(
                "if (!a64_vfcmp(c,c->vreg[{}],c->vreg[{}],c->vreg[{}],{},{})) {}",
                &c[5],
                &c[1],
                &c[2],
                &c[3],
                op_index(&c[4]),
                &c[0]
            )
        })
        .into_owned();
    let vfcmpz = Regex::new(r"(?s)\{ uint32_t _n\[4\],_r\[4\]=\{0\}; memcpy\(_n,c->vreg\[(\d+)\],16\); for\(int _i=0;_i<([24]);_i\+\+\) \{ uint64_t (('_a=0,_b=_n\[_i\]|'_a=_n\[_i\],_b=0)),_v=0;.*?if\((_eq\|\|_gt|_gt|_eq)\) _v=~\(uint64_t\)0; \} _r\[_i\]=\(uint32_t\)_v; \} memcpy\(c->vreg\[(\d+)\],_r,16\); \}").unwrap();
    vfcmpz
        .replace_all(&text, |c: &regex::Captures| {
            let src = format!("c->vreg[{}]", &c[1]);
            let (a, b) = if c[3].starts_with("_a=0") {
                ("a64_vzero".to_owned(), src)
            } else {
                (src, "a64_vzero".to_owned())
            };
            format!(
                "if (!a64_vfcmp(c,c->vreg[{}],{}, {}, {},{})) {}",
                &c[5],
                a,
                b,
                &c[2],
                op_index(&c[4]),
                &c[0]
            )
        })
        .into_owned()
}

/// The full op-text pipeline used before emission and liveness.
pub fn op_text(text: &str) -> String {
    stack_stores(&widen_vstores(&vectorize_fcmp(&outline_fpx(text))))
}

/// Replace an inline FPX guard by one helper call when the fast and exact
/// argument lists agree and both write the same variable.
pub fn outline_fpx(text: &str) -> String {
    if !text.contains("RECOMP_FPX_OPEN") {
        return text.to_owned();
    }
    let guard = Regex::new(r"\{if\(RECOMP_FPX_OPEN\(c\)&&RECOMP_LIKELY\(recomp_fpx_(\w+)\(([^;]*?),RECOMP_FPX_FZ\(c\),&(\w+)\)\)\)\{RECOMP_FPX_PROBE\(\d+\);\}else\{RECOMP_FPX_PROBE\(\d+\);(\w+)=recomp_fpe_(\w+)\(c,([^;]*?)\);\}\}").unwrap();
    guard
        .replace_all(text, |c: &regex::Captures| {
            let fast = &c[1];
            let fast_args = &c[2];
            let out = &c[3];
            let out2 = &c[4];
            let exact = &c[5];
            let exact_args = &c[6];
            let nf = fast_args.split(',').count();
            let ne = exact_args.split(',').count();
            let prefix_matches = {
                let a: Vec<&str> = fast_args.split(',').map(str::trim).collect();
                let b: Vec<&str> = exact_args.split(',').map(str::trim).collect();
                a.len() >= nf && b.len() >= nf && a[..nf] == b[..nf]
            };
            if out != out2 || !prefix_matches || nf == 0 || ne == 0 {
                return c[0].to_owned();
            }
            format!("{out}=a64_fpx_{fast}_{exact}(c,{exact_args});")
        })
        .into_owned()
}

pub fn widen_vstores(text: &str) -> String {
    if !text.contains("c->vreg[") || std::env::var_os("A64_NO_VSET").is_some() {
        return text.to_owned();
    }
    let low = Regex::new(r"c->vreg\[(\d+)\]\[0\]=0; ?c->vreg\[(\d+)\]\[1\]=0; ?memcpy\(c->vreg\[(\d+)\],(&?\w+),([48])\);").unwrap();
    let text = low
        .replace_all(text, |c: &regex::Captures| {
            if c[1] != c[2] || c[1] != c[3] {
                return c[0].to_owned();
            }
            format!("a64_vset_lo(c->vreg[{}],{},{});", &c[1], &c[4], &c[5])
        })
        .into_owned();
    let pair =
        Regex::new(r"c->vreg\[(\d+)\]\[0\] ?= ?([^;]+); ?c->vreg\[(\d+)\]\[1\] ?= ?([^;]+);")
            .unwrap();
    let text = pair
        .replace_all(&text, |c: &regex::Captures| {
            if c[1] != c[3] {
                return c[0].to_owned();
            }
            let d = &c[1];
            let lo = &c[2];
            let hi = &c[4];
            if hi.trim() == "0" {
                return format!("a64_vset64(c->vreg[{d}],{lo});");
            }
            if hi.contains(&format!("c->vreg[{d}][0]")) || lo.contains('(') || hi.contains('(') {
                return c[0].to_owned();
            }
            format!("a64_vset128(c->vreg[{d}],{lo},{hi});")
        })
        .into_owned();
    let full = Regex::new(r"memcpy\(c->vreg\[(\d+)\],(\w+),16\);").unwrap();
    full.replace_all(&text, |c: &regex::Captures| {
        format!("a64_vst16(c->vreg[{}],{});", &c[1], &c[2])
    })
    .into_owned()
}

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

pub fn local_count() -> usize {
    std::env::var("A64_LOCAL_REGS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Expand the context marks. With `local` the most used guest registers
/// become C locals; each mark writes back what may have changed or reloads.
pub fn localize_regs(text: &str, local_count: usize, dirty: (&[u32], &[u32])) -> String {
    if local_count == 0 {
        return strip_sync(text);
    }
    let mut uses = std::collections::HashMap::new();
    let mut at = 0usize;
    while let Some(found) = text[at..].find("c->x[") {
        let start = at + found + 5;
        let mut end = start;
        while text.as_bytes().get(end).is_some_and(|b| b.is_ascii_digit()) {
            end += 1;
        }
        if let Ok(number) = text[start..end].parse::<usize>() {
            *uses.entry(number).or_insert(0usize) += 1;
        }
        at = end;
    }
    let mut top: Vec<usize> = uses
        .iter()
        .filter(|(n, _)| **n != 31)
        .map(|(n, _)| *n)
        .collect();
    top.sort_by_key(|n| (std::cmp::Reverse(uses[n]), *n));
    top.truncate(local_count);
    if uses.contains_key(&31) {
        top.push(31);
    }
    top.sort_unstable();
    top.dedup();
    let chosen = &top;
    let mut replaced = text.to_owned();
    for n in chosen {
        replaced = replaced.replace(&format!("c->x[{n}]"), &format!("x{n}_"));
    }
    let store = |mask: u32| {
        chosen
            .iter()
            .filter(|n| mask >> *n & 1 != 0)
            .map(|n| format!("c->x[{n}] = x{n}_; "))
            .collect::<String>()
    };
    let load = |mask: u32| {
        chosen
            .iter()
            .filter(|n| mask >> *n & 1 != 0)
            .map(|n| format!("x{n}_ = c->x[{n}]; "))
            .collect::<String>()
    };
    let marker = Regex::new("\x01([CBRALT])([0-9]+)\x01").unwrap();
    let expanded = marker
        .replace_all(&replaced, |caps: &regex::Captures| {
            let kind = caps[1].chars().next().unwrap();
            let index: usize = caps[2].parse().unwrap_or(0);
            let before = dirty.0.get(index).copied().unwrap_or(0);
            let after = dirty.1.get(index).copied().unwrap_or(0);
            match kind {
                'C' => store((before | 1 << 30) & !0xFE00),
                'R' => store(before & !0xFE00),
                'A' => store(before),
                'T' => store(after),
                'B' => load(0x4007FFFF),
                _ => load(0xFFFFFFFF),
            }
        })
        .into_owned();
    let mut lines = expanded.splitn(2, '\n');
    let head = lines.next().unwrap_or("");
    let rest = lines.next().unwrap_or("");
    let decl = if chosen.is_empty() {
        String::new()
    } else {
        format!(
            "\n    uint64_t {};",
            chosen
                .iter()
                .map(|n| format!("x{n}_ = c->x[{n}]"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!("{head}{decl}\n{rest}")
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
    fn localize_makes_a_register_local_and_stores_it() {
        let text = "void f(){\n\x01C0\x01c->x[0]=1;\n}";
        let out = localize_regs(text, 1, (&[1], &[1]));
        assert!(out.contains("uint64_t x0_ = c->x[0];"));
        assert!(out.contains("c->x[0] = x0_; "));
        assert!(out.contains("x0_=1;"));
        assert!(!out.contains('\x01'));
    }

    #[test]
    fn sync_markers_are_removed() {
        assert_eq!(strip_sync("a\x01C3\x01b\x01L3\x01c"), "abc");
        assert_eq!(strip_sync("plain"), "plain");
    }

    #[test]
    fn vector_compares_are_guarded() {
        let block = "{ uint32_t _n[4],_m[4],_r[4]={0}; memcpy(_n,c->vreg[0],16); memcpy(_m,c->vreg[1],16); for(int _i=0;_i<4;_i++) { uint64_t _a=_n[_i],_b=_m[_i],_v=0; const uint64_t _sign=0x80000000ULL,_exp=0x7f800000ULL,_frac=0x7fffffULL,_quiet=0x400000ULL; if(_an||_bn) { if(x) c->fpsr|=1; } else { int _eq=_a==_b||!((_a|_b)&~_sign); int _gt=!_eq&&(((_a^_b)&_sign)?!(_a&_sign):((_a&_sign)?_a<_b:_a>_b)); (void)_gt; (void)_eq; if(_gt) _v=~(uint64_t)0; } _r[_i]=(uint32_t)_v; } memcpy(c->vreg[2],_r,16); }";
        assert!(
            vectorize_fcmp(block)
                .starts_with("if (!a64_vfcmp(c,c->vreg[2],c->vreg[0],c->vreg[1],4,1))")
        );
    }

    #[test]
    fn fpx_guards_become_one_helper_call() {
        let text = "{if(RECOMP_FPX_OPEN(c)&&RECOMP_LIKELY(recomp_fpx_mul32(_a,_b,RECOMP_FPX_FZ(c),&_v))){RECOMP_FPX_PROBE(1);}else{RECOMP_FPX_PROBE(2);_v=recomp_fpe_mul32(c,_a,_b);}}";
        assert_eq!(outline_fpx(text), "_v=a64_fpx_mul32_mul32(c,_a,_b);");
    }

    #[test]
    fn vector_stores_are_widened() {
        assert_eq!(
            widen_vstores("c->vreg[8][0]=0; c->vreg[8][1]=0; memcpy(c->vreg[8],&x,4);"),
            "a64_vset_lo(c->vreg[8],&x,4);"
        );
        assert_eq!(
            widen_vstores("c->vreg[8][0]=1; c->vreg[8][1]=0;"),
            "a64_vset64(c->vreg[8],1);"
        );
        assert_eq!(
            widen_vstores("c->vreg[8][0]=1; c->vreg[8][1]=2;"),
            "a64_vset128(c->vreg[8],1,2);"
        );
        assert_eq!(
            widen_vstores("memcpy(c->vreg[3],x,16);"),
            "a64_vst16(c->vreg[3],x);"
        );
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
