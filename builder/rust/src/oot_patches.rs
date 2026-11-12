//! The Ocarina of Time lifted-C patches, ported from nx2/oot_patches.py.
//!
//! The Python builder applies these after the lift and before the page marks.
//! This module is the native replacement: one read, the same ordered edits,
//! and one atomic write per chunk. The Rust engine has no Python fallback.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

// --- dynamic resolution -----------------------------------------------------

const DECL: &str = "extern volatile int hos_fixed_resolution;\nextern volatile int hos_dynres_runs;\nextern volatile uint32_t hos_fixed_scale_bits;\n";
const DYN_OLD: &str = "if (!((c->x[8] >> 0) & 1)) { goto L_8082640c; }";
const DYN_NEW: &str = "if (hos_fixed_resolution || !((c->x[8] >> 0) & 1)) { goto L_8082640c; }\n    hos_dynres_runs++;";
const DYN_LABEL: &str = "L_8082640c:;";
const SCALE_OLD: &str = "c->x[9] = 0x3f800000ULL;";
const SCALE_NEW: &str = "c->x[9] = hos_fixed_scale_bits;";
const DECL2: &str = "extern volatile int hos_fixed_resolution;\nextern volatile uint64_t hos_res_module;\nextern volatile uint32_t hos_fixed_scale_bits;\n";
const APPLY_OLD: &str = "L_831f3efc:;\n    c->x[9]=recomp_load32(c,c->x[20] + 3628);";
const APPLY_NEW: &str = "L_831f3efc:;\n    if (hos_fixed_resolution) {   /* (only while +0xe5c is not the chosen scale: a recompute every frame remade the targets) */\n        union { uint32_t u; float f; } now = {(uint32_t)recomp_load32(c,c->x[20] + 3676)}, want = {hos_fixed_scale_bits},\n            e50 = {(uint32_t)recomp_load32(c,c->x[20] + 3664)}, e58 = {(uint32_t)recomp_load32(c,c->x[20] + 3672)};\n        const float d = now.f - (e50.f != 0 ? want.f * e58.f / e50.f : want.f);\n        if (d > 1e-4f || d < -1e-4f) c->x[8] |= 8;\n    }\n    c->x[9]=recomp_load32(c,c->x[20] + 3628);";
const FLAGS_OLD: &str = "L_831f3f08:;\n    c->x[8]=recomp_load64(c,c->x[20] + 3616);\n";
const FLAGS_NEW: &str = "L_831f3f08:;\n    c->x[8]=recomp_load64(c,c->x[20] + 3616);\n    hos_res_module = c->x[20];\n    if (hos_fixed_resolution) c->x[8] &= ~0x18ULL;\n";

// --- text speed / size ------------------------------------------------------

const TEXT_DECL: &str = "extern volatile float hos_text_scale;\nextern void hos_oot_text_pane(Ctx *c, uint64_t anim);\n";
const TEXT_HEAD: &str = "void f_80b1e140(Ctx *__restrict A64_NOESC c) {";
const TEXT_OLD: &str = "L_80b1e1b4:;\n    c->x[9]=recomp_load16(c,c->x[19] + 110);\nif ((uint32_t)c->x[9] == 0) { goto L_80b1e18c; }\n";
const TEXT_OLD_POLLED: &str = "L_80b1e1b4:;\n    c->x[9]=recomp_load16(c,c->x[19] + 110);\nif ((uint32_t)c->x[9] == 0) { { A64_POLL(c); goto L_80b1e18c; } }\n";
const TEXT_ADD: &str = "    if (hos_text_scale != 1.0f) { float _t; uint32_t _u; memcpy(&_t,c->vreg[0],4); _t *= hos_text_scale; memcpy(&_u,&_t,4); a64_vset64(c->vreg[0],_u); }   /* (nx2/oot_patches.py: text speed) */\n    hos_oot_text_pane(c, c->x[19]);   /* (nx2/oot_patches.py: text size; x19 the LetterAnimControl) */\n";

// --- heart protection / flight / free camera ------------------------------

const LIFE_HEAD: &str = "void f_804bd8c0(Ctx *__restrict A64_NOESC c) {\n";
const LIFE_CAS: &str = "{ uint64_t _a = c->x[13]; uint32_t _e = (uint32_t)c->x[11]; __atomic_compare_exchange_n((uint32_t *)A64_PTR(_a), &_e, (uint32_t)c->x[12], 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); A64_MARK(_a, 4); c->x[11] = (uint64_t)_e; }";
const LIFE_GUARD: &str = "    if ((uint32_t)c->x[12] < (uint32_t)c->x[11] && hos_oot_hearts_enabled() &&\n        hos_oot_is_player(c, c->x[19]) &&\n        c->x[13] == recomp_load64(c,c->x[19] + 7184) + 8)\n        c->x[12] = c->x[11];\n";
const LIFE_DECL: &str = "extern int hos_oot_hearts_enabled(void);\nextern int hos_oot_is_player(Ctx *c, uint64_t actor);\n";
const MOVE_HEAD: &str = "void f_82c415e0(Ctx *__restrict A64_NOESC c) {\n";
const MOVE_ANCHOR: &str = "if (((c->x[8] >> 1) & 1)) { goto L_82c41748; }\n";
const MOVE_MATRIX: &str = "L_82c41778:;\n";
const MOVE_DECL: &str = "extern void hos_oot_lift_player(Ctx *c, uint64_t actor);\nextern void hos_oot_flight_pose(Ctx *c, uint64_t actor, uint64_t matrix);\n";
const WARP_HEAD: &str = "void f_806d2530(Ctx *__restrict A64_NOESC c) {\n";
const WARP_ANCHOR: &str =
    "{ uint64_t _t = c->x[8]; c->x[30] = 0x806d25c8ULL; A64_ICALL(c, _t); }\n";
const WARP_DECL: &str =
    "extern void hos_oot_flight_rotation(Ctx *c, uint64_t controller, uint64_t matrix);\n";
const VIEW_HEAD: &str = "void f_804e48c0(Ctx *__restrict A64_NOESC c) {\n";
const VIEW_ANCHORS: [&str; 2] = ["L_804e5068:;\n", "L_804e5068:; A64_POLL(c);\n"];
const VIEW_DECL: &str = "extern void hos_oot_free_view(Ctx *c, uint64_t view);\n";

// --- movement speed ---------------------------------------------------------

