# Hookshot-rs technical reference

This is the dense reference for the `dazemc/Hookshot-rs` fork: what the build system is, what is ported, what was
measured, what is still Python, and where the code lives. It is written to be read by people and by coding agents.

The short usage guide is the [README](../readme.md). Build profiling is in [performance.md](performance.md).

## What this fork is

Hookshot statically recompiles *The Legend of Zelda: Ocarina of Time* (Nintendo Switch 2) into a native Windows or
Linux program and rebuilds its graphics on Vulkan. The upstream project is
[NotRexed/Hookshot](https://github.com/NotRexed/Hookshot). This fork keeps the game, renderer and player documentation,
and replaces the build side with:

- **one Flutter desktop app** in `builder/flutter` (Linux and Windows);
- **one Rust engine** in `builder/rust`, the `hookshot_engine` crate;
- an explicit, versioned bridge between them: the Flutter app runs `hookshot_engine --stream` for builds and calls
  the engine in-process for profiles, saves, checks and diagnostics.

No game data is committed. Every build input and output comes from the player's own copy and stays on their PC.

## Architecture

| Path | Role |
|---|---|
| `gpu/vk` | Vulkan renderer: NVN command replay, texture/shader translation, frame generation, DLSS/FSR, capture, UI, an exploratory ray-traced path |
| `host` | Windows/Linux runtime: HLE, audio, input, video, filesystem, saves, update, controller prompts |
| `builder/rust` | Build engine |
| `builder/flutter` | Linux/Windows builder UI |
| `nx2` | Legacy Python lift/build/prompt tools; release packaging and a missing-engine fallback |
| `scripts` | Release, AppImage and SDK helper scripts |
| `.github/workflows` | CI and release workflows |

### Rust engine map

| Module | Responsibility |
|---|---|
| `nup.rs` | `.nup` reader, verification, extraction; in-place writes; parallel romfs extraction |
| `nso.rs`, `nso_image.rs`, `zbic.rs`, `image.rs`, `standalone.rs` | NSO/ZBIC/RELR, image layout, standalone runtime |
| `lift.rs`, `insn.rs`, `emitter.rs`, `a64.rs`, `text.rs` | AArch64 discovery, jump tables, translation, text transforms |
| `lift_cache.rs` | Per-function lifted-C cache |
| `oot_patches.rs`, `oot_after.rs` | OoT patch table and after-instruction sites |
| `compile.rs` | Content-addressed object cache, `hash_cache.json`, compiler planning |
| `linker.rs`, `host_build.rs` | Linux/Windows host compile and link |
| `vk_build.rs` | Vulkan backend build for Linux `.so` and Windows `.dll`, including NGX and FSR flags |
| `package.rs` | Game folder staging, prompts, runtimes, language/version |
| `prepare.rs` | `--prepare-shaders` run after packaging |
| `shader.rs` | Shader census / seed list |
| `ngx_sdk.rs` | NVIDIA DLSS SDK fetch, verification and runtime placement |
| `prompts/` | Button-prompt font, layouts, size table, texture cache hooks |
| `profiles.rs`, `saves.rs`, `disk.rs`, `diagnostics.rs`, `gamescope.rs`, `toolchain.rs` | Builder services |
| `orchestrate.rs`, `driver.rs`, `stream.rs`, `timing.rs` | `build.all`, stage timing, stream protocol |

### Flutter app

`builder/flutter` is a BLoC app. `BuilderBloc` owns the form, build/update flow, `@@stage`/`@@progress` parsing,
Play, Cancel and profiles; `RustBuild`/`RustEngine` own the engine bridge; `profiles.dart` and saves use engine
commands. `flutter test` covers build routing, profiles, saves, updater, manifest and layouts.

## Build pipeline

A normal game build is one `build.all` call after an optional `.nup` read:

1. **read** — `build.prepare` extracts the `.nup` into `work/exefs` and `out/romfs`, or uses the kept `.hookshot_exefs`.
2. **lift** — `lift.write` discovers functions, resolves jump tables, translates every distinct instruction once,
   applies the OoT patch table and after-sites, writes stable chunks, then marks page groups.
3. **compile** — content-addressed objects for the generated chunks and runtime sources; the object cache is reused
   across builds.
4. **host** — compile and link the Linux executable or Windows `hookshot.exe`.
5. **Vulkan** — use a prebuilt backend or build `hookshot_vk.so`/`hookshot_vk.dll`; NGX and FSR 4 flags are selected
   from the request/environment.
6. **package** — stage image, romfs hard links, libraries, language, version, prompts and DLSS runtimes.
7. **shaders** — make `hookshot_shaders.bin`, then optionally run the built game with `--prepare-shaders`.

Commands: `build.prepare`, `lift.write`, `build.compile_many`, `build.host_link`, `build.vk`, `build.package`,
`build.shaders`, `build.all`. `--stream` is the Flutter protocol; the same commands also take one JSON object on
stdin and answer one JSON object.

## Benchmarks

Measured on one Linux PC: Ryzen 9 5950X (16 cores/32 threads), 15.5 GiB RAM, RTX 4070 Ti, f2fs RAID 0, clang/lld
23.1.4. Absolute wall times vary with CPU, disk, driver, page cache and background load; the before/after pairs
were measured by alternating variants. Development branches are labelled as such.

| Work | Before | Current `main` | Notes |
|---|---:|---:|---|
| Full lift, 294,522 functions | 10 m 30 s | **57 s cold**, **19.8 s cached** (loaded), 15.05 s quiet branch | per-function cache; parallel patch/mark/cache work merged |
| 5,000-function lift | 65 s | **8.1 s** | same cache design |
| `.nup` read/extract, 25.6 GB | 94 s | **64 s** measured (loaded), 46 s quiet branch | 4 romfs workers, one large read per member |
| Shader census | 9.7 s | **3.88 s** | 1,445 archives, 38,809 programs, 42,856 stages, 539 MB |
| Vulkan backend build | — | **100.6 s clean**, 0.7 s repeat | 195 sources, 212 jobs, content-addressed objects |
| Package | — | 10.1 s cold, 0.43 s warm | 186,583 hard links |
| `prepare-shaders` | — | 36.8 s first, 17.6 s cached `build.all` | 2,863 programs, 15,330 graphics + 71 compute pipelines |
| Boot | — | 2,640 presents at 60 fps, 0 traps | Rust-built backend and prompts |
| Compile 22 chunks | — | 412 s at 4 jobs; 6 MB units: 18–20 s for five in parallel | units merged; a full first compile is estimated at 5–8 min, not yet measured end to end |

The [performance page](performance.md) has stage-by-stage profiles, syscall counts, rejected experiments and the
shader-preparation breakdown.

## What is Rust, and what is not

Rust owns the default Linux build path: `.nup` read, lift, patches, compile, Linux host link, Vulkan backend,
package, shader census/preparation, profiles, saves, updates, DLSS SDK fetch/link and runs. Windows host link and the
Windows Vulkan DLL are ported too; a real Windows game link and VM boot are still pending.

Still Python or not ported:

| Area | Status |
|---|---|
| Flutter build/update fallback | **Removed.** Flutter does not invoke Python; a missing engine is a typed error. |
| Release/AppImage/Windows packaging | Linux AppImage and Windows package pipelines build Flutter + Rust; `v2.2.0-rs.1` is published with both. The AppImage needs distribution clang and Vulkan/SPIR-V/VMA headers for a game build |
| Prompt textures (`Common.bntx`, per-face `textures.bin`) | **BC4 letters, ASTC notes and controller labels ported in Rust**: BNTX/GOB parsing, a glyph rasterizer, BC4 blocks, a vendored ASTC encoder/decoder and the HPTX run writer make `Common.bntx`, the `KeyTexFont` shapes, the `OcarinaButton` note textures and the four face labels on `Controller_Pro`/`Controller_Pro1`. The hand-drawn controller bodies and guide lines are still the game's |
| Legacy Tk/WPF builders | Kept for upstream release scripts, not used by the Flutter app |
| Python source-reading tests | Deleted; tests are Rust, Flutter and C/C++ |

The Python path remains usable for the release scripts and as a local legacy builder, but it is no longer the app's
fallback.

## Upstream parity

Upstream moves independently; `main..upstream/main` currently reports ~145 commits because semantic ports are separate
commits, not upstream ancestors. Ported recently:

- **Keyboard button prompts (#2435)**: a sixth prompt face draws the keys and mouse buttons the Controls page binds,
  with the key glyphs, stick icons and live binding changes, and Auto picks it when no gamepad is present. The Kenney
  Input Prompts keyboard/mouse font is vendored under `third_party/kenney_input_keyboard` (CC0, provenance
  recorded). The Rust prompt build generates the keyboard `textures.bin` and `hint.bin`; host and menu source checks
  cover the face order, glyph codes and live binds.
- **Graphics tab sections (#2177)**: the Graphics rows are grouped under Resolution and upscaling, Frame generation
  and latency, Screen, HDR, Detail, and GPU and memory, with the section titles skipped by selection. A game-free
  source check pins the order and grouping.
- **A native 5120x2160 widescreen picture (#2120)**: a 21:9 screen at the 4K output gets its native shape instead of
  a stretched 4416x1864 one; the pixel cap is 5120x2160's, `HOS_WIDE_MAX_PX` lowers it, and a 32:9 screen stays
  capped. `tests/wide_size_test.c` checks the shapes in CI.
- **GPU hang watch (#2407)**: submitted work that stops finishing now ends the game with the checkpoint report after
  `RX_VK_GPU_HANG_SEC` (default 30) instead of hanging; a real device loss keeps its existing path. A compiled
  fixture cuts the watch out of `vk_replay.cpp` and checks the exit, the reason and the off switch.
- **Prompt generation perf**: one ASTC context per thread and profile, only the block-aligned button-mark region is
  re-encoded, and the output digests are read once. Forced generation fell from 3.25 s to 1.60 s with byte-identical
  `Common.bntx`, note and letter runs; unchanged cache hits are unchanged.
- **Bug report (#2347, upstream `6f7ab2a7`)**: Gameplay's **System → Bug report** row (before Restart) writes
  `bug-reports/<date>_<time>` in the game folder with `report.txt`, this run's and the previous run's logs (home folder
  and user name redacted), and the game's next frame as `picture.png`. The report answers the game-problem form's
  fields, lists the changed menu rows and the `RX_`/`HOS_`/`HOOKSHOT_`/`EDEN_`/`NVN_`/`SDL_` switches (a value only
  when it is a number, a size or on/off, and never for a secret-like name), and ends with a pre-filled issue link. The
  host logs `map: <Sequence_CurrentBanc> (child|adult)` when the map or age changes, which supplies "Where in the game".
  `gpu/vk/vk_bug_report.cpp` is compiled alone by `builder/rust/tests/bug_report.rs` with `tests/bug_report_test.cpp`;
  the menu row, host map line and picture wiring are source-checked there too. The fork has no separate System/About
  tab, so the row sits under a System section heading with Restart and Quit on Gameplay.
- **FSR 3.1 on the temporal chain (#2305, upstream `579c6d48`)**: FSR 3.1 is now the chain's third engine, so it
  receives the same colour, depth, jitter and skinned-character motion as DLSS and FSR 4 instead of camera motion
  alone; Link and the other characters keep their outlines while they move. The chain reverses depth for FSR's
  passes (`TemporalFrame::depth_reversed`, with DLSS-G's `depthInverted` following it), FSR's depth constants come
  from the unjittered projection, a failed evaluation retries next frame instead of turning the engine off, and the
  reactive mask is shared with DLSS. `RX_VK_FSR3_CHAIN=0` keeps the old own-inputs path for comparisons. A
  game-free Rust source check (`builder/rust/tests/fsr3_chain.rs`) covers the wiring; `tests/temporal_jitter_test.cpp`
  covers FSR 3.1's jitter slot and `RX_VK_FSR_JITTER=0`.
- **Adjustable field of view #2357**: Gameplay's **Field of view** row scales camera zero's vertical angle from 50% to
  150% before projection and culling, saving `field_of_view`; 100% is the game's own view, and cutscenes, other views
  and scripted events keep their angles. The Python patch's site check is a Rust after-site with all three original
  words, and `host/oot_fov.c` applies the live choice. Game-free fixtures cover the hook, the host helper's bounds and
  camera ownership, the menu row and option IDs; the Vulkan backend builds with the change.
- **2 DLSS/V-Sync fixes**: V-Sync Auto follows the Frame Gen runtime's real frame limit (#2701); a failed DLSS
  feature is retried when a menu setting changes, not for the rest of the run (#2550).
- **4 Vulkan memory/perf fixes**: GPU-written mirrors are kept instead of remade empty (cyan after a pause or reload,
  #2154); six background workers on 16-thread machines (#2440); part of the image pool's blocks are made at the start
  (#2438); retired imports on Windows AMD/Intel go back after a grace wait instead of being kept for the session
  (#1906).
- **Shader fix #1830**: noperspective varyings are written times Position.w and undone in the fragment stage, so AMD
  interpolates the Royal Family Tomb water right; sirit names an undefined operand instead of aborting, and
  `RX_VK_LOWER_SKIP` leaves programs unlowered for bisection. A headless RTX 4070 Ti boot ran 10,185 presents at 60 fps
  with 52,729 programs translated and no traps; the AMD water itself has not been checked on AMD hardware.
- **Video perf #2114**: planar YUV420P frames stay NV12 and defer RGBA to the pool, U/V interleaves 16 pairs at a time
  with SSE2, and decode threads go to 8 for AV1 and 4 for others. A headless boot played a 1920x1080 movie at 60 fps
  with no traps; `tests/video_uv_interleave_test.c` covers every pair count from 0 to 37 in CI.
- **GCN #2343**: f16 is used only where the GPU reports f16 shaders (RADV gives Polaris none), and GCN runs with VRAM
  mirrors and batched submits off by default. The f16 programs carry a cache marker. A headless NVIDIA boot after the
  change ran 13,496 presents at 60 fps with no traps; the RX 580 measurements are upstream's.
- **RADV/Navi #1965, #2215**: the AMD no-optimizer rule for task and mesh programs excludes Mesa's RADV (the unoptimized
  task program reset the GPU while the Great Deku Tree save loads), and the task+mesh fallback covers 0x73af (RX 6900
  XT XTXH) and 0x73a5 (RX 6950 XT).
- **Prompt textures, BC4 letters, ASTC notes and controller labels**: the Rust prompt build writes `Common.bntx` and
  each non-Switch face's `textures.bin`. ARM's astc-encoder is vendored behind a C wrapper (no bindgen/libclang); the
  note icons are compressed as 4x4 ASTC, and the game's `Controller_Pro`/`Controller_Pro1` pictures get the face's
  four button labels. The in-game Xbox face logged 102,568 runs and ran at 56 fps with no traps. Fixtures cover the
  BNTX table, GOB addresses, BC4 levels, masks, the ASTC wrapper and the HPTX header; the controller bodies and guide
  lines remain.
- **11 Vulkan fixes**: texture table, span aging, split fence wait, recorder packet, Intel storage loop, subgroup
  barrier, mesh grass hold, pipeline-cache header, 250 ms readback, RADV mesh default, Intel uniform snapshot.
- **7 host fixes**: update downgrade guard, menu option order, Steam gyro, Aux/splitter audio, window title/class,
  camera drift, mouse-look reseed.
- **8 build fixes**: NUP in-place extraction, damaged zstd textures, comma paths, work-folder safety, folder/system
  language, stat-based object keys, prompt asset cache, DLSS-G refusal logging.
- Earlier: teleport minimap repair, save-folder links, FPS sound/solver/water/time-of-day, AMD/Intel shader fixes,
  terrain/writeback/heap fixes, NUP/Python builder fixes.

Remaining large areas are FSR 3.1's HDR colour (it still takes the chain's LDR pair; upstream lists that as a
follow-up), Native HDR, the guide lines and hand-drawn controller bodies, AV1/dav1d edge cases, aspect-ratio and
translated-menu features, hardware-specific Vulkan fixes, Flutter equivalents of the Tk/WPF UI commits, docs and
game-derived pipeline assets.
Do not port game-derived pipeline lists or generated game data.

## CI and release

`source-regressions.yml` runs Rust tests on Linux and Windows, Flutter tests/analyze, three SPIR-V fixture builds and
actionlint. `linux-appimage.yml`, `windows-builder.yml`, `vulkan-dlss.yml` and `release.yml` build release artifacts.
All five workflows are green on `acf9b000`. The pre-release `v2.2.0-rs.9` was built from that commit and adds the
controller-picture labels to the vendored ASTC notes, RADV/Navi, GCN, BC4 prompt-texture, #1830 shader, #2114 video,
DLSS/V-Sync, compile-wave scheduler, worker/mirror/image-pool and retired-import work. `v2.2.0-rs.8` (from
`8fbdb957`) has everything but the controller labels; `v2.2.0-rs.7` (from `b961fc43`) has neither of the last two.

The fork was renamed to `dazemc/Hookshot-rs`. The Linux AppImage and Windows package workflows now build the Flutter
app and Rust engine without Python; the pre-release (`v2.2.0-rs.9`) is published with the AppImage and the
Windows setup. The Linux image is a development image: a game build needs distribution clang and Vulkan/SPIR-V/VMA
headers (or a source build). The repository never contains game files or generated game data.

## Testing

```sh
cd builder/rust && cargo test --workspace
cd builder/flutter && flutter test && flutter analyze
```

C and C++ fixtures live under `tests/` and `gpu/vk/tests`; the SPIR-V fixtures have their own CMake projects. No test
needs the game or a dump unless it says so. Add a Rust or Flutter test for a fix where the behavior can be tested
without the game.

## Performance work in progress

- **Compile speed**: 25 MB chunks, memory-aware job count, PCH, chunk split for finer object-cache granularity.
- **Speed scout**: package/link/shader-startup/NUP scaling findings in [performance.md](performance.md); staged romfs
  tree linking is merged.
- **Ray tracing**: exploratory `feat/rt-hybrid` branch, off by default, feeds guide buffers and a hybrid RT pass; it is
  not part of the default renderer.

## Conventions

- Keep game files, keys and generated game data out of git and CI.
- Port upstream commits semantically: one local commit per upstream item, tests with the change.
- Never push or comment on the upstream repository.
- Rust/Flutter are the maintained build path; Python is reduced only when the replacement is verified.
