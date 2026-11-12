<h1 align="center">
  <br>
  <img src="assets/hookshot.png" alt="Hookshot" width="200">
  <br>
  <b>Hookshot-rs</b>
  <br>
</h1>

<h4 align="center">The Legend of Zelda: Ocarina of Time (Nintendo Switch 2) running natively on Windows & Linux, with no emulator.
</h4>

<p align="center">
  <a href="#download-and-play">Download</a> |
  <a href="#build-your-game">Build your game</a> |
  <a href="#updating-profiles-and-saves">Updating and saves</a> |
  <a href="#benchmarks">Benchmarks</a> |
  <a href="docs/fork.md">Technical reference</a> |
  <a href="#if-something-goes-wrong">Problems</a> |
  <a href="#contributing">Contributing</a> |
  <a href="#credits">Credits</a> |
  <a href="#license">License</a>
</p>

Hookshot is a static recompilation: the game's code is translated to C, compiled into a native Windows or Linux
program, the console's system software is reimplemented, and the graphics are rebuilt on Vulkan. Everything happens
**on your PC, from your own copy of the game**.

This is **Hookshot-rs**, the build-system fork of [NotRexed/Hookshot](https://github.com/NotRexed/Hookshot). It keeps
the game and renderer and replaces the builder with one Flutter desktop app and one Rust engine. Every build stage the
app runs is Rust and the app never invokes Python; the measured build times are in
[docs/build-performance-report.md](docs/build-performance-report.md).

> [!IMPORTANT]
> This repository **does not contain the game, keys or an executable**, and it does not explain how to get them. You
> need your own copy. Everything a build creates — the executable, generated code, shader caches and saves — comes
> from your game and stays on your PC. Never share those files.

> [!WARNING]
> Some users report the release file as a Trojan. It is a false positive. You can check it with any antivirus
> software.

## Download and play

The pre-release `v2.2.0-rs.10` is built entirely by the Flutter + Rust pipeline: the Windows setup and the Linux
AppImage below. The AppImage is a development image, so building a game from it needs your distribution's clang and
Vulkan, SPIR-V and VMA headers; a source build needs nothing extra.

- **Windows:** download `HookshotBuilder.exe` from [Releases](https://github.com/dazemc/Hookshot-rs/releases) or the
  latest development build. For a portable install on an external SSD, run it with `--portable`; see
  [Portable Hookshot Builder](docs/windows-portable-builder.md) for the folder, move and repair behavior.
- **Linux:** download the AppImage, make it executable, and run it:
  ```sh
  chmod +x Hookshot*.AppImage
  ./Hookshot*.AppImage
  ```
- **Requirements:** 64-bit Windows 10/11 or a 64-bit Linux with glibc 2.35+ (Ubuntu 22.04, Debian 12, Fedora, Arch,
  SteamOS); a CPU with AVX2; a Vulkan-capable GPU; 16 GB of RAM recommended; about 50 GB free for the build.

## Build your game

1. Start the builder and choose your game:
   - a `.nup` package, or
   - an `exefs` folder and a `romfs` folder.
2. Choose an output folder with about 50 GB free. The builder keeps the translated code and compiled objects in
   `~/.cache/hookshot-builder` on Linux or `%LOCALAPPDATA%\HookshotBuilder` on Windows, so later builds are faster.
3. Pick the language and the options you want (DLSS, FSR, shader preparation).
4. Press **Build**, then **Play**.

The first build is the long one. After it, Play starts the existing game folder immediately, and a game update only
rebuilds what changed.

| First build | Time on one Linux PC |
|---|---:|
| Typical | 15–30 minutes |
| This fast 32-core PC | ~12–16 minutes |

The first build reads and verifies the game files, translates the game, compiles it, builds the Vulkan backend,
packages the folder and optionally prepares shaders. See [Benchmarks](#benchmarks) for the stage details.

### Build from source

For development, see [docs/development.md](docs/development.md). The short version:

```sh
cd builder/rust && cargo build --release
cd ../flutter && flutter pub get && flutter run
```

The engine also takes one JSON command on stdin; `hookshot_engine --help` and the
[technical reference](docs/fork.md) list the commands.

## Updating, profiles and saves

- **Update:** the launcher offers an update when a newer release exists. Press **Update**; a built folder keeps the
  game files it needs, so an update does not ask for the `.nup` again.
- **Profiles:** each profile has its own save folder, settings and controller layout. Create and switch them in the
  launcher.
- **Saves:** on Linux saves live in `~/.local/share/hookshot`; on Windows in the `user` folder in the game folder.
  The launcher can import saves from Eden, NX2navi, Lichii and other emulators.
- **Moving a game folder:** keep the `romfs`, `.hookshot_exefs` and the launcher's save folder together; the launcher
  can move the saves for you.

## In-game menu

Press **Insert**, **F1** or **Esc** to open the Insert menu. It has Graphics, Audio, Gameplay, Controls, Debug/cheats
and developer rows; HDR, DLSS/FSR, frame generation, the FPS counter, field of view, camera speed, text speed and the
debug teleport are there. The Graphics tab groups its rows under section titles: Resolution and upscaling, Frame
generation and latency, Screen, HDR, Detail, and GPU and memory (last: the rows to change when something is wrong).
The Quality preset's **Auto (this PC)** picks the scaling rows, upscaler and output from the graphics card, its video
memory and the screen, and measures the Render resolution in the first seconds of play; the log's `vk: Auto (this PC)`
line says what it chose and why. A preset or row you saved is never changed by it.
Mesh shaders is not a row: `RX_VK_MESH_SHADERS=native` or `compute` in `hookshot_env.txt` (or the environment) chooses
it for a launch, and an existing `mesh_shaders=` line in `hookshot_options.ini` still chooses as its old row did. With
frame generation on, **Frame generation by** and the FPS counter name who makes the frames: `DLSS FG`, `FSR 3.1 FG` or
`Built-in FG` (the FPS counter's second line is `Base 60, DLSS FG`). The
Controls page also gives the ocarina's five note rows of their own, `Ocarina note (L)`, `(R)`, `(A)`, `(Y)` and `(X)`,
unset by default: until you bind one, the game's own button plays it, and a bound note answers only its own key or
gamepad input while the game waits for a note. With HDR set to Native, the HDR section's tone mapper row chooses the
game's vanilla tone mapping or PsychoV35. The menu itself speaks English, Português (Brasil), Español (Latinoamérica and
España), Français, Deutsch, Italiano, Nederlands, Polski, Русский, 日本語, 한국어, 简体中文 and 繁體中文:
**Gameplay → Menu language** (Auto follows the game's language). The game's own language is the separate
**Game language** row, which writes `language.txt` and applies after a restart; English is the fallback where a text has
no translation. The in-game controls and the full option list are in the [technical reference](docs/fork.md) and in the
builder's own help.

**Gameplay → System → Bug report** saves what an issue needs into `bug-reports/<date>_<time>` in the game folder in one press:
`report.txt` (the version, the map you are in, your graphics card and driver, CPU, RAM and system, the menu options and
`RX_`/`HOS_` switches you changed, and a link that fills in the issue form), this run's `hookshot_log.txt` and the run
before's `hookshot_log.1.txt`, and `picture.png` of the game without the menu. The logs' home folder is written `~`
(`%USERPROFILE%` on Windows) and the user name `<user>`; a switch value is written only when it is a number, a size or
on/off, and never for a name with `TOKEN`, `KEY`, `ROOM`, `SECRET`, `PASS`, `SERVER` or `USER` in it. No saves, game
files, `hookshot_options.ini` or `hookshot_env.txt` go in, and nothing is sent: attach the folder's files to an issue.

## Benchmarks

Measured on one Linux PC (Ryzen 9 5950X, 16 cores/32 threads, 15.5 GiB RAM, RTX 4070 Ti, f2fs RAID 0). Other hardware
and background load change these; they are development measurements, not universal guarantees.

| Work | Before | Hookshot-rs now |
|---|---:|---:|
| 5,000-function lift | 65 s | **8.1 s** |
| Full 294,522-function lift | 10 m 30 s | **57 s cold**, **19.8 s cached** (15.05 s quiet) |
| 25.6 GB `.nup` read | 94 s | **64 s** (46 s quiet) |
| Shader census | 9.7 s | **3.88 s** |
| Vulkan backend build | — | **100.6 s clean**, 0.7 s repeat |
| First compile (66 objects → 335 units) | 10–20 min | **13.8 min cold** with memory-sized waves (up to 32 jobs as the giants finish); **4.3 s warm** (335/335 units cached) |
| Game boot | — | 2,640 presents at 60 fps, 0 traps |

The [technical reference](docs/fork.md) explains what is Rust, what is still Python, the upstream-port status, the
build pipeline and the remaining gaps. [docs/performance.md](docs/performance.md) has the full build-stage profile and
the compile-speed work.

## If something goes wrong

| Problem | What to do |
|---|---|
| The build stops on the game version | Use the supported build: Ocarina of Time 1.1.1, build `e0f3ccfa…`. |
| “Not enough free space” | The build needs about 50 GB free; free space or choose another output folder. |
| No picture / black window | Update the GPU driver; make sure the game folder has `bin/hookshot_vk.so` (Linux) or `bin\hookshot_vk.dll` (Windows); try `RX_BACKEND=vulkan`. |
| The game is slow or stutters | Turn on shader preparation, use the Insert menu's graphics settings, and see the GPU notes in the [technical reference](docs/fork.md). |
| A save is missing after switching profiles | Check the profile's `save` folder; the launcher's import can bring saves from an emulator. |
| Where is the log? | `hookshot_log.txt` in the game folder; older runs are `hookshot_log.1.txt` to `.4.txt`. **Gameplay → System → Bug report** gathers the logs, hardware and options for an issue in one press. |
| The launcher/engine is missing | Install Hookshot Builder again. The builder no longer falls back to Python; a missing Rust engine stops with a clear error. |

## Contributing

Rust, Flutter and C/C++ only; the Python test suite was removed. Run:

```sh
cd builder/rust && cargo test --workspace
cd builder/flutter && flutter test && flutter analyze
```

Never commit game files, keys or generated game data. New here? Start with
[docs/good-first-issues.md](docs/good-first-issues.md) and the open `good first issue` labels; the
[contributor discussion](https://github.com/dazemc/Hookshot-rs/discussions/8) has the build-time numbers and the
upstream reference PR. [CONTRIBUTING.md](CONTRIBUTING.md) has the full rules, and
[docs/fork.md](docs/fork.md) has the architecture and porting status.

## Credits

Hookshot exists thanks to the work of many people:

- **NotRexed**: Hookshot, BlueShell (Mario Kart World), Skybound (Tears of the Kingdom) and the a64-kit tools.
- **[Eden](https://git.eden-emu.dev/eden-emu/eden)** and **Eglantine**: the shader recompiler and Switch 2 shader work.
- **suyu**: the graphics decoders and the AArch64 translator.
- **[seedds/nxnative](https://github.com/seedds/nxnative)**: reference for how task and mesh shaders work.
- **[EvilBoyce / evilmfkr](https://github.com/evilmfkr/Hookshot)** (EvilOOT): GPU culling, ambient light probes, pond
  water, fog, the Load freeze fix and the exposure flicker fix.
- **[zortos293/Lichii](https://github.com/zortos293/Lichii)**: Ampere shader lowering and the compare-exchange retry fix.
- **[mstorsjo/llvm-mingw](https://github.com/mstorsjo/llvm-mingw)**: the bundled Windows compiler.
- **NVIDIA DLSS**: optional, downloaded by each build, never bundled. NVIDIA and DLSS are NVIDIA trademarks.
- **AMD FSR 3.1 and FSR 4**: FSR 3.1 is built from AMD's MIT-licensed shaders; FSR 4 uses AMD's MIT-licensed API
  headers and optional DLLs. AMD, Radeon and FidelityFX are AMD trademarks.
- **[kinnay/zbic](https://github.com/kinnay/zbic)**, **Opus** and **SDL**: decompression, audio and controllers.
- **[sostenesfreitas/ocarina-recomp](https://github.com/sostenesfreitas/ocarina-recomp)**: this README's layout.
- **[MythosOfs/Orchid](https://github.com/MythosOfs/Orchid)**: the early fog and exposure workaround.
- **Mesa (NAK)**: reference for Ampere instruction encoding.
- Everyone who sent fixes, including SkippySteve, monsterlag, klimovich008, hckrman101, madebyjordan, N3oTraX,
  shayderrr, gab9281, Wpaif, nicholasgower, ZurielAtr, Joey-Magnus, Zorkats, d3zessete, chrispository, vzeol,
  leonidasmark, Segmentree, ShyVortex, hiKareeem and i900gaming.

## License

Hookshot is free software under the [GNU General Public License, version 3 or later](LICENSE) (GPL-3.0-or-later).
It includes code from Eden and suyu, which are under the GPL too. Third-party parts under other licenses that work
with the GPL (Sirit, VMA, SDL, Opus, Dear ImGui, zstd, the Libertinus fonts, zbic and a64-kit) keep their notices in
their folders.

Hookshot is an unofficial fan project, not affiliated with or endorsed by Nintendo. The Legend of Zelda and Ocarina
of Time are Nintendo trademarks.