const SPEED_HOOK: &str = "    hos_oot_cct_speed(c, c->x[0], %d);   /* (nx2/oot_patches.py: movement speed; x0 the controller, s0 the speed, in metres a %s) */\n";
const SPEED_PLACES: [(&str, &str, u32); 4] = [
    (
        "void f_809552f0(Ctx *__restrict A64_NOESC c) {\n",
        "L_80955424:;\n    c->x[0]=recomp_load64(c,c->x[8] + 8);\n",
        0,
    ),
    (
        "void f_809552f0(Ctx *__restrict A64_NOESC c) {\n",
        "L_809554dc:;\n    c->x[0]=recomp_load64(c,c->x[8] + 8);\n",
        0,
    ),
    (
        "void f_80559df0(Ctx *__restrict A64_NOESC c) {\n",
        "void f_80559df0(Ctx *__restrict A64_NOESC c) {\n",
        1,
    ),
    (
        "void f_80964bf0(Ctx *__restrict A64_NOESC c) {\n",
        "void f_80964bf0(Ctx *__restrict A64_NOESC c) {\n    c->x[0]=recomp_load64(c,c->x[0] + 8);\n",
        1,
    ),
];
const SPEED_DECL: &str =
    "extern void hos_oot_cct_speed(Ctx *c, uint64_t controller, int per_second);\n";

// --- camera ----------------------------------------------------------------

const CAM_HEAD: &str = "void f_80644c80(Ctx *__restrict A64_NOESC c) {\n";
const CAM_FOLLOW_ANCHOR: &str = "L_8064720c:;\nif (!c->z) { goto L_806492c4; }\n";
const CAM_FOLLOW_JOIN: &str = "L_80645ad8:;";
const CAM_FOLLOW_HOOK: &str = "    if (hos_disable_camera_centering()) { c->x[25] = c->x[26]; goto L_80645ad8; }   /* (nx2/oot_patches.py: disable camera centering) */\n";
const CAMERA_SPEED_HEADS: [&str; 4] = [
    "void f_80643f10(Ctx *__restrict A64_NOESC c) {",
    "void f_80643b60(Ctx *__restrict A64_NOESC c) {",
    "void f_82ea21f0(Ctx *__restrict A64_NOESC c) {",
    "void f_82ea2220(Ctx *__restrict A64_NOESC c) {",
];
const CAMERA_SPEED_DECL: &str = "extern volatile float hos_camera_scale;\n";
const CAMERA_SPEED_RETURN: &str = "{ if (hos_camera_scale != 1.0f) { float _t; uint32_t _u; memcpy(&_t,c->vreg[0],4); _t *= hos_camera_scale; memcpy(&_u,&_t,4); a64_vset64(c->vreg[0],_u); } return; }   /* (nx2/oot_patches.py: camera speed) */";

// --- menus / prompts --------------------------------------------------------

const MENU_FOCUS_HEAD: &str = "void f_802ed660(Ctx *__restrict A64_NOESC c) {\n";
const MENU_FOCUS_CALL: &str = "    hos_oot_menu_focus(c, c->x[0], c->x[1], c->x[30]);   /* (nx2/oot_patches.py: the menus' focus) */\n";
const MENU_FOCUS_DECL: &str =
    "extern void hos_oot_menu_focus(Ctx *c, uint64_t screen, uint64_t pane, uint64_t lr);\n";
const MENU_TICK_HEAD: &str = "void f_802f1380(Ctx *__restrict A64_NOESC c) {\n";
const MENU_TICK_CALL: &str =
    "    hos_oot_menu_tick(c, c->x[0]);   /* (nx2/oot_patches.py: the menus' mouse) */\n";
const MENU_TICK_DECL: &str = "extern void hos_oot_menu_tick(Ctx *c, uint64_t screen);\n";
const TEXT_DRAW_HEAD: &str = "void f_8152b670(Ctx *__restrict A64_NOESC c) {\n";
const TEXT_DRAW_CALL: &str = "    hos_prompts_text_draw(c->x[0]);   /* (nx2/oot_patches.py: the button prompts changed) */\n";
const TEXT_DRAW_DECL: &str = "extern void hos_prompts_text_draw(uint64_t box);\n";
const TEXT_LAYOUT_HEAD: &str = "void f_8152aec0(Ctx *__restrict A64_NOESC c) {\n";
const TEXT_LAYOUT_CALL: &str = "    hos_prompts_text_layout(c->x[0]);   /* (nx2/oot_patches.py: the button prompts changed) */\n";
const TEXT_LAYOUT_DECL: &str = "extern void hos_prompts_text_layout(uint64_t box);\n";
const GLYPH_HEAD: &str = "void f_81680c60(Ctx *__restrict A64_NOESC c) {\n";
const GLYPH_CALL: &str = "    hos_prompts_glyph(c->x[1]);   /* (nx2/oot_patches.py: where the button prompts' font is) */\n";
const GLYPH_DECL: &str = "extern void hos_prompts_glyph(uint64_t slot);\n";

// --- frame time / whole frames / movie -------------------------------------

const STEP_SITES: [(&str, u32, &str); 2] = [
    (
        "void f_80706be0(Ctx *__restrict A64_NOESC c) {\n",
        8,
        "\n    hos_step_probe = c->x[19];",
    ),
    ("void f_83b6ea90(Ctx *__restrict A64_NOESC c) {\n", 0, ""),
];
const STEP_OLD: &str = "    _v=a64_fpx_div32_div32(c,_a,_b);\n    a64_vset64(c->vreg[%d],_v);}";
const STEP_HOOK: &str = "    _v=a64_fpx_div32_div32(c,_a,_b);\n    { const uint32_t _hs = hos_frame_seconds_bits; if (_hs) { float _base, _secs; uint32_t _w; memcpy(&_base,&_a,4); memcpy(&_secs,&_hs,4); _base *= _secs; memcpy(&_w,&_base,4); _v = _w; } }   /* (base * the frame's seconds: nx2/oot_patches.py) */\n    a64_vset64(c->vreg[%d],_v);}";
const STEP_MARK: &str = "hos_frame_seconds_bits; if (_hs)";
const STEP_DECL: &str =
    "extern volatile uint32_t hos_frame_seconds_bits;\nextern volatile uint64_t hos_step_probe;\n";
const WHOLE_SITES: [(&str, &str, &str); 3] = [
    (
        "void f_80a87e50(Ctx *__restrict A64_NOESC c) {@",
        "    c->x[12]=recomp_fp_to_int(c->vreg[0][0]&UINT64_C(0xffffffff),32,32,1,3,c->fpcr,&c->fpsr);@",
        "c->x[19] + 864",
    ),
    (
        "void f_800672e0(Ctx *__restrict A64_NOESC c) {@",
        "    c->x[9]=recomp_fp_to_int(c->vreg[0][0]&UINT64_C(0xffffffff),32,32,1,3,c->fpcr,&c->fpsr);@",
        "c->x[21]",
    ),
    (
        "void f_803edf70(Ctx *__restrict A64_NOESC c) {@",
        "    c->x[8]=recomp_fp_to_int(c->vreg[0][0]&UINT64_C(0xffffffff),32,32,1,3,c->fpcr,&c->fpsr);@    recomp_store32(c,c->x[19] + 60,c->x[8]);@",
        "c->x[19] + 60",
    ),
];
const WHOLE_MARK: &str = "/* (the step's whole frames: nx2/oot_patches.py) */";
const WHOLE_DECL: &str = "extern uint32_t hos_frame_whole(uint64_t at, uint32_t amount_bits);\n";
const MOVIE_HEAD: &str = "void f_83b7c710(Ctx *__restrict A64_NOESC c) {\n";
const MOVIE_OLD: &str = "    c->x[8]=recomp_load32(c,c->x[20] + 760);\n    { uint64_t _a=c->x[8], _b=1ULL; uint64_t _r=_a+_b; _r&=0xFFFFFFFFULL; c->x[8]=_r; }\n";
const MOVIE_NEW: &str = "    c->x[8]=recomp_load32(c,c->x[20] + 760);\n    { uint64_t _a=c->x[8], _b=hos_movie_frames(c->x[20]); uint64_t _r=_a+_b; _r&=0xFFFFFFFFULL; c->x[8]=_r; }\n";
const MOVIE_DECL: &str = "uint32_t hos_movie_frames(uint64_t player);\n";

