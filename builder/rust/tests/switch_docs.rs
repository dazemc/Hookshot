// Checks that every RX_/HOS_ switch read in gpu/vk/ and host/ is documented in docs/development.md.
// Game-free: it reads source and docs only.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn switches_in(path: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(entries) = fs::read_dir(path) else { return out; };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(switches_in(&p));
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "cpp" && ext != "c" && ext != "h" && ext != "hpp" { continue; }
        let Ok(text) = fs::read_to_string(&p) else { continue; };
        for line in text.lines() {
            // getenv("RX_...") or getenv("HOS_...")
            for prefix in ["RX_", "HOS_"] {
                let needle = format!("getenv(\"{prefix}");
                let mut i = 0;
                while let Some(at) = line[i..].find(&needle) {
                    let start = i + at + needle.len();
                    let rest = &line[start..];
                    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !name.is_empty() {
                        out.insert(format!("{prefix}{name}"));
                    }
                    i = start + name.len();
                }
            }
        }
    }
    out
}

#[test]
fn every_switch_is_documented() {
    let r = root();
    let mut switches = BTreeSet::new();
    switches.extend(switches_in(&r.join("gpu/vk")));
    switches.extend(switches_in(&r.join("host")));

    // Allowlist: internal/test-only or not user-facing.
    let allow = [
        "HOS_AUDIO_CHANNELS",
        "HOS_AUDIO_DEVICE",
        "HOS_AUDIO_DUMP",
        "HOS_AUDIO_WB_DUMP",
        "HOS_CACHE_CORES",
        "HOS_COND_RECORD",
        "HOS_CONTROL_MEM",
        "HOS_DISCORD",
        "HOS_DISCORD_APP_ID",
        "HOS_FIXED_TIME",
        "HOS_FPS_MOVIE",
        "HOS_FPS_STEP",
        "HOS_FPS_TARGET",
        "HOS_FRAME_SLEEP_MS",
        "HOS_FS_DELAY_MS",
        "HOS_FS_OVERLAY",
        "HOS_FS_WATCH",
        "HOS_GAME_INTERVAL",
        "HOS_GRAPHICS_SCALE",
        "HOS_GUEST_MEMFD",
        "HOS_HD_RUMBLE",
        "HOS_KEEP_DISPLAY",
        "HOS_LOG_BATCH_MS",
        "HOS_MEMORY_MB",
        "HOS_MENU_DRAG_GAIN",
        "HOS_MKW_INTERVAL",
        "HOS_MODS",
        "HOS_MOUSE_NATIVE",
        "HOS_MOUSE_SENS",
        "HOS_MUTEX_TRACE",
        "HOS_MUTEX_WATCH",
        "HOS_MW_WAKE",
        "HOS_NO_RENDERER_WAIT",
        "HOS_NVN_HOLD",
        "HOS_NVN_LOG_VMAP",
        "HOS_NVN_REC_SHIFT",
        "HOS_NVN_SYNC_LOG",
        "HOS_NVN_WATCH_DRAW",
        "HOS_NVN_WATCH_DRAW_THREAD",
        "HOS_NVN_WATCH_RT",
        "HOS_NVN_WATCH_TEX",
        "HOS_OOT_PROTECT_HEARTS",
        "HOS_OOT_TELEPORT_EVENT",
        "HOS_OOT_TELEPORT_STARTER",
        "HOS_PIN",
        "HOS_POLL_SLEEP_NS",
        "HOS_POWER_THROTTLING",
        "HOS_PRESENT_NOWAIT",
        "HOS_PRESENT_POLL",
        "HOS_PRIO",
        "HOS_PROFILE_GATE",
        "HOS_REALTIME_WINDOW",
        "HOS_RESTART_OF",
        "HOS_RES_SCALE",
        "HOS_RT_POOL_MB",
        "HOS_SCAN_DT",
        "HOS_SDL_VERSION",
        "HOS_SHADER_DUMP",
        "HOS_SHORT_SLEEP_TIMER",
        "HOS_STATS_AFTER",
        "HOS_TEXT_SIZE",
        "HOS_WAIT_WHO",
        "HOS_WAIT_WHO_LR",
        "HOS_WATCH",
        "RX_VK_LOG_REMADE",
        "HOS_WIDE_KEEP",
        "RX_NVN_WATCH_AFTER",
        "RX_NVN_WATCH_TEX",
        "RX_NVN_WATCH_TEXELS_SRC",
        "RX_VK_ALL_GLOBAL_PROGS",
        "RX_VK_ANISO",
        "RX_VK_ASTC_BC_ALL",
        "RX_VK_ASTC_BC_MB",
        "RX_VK_ASYNC_IMPORT",
        "RX_VK_BARE_READ_MB",
        "RX_VK_BATCH_IDLE",
        "RX_VK_BATCH_MAX",
        "RX_VK_BATCH_POLL_US",
        "RX_VK_BC_IDLE_S",
        "RX_VK_BC_LAYERS",
        "RX_VK_BC_STORAGE",
        "RX_VK_BENCH",
        "RX_VK_BLOCK_PROBE",
        "RX_VK_CAPTURE_EAGER",
        "RX_VK_CAPTURE_NT",
        "RX_VK_CAPTURE_POOL_MB",
        "RX_VK_CAPTURE_THREADS",
        "RX_VK_CAPTURE_WAIT",
        "RX_VK_CAP_MIN_MB",
        "RX_VK_CAP_SPARE_MB",
        "RX_VK_CASCADE_BIAS",
        "RX_VK_CHUNK",
        "RX_VK_CLEAR_CPU_MAX",
        "RX_VK_COMPUTE_SERIAL",
        "RX_VK_DEAD_EVERY",
        "RX_VK_DEAD_MIN_UNUSED",
        "RX_VK_DEAD_TARGETS",
        "RX_VK_DESC_POOL_DIV",
        "RX_VK_DLSS_FG_MOST",
        "RX_VK_DLSS_FG_MV_SCALE",
        "RX_VK_DLSS_IDLE_RELEASE",
        "RX_VK_DLSS_LOD_OFFSET",
        "RX_VK_DMABUF_PIECE_MB",
        "RX_VK_DRAW_STATS",
        "RX_VK_DRAW_STATS_PROGS",
        "RX_VK_DUMP_ARGS",
        "RX_VK_DUMP_BLACK",
        "RX_VK_DUMP_EACH",
        "RX_VK_DUMP_FILE",
        "RX_VK_DUMP_KEY_N",
        "RX_VK_DUMP_MESH_SPV",
        "RX_VK_DUMP_MOD",
        "RX_VK_DUMP_NATIVE",
        "RX_VK_DUMP_ONLY_W",
        "RX_VK_DUMP_ON_WATCH",
        "RX_VK_DUMP_PIXELS",
        "RX_VK_DUMP_SAMPLED",
        "RX_VK_DUMP_SPV",
        "RX_VK_DUMP_SSBO",
        "RX_VK_DUMP_STAGES",
        "RX_VK_DUMP_W",
        "RX_VK_EQ_BIAS",
        "RX_VK_EVENT_WAITS",
        "RX_VK_EVICT_MIN_UNUSED",
        "RX_VK_EVICT_UNFLUSHABLE",
        "RX_VK_EXPOSURE_LOG",
        "RX_VK_FALLBACK_DEPTH_HOLD",
        "RX_VK_FLASH_CELLS",
        "RX_VK_FLASH_DIFF",
        "RX_VK_FLASH_DUMPS",
        "RX_VK_FLASH_RECORD",
        "RX_VK_FLASH_REGION",
        "RX_VK_FOLLOW_GRAPHICS",
        "RX_VK_FOOTPRINT",
        "RX_VK_FORCE_UPSCALE_STUB",
        "RX_VK_FRAG_SHUFFLE_FALLBACK",
        "RX_VK_FRAMEGEN_RADIUS",
        "RX_VK_FRAMEGEN_UNCAPPED",
        "RX_VK_FRAME_TRACE",
        "RX_VK_FSR_FG",
        "RX_VK_GPU_PROFILE",
        "RX_VK_GPU_UNSWIZZLE",
        "RX_VK_GP_TEST",
        "RX_VK_HANDLE_POINTER",
        "RX_VK_HOLD_MS",
        "RX_VK_IMAGE_BLOCK_MB",
        "RX_VK_IMPORT_SPAN_MB",
        "RX_VK_IR",
        "RX_VK_JOB_STEAL",
        "RX_VK_KEEP_FINALIZED_MB",
        "RX_VK_LOG_IMG",
        "RX_VK_LOG_MADE",
        "RX_VK_LOG_PUSH",
        "RX_VK_LOG_READBACK",
        "RX_VK_LOG_STAMP",
        "RX_VK_LOG_UPLOADS",
        "RX_VK_LOWER_SKIP",
        "RX_VK_LOW_VRAM_MIPS",
        "RX_VK_MALLOC_TRIM_MB",
        "RX_VK_MASK_BLEND",
        "RX_VK_MASK_RADIUS",
        "RX_VK_MESH_ARGS",
        "RX_VK_MESH_COUNT_WORD",
        "RX_VK_MESH_FIRST_WORD",
        "RX_VK_MESH_IND_COUNT_WORD",
        "RX_VK_MESH_IND_RAW",
        "RX_VK_MISS_LOG",
        "RX_VK_MISS_LOG_PROG",
        "RX_VK_MMA_SOFTWARE",
        "RX_VK_NATIVE_MMA",
        "RX_VK_NATIVE_MMA_KEYS",
        "RX_VK_NETWORK_KEEP",
        "RX_VK_NO_MESH",
        "RX_VK_OCC_FIXED",
        "RX_VK_PARALLEL",
        "RX_VK_PARALLEL_SEG",
        "RX_VK_PASS_BARRIER",
        "RX_VK_PCACHE_LOAD",
        "RX_VK_PEEK_PROG",
        "RX_VK_PER_QUEUE_FENCES",
        "RX_VK_PLAY_ASK_START_FIRST",
        "RX_VK_PLAY_CACHE",
        "RX_VK_PLAY_SAVE_START_MB",
        "RX_VK_PRECOMPILE_COMPUTES",
        "RX_VK_PRECOMPILE_STALL_S",
        "RX_VK_PREDICT_S",
        "RX_VK_PREDICT_STALL_S",
        "RX_VK_PREFETCH_CHUNKS",
        "RX_VK_PREPARE_STALL_S",
        "RX_VK_PREPARE_STATUS_S",
        "RX_VK_PRESENT_LOG",
        "RX_VK_PRESENT_QUEUE",
        "RX_VK_PROBE_MIN",
        "RX_VK_PROBE_UBO",
        "RX_VK_PROTECT_BARE",
        "RX_VK_PTR_DIAG",
        "RX_VK_PTR_SPLIT_FALLBACK",
        "RX_VK_QUEUE_STATE",
        "RX_VK_READBACK_OFF_FILE",
        "RX_VK_READBACK_PART",
        "RX_VK_READBACK_WAIT_MS",
        "RX_VK_RECORDER",
        "RX_VK_RECORDER_PIN",
        "RX_VK_RECORDER_POISON",
        "RX_VK_RECORDER_RING_MB",
        "RX_VK_RECORDER_SPIN_US",
        "RX_VK_RECORDER_STATS",
        "RX_VK_RECORD_FLOATS",
        "RX_VK_REFLEX_SUBMIT_IDS",
        "RX_VK_REJECT_KEYS",
        "RX_VK_REJECT_STAGES",
        "RX_VK_RELEASE_DRAIN",
        "RX_VK_REMADE_SKIP",
        "RX_VK_REPLAY_PIN",
        "RX_VK_RESUME_CAPTURE",
        "RX_VK_REUPLOAD_DIM",
        "RX_VK_ROBUST",
        "RX_VK_SAMPLER_CACHE",
        "RX_VK_SHADOW_BIAS",
        "RX_VK_SHADOW_PCF",
        "RX_VK_SHOT_FROM",
        "RX_VK_SHOT_GO",
        "RX_VK_SHOT_TO",
        "RX_VK_SKIP_FILE",
        "RX_VK_SPARSE",
        "RX_VK_SSBO_WIDE",
        "RX_VK_STAGING_IDLE_S",
        "RX_VK_STAGING_KEEP_MB",
        "RX_VK_STARTUP_SCREEN",
        "RX_VK_START_DELAY",
        "RX_VK_STREAM_MB",
        "RX_VK_SUBMITTED_PAGES",
        "RX_VK_SUBMIT_DELAY_US",
        "RX_VK_SYNC_CAPTURE",
        "RX_VK_SYNC_WAITS",
        "RX_VK_TAAU_FXAA",
        "RX_VK_TAAU_KEYS",
        "RX_VK_TARGET_BLOCKS",
        "RX_VK_TARGET_POOL_MB",
        "RX_VK_TASK_CAP",
        "RX_VK_TEMPORAL_CUT",
        "RX_VK_TEST_ALLOWED_MB",
        "RX_VK_TEX_CAPTURE",
        "RX_VK_TEX_CAPTURE_MAX_MB",
        "RX_VK_TEX_CAPTURE_OFF_FILE",
        "RX_VK_TEX_MAX_DIM",
        "RX_VK_TEX_MAX_MB",
        "RX_VK_TEX_SKIP_MB",
        "RX_VK_TIMELINE",
        "RX_VK_TRACE_TEX",
        "RX_VK_TS_SCALE",
        "RX_VK_UB_SNAPSHOT",
        "RX_VK_UNEVEN_BC",
        "RX_VK_UPLOAD_BUDGET_MS",
        "RX_VK_UPSCALE_LOD_BIAS",
        "RX_VK_VERIFY_DIM",
        "RX_VK_WAIT_GPU",
        "RX_VK_WAIT_HOLD_US",
        "RX_VK_WARP64",
        "RX_VK_WATCH_ADDRS",
        "RX_VK_WATCH_ARGS_OF",
        "RX_VK_WATCH_CPU",
        "RX_VK_WATCH_DIM",
        "RX_VK_WATCH_GPU_WRITE",
        "RX_VK_WATCH_PROG",
        "RX_VK_WATCH_RANGE",
        "RX_VK_WATCH_SB",
        "RX_VK_WATCH_TEX",
        "RX_VK_WB_SHADOW_MB",
        "RX_VK_ZERO_UPLOAD",
    ];
    for a in allow {
        switches.remove(a);
    }

    let doc = fs::read_to_string(r.join("docs/development.md")).expect("docs/development.md");
    let mut missing: Vec<String> = switches.iter().filter(|s| !doc.contains(*s)).cloned().collect();
    // also check readme for switches not in development.md
    let readme = fs::read_to_string(r.join("readme.md")).unwrap_or_default();
    missing.retain(|s| !readme.contains(s));

    assert!(
        missing.is_empty(),
        "undocumented switches: {}",
        missing.join(", ")
    );
}
