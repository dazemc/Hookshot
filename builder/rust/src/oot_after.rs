//! C added after individual instructions for Ocarina of Time 1.1.1, ported
//! from nx2/oot_wide.py, nx2/oot_jitter.py and nx2/oot_occlusion.py.
//!
//! Python installs these into `Lifter.after` for every standalone lift; the
//! emission appends the text after the plain instruction's C. A site whose
//! original word differs (another game version) is left out, as there.

use std::collections::BTreeMap;

use crate::lift::Code;

pub const MAIN: u64 = 0x8002_0000;

fn size(name: &str) -> String {
    r#"{ extern volatile uint64_t @N@; const uint64_t _s = @N@; if (_s) c->x[8] = _s; }   /* (nx2/oot_wide.py: @N@) */"#
        .replace("@N@", name)
}

fn ratio(vreg: u32, base: Option<u32>) -> String {
    let store = base
        .map(|base| format!("recomp_store32(c, c->x[{base}] + 0xb0, _u); "))
        .unwrap_or_default();
    r#"{ extern volatile float hos_wide_ratio; const float _w = hos_wide_ratio; if (_w > 0.0f) { float _r; memcpy(&_r, c->vreg[@V@], 4); const float _d = _r - (16.0f / 9.0f); if (_d <= 0.01f && _d >= -0.01f) { uint32_t _u; memcpy(&_u, &_w, 4); a64_vset64(c->vreg[@V@], _u); @S@} } }   /* (nx2/oot_wide.py: camera ratio) */"#
        .replace("@V@", &vreg.to_string())
        .replace("@S@", &store)
}

fn hud() -> String {
    // (bit k of host/hos_main.c's hos_wide_keep_mask keeps screen k whole)
    let keep: Vec<String> = [
        (0x4bed098u64, 1u32),
        (0x4be4fb0, 2),
        (0x4be61c8, 4),
        (0x4be7b30, 8),
    ]
    .iter()
    .map(|(off, bit)| format!("_vt == 0x{:x}ULL ? {}u", MAIN + off, bit))
    .collect();
    let mask = keep.join(" : ") + " : 0u";
    r#"{ extern volatile float hos_wide_squeeze; const float _q = hos_wide_squeeze; if (_q > 0.0f) { float _m00, _m11; memcpy(&_m00, c->vreg[1], 4); memcpy(&_m11, (const uint8_t *)c->vreg[4] + 4, 4); if (_m00 == (2.0f / 3840.0f) && _m11 == (2.0f / 2160.0f)) { const uint64_t _vt = recomp_load64(c, c->x[0] - 0x1b8); extern volatile uint32_t hos_wide_keep_mask; const uint32_t _kb = @M@; if (!(_kb & hos_wide_keep_mask)) { float _row[4]; memcpy(_row, c->vreg[1], 16); for (int _k = 0; _k < 4; _k++) _row[_k] *= _q; memcpy(c->vreg[1], _row, 16); } } } }   /* (nx2/oot_wide.py: HUD in the middle 16:9) */"#
        .replace("@M@", &mask)
}

fn movie() -> String {
    r#"{ extern volatile float hos_wide_margin; const float _m = hos_wide_margin; if (_m > 0.0f) { const uint64_t _vp = c->x[29] - 40; uint32_t _ua = (uint32_t)recomp_load32(c, _vp + 8), _ub = (uint32_t)recomp_load32(c, _vp + 16); float _a, _b; memcpy(&_a, &_ua, 4); memcpy(&_b, &_ub, 4); const float _in = (_b - _a) * _m; _a += _in; _b -= _in; memcpy(&_ua, &_a, 4); memcpy(&_ub, &_b, 4); recomp_store32(c, _vp + 8, _ua); recomp_store32(c, _vp + 16, _ub); } }   /* (nx2/oot_wide.py: movies in the middle 16:9) */"#
        .to_owned()
}

fn thumb() -> String {
    r#"{ extern volatile float hos_wide_ratio; const float _r = hos_wide_ratio; if (_r > 0.0f && (uint32_t)c->x[1] == 0 && (uint32_t)c->x[2] == 0 && (uint32_t)c->x[3] == 543 && (uint32_t)c->x[4] == 306) { if (_r > 16.0f / 9.0f) { const int _w = (int)(543.0f * _r / (16.0f / 9.0f) + 0.5f); c->x[1] = (uint32_t)((543 - _w) / 2); c->x[3] = (uint32_t)_w; } else { const int _h = (int)(306.0f * (16.0f / 9.0f) / _r + 0.5f); c->x[2] = (uint32_t)((306 - _h) / 2); c->x[4] = (uint32_t)_h; } } }   /* (nx2/oot_wide.py: the save's picture, the middle 16:9) */"#
        .to_owned()
}

