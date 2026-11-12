//! Switch 2 instruction forms a64emit does not translate, ported from
//! nx2/insn.py: pointer-authenticated returns/branches, LSE atomics, CAS,
//! LDAPR/STLR, PAC, MTE and a few SIMD forms.

use crate::a64::Kind;

const UTYPE: [&str; 4] = ["uint8_t", "uint16_t", "uint32_t", "uint64_t"];
const STYPE: [&str; 4] = ["int8_t", "int16_t", "int32_t", "int64_t"];
const LSE_OPS: [&str; 8] = ["add", "clr", "eor", "set", "smax", "smin", "umax", "umin"];
const QC: &str = "c->fpsr |= 1u << 27;";

fn xr(n: u32) -> String {
    if n == 31 {
        "0ULL".into()
    } else {
        format!("c->x[{n}]")
    }
}

fn xw(n: u32, value: &str) -> String {
    if n == 31 {
        String::new()
    } else {
        format!("c->x[{n}] = {value};")
    }
}

fn sx(value: u32, bits: u32) -> i64 {
    crate::a64::sx(value, bits)
}

/// RETAA/RETAB and BRAA/BRAB/BLRAA/BLRAB are ordinary returns and branches
/// with pointer authentication, which is the identity here.
pub fn classify(word: u32) -> Option<Kind> {
    if word == 0xD65F_0BFF || word == 0xD65F_0FFF {
        return Some(Kind::Ret(30));
    }
    if word & 0xFEFF_F800 == 0xD61F_0800 {
        return Some(Kind::Br(((word >> 5) & 31) as u8));
    }
    if word & 0xFEFF_F800 == 0xD63F_0800 {
        return Some(Kind::Blr(((word >> 5) & 31) as u8));
    }
    None
}

fn lse(word: u32) -> Option<String> {
    let size = word >> 30;
    let a = (word >> 23) & 1;
    let r = (word >> 22) & 1;
    let rs = (word >> 16) & 31;
    let o3 = (word >> 15) & 1;
    let opc = ((word >> 12) & 7) as usize;
    let rn = (word >> 5) & 31;
    let rt = word & 31;
    let t = UTYPE[size as usize];
    let n = 1 << size;
    let head = format!("{{ uint64_t _a = c->x[{rn}]; {t} *_p = ({t} *)A64_PTR(_a); ");
    if o3 == 1 && opc == 4 && a == 1 && r == 0 && rs == 31 {
        return Some(format!(
            "{head}{} }}",
            xw(rt, "(uint64_t)__atomic_load_n(_p, __ATOMIC_ACQUIRE)")
        ));
    }
    if o3 == 1 && opc == 0 {
        let old = xw(rt, "(uint64_t)_o");
        return Some(format!(
            "{head}{t} _o = __atomic_exchange_n(_p, ({t}){}, __ATOMIC_SEQ_CST); A64_MARK(_a, {n}); {old} }}",
            xr(rs)
        ));
    }
    if o3 != 0 {
        return None;
    }
    let op = LSE_OPS[opc];
    let st = STYPE[size as usize];
    let s = format!("({t}){}", xr(rs));
    let body = if matches!(op, "add" | "clr" | "eor" | "set") {
        let (func, argument) = match op {
            "add" => ("__atomic_fetch_add", s.clone()),
            "clr" => ("__atomic_fetch_and", format!("({t})~{s}")),
            "eor" => ("__atomic_fetch_xor", s.clone()),
            _ => ("__atomic_fetch_or", s.clone()),
        };
        format!("{t} _o = {func}(_p, {argument}, __ATOMIC_SEQ_CST);")
    } else {
        let cmp = match op {
            "smax" => format!("({st})_o > ({st})_s"),
            "smin" => format!("({st})_o < ({st})_s"),
            "umax" => "_o > _s".into(),
            _ => "_o < _s".into(),
        };
        let cmp = cmp
            .replace("(int8_t) _s", "(int8_t)_s")
            .replace("(int16_t) _s", "(int16_t)_s")
            .replace("(int32_t) _s", "(int32_t)_s")
            .replace("(int64_t) _s", "(int64_t)_s");
        format!(
            "{t} _s = {s}, _o = __atomic_load_n(_p, __ATOMIC_RELAXED), _n; do {{ _n = ({cmp}) ? _o : _s; }} while (!__atomic_compare_exchange_n(_p, &_o, _n, 1, __ATOMIC_SEQ_CST, __ATOMIC_RELAXED));"
        )
    };
    Some(format!(
        "{head}{body} A64_MARK(_a, {n}); {} }}",
        xw(rt, "(uint64_t)_o")
    ))
}

