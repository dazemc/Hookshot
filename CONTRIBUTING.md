# Contributing to Hookshot

Hookshot is a static recompilation of the Switch 2 *The Legend of Zelda: Ocarina of Time* remake: the game's AArch64
code is lifted to C, the Horizon OS and NVN2 are implemented natively, and the GPU work is replayed on Vulkan. No
emulator is involved, and the game is built on your PC from your own copy.

This page covers what to send, how to build and test it, and the rules that keep the project out of trouble. The
[development guide](docs/development.md) has the full pipeline and its commands; the [design notes](docs/design.md)
have the architecture and the lessons the code follows.

## The rule with no exceptions

**No game files, keys, or anything made from them leave your PC.** The repository does not contain the game and
never will. That includes:

- the `.nup`, `romfs`, `exefs`, `.nso`, `.nca`, `.nsp`, `.xci` or `.nsz`/`.xcz` files, and any title keys;
- `work/` and everything in it: the extracted dump, `image.bin`, the lifted C, test builds, runs and screenshots;
- `hookshot.exe`, `hookshot_vk.dll`, `hookshot_programs.bin` (and its `hookshot_programs.idx`), `hookshot_pipelines.bin`,
  `mkw_vk_pipelines.bin` and other build output;
- saves (`user/`), `hookshot_options.ini`, `hookshot_env.txt` and `language.txt`.

Do not open an issue or a PR asking where to get the game, how to decrypt it, or for someone else's files. A
screenshot, a video and `hookshot_log.txt` are fine and often necessary; the log holds no game data.

`.gitignore` covers most of this, but check `git status` before you commit. If you are not sure whether a file is
game data, leave it out.

## Ways to help

What to work on first, and the steps from claiming a problem to its release, are in the [workflow](docs/workflow.md):
priorities by severity and by how many players run the affected hardware, and the hardware tiers a change must not
break. Claim in Discord #dev-work before you start. The small, game-free tasks are in
[good first issues](docs/good-first-issues.md).