// --- above 60 fps -----------------------------------------------------------

const SOUND_HEAD: &str = "void f_80435300(Ctx *__restrict A64_NOESC c) {\n";
const SOUND_OLD: &str = "    c->x[8]=g_module_base + 0x80435000ULL + (int64_t)75927552;\n    { uint64_t _r = ((uint64_t)0 | ((c->x[0]) << 0)); c->x[19] = _r; }\n    c->x[8]=recomp_load64(c,c->x[8] + 2192);\n    c->x[8]=recomp_load64(c,c->x[8] + 0);\n    { { uint64_t _v=recomp_load32(c,(c->x[8]+16ULL)); a64_vset64(c->vreg[0],_v); } }\n";
const SOUND_NEW: &str = "    c->x[8]=g_module_base + 0x80435000ULL + (int64_t)75927552;\n    { uint64_t _r = ((uint64_t)0 | ((c->x[0]) << 0)); c->x[19] = _r; }\n    c->x[8]=recomp_load64(c,c->x[8] + 2192);\n    c->x[8]=recomp_load64(c,c->x[8] + 0);\n    { { uint64_t _v=hos_oot_sound_step((uint32_t)recomp_load32(c,(c->x[8]+16ULL))); a64_vset64(c->vreg[0],_v); } }\n";
const SOUND_GATE: &str = "    if (!hos_oot_sound_gate(c, (uint32_t)c->x[2], g_module_base + 0x80435000ULL + (int64_t)75927552 + 2192)) return;   /* (nx2/oot_patches.py: the sound job at 60 Hz above 60 fps) */\n";
const SOUND_DECL: &str = "extern int hos_oot_sound_gate(Ctx *c, uint32_t phase, uint64_t slot);\nextern uint32_t hos_oot_sound_step(uint32_t step_bits);\n";
const SOLVER_HEADS: [&str; 2] = [
    "void f_838ceb80(Ctx *__restrict A64_NOESC c) {\n",
    "void f_838cfc10(Ctx *__restrict A64_NOESC c) {\n",
];
const SOLVER_OLD: &str = "    c->x[1]=recomp_fp_to_int(c->vreg[0][0]&UINT64_C(0xffffffff),32,32,1,3,c->fpcr,&c->fpsr);\n    recomp_store32(c,c->x[0] + 712,c->x[1]);\n";
const SOLVER_NEW: &str = "    c->x[1]=recomp_fp_to_int(c->vreg[0][0]&UINT64_C(0xffffffff),32,32,1,3,c->fpcr,&c->fpsr);\n    c->x[1]=hos_oot_solver_steps((uint32_t)c->x[1], (uint32_t)c->vreg[0][0], (uint32_t)c->vreg[1][0]);   /* (nx2/oot_patches.py: solver steps rounded up above 60 fps) */\n    recomp_store32(c,c->x[0] + 712,c->x[1]);\n";
const SOLVER_DECL: &str = "extern uint32_t hos_oot_solver_steps(uint32_t game, uint32_t scaled_bits, uint32_t per_frame_bits);\n";

// --- heap retry -------------------------------------------------------------

const GPU_ALLOC_SITES: [u64; 2] = [0x80456f1c, 0x80457098];
const HEAP_ALLOC_SITES: [u64; 2] = [0x80456910, 0x80456da4];
const GPU_ALLOC_DECL: &str = "void hos_oot_gpu_alloc(Ctx *A64_NOESC c, A64Fn game);\n";
const HEAP_ALLOC_DECL: &str = "int hos_oot_heap_refuse(void);\nvoid hos_oot_heap_retry(Ctx *c, uint64_t heap, uint64_t size, uint64_t align, uint64_t lr);\n";

// --- output size ------------------------------------------------------------

const OUT_DECL: &str =
    "uint32_t hos_output_size(int height);\nuint64_t hos_rt_pool_size(uint64_t game);\n";