fn cas(word: u32) -> Option<String> {
    let size = word >> 30;
    let rs = (word >> 16) & 31;
    let rn = (word >> 5) & 31;
    let rt = word & 31;
    let t = UTYPE[size as usize];
    Some(format!(
        "{{ uint64_t _a = c->x[{rn}]; {t} _e = ({t}){}; __atomic_compare_exchange_n(({t} *)A64_PTR(_a), &_e, ({t}){}, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); A64_MARK(_a, {}); {} }}",
        xr(rs),
        xr(rt),
        1 << size,
        xw(rs, "(uint64_t)_e")
    ))
}

fn casp(word: u32) -> Option<String> {
    let sz = (word >> 30) & 1;
    let rs = (word >> 16) & 31;
    let rn = (word >> 5) & 31;
    let rt = word & 31;
    if rs & 1 != 0 || rt & 1 != 0 {
        return None;
    }
    if sz == 0 {
        return Some(format!(
            "{{ uint64_t _a = c->x[{rn}]; uint64_t _e = (uint32_t){} | (uint64_t)(uint32_t){} << 32; uint64_t _n = (uint32_t){} | (uint64_t)(uint32_t){} << 32; __atomic_compare_exchange_n((uint64_t *)A64_PTR(_a), &_e, _n, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); A64_MARK(_a, 8); {} {} }}",
            xr(rs),
            xr(rs + 1),
            xr(rt),
            xr(rt + 1),
            xw(rs, "(uint32_t)_e"),
            xw(rs + 1, "_e >> 32")
        ));
    }
    Some(format!(
        "{{ uint64_t _a = c->x[{rn}]; unsigned __int128 _e = (unsigned __int128){} | (unsigned __int128){} << 64; unsigned __int128 _n = (unsigned __int128){} | (unsigned __int128){} << 64; __atomic_compare_exchange_n((unsigned __int128 *)A64_PTR(_a), &_e, _n, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); A64_MARK(_a, 16); {} {} }}",
        xr(rs),
        xr(rs + 1),
        xr(rt),
        xr(rt + 1),
        xw(rs, "(uint64_t)_e"),
        xw(rs + 1, "(uint64_t)(_e >> 64)")
    ))
}

fn stg(word: u32) -> Option<String> {
    let opc = (word >> 22) & 3;
    let imm = sx((word >> 12) & 0x1FF, 9) << 4;
    let op2 = (word >> 10) & 3;
    let rn = (word >> 5) & 31;
    if !matches!(opc, 0 | 2) || op2 == 0 {
        return None;
    }
    if op2 == 2 {
        return Some("/* stg: memory tags are not modelled */".into());
    }
    Some(format!(
        "c->x[{rn}] += (uint64_t)(int64_t){imm}; /* st{}g, writeback: tags are not modelled */",
        if opc == 2 { "2" } else { "" }
    ))
}

fn sqdmlal(word: u32) -> Option<String> {
    let q = (word >> 30) & 1;
    let size = (word >> 22) & 3;
    let rn = (word >> 5) & 31;
    let rd = word & 31;
    if !matches!(size, 1 | 2) {
        return None;
    }
    let (rm, idx) = if word & 0xBF20_FC00 == 0x0E20_9000 {
        ((word >> 16) & 31, None)
    } else {
        let l = (word >> 21) & 1;
        let m = (word >> 20) & 1;
        let h = (word >> 11) & 1;
        if size == 1 {
            ((word >> 16) & 15, Some((h << 2) | (l << 1) | m))
        } else {
            ((word >> 16) & 31, Some((h << 1) | l))
        }
    };
    let (e, n) = if size == 1 { (16, 4) } else { (32, 2) };
    let (t, w) = if size == 1 {
        ("int16_t", "int32_t")
    } else {
        ("int32_t", "int64_t")
    };
    let lo = n * q;
    let b = match idx {
        Some(idx) => format!("_m[{idx}]"),
        None => format!("_m[{} + _i]", lo),
    };
    let max_e = (1i64 << (e - 1)) - 1;
    let max_2e = (1i64 << (2 * e - 1)) - 1;
    let shift = 2 * e - 1;
    Some(format!(
        "{{ {t} _n[{n2}], _m[{n2}]; {w} _d[{n}]; memcpy(_n, c->vreg[{rn}], 16); memcpy(_m, c->vreg[{rm}], 16); memcpy(_d, c->vreg[{rd}], 16); for (int _i = 0; _i < {n}; _i++) {{ {w} _p; if (_n[{lo} + _i] == ({t})(-{max_e}LL - 1) && {b} == ({t})(-{max_e}LL - 1)) {{ _p = ({w})((1ULL << {shift}) - 1); {QC} }} else _p = ({w})2 * _n[{lo} + _i] * {b}; if (__builtin_add_overflow(_d[_i], _p, &_d[_i])) {{ _d[_i] = _p < 0 ? ({w})(-{max_2e}LL - 1) - 0 : ({w})((1ULL << {shift}) - 1); {QC} }} }} memcpy(c->vreg[{rd}], _d, 16); }}",
        n2 = 2 * n,
        lo = lo,
        b = b,
        t = t,
        w = w,
        n = n,
        rn = rn,
        rm = rm,
        rd = rd,
        max_e = max_e,
        max_2e = max_2e,
        shift = shift,
    ))
}

