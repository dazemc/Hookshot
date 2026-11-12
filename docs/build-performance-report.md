# Build performance: Python and Rust, measured

This report compares the maintained build paths on the same class of PC. It is written for upstream
maintainers and for people deciding whether the Rust/Flutter builder is worth trying. The numbers are
measurements, not estimates; the machines and the load are named with each one.

The current pre-release is [v2.2.0-rs.10](https://github.com/dazemc/Hookshot-rs/releases/tag/v2.2.0-rs.10)
(2026-11-12). The numbers below were measured across the Rust/Flutter pre-release line, not as a benchmark of that
tag alone.

## Short version

| Work | Upstream Python path | Hookshot-rs Rust path | Notes |
|---|---:|---:|---|
| Full first build | 36:17 measured (`#2890`); 45-60 min reported on older/slower PCs | **15-30 min typical; 12-16 min on the 32-core reference PC** | different machines, same class of game dump |
| Compile stage, cold | **20.3 min** for 335 units at a 4-job memory gate (`#2693`, `0819e412`) | **13.8 min** for 335 units with memory-sized waves (up to 32 jobs as the giants finish) | Ryzen 9 5950X, 32 threads, 15.5 GiB |
| Compile stage, warm | 4.3 s with every unit cached | **4.3 s** with every unit cached | same cache-hit shape |
| Full lift | 10 min 30 s (older Python lift) | **57 s cold / 19.8 s cached** | 294,522 functions |
| 25.6 GB `.nup` read | 94 s | **64 s** (46 s quiet) | f2fs RAID 0 |
| Prompt generation, forced | not measured here | **3.25 s before, 1.60 s after** | this fork's own before/after; byte-identical outputs |
| Shader census | not measured here | **9.7 s before, 3.88 s after** | this fork's own before/after; 1,445 archives |
| Package stage | not measured here | **10.1 s cold, 0.43 s warm** | 186,583 hard links |

The largest single change is not a faster compiler: it is doing less work. The Rust lift caches per
function, the compile stage reuses stat-keyed objects, prompt assets are kept when their inputs did not
change, and the package stage hard-links the ROM filesystem instead of copying it.

## Where a cold build spends its time

Hookshot-rs, from an empty work folder, on the reference PC (Ryzen 9 5950X, 16 cores/32 threads,
15.5 GiB RAM, RTX 4070 Ti, f2fs on RAID 0):

| Stage | Time | What it is |
|---|---:|---|
| Unpack the `.nup` | 50.5 s with 4 workers | 186,588 members, read, verified and written |
| Lift | 25.8 s idle (72 s loaded) | discovery, translation, cache, chunks, patches, tables |
| Compile | **13.8 min cold** | 328-335 units; giants first at 2-3 GB each, then ordinary 6 MB units at up to 32 jobs |
| Link the game/host/Vulkan | 3.5-7 s | 66 + 255 + 211 objects |
| Package | 10.1 s cold | hard links, image and Vulkan copies, prompts |
| Shader census | 10.3-15.0 s | 1,445 archives, 539 MB output |
| Shader preparation, first start | minutes | translate the seed, import the state list, compile pipelines |
| Shader preparation, warm | 5.3 s | 15,330 + 71 pipelines from a warm cache |

The compile stage dominates a first build; the shader preparation dominates a first start. The Rust
builder's advantage is concentrated in the stages around the compiler: lift, NUP, package and prompts.

## What upstream changed, and what this fork changed

Upstream's own performance work is the baseline this fork builds on:

- `#2693` / `0819e412`: `build_times.json`, every stage timed, and the measured **20.3 min** cold compile.
- `#2890`: 36:17 full build on `main` against 32:30 with ~4 MB lifted chunks.
- `#2732`: a 69-minute cold install on an RX 9070 XT when shader preparation is included.
- `#2702`: planning a one-file edit build fell from 6-7.5 s to 2 s with a stat cache.
- `#2416`: the `.nup` unpack became 2.2-4.5x faster by writing in place and checking each folder once.
- `#2698`: an unchanged prompt finish stage fell from ~67 s to 0.6 s.
- `#2775`: `-Xclang -disable-lifetime-markers` cut a lifted compile's peak memory by about 10%.

Hookshot-rs carries the same ideas into the Rust engine and adds:

- per-function lift caching (`lift_cache`), so a warm build skips the lift entirely;
- stat-keyed compile objects and memory-sized compile waves;
- hard-linked ROM filesystem staging;
- Rust prompt generation (BC4 letters, ASTC notes, keyboard face, guide lines, controller labels);
- a Rust Vulkan build and host link, with no Python in the app path.

## Rust/Python parity

**Every build stage the Flutter app performs is implemented in Rust, and the app never invokes
Python.** The maintained Rust path is at parity with the Python pipeline for:

| Stage | Rust implementation | Status |
|---|---|---|
| `.nup` read and extract | `builder/rust/src/nup.rs` | parity |
| Game check and version | `builder/rust/src/version.rs`, `version_policy.rs` | parity |
| Lift, chunks, patches | `lift.rs`, `lift_cache.rs`, `chunks.rs`, `oot_patches.rs`, `oot_after.rs` | parity |
| Compile and toolchain | `compile.rs`, `toolchain.rs`, `host.rs` | parity |
| Host link | `host_build.rs` | parity |
| Vulkan build | `vk_build.rs` | parity |
| Package and saves | `package.rs`, `saves.rs`, `profiles.rs` | parity |
| Button prompts | `prompts/` | parity (BC4, ASTC, keyboard, guide lines, labels) |
| Shader census and prepare | `shader.rs`, `prepare.rs` | parity |

The remaining Python is not on the app's path:

- `scripts/` release tooling (version notes, AppImage/Windows packaging helpers);
- the legacy `nx2/` and `builder/py` builders, kept for upstream release scripts and local comparison;
- the merge of `assets/pipeline_states/*.xz` into the shipped state list, which is build-asset
  tooling rather than a game build stage.

If a change makes the Flutter app call Python, that is a regression. `builder/rust/tests/rust_only_app.rs`
fails when the app or engine starts Python or `make_game.py`, `builder/rust/tests/docs.rs` fails when the
contributor docs send people back to the Python builder, and the release workflows build the app without Python.

## What is not claimed

- These are not controlled cross-machine benchmarks. The upstream 20.3 min compile and 36:17 full build
  are on a Ryzen 9 3900X (Windows, NVMe); the Rust numbers are on a Ryzen 9 5950X (Linux, f2fs RAID 0).
  Other work was running during some runs; absolute wall times are upper bounds.
- The 45-60 minute figure is the older reported range, not a single measured run in this report.
- Shader preparation is hardware- and driver-cache-dependent. A first start with no driver cache can
  still spend minutes compiling pipelines; a warm start is seconds.
- No game files, keys or generated game data are included. The commands below read a dump where it
  lies and write only to a local work folder.

## Reproduce

```sh
# Rust engine
cd builder/rust && cargo build --release --workspace
E=$PWD/target/release/hookshot_engine

# read a .nup first; build.all takes an unpacked exefs/romfs pair
printf '%s' '{"command":"build.prepare","nup":"<game>.nup","out":"<out>","work":"<work>"}' | "$E"

# one stage at a time
printf '%s' '{"command":"lift.write","exefs":"<work>/exefs","out":"<work>/gen","standalone":true}' | "$E"
printf '%s' '{"command":"build.shaders","romfs":"<out>/romfs","out":"<out>/hookshot_shaders.bin"}' | "$E"
printf '%s' '{"command":"build.package","work":"<work>","out":"<out>","romfs":"<out>/romfs","assets":"","language":"en-US"}' | "$E"

# the whole build
printf '%s' '{"command":"build.all","exefs":"<work>/exefs","romfs":"<out>/romfs","out":"<out>","work":"<work>","language":"en-US"}' | "$E"
```

The engine reports each stage, the unit count, the cache hits and the wall time in its JSON responses;
upstream's `build_times.json` (`#2693`) is the same idea on the Python path.