const OUT_EDITS: [(&str, &str); 8] = [
    (
        "    c->x[8] = 0xa00ULL;\n    c->x[8] = (c->x[8] & ~(0xFFFFULL<<32)) | (0x5a0ULL<<32);\n{ extern volatile uint64_t hos_wide_out; const uint64_t _s = hos_wide_out; if (_s) c->x[8] = _s; }   /* (nx2/oot_wide.py: hos_wide_out) */\n    { uint64_t _b=c->x[21]; int64_t _o=20; recomp_store64(c,(_b+_o),c->x[8]); }",
        "    c->x[8] = hos_output_size(0) | ((uint64_t)hos_output_size(1) << 32);\n{ extern volatile uint64_t hos_wide_out; const uint64_t _s = hos_wide_out; if (_s) c->x[8] = _s; }   /* (nx2/oot_wide.py: hos_wide_out) */\n    { uint64_t _b=c->x[21]; int64_t _o=20; recomp_store64(c,(_b+_o),c->x[8]); }",
    ),
    (
        "    { uint64_t _a=c->x[0], _b=724ULL; uint64_t _r=_a+_b; c->x[21]=_r; }\n    { uint64_t _r = ((uint64_t)0 | ((c->x[1]) << 0)); c->x[20] = _r; }\n",
        "    { uint64_t _a=c->x[0], _b=724ULL; uint64_t _r=_a+_b; c->x[21]=_r; }\n    { uint64_t _r = ((uint64_t)0 | ((c->x[1]) << 0)); c->x[20] = _r; }\n    if ((hos_output_size(0) != 2560 || hos_output_size(1) != 1440) && recomp_load32(c, c->x[20] + 8) == 2560) { recomp_store32(c, c->x[20] + 8, hos_output_size(0)); recomp_store32(c, c->x[20] + 12, hos_output_size(1)); }\n",
    ),
    (
        "c->x[30] = 0x80ea2ffcULL; f_831cca50(c);\n    { uint64_t _b=c->x[0]; int64_t _o=0; { uint64_t _p0,_p1; recomp_ldp32(c,(_b+_o),&_p0,&_p1); c->x[24]=_p0; c->x[25]=_p1; } }\n",
        "c->x[30] = 0x80ea2ffcULL; f_831cca50(c);\n    { uint64_t _b=c->x[0]; int64_t _o=0; { uint64_t _p0,_p1; recomp_ldp32(c,(_b+_o),&_p0,&_p1); c->x[24]=_p0; c->x[25]=_p1; } }\n    if (hos_output_size(0) != 2560 || hos_output_size(1) != 1440) { c->x[24] = (uint32_t)c->x[24] * hos_output_size(0) / 2560; c->x[25] = (uint32_t)c->x[25] * hos_output_size(1) / 1440; }\n",
    ),
    (
        "    c->x[8] = 0x44f00000ULL;\n    c->x[8] &= 0xFFFFFFFFULL;\n    recomp_store32(c,c->x[19] + 3668,c->x[8]);\n    c->x[8] = 0x44870000ULL;\n    c->x[8] &= 0xFFFFFFFFULL;\n",
        "    c->x[8] = hos_output_size(0) * 3 / 4;\n    { float _f = (float)c->x[8]; uint32_t _u; memcpy(&_u, &_f, 4); c->x[8] = _u; }\n    recomp_store32(c,c->x[19] + 3668,c->x[8]);\n    c->x[8] = hos_output_size(1) * 3 / 4;\n    { float _f = (float)c->x[8]; uint32_t _u; memcpy(&_u, &_f, 4); c->x[8] = _u; }\n",
    ),
    (
        "L_836cea54:;\n",
        "L_836cea54:;\n    if (recomp_load64(c, c->x[19] + 1464) == 0x13100000ULL) recomp_store64(c, c->x[19] + 1464, hos_rt_pool_size(0x13100000ULL));\n",
    ),
    (
        "c->x[30] = 0x83b7b440ULL; f_809df710(c);\n",
        "if (c->x[0] == 0x34900000ULL || c->x[0] == 0x348ffe20ULL) c->x[0] += hos_rt_pool_size(0x13100000ULL) - 0x13100000ULL;\nc->x[30] = 0x83b7b440ULL; f_809df710(c);\n",
    ),
    (
        "{ uint64_t _t = c->x[8]; c->x[30] = 0x83b7b4e0ULL; A64_ICALL(c, _t); }\n",
        "{ uint64_t _t = c->x[8]; c->x[30] = 0x83b7b4e0ULL; A64_ICALL(c, _t); }\n{ const uint64_t _b = recomp_load64(c, c->x[21] + (c->x[22] << 3)), _g = hos_rt_pool_size(0x13100000ULL) - 0x13100000ULL; if ((_b == 0x34900000ULL || _b == 0x348ffe20ULL) && c->x[0] > _g) c->x[0] -= _g; }\n",
    ),
    (
        "c->x[30] = 0x80b723acULL; f_809df700(c);\n",
        "if (c->x[0] > hos_rt_pool_size(0x13100000ULL) - 0x13100000ULL) c->x[0] -= hos_rt_pool_size(0x13100000ULL) - 0x13100000ULL;\nc->x[30] = 0x80b723acULL; f_809df700(c);\n",
    ),
];

fn replace_once(hay: &str, old: &str, new: &str) -> Option<String> {
    let at = hay.find(old)?;
    let mut out = String::with_capacity(hay.len() + new.len());
    out.push_str(&hay[..at]);
    out.push_str(new);
    out.push_str(&hay[at + old.len()..]);
    Some(out)
}

fn add_decl(s: &str, decl: &str) -> String {
    if s.contains(decl) {
        return s.to_owned();
    }
    replace_once(
        s,
        "#include \"a64.h\"\n",
        &format!("#include \"a64.h\"\n{decl}"),
    )
    .unwrap_or_else(|| s.to_owned())
}

fn function(s: &str, head: &str) -> Option<(usize, usize)> {
    let start = s.find(head)?;
    let after = start + head.len();
    let end = s[after..]
        .find("\nvoid f_")
        .map(|at| after + at)
        .unwrap_or(s.len());
    Some((start, end))
}

fn function_body<'a>(s: &'a str, head: &str) -> Option<(usize, usize, &'a str)> {
    let (start, end) = function(s, head)?;
    Some((start, end, &s[start..end]))
}

fn patch_dynres(s: &str) -> Option<String> {
    if !s.contains(DYN_OLD) {
        return None;
    }
    let s = replace_once(s, DYN_OLD, DYN_NEW)?;
    let at = s.find(DYN_LABEL)?;
    let scale = s[at..].find(SCALE_OLD).map(|i| at + i)?;
    let mut out = String::with_capacity(s.len());
    out.push_str(&s[..scale]);
    out.push_str(SCALE_NEW);
    out.push_str(&s[scale + SCALE_OLD.len()..]);
    Some(add_decl(&out, DECL))
}

fn fixed_scale(s: &str) -> (String, u32) {
    if !s.contains(APPLY_OLD) || !s.contains(FLAGS_OLD) {
        return (s.to_owned(), 0);
    }
    let out =
        replace_once(s, APPLY_OLD, APPLY_NEW).and_then(|t| replace_once(&t, FLAGS_OLD, FLAGS_NEW));
    match out {
        Some(t) => (add_decl(&t, DECL2), 1),
        None => (s.to_owned(), 0),
    }
}

fn patch_text_speed(s: &str) -> Option<(String, bool)> {
    let (start, end, body) = function_body(s, TEXT_HEAD)?;
    let old = [TEXT_OLD_POLLED, TEXT_OLD]
        .into_iter()
        .find(|old| body.contains(&format!("{old}{TEXT_ADD}")) || body.matches(old).count() == 1);
    let Some(old) = old else {
        return Some((s.to_owned(), false));
    };
    if body.contains(&format!("{old}{TEXT_ADD}")) || body.matches("goto L_80b1e1b4;").count() != 1 {
        return Some((s.to_owned(), false));
    }
    let body = replace_once(body, old, &format!("{old}{TEXT_ADD}"))?;
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    Some((add_decl(&out, TEXT_DECL), true))
}

fn patch_movie_frames(s: &str) -> Option<(String, bool)> {
    let (start, end, body) = function_body(s, MOVIE_HEAD)?;
    if body.matches(MOVIE_OLD).count() != 1 {
        return Some((s.to_owned(), false));
    }
    let body = replace_once(body, MOVIE_OLD, MOVIE_NEW)?;
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    Some((add_decl(&out, MOVIE_DECL), true))
}

fn whole_new(old: &str, key: &str) -> String {
    let mut lines: Vec<String> = old.split('\n').map(str::to_owned).collect();
    for line in &mut lines {
        if line.contains("recomp_fp_to_int(c->vreg[0][0]") {
            let reg = line.split('=').next().unwrap_or_default();
            *line =
                format!("{reg}=hos_frame_whole({key}, (uint32_t)c->vreg[0][0]);   {WHOLE_MARK}");
        }
    }
    lines.join("\n")
}