| | What |
|---|---|
| **Test on other hardware** | NVIDIA cards are the most tested; [tested configurations](docs/tested-configurations.md) lists the runs behind the player-facing summary (build, hardware, driver, where, fps). Reports from AMD and Intel cards, other NVIDIA drivers, other distributions and SteamOS are useful even when everything works: add a row there. |
| **Play further** | The tested area is the start of the game. Reaching new areas, cutscenes, menus and items finds problems the developer has not seen. |
| **Report what is broken** | Use the [issue forms](https://github.com/dazemc/Hookshot-rs/issues/new/choose): game problem, build problem or idea. Include the version, your hardware, where you were and `hookshot_log.txt`. |
| **Send a fix** | Fork, fix, test, open a pull request. Small, focused changes are easiest to review and merge. |
| **Improve the docs** | The README, the development guide, the design notes and this page are part of the project. Corrections and clearer steps are welcome; docs that still describe the Python builder are bugs too. |

## Before you open an issue or a PR

- Try the [newest release](https://github.com/dazemc/Hookshot-rs/releases/latest) first. It includes every fix before it.
- Search the open issues. If yours is already there, add your details instead of opening another.
- English, please.
- One topic per issue or pull request. Do not mix a fix with a refactor or a batch of unrelated changes.
- Report the version you actually ran: the release name, or the first line of `hookshot_log.txt`.
- Test without ReShade, shader-cache tools, overlays and frame-generation injectors when you report a graphics or
  performance problem. They change what Hookshot does.

## Development setup

You need your own supported dump: program `0400486024DEC000`, update v131072, build `e0f3ccfa`. Other versions do
not work, and the build reads the dump where it lies without changing it. Builds need about 50 GB free and 16 GB of
RAM recommended. Build on an SSD if you can: a hard drive works, but takes much longer, and the builder warns you
before it starts.

The maintained builder is the Flutter desktop app in `builder/flutter`; the Rust engine and its CLI are in
`builder/rust`. Both platforms need stable Rust (1.99 or newer) and Flutter 3.47 or newer. The app never invokes
Python; Python remains only in the release tooling, the legacy `nx2/` and `builder/py` builders, and the pipeline
state merge.

### Windows, from the command line

1. Install [stable Rust](https://rustup.rs/) and [Flutter](https://docs.flutter.dev/get-started/install/windows)
   3.47 or newer, and put `cargo` and `flutter` on `PATH`.

2. Get [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) (the `ucrt-x86_64` build) and set `HOOKSHOT_MINGW` to its
   folder. The engine builds the game and the Vulkan back end with it; without it the Windows build stops with a
   message. This engine does not use the old emsdk/Visual Studio route.

3. Copy the pieces a clone does not carry from the `tools` folder Hookshot Builder installs. Run `HookshotBuilder.exe`
   from the latest release once (releases no longer carry a `HookshotBuilder.zip`): the folder is
   `%LOCALAPPDATA%\HookshotBuilder\versions\<tag>\HookshotBuilder\tools`.

   | From `tools` | To | Variable |
   |---|---|---|
   | `tools\cpm` | anywhere | `RX_CPM` points at it (Vulkan, SPIR-V and VMA headers, FFmpeg; without it, system headers or `HOOKSHOT_*_HEADERS`, the build stops) |
   | `tools\wgpu-native` | anywhere | `WGPU_NATIVE` points at it (the optional WebGPU fallback renderer; without it the game is built without one) |
   | `tools\third_party` | `third_party\` | (its `.dll`, `.exe`, `.lib` and `.sha1` files, to the same places: `a64emit`, `lld-link`, `zbic` and SDL2) |
   | `tools\work` | `work\` | (hot-function list, link order, profile and icon; without it the game builds but runs slower) |

4. Build the engine and run the app:

   ```powershell
   cargo build --release --workspace --manifest-path builder/rust/Cargo.toml
   cd builder/flutter
   flutter pub get
   flutter run -d windows
   ```

   The app looks for `builder/rust/target/release/hookshot_engine`; `HOOKSHOT_ENGINE_CLI` points it at another engine.

### Linux

Install stable Rust, Flutter 3.47 or newer, clang and lld, cmake, ninja, pkg-config, the GTK 3 development headers,
OpenSSL headers, and Vulkan, SPIR-V and VMA headers (or set `RX_CPM` to a CPM cache). Then:

```sh
cargo build --release --workspace --manifest-path builder/rust/Cargo.toml
(cd builder/flutter && flutter pub get && flutter run -d linux)
```

The release AppImage is built by `scripts/linux/appimage.sh` into `build-linux/`; it needs the same toolchain plus
`curl`, `zstd` and `file`. The AppImage is a development image: a game build from it needs the distribution's clang
and Vulkan/SPIR-V/VMA headers. The README's [download and play](readme.md#download-and-play) has the player-facing
Linux notes. `scripts/linux/deps.sh` belongs to the old Python/Tk AppImage and is not used by the Flutter builder.

### NVIDIA DLSS (optional)

DLSS (`gpu/vk/vk_dlss.cpp`) is built in only with NVIDIA's DLSS SDK, which is NVIDIA's, under NVIDIA's own license, and
is never committed, bundled or uploaded: not in a PR, not in a release, not in a CI artifact. The Flutter app fetches
it for your own builds when you tick DLSS and accept NVIDIA's license. For a headless build, fetch it with the engine:

```sh
E=builder/rust/target/release/hookshot_engine
printf '%s' '{"command":"build.ngx_fetch","sdk":"C:\\path\\ngx-sdk"}' | "$E"
```

Then pass `"ngx":"C:\\path\\ngx-sdk","dlss":true` to `build.all`, or set `HOOKSHOT_NGX_SDK=<sdk folder>`. On Windows
the llvm-mingw build links NVIDIA's static `nvsdk_ngx_s.lib` (built for Microsoft's static runtime) into
`hookshot_vk.dll`, with `gpu/vk/vk_ngx_msvc_rt.cpp` and `host/mingw_msvc_shims.c` for what it needs of that runtime.
`build.all` copies `nvngx_dlss.dll` and `nvngx_dlssg.dll` into the game folder's `ngx` folder; never put NGX's core,
`_nvngx.dll`, beside the game (it is the driver's). On Linux it uses `libnvsdk_ngx.a` and `libnvidia-ngx-dlss.so`. A
Windows game with DLSS can be built on Linux too, with `RX_TARGET=windows` as below.

CI's `vulkan-dlss` workflow (`.github/workflows/vulkan-dlss.yml`) checks only that `hookshot_vk.dll` still links with the
SDK. It runs by hand, and on a push or pull request only when a file DLSS's build depends on changes; it accepts NVIDIA's
license for that run alone and uploads nothing.

Never share a game folder built with the SDK. A change to the pinned release updates the file table and SHA-256s in
`builder/rust/src/ngx_sdk.rs` (and its tests), and the license text in `builder/flutter/lib/config.dart`.

### Hookshot Builder

The builder is a Flutter desktop app. Open `builder/flutter` in an editor with the Flutter plugin, or run the commands
above. The old WPF app in `builder/HookshotBuilder` and the Tk app in `builder/py` are legacy and not the maintained
builder; do not add features to them for the Flutter app.

`HOOKSHOT_TOOLS` points the app at another source tree. `HOOKSHOT_BUILDER_HOME` moves its settings, downloads and
backups; an empty `portable.txt` beside the executable keeps them beside the app instead.

## Building

The engine takes one JSON object on stdin and answers one JSON object; `--stream` is the Flutter protocol. From a
checkout:

```sh
E=builder/rust/target/release/hookshot_engine
printf '%s' '{"command":"build.prepare","nup":"<game>.nup","out":"<out>","work":"<work>"}' | "$E"
printf '%s' '{"command":"build.all","exefs":"<work>/exefs","romfs":"<out>/romfs","out":"<out>","work":"<work>","language":"en-US"}' | "$E"
```

| Command | What it does |
|---|---|
| `cargo build --release --workspace --manifest-path builder/rust/Cargo.toml` | Build the engine, its CLI and ZBIC into `builder/rust/target/release`. |
| `cd builder/flutter && flutter pub get && flutter run -d windows` (or `-d linux`) | Run the builder app against the engine. |
| `scripts/linux/appimage.sh` | Build the Linux AppImage in `build-linux/`. |
| `printf '%s' '{"command":"build.prepare","nup":"...","out":"...","work":"..."}' \| "$E"` | Read a `.nup` into `work/exefs` and `out/romfs`. |
| `printf '%s' '{"command":"build.all","exefs":"...","romfs":"...","out":"...","work":"..."}' \| "$E"` | Lift, compile, link and package a game folder from an exefs/romfs pair. |
| `printf '%s' '{"command":"build.shaders","romfs":"<dump>/romfs","out":"<out>/hookshot_shaders.bin"}' \| "$E"` | Build the shader census and seed list. |
| `printf '%s' '{"command":"build.ngx_fetch","sdk":"..."}' \| "$E"` | Fetch NVIDIA's DLSS SDK after accepting its license. |

A build reads and writes tens of gigabytes. Keep each game's work folder separate (the `work` field) and never commit
it: the lifted C and every build output are local artifacts.

### Build switches

The builder and engine read these; the runtime and renderer switches are in the
[development guide](docs/development.md#switches-for-testing-and-debugging), where the surrounding switch tables are
kept.

| Switch | What it does |
|---|---|
| `HOOKSHOT_TOOLS=<dir>` | Source tree the engine builds from (default: this checkout). |
| `HOOKSHOT_ENGINE_CLI=<file>` | Engine binary the Flutter app starts. |
| `HOOKSHOT_BUILDER_HOME=<dir>` | Builder settings, downloads and backups. |
| `HOOKSHOT_LIFT_JOBS=<n>`, `HOOKSHOT_NUP_JOBS=<n>`, `HOOKSHOT_CENSUS_JOBS=<n>` | Lift, `.nup` extraction and shader census workers. |
| `HOOKSHOT_MINGW=<dir>`, `RX_CPM=<dir>`, `WGPU_NATIVE=<dir>` | Windows compiler, Vulkan/SPIR-V/VMA headers, optional WebGPU renderer. |
| `HOOKSHOT_VK_PREBUILT=<file>`, `HOOKSHOT_NGX_SDK=<dir>` | Use a prebuilt Vulkan back end; build DLSS with NVIDIA's SDK. |
| `HOOKSHOT_A64EMIT_LIB=<file>`, `HOOKSHOT_ZBIC_LIB=<file>`, `HOOKSHOT_SDL2=<dir>` | Translator, ZBIC and SDL2 libraries to package. |
| `HOOKSHOT_BUILD_TIMING=1`, `HOOKSHOT_FORCE_PROMPTS=1` | Print stage timings; regenerate prompt assets. |
| `RX_TARGET=windows`, `RX_RELEASE=1`, `RX_PGO=gen`, `RX_MARCH=<level>`, `RX_MTUNE=<tune>`, `RX_STEAMDECK=1` | Cross-build for Windows, release layout, profile generation, CPU level and scheduling. |

## Testing

The old Python source-reading suite and `tests/run_all.py` are gone. The checks are now:

| Command | What it runs |
|---|---|
| `cargo test --workspace` in `builder/rust` | Rust engine, CLI, C/C++ fixtures and the docs checks (`docs.rs`, `rust_only_app.rs`). |
| `flutter test` and `flutter analyze` in `builder/flutter` | Builder state, routing and UI tests. |
| CMake under `tests/` and `gpu/vk/tests` | The C and C++ fixtures that do not need the game. |
| A run in the game on your hardware | Runtime, renderer and performance changes. Record what you did, what you saw, and the numbers from `hookshot_log.txt`. |

Add a Rust or Flutter test for a fix where the behavior can be tested without the game. Never add game files or
generated game data to the repository or to CI.

## Code and data conventions

- Match the file you are editing: there is no formatter configuration, so use the indentation, comment style and
  naming around you. Do not reformat unrelated code in the same change.
- Keep OoT-specific code in new files where possible. `nx2/`, `host/` and `gpu/` are kept close to mkw-recomp so
  upstream fixes can be applied as a patch; keeping the paths and the shared files recognizable is what makes that
  work. Each sync names its new mkw-recomp base commit in the commit message.
- Do not modify `third_party/`. Its parts are unmodified upstream copies with their notices in `PROVENANCE.md`; add a
  new folder and a provenance note rather than editing one.
- Treat the local mkw-recomp and Eglantine checkouts as read-only references: read from them, run them, never edit
  them.
- The app's path is Rust and Flutter. Python remains only for release tooling, the legacy `nx2/` and `builder/py`
  builders, and the pipeline-state merge; do not add a new build stage there. Legacy Python stays on the standard
  library except the existing pipeline dependencies (lz4, numpy, capstone, unicorn, zstandard) and the button prompts'
  packages (`nx2/prompt_requirements.txt`), which are accepted for `nx2/prompt_assets.py` and `nx2/prompt_draw.py`
  alone, at build time.
- New switches use the `RX_` prefix in the renderer, `HOS_` in the runtime and `HOOKSHOT_` in the builder and engine.
  They are off, or neutral, unless asked for, and they do not cost anything while off. Document them where the
  surrounding switches are documented.
- The GPU replay rules in [design notes, section 4](docs/design.md#4-the-lessons-from-mkw-as-rules-here) are not
  suggestions: nothing the replay reads may come from live guest memory, waits are honoured per queue, and texture
  and sampler registrations stay in submission order. A change that reads guest memory at replay time is a
  regression, even if it looks right on one frame.
- Keep line endings LF (`.gitattributes`).
- New code has to be GPL-compatible. See below.

## Commits and pull requests

1. Fork the repository and branch from `main`. Name the branch for the change: `fix/...`, `linux/...`, `builder/...`,
   `docs/...`.
2. Keep the pull request to one topic. Bug fixes do not carry refactors, and two unrelated fixes are two PRs.
3. Write the title as a Conventional Commit (see [Pull request titles](#pull-request-titles) below). Start the body
   with why the PR is needed, in one or two plain sentences a player could follow: the bug, the missing feature or the
   cost it removes ("The menu's text is cut off at 4K", "First starts spend 20 s waiting on a few shaders"). Then say
   what changed and how it was tested (GPU, driver, game area, fps before and after).
4. Link the issues the PR fixes, if there are any. Write `Fixes #123` in the body (one line per issue) so that merging
   the PR closes the issue. If the PR only addresses part of an issue, or is related to one, write `Part of #123` or
   `See #123` instead, so the issue stays open. Search the open issues before you open the PR: a report may already
   describe the bug you fixed.
5. Include screenshots when the change shows on screen: graphics, the menu, the start-up screen, an overlay, a
   message. Before and after for a fix, the new screen for a feature, and the sizes or settings it was checked at. A
   screenshot of the game is fine; the rule above is about game files, not pictures of the game.
6. Run the relevant checks. CI must pass; a change that only works on your machine is not ready.
7. Rebase or merge `main` when it has moved, and resolve conflicts in your branch rather than in the review.
8. Do not include build output, dumps, generated files or a `.gitignore` bypass. Check `git status`.

### Pull request titles

A pull request is squash-merged, so its title becomes the commit on `main`, and that commit decides the next release
and its changelog line. The title is a [Conventional Commit](https://www.conventionalcommits.org/): a type, an optional
scope in parentheses, a colon, and a short description. The *PR title* check fails until it is one; edit the title
and the check runs again.

| Type | For | Example from this repository |
|---|---|---|
| `feat` | something new for players or modders | `feat(menu): tabbed category layout for in-game options with bumper and Q/E navigation` |
| `fix` | a bug fix | `fix: preserve the changelog when release generation fails` |
| `perf` | faster, or less memory | `perf(linux): hand guest memory to the GPU as udmabuf dma-bufs, not host pointers` |
| `refactor` | a change with no effect on behaviour | `refactor(vk): ...` |
| `docs` | documentation only | `docs: document the built game's command-line arguments` |
| `test` | tests only | `test: cover pool-less NVN buffer mappings` |
| `ci` | the GitHub workflows | `ci: actions pinned by commit, least privilege, realistic timeouts, Dependabot and actionlint` |
| `build` | the build scripts, the AppImage, the builder package | `build: scope the shader cache stamp to its consumer` |
| `chore` | anything else | `chore: remove patch backup files added by mistake` |
| `revert` | undoing an earlier change | `revert: ...`, with the reverted PR in the body |

The scope is optional and names the part of the project: `vk`, `linux`, `builder`, `menu`, `video`, `runtime` and so
on. `feat` makes the next release a minor version, and `fix` and `perf` a patch (each first as a release candidate). A `!` after the type or scope (`feat(builder)!: ...`)
marks a breaking change. The other types make no release on their own. The commits inside a branch can be written
any way you like: only the title reaches `main`. The version rules are in [docs/versioning.md](docs/versioning.md).

Maintainer commits record AI assistance with `Assisted-by:` and `Co-Authored-By:` trailers. If you used a tool, add
the trailers that describe it; if you wrote the change yourself, there is nothing to add.

## Licensing

Hookshot is free software under the [GNU General Public License, version 3 or later](LICENSE). By submitting a change
you agree to license it under those terms. Only send code you can license that way. Do not paste code from a project
with an incompatible license, and do not paste decompiled game code or anything generated from the dump into a PR.

The third-party parts (Eden, suyu, Sirit, the Vulkan Memory Allocator, SDL, Opus, Dear ImGui, zstd, the Libertinus
fonts, zbic and the a64-kit tools) keep their own notices. Preserve them, and do not relicense their code.

NVIDIA's DLSS SDK is not one of them: it is never in the repository, and nothing of it may be added (see
[NVIDIA DLSS](#nvidia-dlss-optional)).

## Where to talk

- **Issues:** [use the forms](https://github.com/dazemc/Hookshot-rs/issues/new/choose).
- **Discord:** https://discord.gg/EajnD2AJD9. Use the `hookshot-discussion` channel; the server is mainly for
  web ports, and that channel is where Hookshot talk belongs.
- **Newest release:** https://github.com/dazemc/Hookshot-rs/releases/latest.

Thanks for testing, reporting and sending fixes. Every report from a different GPU and every area played further than
before moves the port forward.