fn sqshl_imm(word: u32) -> Option<String> {
    let q = (word >> 30) & 1;
    let u = (word >> 29) & 1;
    let immh = (word >> 19) & 15;
    let immb = (word >> 16) & 7;
    let rn = (word >> 5) & 31;
    let rd = word & 31;
    if immh == 0 || (immh & 8 != 0 && q == 0) {
        return None;
    }
    let e = 8u32 << (31 - immh.leading_zeros());
    let shift = (immh << 3 | immb) - e;
    let lanes = (if q != 0 { 16 } else { 8 }) * 8 / e;
    let s = match e {
        8 => "int8_t",
        16 => "int16_t",
        32 => "int32_t",
        _ => "int64_t",
    };
    let us = match e {
        8 => "uint8_t",
        16 => "uint16_t",
        32 => "uint32_t",
        _ => "uint64_t",
    };
    let smax = (1i64 << (e - 1)) - 1;
    let smin1 = -smax;
    let umax = (1u64 << e) - 1;
    let (body, out) = if u != 0 {
        (
            format!(
                "if (_x < 0) {{ _r[_i] = 0; if (_x) {QC} }} else {{ __int128 _v = (__int128)_x * ((__int128)1 << {shift}); if (_v > (__int128){umax}ULL) {{ _r[_i] = ({us}){umax}ULL; {QC} }} else _r[_i] = ({us})_v; }}"
            ),
            us,
        )
    } else {
        (
            format!(
                "__int128 _v = (__int128)_x * ((__int128)1 << {shift}); if (_v > (__int128){smax}LL) {{ _r[_i] = ({s}){smax}LL; {QC} }} else if (_v < (__int128)({smin1}LL - 1)) {{ _r[_i] = ({s})({smin1}LL - 1); {QC} }} else _r[_i] = ({s})_v;"
            ),
            s,
        )
    };
    Some(format!(
        "{{ {s} _a[{lanes}]; {out} _r[{lanes}]; memcpy(_a, c->vreg[{rn}], 16); for (int _i = 0; _i < {lanes}; _i++) {{ {s} _x = _a[_i]; {body} }} c->vreg[{rd}][0] = 0; c->vreg[{rd}][1] = 0; memcpy(c->vreg[{rd}], _r, {bytes}); }}",
        lanes = 128 / e,
        bytes = lanes * e / 8,
    ))
}

fn fneg_h(word: u32) -> Option<String> {
    let q = (word >> 30) & 1;
    let rn = (word >> 5) & 31;
    let rd = word & 31;
    let mask = "0x8000800080008000ULL";
    Some(format!(
        "{{ uint64_t _lo = c->vreg[{rn}][0] ^ {mask}, _hi = {hi}; c->vreg[{rd}][0] = _lo; c->vreg[{rd}][1] = _hi; }}",
        hi = if q != 0 {
            format!("c->vreg[{rn}][1] ^ {mask}")
        } else {
            "0".into()
        }
    ))
}

fn dot(word: u32) -> Option<String> {
    let q = (word >> 30) & 1;
    let u = (word >> 29) & 1;
    let rm = (word >> 16) & 31;
    let rn = (word >> 5) & 31;
    let rd = word & 31;
    let elem = (word >> 24) & 1;
    let t = if u != 0 { "uint8_t" } else { "int8_t" };
    let mb = if elem != 0 {
        let index = (((word >> 11) & 1) << 1) | ((word >> 21) & 1);
        format!("_m[4 * {index} + _j]")
    } else {
        "_m[4 * _i + _j]".into()
    };
    let lanes = if q != 0 { 4 } else { 2 };
    Some(format!(
        "{{ {t} _n[16], _m[16]; uint32_t _d[4]; memcpy(_n, c->vreg[{rn}], 16); memcpy(_m, c->vreg[{rm}], 16); memcpy(_d, c->vreg[{rd}], 16); for (int _i = 0; _i < {lanes}; _i++) {{ int32_t _s = 0; for (int _j = 0; _j < 4; _j++) _s += (int32_t)_n[4 * _i + _j] * (int32_t){mb}; _d[_i] += (uint32_t)_s; }} c->vreg[{rd}][1] = 0; memcpy(c->vreg[{rd}], _d, {bytes}); }}",
        bytes = lanes * 4,
    ))
}