fn jitter(xreg: Option<u32>, yreg: Option<u32>) -> String {
    let mut add = String::new();
    for (axis, reg) in [(0u32, xreg), (1, yreg)] {
        let Some(reg) = reg else { continue };
        let part = r#"{ float _o; memcpy(&_o, (uint8_t *)c->vreg[@R@] + 8, 4); _o += _j[@X@]; memcpy((uint8_t *)c->vreg[@R@] + 8, &_o, 4); } "#
            .replace("@R@", &reg.to_string())
            .replace("@X@", &axis.to_string());
        add.push_str(&part);
    }
    r#"{ extern int hos_dlss_jitter_clip(uint64_t projection, float out[2]); float _j[2]; if (hos_dlss_jitter_clip(c->x[28] + 0x1a8, _j)) { @A@} }   /* (nx2/oot_jitter.py: the upscalers' camera jitter) */"#
        .replace("@A@", &add)
}

fn occlusion() -> String {
    r#"{ extern volatile int hos_occlusion_off; if (hos_occlusion_off) c->x[8] = 0; }   /* (nx2/oot_occlusion.py: no occlusion queries) */"#
        .to_owned()
}

// --- field of view ---------------------------------------------------------

const FOV_SITE: u64 = 0x4c52f8;
/// The angle load, half-angle multiply and projection angle store at FOV_SITE.
/// Checking all three keeps another build's nearby code out, as oot_fov.py did
/// (the engine supports one build, nso::SUPPORTED_BUILD, as the Python patch checked).
const FOV_WORDS: [u32; 3] = [0xbd413e60, 0x1e210808, 0xbd00a280];

fn fov() -> String {
    r#"{ extern void hos_oot_fov(Ctx *c, uint64_t camera); hos_oot_fov(c, c->x[19]); }   /* (nx2/oot_fov.py: gameplay field of view) */"#
        .to_owned()
}

fn wide_sites() -> Vec<(u64, u32, String)> {
    vec![
        (0x1202ee4, 0xf2c0b408, size("hos_wide_out")),
        (0x31f39f8, 0xf2c0b408, size("hos_wide_out")),
        (0x31f3a04, 0xf2c08708, size("hos_wide_scene")),
        (0x4c5334, 0xbd414260, ratio(0, None)),
        (0x3485cc0, 0x2d558400, ratio(1, Some(0))),
        (0x4f1e8, 0x2d558ea0, ratio(3, Some(21))),
        (0x23b9a8, 0x2d558c00, ratio(3, Some(0))),
        (0x33b82c, 0x2d558440, ratio(1, Some(2))),
        (0x774368, 0xad401041, hud()),
        (0x3b5c758, 0xa941a269, movie()),
        (0x284824, 0xf9400108, thumb()),
    ]
}

fn jitter_sites() -> Vec<(u64, u32, String)> {
    vec![
        (0x6bc708, 0xad415915, jitter(Some(19), Some(20))),
        (0x6bcf8c, 0x3dc0013a, jitter(Some(26), None)),
        (0x6bcf9c, 0x3dc00126, jitter(None, Some(6))),
    ]
}

fn occlusion_sites() -> Vec<(u64, u32, String)> {
    [
        0x11ace68u64,
        0x11b828c,
        0x11b82a4,
        0x11acec8,
        0x11b82dc,
        0x11b82f4,
    ]
    .iter()
    .map(|off| (*off, 0x39400008, occlusion()))
    .collect()
}

fn install<C: Code>(code: &C) -> BTreeMap<u64, String> {
    let mut after: BTreeMap<u64, String> = BTreeMap::new();
    let all = wide_sites()
        .into_iter()
        .chain(jitter_sites())
        .chain(occlusion_sites());
    for (off, word, text) in all {
        let pc = MAIN + off;
        if code.word(pc) != Some(word) {
            continue;
        }
        match after.get_mut(&pc) {
            Some(old) => {
                old.push('\n');
                old.push_str(&text);
            }
            None => {
                after.insert(pc, text);
            }
        }
    }
    let fov_pc = MAIN + FOV_SITE;
    if FOV_WORDS
        .iter()
        .enumerate()
        .all(|(i, word)| code.word(fov_pc + 4 * i as u64) == Some(*word))
    {
        after.insert(fov_pc, fov());
    }
    after
}