fn patch_whole_frames(s: &str) -> (String, u32) {
    let mut out = s.to_owned();
    let mut done = 0u32;
    for (head, old, key) in WHOLE_SITES {
        let head = head.replace('@', "\n");
        let old = old.replace('@', "\n");
        let Some((start, end, body)) = function_body(&out, &head) else {
            continue;
        };
        let new = whole_new(&old, key);
        if body.contains(&new) || body.matches(&old).count() != 1 {
            continue;
        }
        let body = replace_once(body, &old, &new).unwrap_or_else(|| body.to_owned());
        out = format!("{}{}{}", &out[..start], body, &out[end..]);
        done += 1;
    }
    if done > 0 {
        out = add_decl(&out, WHOLE_DECL);
    }
    (out, done)
}

fn patch_sound_job(s: &str) -> (String, u32) {
    let Some((start, end, body)) = function_body(s, SOUND_HEAD) else {
        return (s.to_owned(), 0);
    };
    if body.matches(SOUND_NEW).count() == 1 && body.matches(SOUND_GATE).count() == 1 {
        return (s.to_owned(), 1);
    }
    if body.matches(SOUND_OLD).count() != 1 || body.contains(SOUND_GATE) {
        return (s.to_owned(), 0);
    }
    let body = replace_once(body, SOUND_OLD, SOUND_NEW).unwrap_or_else(|| body.to_owned());
    let body = format!("{SOUND_HEAD}{SOUND_GATE}{}", &body[SOUND_HEAD.len()..]);
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    (add_decl(&out, SOUND_DECL), 1)
}

fn patch_solver_steps(s: &str) -> (String, u32) {
    let mut out = s.to_owned();
    let mut done = 0u32;
    for head in SOLVER_HEADS {
        let Some((start, end, body)) = function_body(&out, head) else {
            continue;
        };
        if body.matches(SOLVER_NEW).count() == 1 {
            done += 1;
        } else if body.matches(SOLVER_OLD).count() == 1 {
            let body =
                replace_once(body, SOLVER_OLD, SOLVER_NEW).unwrap_or_else(|| body.to_owned());
            out = format!("{}{}{}", &out[..start], body, &out[end..]);
            done += 1;
        }
    }
    if done > 0 {
        out = add_decl(&out, SOLVER_DECL);
    }
    (out, done)
}

fn patch_frame_seconds(s: &str) -> (String, u32) {
    let mut out = s.to_owned();
    let mut done = 0u32;
    for (head, reg, extra) in STEP_SITES {
        let Some((start, end, body)) = function_body(&out, head) else {
            continue;
        };
        let old = STEP_OLD.replace("%d", &reg.to_string());
        let new = format!("{}{extra}", STEP_HOOK.replace("%d", &reg.to_string()));
        if body.contains(&new)
            || body.matches(&old).count() != 1
            || body.matches("a64_fpx_div32_div32").count() != 1
        {
            continue;
        }
        let body = replace_once(body, &old, &new).unwrap_or_else(|| body.to_owned());
        out = format!("{}{}{}", &out[..start], body, &out[end..]);
        done += 1;
    }
    if done > 0 {
        out = add_decl(&out, STEP_DECL);
    }
    (out, done)
}

fn patch_heap_retry(s: &str) -> (String, [u32; 2]) {
    let mut out = s.to_owned();
    let mut found = [0u32, 0u32];
    let mut decls: Vec<&str> = Vec::new();
    for lr in GPU_ALLOC_SITES {
        let old = format!("c->x[30] = {lr:#x}ULL; f_80522050(c);");
        let new = format!(
            "c->x[30] = {lr:#x}ULL; hos_oot_gpu_alloc(c, f_80522050);   /* (nx2/oot_patches.py: host/oot_heap_retry.c) */"
        );
        if out.contains(&new) {
            found[0] += 1;
        } else if out.matches(&old).count() == 1 && out.matches("#include \"a64.h\"\n").count() == 1
        {
            out = replace_once(&out, &old, &new).unwrap_or(out);
            found[0] += 1;
            if !decls.contains(&GPU_ALLOC_DECL) {
                decls.push(GPU_ALLOC_DECL);
            }
        }
    }
    for lr in HEAP_ALLOC_SITES {
        let old =
            format!("{{ uint64_t _t = c->x[8]; c->x[30] = {lr:#x}ULL; A64_ICALL(c, _t); }}\n");
        let new = format!(
            "{{ const uint64_t _hh = c->x[0], _hn = c->x[1], _ha = c->x[2];   /* (nx2/oot_patches.py: host/oot_heap_retry.c) */\n  if (hos_oot_heap_refuse()) c->x[0] = 0;\n  else {{ uint64_t _t = c->x[8]; c->x[30] = {lr:#x}ULL; A64_ICALL(c, _t); }}\n  if (!c->x[0]) hos_oot_heap_retry(c, _hh, _hn, _ha, {lr:#x}ULL); }}\n"
        );
        if out.contains(&new) {
            found[1] += 1;
        } else if out.matches(&old).count() == 1 && out.matches("#include \"a64.h\"\n").count() == 1
        {
            out = replace_once(&out, &old, &new).unwrap_or(out);
            found[1] += 1;
            if !decls.contains(&HEAP_ALLOC_DECL) {
                decls.push(HEAP_ALLOC_DECL);
            }
        }
    }
    for decl in decls {
        out = add_decl(&out, decl);
    }
    (out, found)
}

fn patch_camera_speed(s: &str) -> (String, u32) {
    let mut out = s.to_owned();
    let mut found = 0u32;
    for head_text in CAMERA_SPEED_HEADS {
        let Some((start, end, body)) = function_body(&out, head_text) else {
            continue;
        };
        if body.contains(CAMERA_SPEED_RETURN) {
            found += 1;
            continue;
        }
        let re = regex::Regex::new(r"\breturn;").expect("return regex");
        if !re.is_match(body) {
            continue;
        }
        let body = re.replace_all(body, CAMERA_SPEED_RETURN).into_owned();
        out = format!("{}{}{}", &out[..start], body, &out[end..]);
        found += 1;
    }
    if found > 0 {
        out = add_decl(&out, CAMERA_SPEED_DECL);
    }
    (out, found)
}

