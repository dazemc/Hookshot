# Parity wave 2 — prepared, not launched

This is the next agent wave, held until the current pool drains. Each entry names the owner, the files it may touch,
what it must not duplicate, and the acceptance gate. Upstream was refreshed at `e0e60920`; no newer commits exist, so
these are the remaining second-order gaps rather than new upstream work.

## Launch order

1. `parity/hdr-upscalers` — the HDR agent's own remaining gap.
2. `parity/spirv-packed` — memory reduction, independent of the HDR pass.
3. `parity/pass-order` — renderer ordering, independent of the menu.
4. `parity/pipeline-binary` — opt-in cache, after `parity/pipeline` merges.
5. `parity/devpacks` — build/test workflow, after `parity/buildperf` merges.
6. `parity/profile` — HLE call counters, independent.
7. `parity/rt` — audit only; launch if the RT branch still applies.

## 1. `parity/hdr-upscalers`

**Gap:** the HDR agent recorded that DLSS, DLAA, FSR 3.1, FSR 4 and TAAU still accept only the `B10G11R11` scene, so
PsychoV35 runs the game's own upscale. Native HDR needs the upscalers to take the HDR scene format.

**Files:** `gpu/vk/vk_temporal.*`, `vk_dlss.cpp`, `vk_fsr3.*`, `vk_fsr4.*`, `vk_shader.*`, the temporal shaders.
**Do not duplicate:** `parity/hdr` (merged), `parity/fsr3hdr`, `parity/fsr4`, `parity/nvidia`, `parity/upscaler`.
**Gate:** authored scene-format fixtures; DLSS/FSR/FSR4 accept the HDR format; fallback to the game upscale when an
engine cannot; Vulkan build. No in-game HDR claim without a run.

## 2. `parity/spirv-packed`

**Upstream:** `#2309` keep program stages' SPIR-V packed and make shader modules only per pipeline (~1.5 GB less RAM
on NVIDIA).

**Files:** `gpu/vk/vk_programs.cpp`, `vk_shader.cpp`, `vk_pipeline*`, `vk_draw.cpp`.
**Do not duplicate:** `parity/pipeline` (merged), `parity/shaders`.
**Gate:** game-free fixture for packed/unpacked round-trip, module lifetime, and a measured RAM delta on authored
SPIR-V. Vulkan build.

## 3. `parity/pass-order`

**Upstream:** `#2559` back-to-back passes and dispatches ordered by the game's barriers (NVIDIA by default).

**Files:** `gpu/vk/vk_replay.cpp`, `vk_draw.cpp`, `vk_barrier*`, docs.
**Do not duplicate:** `parity/vkmemory`, `parity/pipeline`, `parity/mesh`.
**Gate:** source checks for the barrier ordering and the default switch; a fixture that replays authored pass
sequences; no change to the conservative path.

## 4. `parity/pipeline-binary`

**Upstream:** `#2814` opt-in pipeline binary cache, lighter precompile and a wait budget for AMD Windows.

**Files:** `gpu/vk/vk_pipeline*`, `vk_progcache.cpp`, `vk_jobs.cpp`, docs.
**Do not duplicate:** `parity/pipeline` (file-backed cache, zstd program cache, translate wait).
**Gate:** opt-in only; binary cache round-trip and driver-change invalidation; a game-free fixture; Vulkan build.

## 5. `parity/devpacks`

**Upstream:** `#2732` test a change against a base, as a pack of what it changes.

**Files:** `scripts/` (new dev tool), `builder/rust/src/` if it needs a pack command, docs.
**Do not duplicate:** `parity/buildperf`, `parity/bench`.
**Gate:** authored base/pack/run flow, no game data, refuses packs from another base, and a test for the refusal.

## 6. `parity/profile`

**Upstream:** `#2580` opt-in HLE call counters.

**Files:** `host/` HLE entry points, `builder/rust` if the counters are surfaced, docs.
**Do not duplicate:** `parity/bench`, `parity/hostperf`.
**Gate:** opt-in only, zero cost off, game-free counter fixture, and a documented output format.

## 7. `parity/rt`

**Upstream branch:** `feat/rt-hybrid` (not `main`). Audit only first: does the branch apply to this tree, and what is
the smallest safe slice? Do not port a half-working ray tracer.
**Files:** renderer only, no menu until a working slice exists.
**Gate:** a precise gap report or a compiling, tested slice; no fake feature.

## Merge dependencies

| Wave-2 branch | Wait for |
|---|---|
| `hdr-upscalers` | `fsr3hdr`, `fsr4`, `nvidia`, `upscaler` merged |
| `spirv-packed` | `shaders`, `pipeline` merged |
| `pass-order` | `vkmemory`, `pipeline`, `mesh` merged |
| `pipeline-binary` | `pipeline` merged |
| `devpacks` | `buildperf`, `bench` merged |
| `profile` | `hostperf` merged |
| `rt` | none; audit can start now |

## Current pool (do not duplicate)

`bodies`, `portable`, `input`, `fsr4`, `actormotion`, `quality`, `i18n`, `textures`, `display`, `hostperf`,
`buildperf`, `shaders`, `vkmemory`, `nvn`, `linux`, `audio`, `fsr3hdr`, `amd`, `nvidia`, `saves`, `docs`, `windows`,
`mods`, `cheats`, `ci`, `tests`, `prompts2`, `bench`, `security`, `upscaler`, `network`, `accessibility`.

Merged already: `hdr`, `intel`, `pipeline`, `av1`, `mesh`, `release`.