/// The snippets whose original word matches `code`, by guest address.
pub fn sites<C: Code>(code: &C) -> BTreeMap<u64, String> {
    install(code)
}

/// The same over explicit `{pc: word}` data, for tests and inspection.
pub fn sites_from(words: &BTreeMap<u64, u32>) -> BTreeMap<u64, String> {
    struct Words<'a>(&'a BTreeMap<u64, u32>);
    impl Code for Words<'_> {
        fn in_code(&self, _address: u64) -> bool {
            true
        }
        fn word(&self, address: u64) -> Option<u32> {
            self.0.get(&address).copied()
        }
        fn read(&self, _address: u64, _size: usize) -> Option<&[u8]> {
            None
        }
    }
    install(&Words(words))
}

pub fn sites_json(request: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut words = BTreeMap::new();
    if let Some(items) = request.get("words").and_then(serde_json::Value::as_array) {
        for item in items {
            let pair = item.as_array().ok_or("a word is [pc, word]")?;
            let pc = pair
                .first()
                .and_then(serde_json::Value::as_u64)
                .ok_or("pc")?;
            let word = pair
                .get(1)
                .and_then(serde_json::Value::as_u64)
                .ok_or("word")? as u32;
            words.insert(pc, word);
        }
    }
    let sites = sites_from(&words);
    Ok(serde_json::json!(
        sites
            .into_iter()
            .map(|(pc, text)| serde_json::json!([pc, text]))
            .collect::<Vec<_>>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words_with_sites() -> BTreeMap<u64, u32> {
        wide_sites()
            .into_iter()
            .chain(jitter_sites())
            .chain(occlusion_sites())
            .map(|(off, word, _)| (MAIN + off, word))
            .chain(
                FOV_WORDS
                    .iter()
                    .enumerate()
                    .map(|(i, word)| (MAIN + FOV_SITE + 4 * i as u64, *word)),
            )
            .collect()
    }

    #[test]
    fn installs_every_site_of_the_supported_words() {
        let sites = sites_from(&words_with_sites());
        assert_eq!(sites.len(), 21);
        assert_eq!(
            sites[&(MAIN + 0x1202ee4)],
            "{ extern volatile uint64_t hos_wide_out; const uint64_t _s = hos_wide_out; if (_s) c->x[8] = _s; }   /* (nx2/oot_wide.py: hos_wide_out) */"
        );
        assert!(sites[&(MAIN + 0x774368)].contains("_vt == 0x84c0d098ULL ? 1u"));
        assert!(sites[&(MAIN + 0x6bc708)].contains("_j[0]"));
        assert!(sites[&(MAIN + 0x6bc708)].contains("_j[1]"));
        assert!(sites[&(MAIN + 0x11ace68)].contains("hos_occlusion_off"));
    }

    #[test]
    fn leaves_out_a_site_with_another_word() {
        let mut words = words_with_sites();
        words.insert(MAIN + 0x1202ee4, 0);
        let sites = sites_from(&words);
        assert_eq!(sites.len(), 20);
        assert!(!sites.contains_key(&(MAIN + 0x1202ee4)));
    }

    #[test]
    fn stacks_snippets_at_one_address() {
        let sites = sites_from(&words_with_sites());
        assert_eq!(sites.len(), 21);
        assert!(sites.values().all(|text| !text.is_empty()));
    }

    #[test]
    fn installs_the_field_of_view_hook_only_when_all_three_words_match() {
        let words = words_with_sites();
        let pc = MAIN + FOV_SITE;
        let sites = sites_from(&words);
        assert!(sites[&pc].contains("hos_oot_fov(c, c->x[19])"));
        assert!(sites[&pc].contains("nx2/oot_fov.py: gameplay field of view"));
        for i in 0..FOV_WORDS.len() {
            let mut changed = words.clone();
            changed.insert(pc + 4 * i as u64, FOV_WORDS[i] ^ 1);
            assert!(
                !sites_from(&changed).contains_key(&pc),
                "the hook stayed with word {i} changed"
            );
        }
    }
}
