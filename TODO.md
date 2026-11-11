# Linux-focused development TODO

This is the fork-local plan for Linux-only performance, bug fixing, debugging, and build-speed work.
It supplements, but does not replace, `AGENTS.md`, `docs/workflow.md`, `docs/worklist.md`, `CONTRIBUTING.md`,
and `docs/development.md`.

## Scope

- Linux only.
- Performance, correctness, observability, compile speed, and rebuild caching.
- No Windows-specific work unless a shared file must remain portable.
- No full-game build or game run until the user explicitly authorizes it.
- No game files, lifted game code, dumps, saves, logs, screenshots, profiles, caches, or build outputs in git.

## Active task: one Flutter builder (replaces Tk + WPF)

Scope decision: desktop Linux + Windows only; no iOS/Android (the builder must
run compilers and produce desktop binaries). BLoC architecture (`flutter_bloc`).

- [x] Branch `feat/flutter-builder` (renamed from the damaged-warning branch).
- [x] `nx2/make_game.py`: `stand_in_damaged_files()` writes `damaged_files.txt` + returns pairs (engine side of #2237; reused by Flutter).
- [x] `tests/test_make_game_damaged_files.py` (game-free, passing).
- [x] Flutter scaffold builds for Linux; analyzer clean under `builder/flutter/analysis_options.yaml`; 21 flutter tests pass.
- [x] Files/progress/ready/problem/launcher pages; updater port (manifest parse, version compare, download/verify/install); damaged-dump warning in ready page + dialog.
- [ ] Smoke-run the Flutter window; verify Browse dialogs, a `--check` version read, and Play path.
- [ ] AppImage packaging for the Flutter app (replaces Python/Tk payload).
- [ ] Windows packaging story (replaces WPF setup).
- [ ] Decide the Tk (`builder/linux`) and WPF (`builder/HookshotBuilder`) retirement path with upstream.
- [ ] PR opened against upstream after user reviews the draft; nothing else in the PR.

Dropped per user direction: Tk-side Builder warning dialog and in-game Insert-menu warning (warning lives in the Flutter builder instead).


### Version axes to separate (follow-up PRs)

- [ ] **Builder GUI version vs compiler/engine version.** The Flutter window, the `nx2` build engine and the game it
  produces currently share one release tag (`toolsTag`). Split them into three explicit versions: the desktop builder
  app (its own semver, what the updater installs), the tools/engine (`nx2` + compiler/toolchain, what a game folder's
  `hookshot_version.txt` records), and the built game/version. Show both app and engine versions in the header/About,
  let a builder update leave a compatible engine in place, and record the pair in build logs and issue reports.
  - [x] Show them separately now: the header says `Builder 1.0.0` and `tools vX`; the launcher shows the game version.
- [ ] Manifest and updater fields for the pair (`builder_version`, `engine_version`; compatibility range), with tests
  that a newer GUI accepts an older engine only inside its declared range.
- [ ] Windows and Linux packaging advertise the same pair; the setup/AppImage no longer implies one version for both.

### Upstream coverage ledger (for the final PR description)

Ported into the shared engine (game-free tests included):
- #2254 / #2237 — every build writes `hookshot_damaged_files.txt` (and the legacy `damaged_files.txt`) when it stood in for damaged files, removes both on a clean check, and stops when damage has no replacement (#2096). The Flutter ready/problem screens show the same list.
- #2253 / #2246 — cache stamps are read as UTF-8 with a legacy code-page fallback and decoded exactly or not at all; a `.nup` name outside the Windows code page no longer stops a build.
- #2250 / #2142 — stale-process cleanup only claims processes whose work folder or output paths prove ownership; tests cover the Windows quoting and child rules.
- #2137 / #2257 — linker options are passed one per `-Xlinker`, so a comma in the build-files path no longer truncates the map path.
- #2242 — the shader-preparation watchdog now treats growing child CPU time as work, not silence, so slow AMD pipeline compiles are not stopped after 15 quiet minutes.
- #1958 — profiles: the game reads `HOOKSHOT_PROFILE` and validates it against `profiles.txt`; `nx2/profiles.py` owns add/rename/remove/export and `scripts/oot_saves.py` imports into a new profile. Flutter's launcher has the profile picker and Manage dialog; `hookshot-launch` passes the saved profile.
- #1414 — the game's update check, the setup, the Tk updater and the Flutter updater use `repository.txt` / `HOOKSHOT_REPOSITORY`, so a fork updates from itself.
- #2275 — the AppImage workflow retries PPA enrollment before the signed-index refresh (supporting the Linux package path).

Flutter builder (the replacement UI):
- #1759 — the launcher and every page are redesigned on the shared Slate-style chrome; Lucide icons throughout.
- #2160 — Cancel stops a Linux update mid-download; staged parts are removed.
- #2159 — each run starts with fresh state, so a stop reason cannot leak into the next build.
- #2150 — the progress clock/countdown always runs.
- #2141 / #2129 — Back from a stopped/failed update returns to the form with the mode recomputed, so the update can be offered again.
- #2201 — launcher version selector (stable releases with this platform's assets), "Stay on the selected version" pin, save/settings backup before a switch, and the game-side pin check.
- #1388 — a build stops before starting when the destination has less than the ~45 GB it needs, with the path and free space in the message.
- #2287 — a Rebuild action in the launcher rebuilds from the game files the folder kept, without downloading.
- Every Play (form, ready page, game folder) opens the game launcher first, so the profile, pinned version, rebuild and
  Linux gamescope options live in one place.
- Linux gamescope options: size, refresh, fullscreen, HDR, cursor grab, real-time scheduling, gamemoderun and extra
  arguments; Play wraps the game only when not already inside a gamescope session.
- "Close the builder after the game starts" is a launcher option, off means the builder stays open beside the game.
- Motion: page fades/slides, animated step and stage markers, pop-in status icons, animated launcher status.
- #2045 item 3 — setup-list validation (tags, destinations, URLs, checksums, symlinks).

Related but not implemented here (follow-ups):
- #1896 — the Windows setup still says how to restore `tar.exe`; an extractor fallback is a separate setup change.
- #2218 — shader preparation is still long on slow AMD drivers; #2242 only stops the builder from killing it.
- #2005, #1700, #1002 — damage evidence and tracking; motivation for #2237.
- Builder GUI vs engine/tools/game versions (see the follow-up list above).


Pending: Windows testing of this branch.

## Ground rules

- Follow `docs/workflow.md` for triage, claiming, measurement, testing, PRs, and verification.
- Score new Linux work by severity, Linux reach, and confidence.
- Prefer problems reproducible on the hardware being used.
- Measure before and after every performance claim.
- Always surface dump-integrity findings (damaged or incomplete game files) before proceeding with a build.
- Add a game-free regression test for every fix where possible.
- Run the full game-free suite with `python -I -B tests/run_all.py`.
- Keep changes small and one-topic per branch/PR.
- Do not edit `third_party/`.
- Check `git status` before every commit.

## Baseline

- Linux dependency setup is complete.
- Game-independent Vulkan backend compilation is complete.
- Linux Builder UI has been launched successfully.
- Full-game translation, compilation, packaging, and gameplay remain not started.
- Detailed Linux build/runtime/debug/cache context is in `.llm/`.

## Phase 0 — Linux context baseline

Status: implemented by this planning change.

- [x] Add `.llm/index.llm` as the Linux working-context entry point.
- [x] Add `.llm/linux-build.llm` for the Linux build pipeline and risks.
- [x] Add `.llm/linux-runtime.llm` for the Linux runtime and performance risks.
- [x] Add `.llm/linux-debugging.llm` for observability and future game-run diagnostics.
- [x] Add `.llm/linux-build-cache.llm` for existing caching behavior and gaps.
- [x] Add this `TODO.md` with Linux-only phased work.

Acceptance:

- [x] No source behavior changed.
- [x] No game data was read or added.
- [x] New files are documentation/context only.

## Phase 1 — Linux development/debug observability

Goal: make Linux development builds maximally observable while keeping release builds unchanged.

### 1.1 Define an opt-in Linux development build profile

- [ ] Design a documented development-build contract, such as:
  - `HOOKSHOT_DEV=1`
  - `HOOKSHOT_DEBUG_SYMBOLS=1`
  - `HOOKSHOT_DEV_OPT=<level>`
  - `HOOKSHOT_KEEP_SYMBOLS=1`
- [ ] Allow usable debug symbols for selected host/runtime components instead of unconditional `-g0`.
- [ ] Allow lower optimization for debugging without forcing the entire game to `-O0`.
- [ ] Preserve maps and unstripped development artifacts in ignored work directories.
- [ ] Keep release defaults byte-for-byte behaviorally unchanged.
- [ ] Document which combinations are supported and which are explicitly unsupported.

Acceptance:

- [ ] Release build behavior is unchanged when development variables are unset.
- [ ] Development variables are visible in build logs.
- [ ] A game-independent development backend build completes.
- [ ] Debug artifacts remain outside git.

### 1.2 Standardize a Linux observation profile for game runs

Only for explicitly authorized future game runs.

- [ ] Create a reusable Linux observation checklist covering:
  - CPU/GPU/driver/kernel/distro/display-server/SDL configuration.
  - `/dev/udmabuf` availability, permissions, and size limit.
  - Vulkan validation.
  - CPU profiling and sampling.
  - GPU pass timing and frame logging.
  - Screenshots, frame dumps, probes, and hang dumps.
  - Scripted input and headless reproduction.
  - Log collection and rotation.
- [ ] Use the switch inventory in `.llm/linux-debugging.llm` and `docs/development.md`.
- [ ] Store every capture in an ignored run directory.
- [ ] Never commit logs, screenshots, dumps, profiles, saves, or settings.

Acceptance:

- [ ] Two runs of the same scenario produce comparable logs and traces.
- [ ] A performance report names hardware, settings, switches, version, and measured numbers.
- [ ] No observation artifact enters git.

### 1.3 Add game-free observability regression coverage first

- [ ] Add tests for development-flag plumbing without requiring game code.
- [ ] Add tests for Linux display/SDL environment handling where behavior is game-free.
- [ ] Add tests for Linux update/launcher path handling where behavior is game-free.
- [ ] Keep new tests fast and compatible with existing `tests/run_all.py` conventions.

Acceptance:

- [ ] New tests pass locally.
- [ ] Full game-free suite result is recorded.
- [ ] Failures caused by missing local tools are distinguished from real regressions.

## Phase 2 — Compile speed, rebuild speed, and compilation cache

Goal: never compile something that does not need compiling, while preserving correctness.

### 2.1 Measure the current Linux rebuild behavior

- [ ] Record backend-only build time, object count, cache hits, and memory behavior.
- [ ] Record what happens on a consecutive no-change rebuild.
- [ ] Record what happens after touching:
  - An unrelated Python file.
  - An unrelated C/C++ source file.
  - A header used by many translation units.
  - A build flag or environment variable.
- [ ] Record lift-key behavior after touching files without changing their contents.
- [ ] Save measurements outside git.

Acceptance:

- [ ] Baseline has numbers, not guesses.
- [ ] Rebuild causes are identifiable from logs or stamps.

### 2.2 Harden and extend the object cache

- [ ] Keep the existing content-sensitive object-key design.
- [ ] Add an optional shared content-addressed cache, such as `HOOKSHOT_OBJ_CACHE`.
- [ ] Namespace shared entries by:
  - Compiler name and version.
  - Sysroot or system-header fingerprint.
  - Architecture/microarchitecture flags.
  - Vulkan/SPIR-V/VMA header identity.
  - FFmpeg/SDL identity where relevant.
  - DLSS/FSR/profile settings where relevant.
- [ ] Add cache locking for concurrent builds.
- [ ] Add size cap, pruning, corruption detection, and hit/miss telemetry.
- [ ] Do not allow cross-toolchain or cross-header reuse.
- [ ] Do not store game-derived lifted code anywhere publishable.

Acceptance:

- [ ] A consecutive no-change rebuild does negligible compilation work.
- [ ] Changing one source file rebuilds only affected objects.
- [ ] Cache hits and misses are observable.
- [ ] Corrupt or mismatched cache entries cannot silently enter a link.

### 2.3 Avoid destructive cache loss

- [ ] Replace blind post-failure cache deletion with quarantine of suspect outputs where safe.
- [ ] Preserve known-good cache entries across failed builds.
- [ ] Log why cache entries were retained, quarantined, or deleted.
- [ ] Retain the current safe behavior when corruption cannot be isolated.

Acceptance:

- [ ] A failed build does not force a full healthy-cache rebuild unless corruption is proven.
- [ ] Recovery behavior is deterministic and logged.

### 2.4 Make lift reuse content-based rather than mtime-sensitive

- [ ] Validate unchanged lifted files by content hash, not mtime alone.
- [ ] Narrow lift-key inputs toward files actually read by the lift.
- [ ] Avoid relifting after checkout/branch operations that only update timestamps.
- [ ] Keep broad safety behavior until narrower tracking is proven correct.

Acceptance:

- [ ] Identical lifted content with new mtimes does not force a relift.
- [ ] A real lift input change still forces a relift.
- [ ] Lift validation failures remain explicit and debuggable.

### 2.5 Evaluate external compiler caches

- [ ] Measure ccache/sccache against the existing custom object cache.
- [ ] Test only game-independent and authorized full-game workloads.
- [ ] Adopt an external cache only if it provides a measurable incremental benefit.
- [ ] Do not add redundant cache layers that complicate invalidation.

Acceptance:

- [ ] Decision is recorded with measurements.
- [ ] No adopted cache weakens correctness guarantees.

## Phase 3 — Linux runtime performance and bug backlog

Goal: fix Linux-affecting performance and correctness problems in priority order.

### 3.1 Intake only Linux-affecting work

- [ ] Pull candidates from `docs/worklist.md` that affect Linux, Steam Deck, Mesa/RADV, AMD, Intel, Wayland/X11, SDL, udmabuf, VRAM, shaders, or Linux packaging.
- [ ] Exclude Windows-only rows unless a shared upstream file must stay portable.
- [ ] Do not duplicate upstream issue records; link them.
- [ ] Add a fork-local row only when new Linux evidence exists.
- [ ] Score each candidate by severity, Linux reach, and confidence.

Acceptance:

- [ ] Every active Linux task has a severity, reach, confidence, owner, and evidence link.
- [ ] Windows-only work is not in the Linux active set.

### 3.2 Guest-memory and GPU performance

- [ ] Verify udmabuf availability, permissions, size limits, and fallback behavior.
- [ ] Measure host-pointer fallback cost on affected AMD/RADV hardware.
- [ ] Investigate pool retirement, merges, writebacks, barriers, and submission costs.
- [ ] Investigate texture budget, eviction, render-target sweeps, and long-play texture corruption.
- [ ] Measure shader/pipeline warmup separately from steady-state gameplay.
- [ ] Record GPU timings with `RX_VK_PASS_TIMING`, frame logs, and slow-frame logs.

Acceptance:

- [ ] Each optimization has before/after GPU or frame-time numbers.
- [ ] No lower-tier hardware optimization regresses higher-tier hardware.
- [ ] Fixes include game-free regression tests where possible.

### 3.3 Display, input, and SDL behavior

- [ ] Validate Wayland, X11, XWayland, and Gamescope paths separately.
- [ ] Validate SDL driver selection and bundled/system SDL behavior.
- [ ] Reproduce and harden Wayland/libdecor startup behavior.
- [ ] Validate fullscreen, V-Sync, pacing, frame generation, scaling, and HDR-related paths where applicable.
- [ ] Add game-free tests for environment parsing, driver hints, and fallback selection.

Acceptance:

- [ ] Display fixes name the tested display server and SDL configuration.
- [ ] No fix assumes one compositor, driver, or window system.

### 3.4 Linux packaging and update robustness

- [ ] Make dependency setup portable beyond Debian/Ubuntu paths.
- [ ] Make AppImage sysroot discovery portable beyond Debian multiarch layout.
- [ ] Validate packaged-builder behavior with `scripts/linux/check_player_build.sh`.
- [ ] Review fork-local update URLs and installed-builder discovery.
- [ ] Test launcher behavior for installed builder, AppImage-built folders, and source-checkout folders.

Acceptance:

- [ ] Packaging changes are validated without game data.
- [ ] Fork-local update behavior is explicit and tested.
- [ ] No packaging change writes to the packaged read-only tree.

### 3.5 Renderer and runtime correctness

- [ ] Prioritize crashes, device loss, hangs, black screens, softlocks, save issues, and shader translation failures.
- [ ] Use validation layers, hang dumps, traces, profiles, screenshots, and frame dumps as appropriate.
- [ ] Separate GPU translation problems from host/runtime problems before fixing.
- [ ] Follow renderer interface versioning and generated-file rules in `AGENTS.md`.
- [ ] Never treat a visually plausible single frame as proof of replay correctness.

Acceptance:

- [ ] Fixes include failing-first regression coverage where feasible.
- [ ] Game runs include hardware, area/scenario, logs, and screenshots when visual.
- [ ] Measured configurations are added to `docs/tested-configurations.md`.

## Phase 4 — Validation and contribution hygiene

- [ ] Run targeted tests for every change.
- [ ] Run the full game-free suite before every PR.
- [ ] Rebase onto current `main` before review.
- [ ] Write PR descriptions starting with why the change is needed.
- [ ] Include hardware, test coverage, untested areas, and screenshots where applicable.
- [ ] Keep PRs one-topic and free of unrelated refactors.
- [ ] Keep build outputs, game artifacts, and local environment workarounds out of PRs.

Acceptance:

- [ ] CI is green for every Linux change.
- [ ] Reviewers can reproduce the result from the PR description.
- [ ] No PR contains game data or local-only absolute paths.

## Explicit non-goals

- Windows-specific builder, installer, DirectX, or MSVC work.
- Full-game translation, compilation, packaging, or gameplay without explicit user authorization.
- Third-party source edits.
- Benchmarking while another benchmark lock is held.
- Sharing game files, keys, lifted code, build outputs, saves, logs, or profiles.