fn ldar_stlr(word: u32) -> Option<String> {
    let size = word >> 30;
    let load = (word >> 22) & 1;
    let rn = (word >> 5) & 31;
    let rt = word & 31;
    let t = UTYPE[size as usize];
    if load != 0 {
        let value = format!("(uint64_t)__atomic_load_n(({t} *)A64_PTR(_a), __ATOMIC_SEQ_CST)");
        let write = if rt != 31 {
            xw(rt, &value)
        } else {
            format!("(void){value};")
        };
        return Some(format!("{{ uint64_t _a = c->x[{rn}]; {write} }}"));
    }
    Some(format!(
        "{{ uint64_t _a = c->x[{rn}]; __atomic_store_n(({t} *)A64_PTR(_a), ({t}){}, __ATOMIC_SEQ_CST); A64_MARK(_a, {}); }}",
        xr(rt),
        1 << size
    ))
}

/// C for one of the forms above, or None when a64emit handles it.
pub fn emit(word: u32) -> Option<String> {
    if word & 0x3FBF_7C00 == 0x089F_7C00 {
        return ldar_stlr(word);
    }
    if word & 0xBF20_FC00 == 0x0E20_9000 || word & 0xBF00_F400 == 0x0F00_3000 {
        return sqdmlal(word);
    }
    if matches!(word & 0xBF80_FC00, 0x0F00_7400 | 0x2F00_6400) {
        return sqshl_imm(word);
    }
    if word & 0xBFFF_FC00 == 0x2EF8_F800 {
        return fneg_h(word);
    }
    if word & 0x9FE0_FC00 == 0x0E80_9400 || word & 0x9FC0_F400 == 0x0F80_E000 {
        return dot(word);
    }
    if word & 0x3F20_0C00 == 0x3820_0000 {
        return lse(word);
    }
    if word & 0x3FA0_7C00 == 0x08A0_7C00 {
        return cas(word);
    }
    if word & 0xBFA0_7C00 == 0x0820_7C00 {
        return casp(word);
    }
    if word & 0xFFE0_FC00 == 0x9AC0_3000 {
        let write = xw(word & 31, "0");
        return Some(if write.is_empty() { ";".into() } else { write });
    }
    if word & 0xFFFF_FBE0 == 0xDAC1_43E0 {
        return Some("/* xpac: pointers carry no PAC */".into());
    }
    if word & 0xFFFF_C000 == 0xDAC1_0000 {
        return Some("/* pac/aut: pointer authentication is the identity */".into());
    }
    if word & 0xFF20_0000 == 0xD920_0000 {
        return stg(word);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_returns_are_returns() {
        assert_eq!(classify(0xD65F_0FFF), Some(Kind::Ret(30)));
        assert_eq!(classify(0xD65F_0BFF), Some(Kind::Ret(30)));
        assert_eq!(classify(0xD61F_081F), Some(Kind::Br(0)));
        assert_eq!(classify(0xD63F_081F), Some(Kind::Blr(0)));
        assert_eq!(classify(0x9100_0000), None);
    }

    #[test]
    fn known_forms_match_the_python_text() {
        assert_eq!(
            emit(0x38BF_C108).as_deref(),
            Some(
                "{ uint64_t _a = c->x[8]; uint8_t *_p = (uint8_t *)A64_PTR(_a); c->x[8] = (uint64_t)__atomic_load_n(_p, __ATOMIC_ACQUIRE); }"
            )
        );
        assert_eq!(
            emit(0xF8E9_0108).as_deref(),
            Some(
                "{ uint64_t _a = c->x[8]; uint64_t *_p = (uint64_t *)A64_PTR(_a); uint64_t _o = __atomic_fetch_add(_p, (uint64_t)c->x[9], __ATOMIC_SEQ_CST); A64_MARK(_a, 8); c->x[8] = (uint64_t)_o; }"
            )
        );
        assert_eq!(
            emit(0x88AB_7D49).as_deref(),
            Some(
                "{ uint64_t _a = c->x[10]; uint32_t _e = (uint32_t)c->x[11]; __atomic_compare_exchange_n((uint32_t *)A64_PTR(_a), &_e, (uint32_t)c->x[9], 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); A64_MARK(_a, 4); c->x[11] = (uint64_t)_e; }"
            )
        );
    }
}