fn patch_cct_speed(s: &str) -> Option<(String, u32)> {
    if !SPEED_PLACES.iter().any(|(head, _, _)| s.contains(head)) {
        return None;
    }
    let mut out = s.to_owned();
    let mut n = 0u32;
    for (head, anchor, per_second) in SPEED_PLACES {
        let Some(start) = out.find(head) else {
            continue;
        };
        let end = out[start..].find("\n}").map(|at| start + at);
        let body = &out[start..end.unwrap_or(out.len())];
        let hook = SPEED_HOOK
            .replacen("%d", &per_second.to_string(), 1)
            .replacen("%s", if per_second == 1 { "second" } else { "frame" }, 1);
        if body.matches(&format!("{anchor}{hook}")).count() == 1 {
            n += 1;
        } else if end.is_some() && body.matches(anchor).count() == 1 {
            let replaced = replace_once(body, anchor, &format!("{anchor}{hook}"))
                .unwrap_or_else(|| body.to_owned());
            out = format!(
                "{}{}{}",
                &out[..start],
                replaced,
                &out[end.unwrap_or(out.len())..]
            );
            n += 1;
        }
    }
    if n > 0 {
        out = add_decl(&out, SPEED_DECL);
    }
    Some((out, n))
}

fn hook_head(s: &str, head: &str, call: &str, decl: &str) -> Option<String> {
    let start = s.find(head)?;
    let marker = format!("{}(", call.split('(').next().unwrap_or_default());
    if s.contains(&marker) {
        return Some(s.to_owned());
    }
    let at = start + head.len();
    let mut out = String::with_capacity(s.len() + call.len());
    out.push_str(&s[..at]);
    out.push_str(call);
    out.push_str(&s[at..]);
    Some(add_decl(&out, decl))
}

fn patch_life(s: &str) -> Option<String> {
    let start = s.find(LIFE_HEAD)?;
    let end = s[start..].find("\n}").map(|at| start + at)?;
    let body = &s[start..end];
    if body.matches(LIFE_CAS).count() != 2 {
        return None;
    }
    let body = body.replace(LIFE_CAS, &format!("{LIFE_GUARD}{LIFE_CAS}"));
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    Some(add_decl(&out, LIFE_DECL))
}

fn patch_movement(s: &str) -> Option<String> {
    let start = s.find(MOVE_HEAD)?;
    let end = s[start..].find("\n}").map(|at| start + at)?;
    let body = &s[start..end];
    if body.matches(MOVE_ANCHOR).count() != 1 || body.matches(MOVE_MATRIX).count() != 1 {
        return None;
    }
    let body = body.replace(
        MOVE_ANCHOR,
        &format!("    hos_oot_lift_player(c, c->x[19]);\n{MOVE_ANCHOR}"),
    );
    let body = body.replacen(
        MOVE_MATRIX,
        &format!("{MOVE_MATRIX}    hos_oot_flight_pose(c, c->x[19], c->x[25]);\n"),
        1,
    );
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    Some(add_decl(&out, MOVE_DECL))
}

fn patch_flight_rotation(s: &str) -> Option<String> {
    let start = s.find(WARP_HEAD)?;
    let end = s[start..].find("\n}").map(|at| start + at)?;
    let body = &s[start..end];
    if body.matches(WARP_ANCHOR).count() != 1 {
        return None;
    }
    let body = body.replace(
        WARP_ANCHOR,
        &format!("    hos_oot_flight_rotation(c, c->x[0], c->x[1]);\n{WARP_ANCHOR}"),
    );
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    Some(add_decl(&out, WARP_DECL))
}

fn patch_view(s: &str) -> Option<String> {
    let start = s.find(VIEW_HEAD)?;
    let end = s[start..].find("\n}").map(|at| start + at)?;
    let body = &s[start..end];
    let anchor = VIEW_ANCHORS
        .iter()
        .find(|anchor| body.matches(**anchor).count() == 1)?;
    let body = body.replace(
        anchor,
        &format!("{anchor}    hos_oot_free_view(c, c->x[20]);\n"),
    );
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    Some(add_decl(&out, VIEW_DECL))
}

fn patch_mouse_camera(s: &str) -> Option<String> {
    let start = s.find(CAM_HEAD)?;
    if s.contains("hos_mouse_camera(c->x[0]);") {
        return Some(s.to_owned());
    }
    let at = start + CAM_HEAD.len();
    let mut out = String::with_capacity(s.len() + 120);
    out.push_str(&s[..at]);
    out.push_str(
        "    hos_mouse_camera(c->x[0]);   /* (nx2/oot_patches.py: native mouse look) */\n",
    );
    out.push_str(&s[at..]);
    Some(add_decl(
        &out,
        "extern void hos_mouse_camera(uint64_t camera);\n",
    ))
}

fn patch_camera_centering(s: &str) -> Option<String> {
    let (start, end, body) = function_body(s, CAM_HEAD)?;
    if body.matches(CAM_FOLLOW_ANCHOR).count() != 1 || body.matches(CAM_FOLLOW_JOIN).count() != 1 {
        return None;
    }
    if body.contains(CAM_FOLLOW_HOOK) {
        return Some(s.to_owned());
    }
    let body = replace_once(
        body,
        CAM_FOLLOW_ANCHOR,
        &format!("{CAM_FOLLOW_ANCHOR}{CAM_FOLLOW_HOOK}"),
    )?;
    let out = format!("{}{}{}", &s[..start], body, &s[end..]);
    Some(add_decl(
        &out,
        "extern int hos_disable_camera_centering(void);\n",
    ))
}

// --- driver -----------------------------------------------------------------

fn code_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("code_")
            && name.ends_with(".c")
            && name.len() == "code_000.c".len()
            && name[5..8].bytes().all(|b| b.is_ascii_digit())
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

fn write_whole(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("c.part");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn apply_folder(
    folder: &str,
    protect_hearts: bool,
    required: bool,
) -> Result<Vec<String>, String> {
    let folder = Path::new(folder);
    if let Some(dump) = std::env::var_os("HOOKSHOT_PATCHES_DUMP") {
        let dump = std::path::PathBuf::from(dump);
        std::fs::create_dir_all(&dump).map_err(|e| format!("{}: {e}", dump.display()))?;
        for path in code_files(folder)? {
            let name = path.file_name().unwrap_or_default();
            std::fs::copy(&path, dump.join(name))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    let mut log = Vec::new();
    let mut done: BTreeMap<&'static str, u32> = [
        ("dynamic resolution branch", 0),
        ("text speed block", 0),
        ("fixed path's scale", 0),
        ("movie's frame count", 0),
    ]
    .into_iter()
    .collect();
    if protect_hearts {
        for what in [
            "menu focus hook",
            "menu tick hook",
            "text draw hook",
            "glyph hook",
            "text layout hook",
            "player heart protection",
            "player flight",
            "player flight rotation",
            "free camera",
            "mouse camera",
            "disable camera centering",
        ] {
            done.insert(what, 0);
        }
    }
    let mut speed_hooks = 0u32;
    let mut camera_speed = 0u32;
    let mut step_sites = 0u32;
    let mut step_files: Vec<PathBuf> = Vec::new();
    let mut whole_sites = 0u32;
    let mut sound_jobs = 0u32;
    let mut solver_sites = 0u32;
    let mut heap_sites = [0u32, 0u32];

    for path in code_files(folder)? {
        let s = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let base = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut out = s.clone();
        if let Some(t) = patch_dynres(&out) {
            out = t;
            *done.get_mut("dynamic resolution branch").unwrap() += 1;
            log.push(format!("oot_patches: dynamic resolution switch in {base}"));
        }
        if let Some((t, changed)) = patch_text_speed(&out) {
            if changed {
                out = t;
                *done.get_mut("text speed block").unwrap() += 1;
                log.push(format!(
                    "oot_patches: text speed scale and text size in {base}"
                ));
            }
        }
        if protect_hearts {
            if let Some(t) = hook_head(&out, MENU_FOCUS_HEAD, MENU_FOCUS_CALL, MENU_FOCUS_DECL) {
                out = t;
                *done.get_mut("menu focus hook").unwrap() += 1;
                log.push(format!("oot_patches: the menus' focus hook in {base}"));
            }
            if let Some(t) = hook_head(&out, MENU_TICK_HEAD, MENU_TICK_CALL, MENU_TICK_DECL) {
                out = t;
                *done.get_mut("menu tick hook").unwrap() += 1;
                log.push(format!("oot_patches: the menus' update hook in {base}"));
            }
            if let Some(t) = hook_head(&out, TEXT_DRAW_HEAD, TEXT_DRAW_CALL, TEXT_DRAW_DECL) {
                out = t;
                *done.get_mut("text draw hook").unwrap() += 1;
                log.push(format!("oot_patches: the text boxes' draw hook in {base}"));
            }
            if let Some(t) = hook_head(&out, GLYPH_HEAD, GLYPH_CALL, GLYPH_DECL) {
                out = t;
                *done.get_mut("glyph hook").unwrap() += 1;
                log.push(format!("oot_patches: the glyphs' hook in {base}"));
            }
            if let Some(t) = hook_head(&out, TEXT_LAYOUT_HEAD, TEXT_LAYOUT_CALL, TEXT_LAYOUT_DECL) {
                out = t;
                *done.get_mut("text layout hook").unwrap() += 1;
                log.push(format!(
                    "oot_patches: the text boxes' layout hook in {base}"
                ));
            }
        }
        if let Some((t, changed)) = patch_movie_frames(&out) {
            if t.contains(MOVIE_NEW) {
                *done.get_mut("movie's frame count").unwrap() += 1;
            }
            if changed {
                log.push(format!(
                    "oot_patches: the movie's frame count by the frames' time in {base}"
                ));
            }
            out = t;
        }
        let (t, n) = fixed_scale(&out);
        out = t;
        if n > 0 {
            *done.get_mut("fixed path's scale").unwrap() += 1;
            log.push(format!("oot_patches: the fixed path's scale in {base}"));
        }
        if protect_hearts {
            if let Some(t) = patch_life(&out) {
                out = t;
                *done.get_mut("player heart protection").unwrap() += 1;
                log.push(format!("oot_patches: player heart protection in {base}"));
            }
            if let Some(t) = patch_movement(&out) {
                out = t;
                *done.get_mut("player flight").unwrap() += 1;
                log.push(format!("oot_patches: player flight in {base}"));
            }
            if let Some(t) = patch_flight_rotation(&out) {
                out = t;
                *done.get_mut("player flight rotation").unwrap() += 1;
                log.push(format!("oot_patches: player flight rotation in {base}"));
            }
            if let Some(t) = patch_mouse_camera(&out) {
                out = t;
                *done.get_mut("mouse camera").unwrap() += 1;
                log.push(format!("oot_patches: native mouse camera in {base}"));
            }
            if let Some(t) = patch_camera_centering(&out) {
                out = t;
                *done.get_mut("disable camera centering").unwrap() += 1;
                log.push(format!(
                    "oot_patches: disable camera centering switch in {base}"
                ));
            }
            if let Some((t, n)) = patch_cct_speed(&out) {
                if t != out {
                    log.push(format!("oot_patches: movement speed in {base}"));
                }
                speed_hooks += n;
                out = t;
            }
            if let Some(t) = patch_view(&out) {
                out = t;
                *done.get_mut("free camera").unwrap() += 1;
                log.push(format!("oot_patches: free camera in {base}"));
            }
            let (t, n) = patch_camera_speed(&out);
            if t != out {
                log.push(format!(
                    "oot_patches: camera speed at {n} function(s) in {base}"
                ));
            }
            camera_speed += n;
            out = t;
        }
        let (t, n) = patch_frame_seconds(&out);
        out = t;
        if out.contains(STEP_MARK) {
            step_sites += out.matches(STEP_MARK).count() as u32;
            step_files.push(path.clone());
        }
        if n > 0 {
            log.push(format!(
                "oot_patches: the frame's time at {n} step divide(s) in {base}"
            ));
        }
        let (t, found) = if protect_hearts {
            patch_heap_retry(&out)
        } else {
            (out.clone(), [0, 0])
        };
        out = t;
        heap_sites[0] += found[0];
        heap_sites[1] += found[1];
        if out != s && found != [0, 0] {
            log.push(format!(
                "oot_patches: heap-full retries of the OptimizeChangeBank allocations in {base}"
            ));
        }
        let (t, whole) = patch_whole_frames(&out);
        out = t;
        whole_sites += out.matches(WHOLE_MARK).count() as u32;
        if whole > 0 {
            log.push(format!(
                "oot_patches: {whole} frame counter(s) with the step's whole frames in {base}"
            ));
        }
        let (t, sound) = patch_sound_job(&out);
        out = t;
        sound_jobs += sound;
        if sound > 0 {
            log.push(format!(
                "oot_patches: the sound job at 60 Hz above 60 fps in {base}"
            ));
        }
        let (t, solver) = patch_solver_steps(&out);
        out = t;
        solver_sites += solver;
        if solver > 0 {
            log.push(format!(
                "oot_patches: the solver steps rounded up above 60 fps in {base}"
            ));
        }
        if out != s {
            write_whole(&path, &out)?;
        }
    }

    if protect_hearts && heap_sites != [GPU_ALLOC_SITES.len() as u32, HEAP_ALLOC_SITES.len() as u32]
    {
        log.push(format!(
            "oot_patches: WARNING: the OptimizeChangeBank allocations were found {heap_sites:?} times, not once each: a full heap is not tried again"
        ));
    }
    for (what, n) in &done {
        if *n != 1 {
            log.push(format!(
                "oot_patches: WARNING: the {what} was found {n} times"
            ));
        }
    }
    if protect_hearts && speed_hooks != SPEED_PLACES.len() as u32 {
        log.push(format!(
            "oot_patches: WARNING: the movement speed's places were found {speed_hooks} times, not {}",
            SPEED_PLACES.len()
        ));
    }
    if protect_hearts && camera_speed != CAMERA_SPEED_HEADS.len() as u32 {
        log.push(format!(
            "oot_patches: WARNING: the camera's turn speed was found in {camera_speed} of {} functions (the Camera speed row turns only those)",
            CAMERA_SPEED_HEADS.len()
        ));
    }
    if step_sites != STEP_SITES.len() as u32 {
        for path in &step_files {
            let s =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut out = s.clone();
            for (_, reg, extra) in STEP_SITES {
                let hook = format!("{}{extra}", STEP_HOOK.replace("%d", &reg.to_string()));
                let old = STEP_OLD.replace("%d", &reg.to_string());
                out = out.replace(&hook, &old);
            }
            if !out.contains(STEP_MARK) {
                out = out.replacen(STEP_DECL, "", 1);
            }
            write_whole(path, &out)?;
        }
        log.push(format!(
            "oot_patches: WARNING: the frame's time was found at {step_sites} of {} step divides, none patched (a Frame rate cap above 60 keeps the game's own step)",
            STEP_SITES.len()
        ));
    } else if whole_sites != WHOLE_SITES.len() as u32 {
        log.push(format!(
            "oot_patches: WARNING: {whole_sites} of {} frame counters found for the step's whole frames (above 60 fps a counter can stop: the pick-up indicator, the water ripples, the time of day)",
            WHOLE_SITES.len()
        ));
    }
    if sound_jobs != 1 {
        log.push(format!(
            "oot_patches: WARNING: the sound job was found {sound_jobs} times, not once"
        ));
    }
    if solver_sites != SOLVER_HEADS.len() as u32 {
        log.push(format!(
            "oot_patches: WARNING: the solver step count was found at {solver_sites} of {} places",
            SOLVER_HEADS.len()
        ));
    }
    apply_output_size(folder, required, &mut log)?;
    if let Some(dump) = std::env::var_os("HOOKSHOT_PATCHES_DUMP_OUT") {
        let dump = std::path::PathBuf::from(dump);
        std::fs::create_dir_all(&dump).map_err(|e| format!("{}: {e}", dump.display()))?;
        for path in code_files(folder)? {
            let name = path.file_name().unwrap_or_default();
            std::fs::copy(&path, dump.join(name))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(log)
}

fn apply_output_size(folder: &Path, required: bool, log: &mut Vec<String>) -> Result<(), String> {
    let mut found = [0usize; OUT_EDITS.len()];
    for path in code_files(folder)? {
        let s = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut out = s.clone();
        let mut hit = false;
        for (index, (old, new)) in OUT_EDITS.iter().enumerate() {
            let made = out.matches(new).count();
            let raw = if made > 0 {
                out.replace(new, "\0").matches(old).count()
            } else {
                out.matches(old).count()
            };
            found[index] += made + raw;
            if raw > 0 && made == 0 {
                out = out.replace(old, new);
                hit = true;
            }
        }
        if hit {
            out = add_decl(&out, OUT_DECL);
            write_whole(&path, &out)?;
            log.push(format!(
                "oot_patches: output size in {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    }
    if found != [1usize; OUT_EDITS.len()] {
        let wrong: Vec<usize> = found
            .iter()
            .enumerate()
            .filter(|(_, n)| **n != 1)
            .map(|(i, _)| i)
            .collect();
        if required {
            return Err(format!(
                "oot_patches: the output size places were found {found:?} times, not once each (OUT_EDITS {wrong:?}): the lifted C no longer has the text they match"
            ));
        }
        log.push(format!(
            "oot_patches: WARNING: the output size places were found {found:?} times"
        ));
    }
    Ok(())
}

/// Apply the patches to the chunks in `folder`.
/// `whole` marks a full supported-build lift: its output-size edits are required.
pub fn apply_json(
    folder: &str,
    build: &str,
    protect_hearts: bool,
    whole: bool,
) -> Result<Value, String> {
    if std::env::var_os("HOOKSHOT_NO_PATCHES").is_some() {
        return Ok(json!({"applied": false, "reason": "HOOKSHOT_NO_PATCHES"}));
    }
    if !build.starts_with(crate::nso::SUPPORTED_BUILD) {
        return Ok(json!({"applied": false, "reason": "unsupported build"}));
    }
    let output = apply_folder(folder, protect_hearts, whole)?;
    Ok(json!({
        "applied": true,
        "whole": whole,
        "python": false,
        "output": output,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_once_and_declares() {
        let s = "#include \"a64.h\"\nold old\n";
        let out = replace_once(s, "old", "new").unwrap();
        assert_eq!(out, "#include \"a64.h\"\nnew old\n");
        assert_eq!(
            add_decl(&out, "decl\n"),
            "#include \"a64.h\"\ndecl\nnew old\n"
        );
        assert_eq!(
            add_decl(&add_decl(&out, "decl\n"), "decl\n"),
            "#include \"a64.h\"\ndecl\nnew old\n"
        );
    }

    #[test]
    fn sound_and_solver_are_idempotent() {
        let sound = format!("#include \"a64.h\"\n{SOUND_HEAD}    c->x[2] = 0;\n{SOUND_OLD}}}");
        let (once, n) = patch_sound_job(&sound);
        assert_eq!(n, 1);
        assert!(once.contains(SOUND_GATE));
        assert!(once.contains("hos_oot_sound_step"));
        let (twice, n) = patch_sound_job(&once);
        assert_eq!(n, 1);
        assert_eq!(twice, once);

        let solver_head = SOLVER_HEADS[0];
        let solver = format!("#include \"a64.h\"\n{solver_head}{SOLVER_OLD}}}");
        let (once, n) = patch_solver_steps(&solver);
        assert_eq!(n, 1);
        assert!(once.contains("hos_oot_solver_steps"));
        let (twice, n) = patch_solver_steps(&once);
        assert_eq!(n, 1);
        assert_eq!(twice, once);
    }

    #[test]
    fn whole_frames_rewrite_the_cut() {
        let (head, old, _key) = WHOLE_SITES[2];
        let head = head.replace('@', "\n");
        let old = old.replace('@', "\n");
        let site = format!("#include \"a64.h\"\n{head}{old}}}");
        let (out, n) = patch_whole_frames(&site);
        assert_eq!(n, 1);
        assert!(out.contains("hos_frame_whole(c->x[19] + 60"));
        assert!(out.contains(WHOLE_MARK));
        let (again, n) = patch_whole_frames(&out);
        assert_eq!(n, 0); // (the site is already patched: the apply counts WHOLE_MARK)
        assert_eq!(again, out);
    }

    #[test]
    fn output_size_counts_made_and_raw() {
        let dir = std::env::temp_dir().join(format!("hookshot-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = String::from("#include \"a64.h\"\n");
        for (old, _) in OUT_EDITS {
            text.push_str(old);
        }
        std::fs::write(dir.join("code_000.c"), &text).unwrap();
        apply_output_size(&dir, true, &mut Vec::new()).unwrap();
        let patched = std::fs::read_to_string(dir.join("code_000.c")).unwrap();
        for (_, new) in OUT_EDITS {
            assert!(patched.contains(new));
        }
        apply_output_size(&dir, true, &mut Vec::new()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
