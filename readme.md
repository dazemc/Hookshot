<h1 align="center">
  <br>
  <a>
  <img src="assets/hookshot.png" alt="Hookshot" width="200">
</a>

  <br>
  <b>Hookshot</b>
  <br>
</h1>

<h4 align="center">The Legend of Zelda: Ocarina of Time (Nintendo Switch 2) running natively on Windows & Linux, with no emulator.
</h4>


<p align="center">
  <a href="#this-fork">This fork</a> |
  <a href="#project-status">Status</a> |
  <a href="#install-and-play">Install and play</a> |
  <a href="#in-game-menu">In-game menu</a> |
  <a href="#if-something-goes-wrong">Problems</a> |
  <a href="#build-from-source">Build from source</a> |
  <a href="#how-it-works">How it works</a> |
  <a href="#testing">Testing</a> |
  <a href="#want-to-help">Want to help?</a> |
  <a href="#credits">Credits</a> |
  <a href="#license">License</a>
</p>

Hookshot is a static recompilation. The game's code is translated to C and compiled into a Windows or Linux program,
the console's system software is reimplemented, and the Switch 2 graphics are rebuilt on Vulkan. All of this happens
**on your PC, from your own copy of the game**.

This tree is the build-system fork of [NotRexed/Hookshot](https://github.com/NotRexed/Hookshot), published as
[Hookshot-rs](https://github.com/dazemc/Hookshot-rs). The game, the renderer
and the player documentation come from upstream; the fork moves the build to **one Flutter desktop app** and a **Rust
engine**. The Flutter app is `builder/flutter`; the engine is the `hookshot_engine` crate in `builder/rust`. On Linux, a
game build started from the Flutter app runs no Python: the `.nup` read, lift, patches, compile, host link, Vulkan
backend, package and shader preparation are Rust.

The release AppImage and the Windows builder package are still produced by the older Python and .NET scripts and still
use the Tk and WPF builders; a few build stages are not ported yet. [This fork](#this-fork) says exactly which. The game
itself and the player instructions below are upstream's.
<br>

> [!WARNING]
> Some users report the file in releases to be a Trojan - it is not. Feel free to run it through any antivirus software


Discord: https://discord.gg/DVQQJphbBf
<br>

> [!IMPORTANT]
> This repository **does not contain the game, keys or an executable**, and it does not explain how to get them. You
> need your own copy. Everything the build creates (`hookshot.exe`, the generated code, the shader caches and your
> saves) comes from your game and stays on your PC. Never share those files.

## This fork

**One Flutter desktop UI and one Rust engine.** The builder in this tree is one Flutter app with Linux and Windows
runners: it chooses the game files and install folder, shows the build, launches the game, manages profiles and updates.
The Rust engine does the work behind it. The app calls the engine in-process for small queries (version check, profiles,
saves, diagnostics, gamescope options) and runs `hookshot_engine --stream` for a build, reading the builder's
`@@stage`, `@@progress`, `@@done` and `@@error` lines.

A game build started from the Flutter app on Linux runs no Python, DLSS included: the `.nup` read, lift, patches,
compile, host link, package, Vulkan backend, NVIDIA's DLSS SDK and its NGX-enabled link, AMD's FSR 4 flags and shader
preparation are Rust. It can reuse the AppImage's prebuilt Vulkan renderer (`HOOKSHOT_VK_PREBUILT`), except for a DLSS
build, which compiles its own backend. The AppImage itself, the Windows package, the Windows Vulkan DLL, and the
button-prompt textures are the parts still on the Python path or in progress.

The older Tk and WPF builder sources remain in the tree for the release pipeline. They are not the build path this
README documents.

### Measured on one Linux machine

These are development measurements from this tree, on one Linux PC with the same dump and disk. They are not release
benchmarks and not universal: CPU, RAM, SSD, filesystem, driver and background load all change them, and only the
machine tested is claimed. A "before" figure belongs to that row's earlier implementation; the rows are not one common
baseline.

| Work | Before | Now (Rust) | Check |
|---|---:|---:|---|
| Full lift, 294,523 functions | 10 m 30 s | 57 s cold, 49 s cached | same machine and dump; the cache is the per-function lift cache |
| 5,000-function lift | 65 s | 8.1 s | earlier lift against the current one |
| OoT patches, full 59-chunk lift | Python's output | byte-identical | `diff -r -q` reported no differences; the required output-size edits match |
| OoT after-instruction sites | Python's output | byte-identical | all 20 sites: 11 widescreen, 3 jitter, 6 occlusion |
| Shader census | — | 9.7 s | 1,445 files, 38,809 programs, 42,856 stages, 539 MB list |
| Shader preparation | — | 36.8 s first run; 17.6 s for the whole cached `build.all` | 2,863 programs; 15,330 graphics + 71 compute pipelines |
| `.nup` read | — | 94 s for 25.6 GB | read to exefs + 1.7 GB romfs, 0 unresolved |
| Vulkan backend build | — | 100.6 s clean, 0.7 s repeat | 195 sources, 212 jobs, content-addressed objects; 11,665,496-byte `hookshot_vk.so` with 4 compile jobs |
| End-to-end `build.all` | — | packaged | 66/66 objects cached, host link ok |
| Boot | — | 2,640 presents at 60 fps | Rust-built Vulkan backend, prompts generated, content and save mounted, 0 traps |

The boot run is the Rust-built Vulkan backend with Rust-generated prompts; it predates the latest upstream host-fix
batch, so those host changes are not in that binary. The shader, `.nup` and Vulkan rows have no "before" figure because
the same Rust stage did not exist to time. A separate build-performance branch has measured further lift/NUP/census
gains; those numbers are not in this table until it merges.

### What changed from upstream

| Area | Upstream | This tree |
|---|---|---|
| Builder UI | separate Tk (Linux) and WPF (Windows) apps | one Flutter desktop app, `builder/flutter`, for Linux and Windows |
| Build driver | Python `nx2/make_game.py` | Rust `hookshot_engine`: `build.prepare`, `build.all`, `--stream` |
| NUP and image | Python `nx2/nup.py`, `nx2/image.py` | Rust NUP reader, NSO/ZBIC/RELR and image builder |
| Lift and OoT changes | Python lift; patches through a Python bridge | Rust lift with jump tables and a per-function cache; native patch table and after-sites |
| Compile and object cache | Python `nx2/build.py` | Rust `build.compile_many` with a content-addressed object cache |
| Windows host link | Python `nx2/build.py` | Rust `build.host_link`: llvm-mingw, the same host sources/flags/libraries, content-keyed objects, UTF-8 response files and DLL staging; cross-built with stub objects, not yet a real game link |
| Vulkan backend | Python `nx2/build.py` | Rust `build.vk`: the same 195 sources, 212 jobs, content-addressed objects and a symbol check; NVIDIA's NGX/DLSS link and AMD's vendored FSR 4 flags are ported for Linux, the Windows Vulkan DLL is not |
| Button prompts | Python `nx2/prompt_assets.py` | Rust package step: the font, layouts, ring, hints and size table; the prompt textures are not ported |
| Shaders | Python census and prepare stages | Rust `build.shaders`, and prepare inside `build.all` |
| Updates | `make_game.py --update` | Rust `build.all` from the folder's kept `.hookshot_exefs` and `romfs` |
| Tests | Python source-reading suite, `tests/run_all.py` | Rust, Flutter and C/C++ checks |
| Release packaging | Python and .NET scripts | unchanged so far; port in progress |

### What is Rust now

| Part | Entry points | Notes |
|---|---|---|
| NUP read, verify and extract | `build.prepare`, `nup.list`, `nup.verify`, `nup.extract` | ZBIC NSOs unpacked; damaged-file stand-ins; writes `work/exefs` and `out/romfs` |
| Image, NSO, ZBIC and RELR | `image.build`, `image.adr`, `nso.load`, `nso.dump`, `build.image` | the image layout and relocations the lift reads |
| Lift | `lift.write`, `lift.tables`, `build.driver` | jump tables; per-function cache in `gen/lift_cache` |
| OoT patches and after-sites | `build.driver` | native table, 20 after-sites, no Python fallback |
| Compile and object cache | `build.compile_many`, `build.compile_one` | content-addressed objects; 66/66 cached in the end-to-end run |
| Host link (Linux) | `build.host_link` | `nx2/build.py`'s Linux source list and link line |
| Host link (Windows) | `build.host_link` | llvm-mingw, the same host sources, flags and libraries as `nx2/build.py`, content-keyed objects, UTF-8 response files, PE manifest/map/stack, and SDL, wgpu-native and FFmpeg DLLs staged beside the executable |
| Vulkan backend (Linux) | `build.vk` | the same 195 sources as `nx2/build.py`, 212 jobs, content-addressed objects and an exported-symbol check; with `HOOKSHOT_NGX_SDK` (or a request's `ngx`) `vk_dlss.cpp` sees NVIDIA's headers and the static library is linked, and `third_party/fidelityfx` turns on FSR 4 in `vk_fsr4.cpp`; the Windows Vulkan DLL is not ported |
| Package | `build.package` | stages the executable, image, romfs, shader list and language |
| Button prompts | `build.package` | writes `prompts/External.bfotf`, `Common.blarc`, `ring.bin`, all five `hint.bin` and `rsizetable.zs` from the game's files; no Python |
| Shader census | `build.shaders` | writes `hookshot_shaders.bin` |
| Prepare shaders | `build.all` with `prepare` | runs the built game with `--prepare-shaders` |
| Builder services | `profiles.*`, `diagnostics`, `gamescope.command`, `build.cpu`, `build.toolchain`, `build.jobs`, `build.check`, `version.describe` | profiles list/add/rename/remove/export; diagnostics; gamescope; CPU, jobs and toolchain checks |
| Saves | `saves.list`, `saves.import` | the app's save scan and import: emulator layouts, profile ids, backup to `oot-save-backups`, running-game and NX2navi guards, rollback |
| Updates | `build.all` from `.hookshot_exefs` and `romfs` | a built folder keeps the game files an update needs |
| DLSS | `build.all` with `dlss`, `build.ngx_fetch`, `build.ngx_install` | the SDK's headers, library and runtime, each checked against its SHA-256; the Linux backend is compiled and linked with NGX and DLSS's runtime is packaged in `<game folder>/ngx`. Frame Generation's runtime and the Windows DLL are in progress |

The prompt font is semantically identical to Python's, not byte-identical: the glyphs, outlines, cmap and hmtx match,
while the CFF layout differs (37,360 bytes against Python's 33,452). `Common.blarc`, `ring.bin` and all five `hint.bin`
are byte-identical; `rsizetable.zs` has the same content with a different compressed frame. `Common.bntx` and
`prompts/<face>/textures.bin` are not generated, so the game keeps its own button textures.

### What still needs Python, or is not ported

| Part | What runs today | Status |
|---|---|---|
| Windows DLSS build | The DLSS build stage is Rust on Linux: the SDK fetch, NGX compile and link, and the runtime in the game folder's `ngx`. Windows needs `hookshot_vk.dll`, which the Rust engine does not build yet. | in progress |
| Windows Vulkan DLL | The Rust engine links the Windows host, but `build.vk` still links only the Linux `hookshot_vk.so`; the Windows `hookshot_vk.dll` is still `nx2/build.py`'s. | in progress |
| Prompt textures | The package writes the font, layouts, ring, hints and size table; `Common.bntx` and `prompts/<face>/textures.bin` are not generated, so the game keeps its own button textures. | not ported |
| Release and AppImage packaging | `scripts/linux/appimage.sh`, `scripts/release_parts.py`, `scripts/make_setup_parts.py`; the AppImage still launches the Tk builder and the Windows package is still the .NET builder's. | not ported |

Flutter itself calls no Python. A build or update with no Rust engine stops with a typed
`RustEngineMissing` error instead of falling back to `make_game.py`.

### Still to come

- Windows Vulkan DLL linking in Rust, and with it the Windows DLSS build (the Linux NGX/DLSS and FSR 4 flags are in).
- DLSS Frame Generation's runtime beside DLSS's in `<game folder>/ngx`, on both platforms.
- The prompt textures: `Common.bntx` and `prompts/<face>/textures.bin`.
- A Windows boot test of the cross-built host and a real game link; the VM is prepared but not started while builds run.
- Release, AppImage and Windows packaging in the new builder.
- Further upstream Vulkan and host fixes are being ported; hardware-specific ones need that hardware in the game.
- A build-performance pass is in progress over lift chunks, NUP read, shader census, compile, link and package. On that
  branch the cached full lift measured 50.8 → 15.05 s, `.nup` read 94 → 46 s and shader census 4.02 → 3.08 s, with
  byte-identical output; it is not merged, so the table above is the verified baseline.

The release/AppImage packaging still uses Python. Everything in a Linux Flutter game build is Rust, DLSS included,
and Flutter does not invoke Python; the prompt textures are the one asset it does not write.

## Project status

This table is the game's status, not the builder's. The build system's status is [above](#this-fork).

Experimental. Most testing so far has been around the start of the game. Players have reported on many PCs since:
the table says what works and what does not yet.

| What | Status |
|---|---|
| Kokiri Forest: reaching gameplay and walking around | ✅ Works |
| Sound, controller, keyboard and saves | ✅ Works |
| In-game menu, frame generation and upscaling | ✅ Works |
| Flicker (the picture flashing too bright while moving) | ✅ Fixed in v2.0.2 ([#645](https://github.com/NotRexed/Hookshot/pull/645)) |
| Ground fog vanishing when looking down | ✅ Fixed in v2.0.2 ([#638](https://github.com/NotRexed/Hookshot/pull/638)) |
| Models with missing pieces (doors, fences, parts of trees) | ✅ Fixed where tested, more play needed |
| Stutter when entering new areas | ⚠️ Still happens the first time you visit a place. The next visits are smooth |
| Performance | ⚠️ Depends on the PC: some reach a steady 60 fps, others stay below it, more so in Hyrule Field |
| Hyrule Field, Castle Town, Kakariko and the dungeons | ⚠️ Players got there, with open bugs: striped shadows in Hyrule Field ([#468](https://github.com/NotRexed/Hookshot/issues/468)), some crashes ([#317](https://github.com/NotRexed/Hookshot/issues/317), [#648](https://github.com/NotRexed/Hookshot/issues/648)) |
| Ultrawide and 4:3 | ⚠️ The 3D keeps its shape, but the HUD is stretched and the picture is softer |
| NVIDIA cards | ✅ Works. The most tested |
| AMD cards | ⚠️ Some tested: reports from the RX 6600, RX 7800 XT and RX 7900 XTX. Known issue: the game loses the GPU in the Deku Tree on the RX 6600 ([#708](https://github.com/NotRexed/Hookshot/issues/708), [#614](https://github.com/NotRexed/Hookshot/issues/614)) |
| Intel cards | ❓ Few reports, many of them black screens or crashes ([#17](https://github.com/NotRexed/Hookshot/issues/17)) |
| Cards without mesh shaders (such as AMD RX 5000 and older) | ❌ Not supported: parts of the picture are missing. Since v2.0.6 a fallback runs the mesh programs as compute shaders on cards with buffer device addresses ([#640](https://github.com/NotRexed/Hookshot/issues/640)); it leaves out the mesh draws that take their count from a buffer |
| Linux | ✅ Works with the AppImage. Tested on several distributions (CachyOS, Fedora, Bazzite, SteamOS) and cards, with open problems on some AMD cards ([#604](https://github.com/NotRexed/Hookshot/issues/604), [#528](https://github.com/NotRexed/Hookshot/issues/528)) |

## Install and play

The game does not come ready-made: Hookshot Builder builds it on your PC from your copy. You do this once. After that
you start the game from its launcher, which also updates it.

### What you need

- **Your copy of the game at the supported version:** program `0400486024DEC000`, update v131072, build `e0f3ccfa`.
  Other versions do not work. A `.nup` file, or the `romfs` and `exefs` folders.
- 64-bit Windows 10 or 11, or 64-bit Linux with glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora, Arch,
  SteamOS ...) and a CPU with AVX2. The Linux AppImage needs AVX2; on Windows, and in a build from source, a CPU
  without it gets a game built for what it has, a little slower.
- A graphics card with Vulkan 1.3 and mesh shaders, with an up-to-date driver: NVIDIA GeForce RTX 20 series or newer,
  AMD Radeon RX 6000 series or newer, or Intel Arc. On Linux, Mesa or NVIDIA's driver. Cards without mesh shaders are
  not supported.
- 8 GB of video memory (VRAM) or more is recommended. Cards with less can use the in-game menu's lower Quality presets.
- 16 GB of RAM or more. A run uses about 10 GB.
- About 60 GB free while building, on an SSD if you can: a hard drive makes the build much slower. You can build on one
  anyway; the builder says so first.
- An internet connection for the download. Nothing else to install: the builder brings its own compiler and tools.

### On Windows

1. Open the [latest release](https://github.com/NotRexed/Hookshot/releases/latest) and download `HookshotBuilder.exe`.
2. Run it. It downloads the rest of Hookshot Builder (about 500 MB), checks it, and opens it.
3. Pick your game. The builder looks for `.nup` files in your Downloads, Desktop and Documents folders and lists them
   under **Found on this PC**. Not listed? Press **Browse…**, drop the file on the window, or choose
   **romfs and exefs folders**. The builder checks that it is the right version.
4. Under **Install the game to**, keep the folder (`Games\Hookshot` in your user folder) or press **Change**. Choose
   the **Game language**.
5. Press **Build the game**. It takes about 10 to 45 minutes, depending on your CPU and memory. You can use your PC in
   the meantime.
6. When it says **Your game is ready**, press **Play**. The game's launcher opens first: choose the profile and launch
   options there, then press **Play**. The **Desktop shortcut** and **Start menu entry** boxes are ticked: untick them if
   you don't want them.

Next time, start the game from the Hookshot shortcut, or `hookshot.exe` in the game folder. That is the launcher: it
has **Play**, and **Update** when a newer release is out.

The first start prepares the game's shaders, which takes a while. Later starts are faster.

### On Linux

1. Open the [latest release](https://github.com/NotRexed/Hookshot/releases/latest) and download
   `Hookshot-<version>-x86_64.AppImage`. It is Hookshot Builder with everything it needs inside.
2. Allow it to run as a program. In most file managers: right-click it, open **Properties**, and turn on
   "Allow executing file as program" (the words differ between desktops). Or in a terminal:
   `chmod +x Hookshot-*-x86_64.AppImage`.
3. Double-click it. The Hookshot Builder window opens.
4. Under **Game files**, choose **.nup file** or **romfs and exefs**, then press **Browse** to pick them. The builder
   checks the version.
5. Under **Save the game to**, keep `~/Games/Hookshot` or choose another folder. Choose a **Language**.
6. Press **Build**. It takes about 10 to 45 minutes, depending on your CPU and memory.
7. When it says "Your game is ready", press **Play**. The game's launcher opens first: choose the profile and launch
   options there, then press **Play**.

The game is also added to your applications menu as "Hookshot". That entry, and `hookshot` in the game folder, open the
launcher: **Play**, and **Update** when a newer release is out. Keep the AppImage where it is: the launcher uses it to
update.
**Launch the game in gamescope** is a Linux-only choice in the game's launcher: with it ticked, **Play** starts the game
inside gamescope, as Steam Deck game mode does. **Options…** sets the size, refresh rate, fullscreen, HDR, cursor grab,
real-time scheduling, the `gamemoderun` wrapper, and any extra arguments (for example
`-W 3840 -H 2160 -r 164 -f --hdr-enabled --force-grab-cursor --rt`). A game already started from a gamescope session is
started directly, without a second compositor. It is off by default, and the choice is not shown on Windows. The settings
are saved in `builder.ini`; `HOOKSHOT_GAMESCOPE=0` or `=1` overrides the on/off choice for one start.

**Close the builder after the game starts** is beside it: untick it to leave the builder window open when the game
launches, instead of closing it.


The same AppImage also builds and starts the game without opening the window, for a machine you reach over SSH or for a
script: see [Linux, the AppImage from the command line](#build-from-source).

For fast GPU memory imports on Linux, the game user needs read/write access to `/dev/udmabuf` (the kernel's
`udmabuf` module). Without it, Hookshot falls back to host-pointer imports, which can be much slower on AMD/RADV
(NVIDIA's driver refuses these dma-bufs, so on NVIDIA the game uses host-pointer imports, which are not slow there);
permission errors produce a warning on stderr and in `hookshot_log.txt`. With an NVIDIA card on NVIDIA's own driver
you do not need this access and can ignore that warning: the check runs at the start, before the game knows which
GPU it has. Otherwise, check `ls -l /dev/udmabuf` and `id`.
An administrator can add you to the device's owning group, then you must log out and back in. Only use that group
if access to its other devices is intended: `kvm` also grants virtualization access. For an active local desktop
session using systemd-logind, prefer a device-specific rule in `/etc/udev/rules.d/70-hookshot-udmabuf.rules`:
`SUBSYSTEM=="misc", KERNEL=="udmabuf", TAG+="uaccess"` (before `73-seat-late.rules`). This grants access to the active
local user, not SSH or service users. For those, an administrator can instead create a dedicated `udmabuf` group,
add only the game user, and use `SUBSYSTEM=="misc", KERNEL=="udmabuf", GROUP="udmabuf", MODE="0660"` in that file.
Reload with `sudo udevadm control --reload-rules` and apply with
`sudo udevadm trigger --action=add /sys/class/misc/udmabuf`; restart the session/service after a group change.
If the device is absent, the distribution must provide/load the module first (`sudo modprobe udmabuf`).
Do not run the game as root or make the device world-writable.

### Updating

When a new release is out, the launcher says "Update available". Press **Update**: in `hookshot.exe` on Windows, or in
`hookshot` (or the AppImage) on Linux. It downloads only the parts that changed and builds the game again in the same
folder. The folder keeps the game files and the builder keeps its object cache, so an update reuses what did not change
instead of compiling everything again. Your saves and settings are kept. What changed is on the
[Releases page](https://github.com/NotRexed/Hookshot/releases).

<details>
<summary>What an update keeps and replaces, and updating from the command line</summary>

To be safe, copy your saves somewhere first: the `user` folder in the game folder on Windows, or
`~/.local/share/hookshot` on Linux.

| Kept | Replaced |
|---|---|
| Your saves (`user`, or `~/.local/share/hookshot` on Linux) | The game in `bin` and its files |
| `hookshot_options.ini`, the in-game menu's choices and your controls | `image.bin` and the shader list |
| `hookshot_env.txt`, `language.txt` | |
| `mods` | |

The game files the first build read are kept in the game folder, so an update does not need your `.nup` again. The
exefs is in `.hookshot_exefs` and the romfs is the folder's `romfs`.

From the Flutter builder, an update runs the Rust engine's `build.all` with those two folders. A folder built before
the builder kept them has no copy to update from: it shows an error asking for the game files once, and building into
the same folder again fixes it. Folders built with NVIDIA DLSS update through the same Rust path on Linux (the tick
also fetches the SDK again, or reuses its files). The release AppImage's `--update <game folder>` also still takes its
old path until the packaging port lands. From the command line, the same update is a `build.all` request with `exefs` = `<folder>/.hookshot_exefs`,
`romfs` = `<folder>/romfs`, and `out` = the folder.

</details>

### Profiles, and saves from emulators

Hookshot Builder's launcher (the window **Play** opens for a built game folder) has a **Profile** choice above
**Play**, so that several people can play on one PC, each with saves of their own. **Play** starts the game with the
chosen profile. **Manage** makes a profile (it starts at an empty file select), renames one, imports a save into it,
exports its save, opens its save folder, or removes it. Removing a profile does not delete its saves: they are moved to
`save-backups` in the saves folder.

**Import save** copies a save from an emulator, or from a save folder of your own, into the chosen profile. The
launcher lists the saves it finds in the emulators' usual folders and asks whose save it is when a folder holds more
than one user's. Only the save's files are copied, and never while a game or emulator runs. **Export save** copies the
profile's save into a new folder you choose; it never writes into an emulator and replaces nothing.

Profile list, add, rename, remove and export are Rust, and so are the save scan and **Import save** (`saves.list` and
`saves.import`). `scripts/oot_saves.py` remains only for the command-line modes the app does not use: a human-readable
list, export or import by emulator name, `to-lichii`, and `to-hookshot`.
[Using a save from an emulator](docs/saves.md) covers both paths.

The default profile, first on the list and marked "(default)", has the saves from before profiles; it is called
"Player" until you rename it. The game plays it when it is started without the launcher, so it cannot be removed. A game
folder built before profiles shows no **Profile** choice: build it again (**Rebuild**, or **Switch version** to
another release) to get it.

The list is `profiles.txt` in the saves folder (the `user` folder in the game folder on Windows,
`~/.local/share/hookshot` on Linux), one `<id> <name>` a line; a profile's saves are `save/<id>`. The game itself plays
the default profile when `HOOKSHOT_PROFILE` names one that is not on the list.

### Latest development build

Want a fix before it is in a release? The
[latest development build](https://github.com/NotRexed/Hookshot/releases/tag/nightly) is made from `main` after every
change, with the same files as a release. It is untested and may break your saves or settings: copy them first (see
above).

- **Windows:** download `HookshotBuilder.exe` from it and start it from a terminal as `HookshotBuilder.exe --tag nightly`
  (without `--tag nightly` it installs the latest release instead).
- **Linux:** download `Hookshot-<version>-x86_64.AppImage` from it and use it as above.

To go back, install the [latest release](https://github.com/NotRexed/Hookshot/releases/latest) the usual way.

### Where the files go

- The build reads your game and never changes it. It writes to the game folder you choose and to the builder's own work
  folder: `%LOCALAPPDATA%\HookshotBuilder` on Windows, `~/.cache/hookshot-builder` on Linux.
- The romfs is hard-linked instead of copied where the disk allows it. The folder also keeps `.hookshot_exefs`, the
  game's code as read, and its `romfs`, so an update can build again without your `.nup`.
- The Rust builder keeps its work folder: the lifted C, the generated runtime and the object cache, several GB. Later
  builds and updates compile only what changed. The folder is not deleted when the game is ready; delete it to reclaim
  the space. The old Python path deletes it unless `HOOKSHOT_KEEP_BUILD_FILES=1` is set; the old Windows builder's
  **Free up space** button removes it.
- Saves: the `user` folder in the game folder on Windows, `~/.local/share/hookshot` on Linux (a `user` folder from an
  earlier Linux build is moved there once). `--user <folder>` puts them somewhere else ([command line
  arguments](#running-the-game-from-the-command-line)).
  To carry on with a save from an emulator (Lichii, Eglantine, Orchid, ...) or NX2navi, or to take yours there, see
  [using a save from an emulator](docs/saves.md).
- To uninstall, delete the game folder, the builder's folder and the shortcuts.

### If something goes wrong

| Problem | What to do |
|---|---|
| Where is the log? | `hookshot_log.txt` in the game folder holds the last run. The four runs before are `hookshot_log.1.txt` (the last) to `.4.txt`. Started from a terminal, the game prints there instead. Every 600 frames it has a `vk: status:` line: the video memory in use of what the driver allows, and DLSS's details while it draws; `vk: over the driver's video-memory allowance` says when the game passed it |
| A place has a wrong texture, or a floor or wall is missing | Look for `hookshot_damaged_files.txt` in the game folder. The build writes it when your game dump is incomplete (about 23.3 GB, a complete one is about 23.8 GB): it lists each damaged game file and the file put in its place. Dump the game again and build from the new dump; the file is then removed |
| Linux Builder Play closed before the game log appeared | After the game exits, check `hookshot_startup.log` in the game folder for up to 64 KiB of startup stderr and its exit status. |
| "Hookshot supports build e0f3ccfa" or "This version isn't supported" | Check that you chose the right game with its update installed |
| `hookshot_damaged_files.txt` is beside the game | The dump had damaged romfs files. The builder used stand-ins where possible; dump the game again and rebuild from a complete `.nup` for the original textures. If any damaged file had no replacement, the build stops before compiling. |
| The game opens and closes, or the log says "device lost" | Update your graphics driver. On AMD cards there are known crashes ([#708](https://github.com/NotRexed/Hookshot/issues/708), [#614](https://github.com/NotRexed/Hookshot/issues/614)) |
| Parts of the picture are missing, or a message says your card has no mesh shader support | The card or its driver has no mesh shaders. Update the driver; older cards are not supported ([#640](https://github.com/NotRexed/Hookshot/issues/640)) |
| AMD: blocky textures, solid rectangles in the sky or unusually dark courtyard surfaces | Try the [RGBA8 texture workaround](#amd-texture-workaround). It uses more graphics memory |
| Linux: the game does not start or the picture is broken | The game needs a Vulkan 1.3 driver with mesh shaders. Check with `vulkaninfo \| grep mesh`: it should list `VK_EXT_mesh_shader`. Update Mesa or the NVIDIA driver |
| Steam Deck: the game freezes and closes in Kokiri Forest or the Great Deku Tree (`gfx_0.0.0 timeout` in `journalctl -k`) | `RX_VK_MESH_NOCULL=2c56dc1006942f74` in `hookshot_env.txt` gets past it with every model drawn (the culling pass that runs for seconds is turned off). `RX_VK_SKIP_CODE=2c56dc1006942f74` also gets past it, but the models those draws make go missing (see the [development guide](docs/development.md#switches-for-testing-and-debugging), [#817](https://github.com/NotRexed/Hookshot/issues/817)) |
| "Your antivirus removed part of the build tools" | A false alarm on unsigned programs. Restore the files from Windows Security's Protection history, or add `%LOCALAPPDATA%\HookshotBuilder` to its exclusions, then press **Repair** |
| Stutter the first time you see something | Normal for now: shaders are being prepared. It gets better when you pass through the same place again |
| Windows, NVIDIA: stutter every start in places you have seen, after the shaders were prepared | The driver's shader cache is too small to keep them all (the first start builds about 2 GB of pipelines into it). In the NVIDIA Control Panel, Manage 3D settings, set **Shader Cache Size** to **Unlimited** (or 10 GB). On Linux the game raises the driver's limit itself |
| The game is in the wrong language | Change the code in `language.txt` in the game folder. The file lists them all, and an update keeps the one you set there |

If the problem continues, [report it](https://github.com/NotRexed/Hookshot/issues/new/choose) with the version, where
you were in the game, your graphics card, `hookshot_log.txt` and a screenshot or video. Never send game files.

#### Low FPS or stutter

Walk through the same spot twice after shader preparation finishes. Note whether the slowdown clears on the
second pass or lasts until you restart the game. For a slowdown after an update, compare the same spot and
settings on both versions if you still have the earlier build. Turn off **Frame generation** for both runs so the
FPS reflects game frames, and keep the resolution, upscaler and frame rate cap the same.

Attach the log from the slow run and, if available, the one from the earlier build. Say where the FPS dropped and
keep the lines around the drop that show `presents`, `GPU time`, `waits for shaders or pipelines`, `VRAM:` and
`memory heaps`. Compare total and per-heap memory use with the driver's allowances. The texture cache has a
separate budget on the `VRAM:` line.

If the GPU time stays high, a separate run with `RX_VK_PASS_TIMING=1` in `hookshot_env.txt` reports which GPU
passes take the time every 600 presents. Keep the unprofiled log for the FPS comparison since the timing itself
adds work. Remove the switch after collecting the log. The [development guide](docs/development.md#time-and-profiles)
has more switches.

#### AMD texture workaround

For these AMD texture artifacts, try keeping decoded ASTC textures as RGBA8 instead of re-encoding them to BC3
([#57](https://github.com/NotRexed/Hookshot/issues/57), [#1006](https://github.com/NotRexed/Hookshot/issues/1006)).
Turn on **ASTC as RGBA8** in the menu's Debug tab, under Developer, and restart the game. (`RX_VK_ASTC_RGBA8=1` in
`hookshot_env.txt` does the same, as earlier versions of this readme said; while that line is there the row shows its
value and cannot be changed, so remove the line to choose in the menu.)

This is a workaround for some artifacts, not a general lighting fix. It can substantially increase graphics-memory
use and cause stalls when the card runs out of headroom. To undo it, turn the row off (and remove any `RX_VK_ASTC_RGBA8` line from `hookshot_env.txt`) and restart the game.

## In-game menu

By default, press <kbd>Insert</kbd> or <kbd>F1</kbd> in game. On a controller, press Home or Select and Start together; on
a DualShock 4 or DualSense, clicking the touchpad does the same as <kbd>F1</kbd>.
The same keys close it, and so does <kbd>Esc</kbd>. <kbd>F1</kbd> always works; **Controls → Menu shortcut** can leave
<kbd>Insert</kbd> free for another overlay. The menu has five tabs, Graphics, Audio, Gameplay, Controls and Debug:
<kbd>Q</kbd> and <kbd>E</kbd> (or <kbd>Tab</kbd> and <kbd>Shift</kbd>+<kbd>Tab</kbd>) switch between them, LB and RB on
a controller (or the triggers). <kbd>Up</kbd> and <kbd>Down</kbd> choose a row, <kbd>Left</kbd> and <kbd>Right</kbd>
change it (<kbd>Enter</kbd> changes it too); beside the rows, the help says what the row is and, on a line of its own,
what the value chosen does. On a controller the D-pad chooses and changes, A changes too and opens a page (Motion,
Controls, the Teleport list, the inventory), and B goes back or closes the menu: A and B are the buttons your layout puts the Switch's A and B
on (with the Nintendo layout, A is the right face button). The mouse is free while the menu is open: click a tab, click a row's name to choose it (its
value stays), click the left or right half of a value to lower or raise it, drag a slider, turn the wheel to scroll the rows (it never changes a value), and right-click to go back. The
button prompts at the bottom show the keys or the controller's buttons, whichever you used last. Your choices are saved
in `hookshot_options.ini` in the game folder.

The menu, the FPS counter and the start-up screen are drawn in the style of the game's own menus. They use the game's
own menu font, read into memory from your game files (the romfs folder the game runs from) the first time one of them is
drawn. It is never copied anywhere, and Hookshot does not include it. Without it, or if it has no Latin letters, they use
Libertinus, which Hookshot includes.

The rows marked "after a restart" below are read when the game starts. While one is changed, it is tagged "After
restart" and the help on the Graphics tab lists it; closing the menu then asks whether to restart now. **Later** keeps
the change for the next start (and is not asked again until you change one of those rows); **Restart now** saves your
choices and starts the game again, so progress since your last save is lost.

A row for something this PC cannot use is not shown: NVIDIA Reflex and AMD Anti-Lag 2 without a GPU and driver that
have them, the DLSS model where DLSS does not run. HDR paper white and
HDR peak brightness show only while HDR is on.

| Tab | Option | Choices | Default |
|---|---|---|---|
| Graphics | Quality preset | Custom, Minimum (4GB), Low (6GB), Balanced (8GB), High (12GB), Ultra (16GB+) | Custom |
| | Fullscreen | Off, On (the same as <kbd>F11</kbd>) | Off |
| | GPU device | Auto, or each graphics card by name (after a restart) | Auto |
| | Frame generation | Off, Auto, 2x to 8x | Off |
| | Frame pacing | Off, On | Off |
| | V-Sync | Auto, On, Off | Auto |
| | HDR | Off, Expanded, Native (the game's) | Off |
| | HDR paper white | 80 to 400 nits (while HDR is on) | 200 nits |
| | HDR peak brightness | 400 to 4000 nits (while HDR is on) | 1000 nits |
| | Game frame rate cap | 60, 90, 120, 144, 165, 240 FPS, Uncapped (after a restart) | 60 FPS |
| | NVIDIA Reflex | Off, On, On + Boost (shown only on NVIDIA GPUs with `VK_NV_low_latency2`) | Off |
| | AMD Anti-Lag 2 | Off, On (shown only on AMD GPUs with `VK_AMD_anti_lag`) | Off |
| | Output resolution | 1920x1080, 2048x1152, 2560x1440, 3840x2160, 5120x1440, 5120x2880 (after a restart) | 2048x1152 |
| | Upscaler | Game's own, NVIDIA DLSS, NVIDIA DLAA (only in builds with NVIDIA's DLSS SDK, below; DLSS and DLAA on RTX GPUs), AMD FSR 3.1 (every build, on GPUs it runs on, below), AMD FSR 4, AMD FSR 4 Native AA (only in builds with FSR 4, below), Off (no upscaling) | Game's own |
| | DLSS model | Transformer (K), Transformer 2 (M), Transformer 2 (L), NVIDIA default (shown where DLSS runs; used by DLSS and DLAA) | Transformer (K) |
| | Render resolution | Auto (the game's own dynamic resolution), 100%, 75%, 67%, 50% (each with its size). With DLSS, FSR 3.1 or FSR 4: Auto, Quality (67%), Balanced (58%), Performance (50%), Ultra Performance (33%). With DLAA or FSR 4 Native AA: Native. With Off: Auto, 100%, 75%, 67%, 50%, 33% | Auto |
| | Window scaling filter | Off, Catmull-Rom, Lanczos | Off |
| | Sharpness | 0 to 100%, in steps of 10 | 0% |
| | Anisotropic filtering | Game's, 2x, 4x, 8x, 16x | 16x |
| | Profile | Docked, Handheld (after a restart) | Docked |
| | Ultrawide | Off (16:9), Monitor's shape: wider than 16:9, or narrower down to 5:4 (after a restart) | Off (16:9) |
| Audio | Volume | 0 to 100%, in steps of 5 | 100% |
| | Mute in background | Off, On | Off |
| | Audio output | Auto, Stereo, 5.1 (after a restart) | Auto |
| Gameplay | FPS counter | Off, FPS, Frame time, Graph, Breakdown | Off |
| | FPS counter position | Top right, Top left, Bottom right, Bottom left | Top right |
| | Menu size | Small, Medium, Large | Medium |
| | Game speed | Per frame, Real time | Real time |
| | Text speed | Normal, 2x, 4x, Instant | Normal |
| | Text size | Original, 110%, 125%, 150%, 175% (dialogue, Navi and signs) | Original |
| | Microphone | Off, Default, or each microphone by name | Default |
| Controls | Controller in background | Off, On | Off |
| | Mouse camera | As the stick, Native | As the stick |
| | Disable camera centering | Off, On | Off |
| | Mouse sensitivity | 0.10 to 5.00 | 1.00 |
| | Invert mouse vertical | Off, On | Off |
| | Invert camera X | Off, On | Off |
| | Invert camera Y | Off, On | Off |
| | Camera speed | 50% to 400%, in steps of 25 | 100% |
| | Stick dead zone | Default (24%), 5%, 10%, 15%, 20%, 30% | Default (24%) |
| | Rumble strength | Off, 25% to 150%, in steps of 25 | 100% |
| | Layout preset | Nintendo (Def), Xbox / PC, Swap Face, Custom | Nintendo (Def) |
| | Motion | Opens the Motion page: Motion controls (Off, On; On), Motion sensitivity X (10% to 600%) and Y (10% to 300%; 100%), Invert motion X and Y (Off, On; Off), Motion deadzone (Low, Standard, High; Standard) | |
| | Controls | Opens the Controls page | |
| | Menu shortcut | Insert or F1, F1 only | Insert or F1 |
| Debug | Debug tools | Off, On (the tools below it while on) | Off |
| | Developer: Descriptor reuse | Off, On (after a restart) | On |
| | Developer: Sampler cache | Off, On (after a restart) | Off |
| | Developer: Robust buffers | Auto, Off, On (after a restart) | Auto |
| | Developer: ASTC as RGBA8 | Off, On (after a restart) | Off |

- **Quality preset** sets Render resolution, Window scaling filter, Sharpness, Frame pacing, Game speed, Profile
  and the video memory limits for a card with that much memory. It leaves the Upscaler and Frame generation as
  you chose them: with DLSS, FSR 3.1 or FSR 4, the preset's resolution becomes their nearest mode. Changing most other
  rows yourself sets it back to Custom.
- **Menu size** is how large the menu, the FPS counter and the start-up screen are drawn: Medium unless you choose Small
  (a desk) or Large (a TV across the room). It stays the same whether you use a controller or the keyboard and mouse.
- **GPU device** matters on a PC with more than one graphics card (a laptop's integrated and dedicated GPUs, or a
  second card for Lossless Scaling). Auto prefers a dedicated one, and only cards with Vulkan 1.3 are listed.
  `hookshot_log.txt` lists every card as `vk: device <n>`; `RX_VK_DEVICE=<n>` chooses one for a single run and wins
  over the menu.
- **Frame generation** makes extra frames between the game's own. It adds a little input delay, and fast motion can
  show artifacts. The renderer's own interpolation makes the frames; at 2x, AMD FSR 3.1's frame interpolation can make
  it instead with `RX_VK_FSR_FG=1` (below: it moves the HUD with the scene, so it is not the default). **Frame pacing** shows frames at an even interval: less stutter, up to one frame of delay. It paces
  the frames that frame generation shows, so it has no effect while Frame generation is Off.
- **Game frame rate cap** limits the frames the game renders. With a cap of 60 and Frame generation at 2x, the output
  can reach 120 FPS. Choose **Auto** in Frame generation to target the display's refresh rate instead of a fixed multiplier.
  A cap **past 60 is experimental**: the game keeps real time, but some cutscenes and bosses still count frames and can
  misbehave (Gohma running into the wall, the intro's narration running ahead: #1742). 60 is the safe choice.
  **Uncapped** renders as fast as the PC can (the game still keeps real time). A display that waits for its refresh still
  holds the frames to its refresh rate: V-Sync on, or the driver's FIFO mode. `HOS_FPS_MAX=<61-1000>` sets a cap for one
  run.
- **The scaling rows**, in the picture's order: **Output resolution** is the size of the game's whole frame, HUD
  included. The **Upscaler** fills it from the 3D: the game's own network, NVIDIA DLSS or DLAA (the 3D at full size,
  edges smoothed), AMD FSR 3.1, or AMD FSR 4 or its Native AA (below). DLSS and DLAA look softer than the game's own: set
  **Sharpness** to around 80% with them. **Off** is no upscaler at all: the game's own network is left out too, and the 3D
  is stretched to the output with a plain linear filter (the fastest choice, below). **DLSS model**
  chooses their network (below). **Render
  resolution** is the 3D's share of the output. The **Window scaling filter** then scales the frame to your window,
  and **Sharpness** sharpens it, by the picture's pixels: when the window is much larger than the picture (the Handheld
  Profile's 1536x864 on a 4K screen), a Sharpness kept from an older version is stronger than it was, so a lower one may
  suit.
- **Output resolution:** its rows below show the sizes for the output you chose. Docked only. On a 1920x1080 display,
  the 1920x1080 option keeps HUD and text sharp when the game fills the screen by mapping them pixel for pixel. At
  3840x2160 the game's render targets get a bigger GPU pool, so a full-size 3D fits. On a 5120x1440 (32:9) ultrawide
  monitor, 5120x1440 gives a native frame with a sharp HUD. Set **Ultrawide** to Monitor's shape with it: it widens the
  cameras to the frame's shape and keeps the HUD and movies in the middle 16:9; without it, the game's 16:9 picture is
  stretched across the 32:9 output, sharp but wide. For a size the menu does not offer (3440x1440, 2560x1080, ...), set
  `output_res=<width>x<height>` in `hookshot_options.ini`: each side a multiple of 8, at least 640x360, at most 5120x2880's
  pixels, and between 5:4 and 48:9 in shape; anything else uses the game's 2048x1152. The menu then offers it as its own
  choice. With **Ultrawide** on, a picture wider than 16:9 is kept to 3840x2160's pixels, so above that a wide size is not a
  native frame. A custom output wider than 32:9 also sets the camera shape, even when the OS reports a single
  16:9 primary monitor. For three 16:9 monitors, use `5760x1080` or `7680x1440` and a window or logical display
  that spans all three. The 7680x1440 output uses a smaller 48:9 internal picture to stay within the pixel budget.
  For one run, set `HOS_OUTPUT_RES` to a size, such as `HOS_OUTPUT_RES=3840x2160`.
- **Window scaling filter** matters only when your window's size differs from the Output resolution: for the sharpest
  picture, set the Output resolution to your screen's, and the row shows "Not needed (window = output)". It and
  **Sharpness** also work when Frame generation is Off.
- **Anisotropic filtering** keeps textures sharp on surfaces seen at a slant: the paths and floors ahead of Link. The game
  asks for 1x or 2x, which at a PC's resolutions smears the ground a few metres off; 16x costs little on most cards, and
  Game's is the console's own. `RX_VK_ANISO=<n>` sets it for one run (0: the game's).
- **Upscaler Off** (`dlss=6` in `hookshot_options.ini`) is for slow GPUs and for measuring: no DLSS, no FSR, and not the
  game's own network either, which otherwise runs on every frame (at the game's usual 3/4 share it scales the 3D up and
  smooths its edges; at 100% a pass of the same kind still smooths them). In its place the 3D is stretched to the output
  with a linear filter, and the HUD is drawn over it at the output's size as always. Nothing is kept from frame to frame, so
  there is no ghosting, but no anti-aliasing either: edges are jagged, and below 100% the picture is soft. The Render
  resolution then sets the 3D's size alone (Auto, 100%, 75%, 67%, 50%, 33%): lower is faster. RX_VK_DLSS, RX_VK_FSR4 and
  RX_VK_FSR set for a run go over it, as over the row's other choices.
- **Texture detail with DLSS, FSR 3.1 or FSR 4:** DLSS and DLAA sample the world's textures as NVIDIA's DLSS guide asks:
  at the output's size and one mip step finer (the game's own bias + log2(3D / output) - 1), which DLSS resolves over its
  frames, so they keep the detail of the full-size picture instead of the 3D's smaller size. FSR 3.1 and FSR 4 sample them
  one finer mip step for each halving of the 3D below 75% (the game already does so for its own 75%).
  `RX_VK_UPSCALE_LOD_BIAS=0` turns it off; `RX_VK_DLSS_LOD_OFFSET=<steps>` sets DLSS's extra step for a run (-1 by default,
  0 for none, down to -3).
- **NVIDIA Reflex** Off is the game as without it. `RX_VK_REFLEX_LOG=1` logs the driver's latency timings every 300
  frames while it is on.
- **AMD Anti-Lag 2** Off is the game as without it. Like Reflex, it is not used while Frame generation is on.
- **Profile** runs the game as the console docked (at the Output resolution) or Handheld (1536x864): lighter on the GPU.
- **V-Sync:** Auto is mailbox where the driver has it (no tearing, no wait for the display); otherwise On, or on Windows
  Off. On waits for each refresh (FIFO): no tearing, at most the display's rate. Off shows each frame at once and can
  tear (where the driver cannot, it is Auto). It changes at once. `RX_VK_FIFO=1` (On) or `RX_VK_IMMEDIATE=1` (Off) chooses
  for one run.
- **Robust buffers** (Debug tab, Developer): a read past a buffer's end is dropped instead of stopping the GPU. Auto is
  on, on every card; Off can be faster on some NVIDIA cards.
- **ASTC as RGBA8** (Debug tab, Developer): the AMD texture workaround below.
- **Game speed:** with Per frame, the game runs slower whenever it is below 60 fps. Real time keeps it at normal speed
  down to 15 fps.
- **FPS counter** shows in a corner (**FPS counter position**; at the top right while the menu is open), at one of
  four levels. **FPS** is the number alone (an options file's old On); with frame generation on, it also shows the
  game's own rate. **Frame time** adds the game's frame time and its 1% low (the rate of the slowest 1% of frames over
  the last 10 seconds). From this level on, the number and the frame time are green at the Game frame rate cap's rate,
  amber under it and red under two thirds of it. (The cap's rate is the display's refresh instead with V-Sync On when
  that is slower; with Uncapped and V-Sync not On there is no line, and the colours are against 60.) **Graph** adds
  the frame times of the last 4 seconds (each point the slowest frame of its span, so no stutter is missed), with a
  dashed line at the Game frame rate cap's frame time and a red mark over frames that took more than twice it; its
  scale follows the usual frame, so a spike is drawn at the top rather than flattening the rest. **Breakdown** adds
  where the frame's time went on the game's presenting thread, as a bar and in numbers: the game's own code (Game),
  recording and submitting the GPU's work (Renderer), waiting for the GPU (GPU wait), and waiting for the frame's turn
  (Pacing: the frame rate cap, vsync, Reflex); then the GPU's own time per frame, and the shaders built and draws left
  out in the last 4 seconds (red marks on the graph where they happened, the usual cause of a stutter).
- **Volume** scales the game's whole sound, on top of the game's own sound settings and Windows' volume. Its steps
  follow a curve so they sound about even: 50% is a quarter of the level (-12 dB). **Mute in background** silences the
  game while another window has the focus; an overlay such as Steam's or Discord's does not count as another window
  while the game was in front.
- **Audio output** is the speakers the game mixes its sound for. Auto is 5.1 when your default output device has 5.1
  or more speakers, else stereo; Stereo or 5.1 chooses one whatever the device has (5.1 on stereo speakers is mixed
  down). For one run, `HOS_AUDIO_CHANNELS=2` or `6` chooses stereo or 5.1 and wins over the menu.
- **Microphone** is what the ocarina hears when the game's own microphone option is on (an instrument, or humming).
  Default follows Windows' default recording device; the list shows each microphone by name, and a change applies at
  once. A chosen microphone that is unplugged stays chosen (shown as not connected): Default stands in until it is
  back. On Linux (SDL2) two microphones with the same name cannot be told apart: the first is used. Windows must allow desktop apps to use the microphone
  (Settings, Privacy & security, Microphone). `HOS_MIC_LOG=1` logs what the game reads, a line a second.
- **Controller in background:** On keeps the controller working while another window has the focus. The keyboard and
  mouse still need the game's window.
- **Mouse camera:** Native turns the camera with the mouse one to one, as in a PC game. As the stick uses the mouse in
  place of the right stick. The <kbd>-</kbd> and <kbd>=</kbd> keys change the sensitivity in game too.
  When another camera takes over, Native leaves it alone; on returning, mouse look starts from the game's current
  pitch and yaw rather than restoring the old view.
- **Disable camera centering:** On stops the game gradually turning the camera toward your movement, so it does not fight
  native mouse look. Off keeps the game's standard behavior. Changes apply immediately with either Mouse camera mode.
  Manual controls, explicit recentering and scripted camera headings still work; automatic pitch adjustment is unchanged.
- **Camera speed** is how fast the right stick turns the camera, in percent of the game's own speed. The camera keys
  follow it, and so does the mouse with Mouse camera on As the stick; Native mouse look has its own sensitivity. The
  game's own camera speed option still picks the speed this multiplies.
- **Stick dead zone:** how far a stick moves before it counts, on each axis. Default is XInput's (about 24%); lower suits
  a precise stick, higher one that drifts. The game keeps a dead zone of its own after it.
- **Rumble strength** scales the controller's rumble: 100% is the game's own, Off turns it off. The game's HD rumble
  goes to a controller's heavy and light motors by its frequency.
- **Motion controls** (Controls tab, Motion page): the controller's gyroscope aims where the game uses motion, as on the Switch. It works with
  Switch Pro controllers and pads in Switch mode, DualSense and DualShock 4 (through SDL). Its row
  says whether the connected pad has a gyroscope; Off leaves the game seeing a pad lying still. **Motion sensitivity X/Y**
  scale the left-right and up-down aim (100% is the console's), **Invert motion X/Y** turn either direction around. The game gets the motion
  state as a Switch (and Eden) builds it, so how you hold the pad when motion starts does not matter. Motion follows the pad used
  last. The gyroscope's drift is learned while the pad rests on a table, so set it down for a second if the aim
  creeps. `HOS_MOTION_DEBUG=1` (in `hookshot_env.txt`) logs what the game gets twice a second.
- **Layout preset:** Nintendo has A on the right face button, Xbox / PC and Swap Face put A on the bottom one. Changing
  a button yourself sets it to Custom.

### Controls

Use a controller, or the keyboard and mouse. On the keyboard, <kbd>W</kbd> <kbd>A</kbd> <kbd>S</kbd> <kbd>D</kbd> move
and the arrow keys (or <kbd>I</kbd> <kbd>J</kbd> <kbd>K</kbd> <kbd>L</kbd>) turn the camera.
With the default keyboard bindings, press <kbd>E</kbd> to select **New Game** on the title screen.

**Mouse in the game's menus (Windows):** the title screen and its file list, the pause menu, the Key Items, Threads of
Time and Map pages, the options and the save dialogs free the mouse while they are shown. Point at an entry to select
it, left-click to choose it (A) and right-click to go back (B); the tabs, the L and R signs and the Back prompt can be
clicked too. The wheel turns the pause menu's pages and scrolls lists; on the map it changes floors and zooms. Drag
with the left button to pan the map and the tapestry or turn a stone on Key Items. The cursor stays hidden while you
play on a controller or the keyboard, until the mouse moves.

**Rumble:** a Nintendo Switch Pro Controller or Joy-Con plays the game's HD rumble as the Switch does, each side and
both frequency bands as the game asks, with SDL3 (the Windows game brings it; on Linux the system's SDL3, where it has
one). Other controllers, Switch 2 controllers, a pair of Joy-Cons used as one controller, and SDL2's own library get
the game's rumble on their two motors. The menu's Rumble strength scales it too. `HOS_HD_RUMBLE=0` sends the Switch
controllers the motor rumble too. On Linux, SDL talks to the Switch controller itself through its hidraw device, which
needs read and write access for your user; Steam's udev rules (the `steam-devices` package on most distributions) give
it. Without them the kernel's hid-nintendo driver runs the controller, and it gets the motor rumble. HD rumble goes to
anything SDL reports as a Switch Pro Controller or Joy-Con, and that includes third-party controllers in Switch mode
(8BitDo, GameSir and others; SDL gives them Nintendo's ids). Some of them may turn it into weak rumble or none: set
`HOS_HD_RUMBLE=0` to give them the plain motor rumble.

To change which gamepad button or key presses a Switch button, or which keys move and turn the camera, open the
Controls tab's Controls page. Choose a row and a column (Gamepad or Keyboard), press <kbd>Enter</kbd> (or the bottom
face button), then the new button or key. <kbd>Esc</kbd> cancels, <kbd>Delete</kbd> clears it, and Reset to defaults
puts them all back. A gamepad button taken from another Switch button swaps with it. The page saves them in
`hookshot_options.ini`.

<details>
<summary>Writing the controls in hookshot_options.ini yourself</summary>

```ini
pad_a=0
key_a=69,13
```

`pad_<button>` is a gamepad input by position: 0 bottom face button, 1 right, 2 left, 3 top, 4 Select or Back, 5 Home,
6 Start, 7 and 8 the stick presses, 9 and 10 the shoulder buttons, 11 to 14 the D-pad (up, down, left, right), 15 and
16 the triggers, -1 none. `key_<button>` is one or two
[Windows key codes](https://learn.microsoft.com/windows/win32/inputdev/virtual-key-codes) in decimal (69 is
<kbd>E</kbd>, 13 <kbd>Enter</kbd>), 0 none; the mouse's buttons are 1 (left), 2 (right), 4 (middle), 5 and 6 (side
buttons), and count while the mouse is captured. The buttons are `a` `b` `x` `y` `l` `r` `zl` `zr` `plus` `minus` `ls`
`rs` `left` `up` `right` `down`, and for keys only the left stick's and the camera's directions: `move_up`
`move_down` `move_left` `move_right` `cam_up` `cam_down` `cam_left` `cam_right`. A button the file leaves out keeps
its default.

</details>

## Mods

Put mods in a `mods` folder in the game folder, beside `hookshot.exe` (or point `HOS_MODS` at another folder), in the
layout emulators use: `mods/<name>/romfs/...` holds files that replace or add to the game's own. Deeper layouts such as
`mods/contents/<title id>/<name>/romfs` work too. A folder whose name starts with `-` or `_` is off. When two mods
have the same file, the one whose folder name sorts last wins, so name them `01-...`, `02-...` to set the order.

Only RomFS mods are loaded. `exefs` patches (`.pchtxt`, `.ips`) are left out: Hookshot compiles the game's code ahead
of time, so patching its instructions would need a rebuild. The log (`hookshot_log.txt`) names each mod and the files
read from it.

A mod file larger than the game's own does not load unless the resource size table allows for it:
`System/Resource/ResourceSizeTable.Product.111.Nin_Ounce_NVN2.rsizetable.zs` gives each file the memory the game sets
aside for it (by the CRC32 of its path without `.zs`; for a texture, its decompressed size + 280). Past that, the game
goes on without the mod's file, or a loading screen waits for good. A mod that makes files bigger (HD
textures, longer parameter files) ships the table with its entries grown. Since only one mod's table is used, mods that
each grow files need one table with all their entries, in the mod that sorts last. Hookshot merges the button prompts'
entries into that table (each file gets the larger of the two), so your controller's icons stay in texts; the log says
`(with the button prompts' sizes merged in)` after the table's line. A table it cannot read that way (not the game's
format, or a zstd frame that does not give its size) is used as it is, with the Switch's icons.

## Build from source

For developers, and for players who prefer the command line. The build has three layers: the Flutter app, the Rust
engine, and the Python path kept for the stages in [What still needs Python](#what-still-needs-python-or-is-not-ported).
[CONTRIBUTING.md](CONTRIBUTING.md) has the wider rules.

### The Rust engine

Build the engine and run its tests:

```sh
cd builder/rust
cargo build --release --workspace
cargo test --workspace
```

`target/release/hookshot_engine` (or `hookshot_engine.exe`) takes one JSON request on stdin and prints one JSON answer.
`--stream` is the builder's form: it prints `@@stage`, `@@progress`, `@@done` and `@@error` lines instead.

```sh
printf '%s' '{"command":"build.check","exefs":"/path/to/exefs","work":"/tmp/hookshot-work"}' \
  | builder/rust/target/release/hookshot_engine
```

A `.nup` build is two requests. `build.prepare` reads, checks and extracts it:

```json
{"command":"build.prepare","nup":"/path/to/game.nup","out":"/path/to/game-folder","work":"/path/to/build-files"}
```

It writes `work/exefs` and `out/romfs`. `build.all` takes those two folders:

```json
{"command":"build.all","exefs":"/path/to/build-files/exefs","romfs":"/path/to/game-folder/romfs","out":"/path/to/game-folder","work":"/path/to/build-files","language":"en-US","prepare":true}
```

An unpacked pair goes straight to `build.all` with your own `exefs` and `romfs`. `build.shaders` writes the shader list
alone, `build.vk` builds the Vulkan backend alone, and `build.host_link` links the Linux host. The commands are
dispatched in `builder/rust/src/lib.rs`; `builder/rust/tests/cli_stream.rs` covers the `--stream` protocol.

### The Flutter app

The app is in `builder/flutter`. Build the engine first, then install Flutter with the Linux desktop toolchain (or
Visual Studio's C++ tools on Windows), and run:

```sh
cd builder/flutter
flutter pub get
flutter run -d linux      # or: flutter run -d windows
flutter test
flutter analyze
```

The app looks for the engine beside itself, in `builder/rust/target/release`, or where `HOOKSHOT_ENGINE_CLI` and
`HOOKSHOT_ENGINE_LIB` point. `builder/rust/build.sh` builds the release engine and copies it, the CLI and the ZBIC
library into an existing Flutter bundle; run it after `flutter build linux --release` if you want the engine beside the
app instead of in `target/release`.

The app's arguments match the old builder:

| Command | What it does |
|---|---|
| `hookshot_builder` | the builder window |
| `hookshot_builder --play [folder]` | start the game built last, or the one in that folder, without the window |
| `hookshot_builder --launch <folder>` | that folder's launcher: Play, and Update when a newer release is out |
| `hookshot_builder --update <folder> [--tag <tag>]` | download the release and build that folder again |

With `flutter run`, put the app's arguments after `--`.

### Work and output folders

- `work` holds the build's scratch: the lifted C in `work/sa/gen`, the image in `work/sa/image`, generated runtime and
  assets under `work/rt64` and `work/build/gen`, objects in `work/obj`, and caches in `work/objcache` and
  `work/build/objcache`. The Flutter app uses `~/.cache/hookshot-builder` on Linux and
  `%LOCALAPPDATA%\HookshotBuilder` on Windows. A CLI request names it, or the engine uses `work/` in the checkout;
  `OOT_WORK` moves that default.
- `out` is the game folder: `image.bin`, `romfs`, `bin/hookshot` (or `bin\hookshot.exe`), the shader list, the language
  file and `.hookshot_exefs`. The Flutter app suggests `~/Games/Hookshot`.
- `build.prepare` writes `work/exefs` and `out/romfs`; `build.all` reads an exefs/romfs pair and writes the folder.
- The dump is read in place and never modified. Name the exefs and romfs in each request; `OOT_WORK` moves the default
  `work/` folder.

### Linux

From the AppImage, nothing else is needed: it carries the compiler, libraries and a prebuilt Vulkan renderer. The
AppImage is still built by `scripts/linux/appimage.sh` from the dependencies `scripts/linux/deps.sh` puts in
`build-linux/deps`, and it still launches the older Python/Tk builder, so an AppImage build uses the Python path until
that packaging port lands. A build from the Flutter app in this tree is the Rust path described above: no Python runs.

From a checkout, install `clang` and `lld` (LLVM 23 or newer reads the play profile; an older one builds without it),
FFmpeg's development files with dav1d, and SDL3 (SDL2 works too). Vulkan, SPIR-V and VMA headers come from the distro,
from `HOOKSHOT_*_HEADERS`, or from a CPM cache via `RX_CPM`. Then build the engine and the Flutter app above. On the
machine these measurements were made on, the SPIR-V and VMA headers are in `build-linux/deps/cpm`; set
`RX_CPM=/home/daze/GitHub/Hookshot/build-linux/deps/cpm` before the build (or point it at your checkout's copy).

`build.all` builds `hookshot_vk.so` with `build.vk` when neither an explicit path nor `HOOKSHOT_VK_PREBUILT` is given.
It discovers the same 195 sources as `nx2/build.py`, compiles 212 jobs, caches objects by content and checks the
exported symbols. The AppImage's `AppRun` sets `HOOKSHOT_VK_PREBUILT` to its own `hookshot_vk.so`, so an AppImage build
reuses it. If the Vulkan, SPIR-V or VMA headers are missing, the engine says so and still writes the folder without a
renderer; install the headers, set `HOOKSHOT_*_HEADERS`, or use a CPM cache via `RX_CPM`. The Windows Vulkan DLL link is
not ported. With `dlss` the engine fetches NVIDIA's SDK when it is missing, compiles `hookshot_vk.so` with NGX, links
NVIDIA's static library and puts DLSS's runtime in `<out>/ngx`; `HOOKSHOT_NGX_SDK` alone turns that on too, as in
`build.py`. AMD's FSR 4 headers in `third_party/fidelityfx` (MIT) are compiled in whenever they are present
(`HOOKSHOT_FSR=0` leaves the stubs).

A folder built from a source checkout has no launcher: `hookshot` there is the game.

<details>
<summary>Linux, the AppImage from the command line</summary>

```sh
./Hookshot-<version>-x86_64.AppImage --cli --nup ~/your-game.nup --out ~/Games/Hookshot   # the Python path
./Hookshot-<version>-x86_64.AppImage --play                    # the game the builder built last, without the window
./Hookshot-<version>-x86_64.AppImage --play ~/Games/Hookshot   # the game in that folder
./Hookshot-<version>-x86_64.AppImage --update ~/Games/Hookshot # the new release, and that folder built again
```

`HOOKSHOT_JOBS=<n>` (or `--jobs <n>` with `--cli`) limits how many files compile at once.
`HOOKSHOT_MEM_LIMIT_GB=<n>` lets the compilers take that many GB together, whatever is free now. Linux stops a compiler
that does not fit; it is then compiled again alone at the end.

In a game folder, `hookshot` is the launcher and the game itself is `bin/hookshot`. The builder's settings are in
`~/.config/hookshot/builder.ini`.

</details>

### Windows

The Rust engine now links `hookshot.exe` for Windows with llvm-mingw. It uses the same host source set, flags and
libraries as `nx2/build.py`, content-keyed objects, UTF-8 response files, the PE manifest, map and stack size, and
stages the SDL, wgpu-native and FFmpeg DLLs beside the executable. A cross-build on Linux produced a real PE32+
executable: 192 host objects compiled, 0 failed, link ok, 0 undefined symbols; `llvm-objdump` shows PE32+, an 8 MB
stack reserve and a UTF-8 manifest. That cross-build used scratch lifted code and FFmpeg stubs, not a real game link.

No Windows boot has been made yet. A real game executable still needs the game's lifted objects and a Windows FFmpeg
build, and `hookshot_vk.dll` (the Windows Vulkan backend) is not built by the Rust engine; only the Linux
`hookshot_vk.so` is. A DLSS build on Windows therefore stops with a typed error in the Flutter app rather than falling
back to `make_game.py`; the Python path below still builds one. A Windows VM is available for the boot test, but it is
deliberately not started while builds run; the verification steps are prepared. Until that run, a Windows game build
and gameplay are not verified. For a complete Windows build today, use the Python path below.

You need [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) (the `ucrt-x86_64` build), unpacked as `llvm-mingw` in the
clone or with `HOOKSHOT_MINGW` set to its folder. Without it the Python path uses the clang from
[emsdk](https://github.com/emscripten-core/emsdk) at `C:\emsdk` with Visual Studio's C++ tools and the Windows SDK
(`HOOKSHOT_MSVC=1` forces that route).

<details>
<summary>The Python path: Windows, DLSS and packaging</summary>

`nx2/make_game.py` is still the complete builder for a Windows game, a build with NVIDIA's DLSS SDK, the full prompt
assets including textures, and the release packages. Run it with `python -I` from the repository root. Python 3 from
python.org (not the Microsoft Store build) needs `pip install -r requirements.txt`.

```powershell
python -I nx2/make_game.py --nup "C:\path\your-game.nup" --out "C:\Games\Hookshot" --language en-US
python -I nx2/make_game.py --romfs "C:\path\romfs" --exefs "C:\path\exefs" --out "C:\Games\Hookshot" --language en-US
```

Add `--check` with your `.nup` or `--exefs` to only check the game's version. It builds nothing.
Add `--log "C:\path\build.log"` to also save the build's output to a file (UTF-8, replaced if it exists); it works
with `--check` and `--update` too, and on Linux.
`HOOKSHOT_JOBS=<n>` (or `--jobs <n>`) limits how many files compile at once.
`HOOKSHOT_MEM_LIMIT_GB=<n>` lets the compilers take that many GB together, whatever is free now; up to 4 always run.

A clone still needs what the builder's `tools` folder carries. Run `HookshotBuilder.exe` once: the folder is
`%LOCALAPPDATA%\HookshotBuilder\versions\<version>\HookshotBuilder\tools`. The builder sets the variables itself;
from the command line, set them before you run `make_game.py`:

- `tools\cpm`: the Vulkan headers and FFmpeg. Set `RX_CPM` to this folder. Without it (and without system
  Vulkan / SPIR-V / VMA headers or `HOOKSHOT_*_HEADERS`), the standalone build stops instead of shipping a
  game with no renderer.
- `tools\wgpu-native`: set `WGPU_NATIVE` to this folder.
- `tools\third_party`: the prebuilt files a clone lacks (`a64emit`, `lld-link`, `zbic` and SDL2). Copy its `.dll`,
  `.exe`, `.lib` and `.sha1` files to the same places under `third_party\`.
- `tools\work`: copy it to `work\`. It holds the hot-function list, the link order, the profile and the icon;
  without them the game builds but runs slower.

</details>

<details>
<summary>The Windows game, built on Linux</summary>

Unpack llvm-mingw's Linux build (`ucrt-ubuntu-22.04-x86_64`), then run `make_game.py` with `RX_TARGET=windows` and
`HOOKSHOT_MINGW` set to its folder. The objects go to `~/.cache/hookshot-builder-windows`, apart from the Linux
game's. The game folder runs under Proton or on Windows. This is the Python path; the Rust engine can cross-link the
Windows host now, but it does not build `hookshot_vk.dll` yet and still needs the game's lifted objects and a Windows
FFmpeg build for a real link.

</details>

### Running the game from the command line

The built game is `bin\hookshot.exe` on Windows and `bin/hookshot` in a packaged Linux game folder. A build from a
source checkout has `hookshot.exe` or `hookshot` in its build folder, with no launcher beside it. The `hookshot.exe`
or `hookshot` launcher in the game folder does not pass these arguments to the game, so start the game directly to
use them.

| Argument | What it does |
|---|---|
| `image.bin` | The game image to run. Without it the game uses the `image.bin`, `romfs` and `user` in its folder |
| `--romfs <folder>` | The extracted RomFS folder |
| `--user <folder>` | Where the game keeps per-user data: saves, caches and temp files (and `mouse_sens.txt` when `HOS_MOUSE_SENS` is set; the menu keeps the sensitivity otherwise). Default: `user` in the game folder on Windows, `~/.local/share/hookshot` on Linux; with an explicit image path, `user` in the current folder |
| `--trace` | Log every Horizon OS call (the same as `HOS_TRACE=1`) |
| `--prepare-shaders` | Prepare the shaders as the first start does, then exit without starting the game (the builder runs it before the folder is ready; the first start does it when a build skipped it) |

For example, keep saves somewhere else:

```powershell
cd "C:\Games\Hookshot"
.\bin\hookshot.exe --user "D:\Hookshot data"
```

```sh
cd ~/Games/Hookshot
./bin/hookshot --user "$HOME/hookshot-data"
```

`hookshot_env.txt` and the other test switches are documented below.

<details>
<summary>AMD FSR 3.1 (every build)</summary>

**AMD FSR 3.1** in the Graphics tab's **Upscaler** row (`dlss=3` in `hookshot_options.ini`; `RX_VK_FSR=1` for one run,
`RX_VK_FSR=0` never) upscales in place of the game's own network, and with **Frame generation** at 2x its frame
interpolation can make the frame in between with `RX_VK_FSR_FG=1` (opt-in: the game draws its HUD into the same picture
and FSR 3.1 has no HUD-less colour to keep it still, so the HUD smears; by default the renderer's own makes it). Both are AMD's FidelityFX
SDK 1.1.4 shaders (`third_party/fidelityfx-sdk`, MIT), compiled into the renderer: no AMD runtime or DLL is needed, and
the row offers FSR 3.1 on any Vulkan 1.3 GPU with push descriptors, extended storage image formats and storage writes
without a format (`vk: fsr3: ... can run on this GPU` in the log). Its inputs are the scene's colour, its depth, motion
from the camera, and the same camera jitter DLSS uses (`RX_VK_FSR_JITTER=0` turns it off); things that move by
themselves can trail for now.

</details>

<details>
<summary>NVIDIA DLSS (optional, NVIDIA's SDK under NVIDIA's license)</summary>

A game built with NVIDIA's DLSS SDK offers **NVIDIA DLSS** and **NVIDIA DLAA** in the Graphics tab's **Upscaler** row on
NVIDIA RTX cards (`dlss=` in `hookshot_options.ini`, `RX_VK_DLSS=0/1/2` for one run): DLSS Super Resolution in place of
the game's own upscaler, with camera jitter (**NVIDIA Reflex** needs no SDK and is in
every build). NVIDIA's files are NVIDIA's, under its own license, the
[NVIDIA RTX SDKs license](https://github.com/NVIDIA/DLSS/blob/9981c6350dc58c0ce21f024fd65e31e851c31689/LICENSE.txt).
Hookshot never includes or redistributes them: each build downloads them for itself, and only after you accept that
license. Never share a game folder built with them, and do not post builds made with them. On Linux the Rust engine does
the whole stage: `build.all` with `dlss` fetches the SDK (every file checked against its SHA-256, reused when right),
compiles and links `hookshot_vk.so` with NGX, and puts DLSS's runtime in `<game folder>/ngx`. `build.ngx_fetch` fetches
the same files on its own.

- **Windows, the Hookshot Builder:** tick **NVIDIA DLSS (downloads NVIDIA's SDK under NVIDIA's license)** before you
  build. It is off by default and shows NVIDIA's license when you tick it. The tick downloads the SDK into
  `%LOCALAPPDATA%\HookshotBuilder\ngx-sdk` and puts DLSS's runtime, `nvngx_dlss.dll`, in the game folder's `ngx` folder.
  The Rust engine does not link the Windows Vulkan DLL yet, so the tick stops with an error in the Flutter app instead
  of falling back to `make_game.py`; the Python path below builds a Windows DLSS game.
- **Windows, the command line:** `python -I scripts/ngx_sdk.py --platform windows <sdk folder>` asks you to accept
  NVIDIA's license (or pass `--accept-nvidia-license`) and fetches the headers, `nvsdk_ngx_s.lib` and `nvngx_dlss.dll`,
  each checked against its SHA-256. Then build with `HOOKSHOT_NGX_SDK=<sdk folder>`: `make_game.py` copies
  `nvngx_dlss.dll` into `<game folder>\ngx`. NGX is also pointed at `%LOCALAPPDATA%\Hookshot\ngx`, and at
  `RX_VK_DLSS_DIR` when it is set, and uses the newest `nvngx_dlss.dll` it finds across them (to try a given one, make
  sure no other folder has a newer one). Never copy NGX's core, `_nvngx.dll`, beside the game: it comes with NVIDIA's
  driver.
- **Linux, the Hookshot Builder (the AppImage):** the same tick as on Windows, and nothing else to do: no variable is
  passed to the AppImage and no file is copied by hand. The build downloads the SDK into
  `$XDG_DATA_HOME/hookshot/builder/ngx-sdk` (`~/.local/share/hookshot/builder/ngx-sdk`), builds the game with it and
  puts DLSS's runtime, `libnvidia-ngx-dlss.so.<version>`, in the game folder's `ngx` folder.
- **Linux, a build from source:** `scripts/linux/ngx-sdk.sh <sdk folder>` (it runs `scripts/ngx_sdk.py --platform
  linux`) asks you to accept NVIDIA's license and fetches `libnvsdk_ngx.a` for the build and
  `libnvidia-ngx-dlss.so.<version>`, DLSS's runtime. Then run the build with `HOOKSHOT_NGX_SDK=<sdk folder>` set:
  `build.all` (or `build.vk`) compiles the backend with NGX and `build.all` puts the runtime in the game folder's `ngx`
  folder; `make_game.py` does the same. NGX's core comes with NVIDIA's Linux driver.
- **Linux, a game folder with other than plain English letters in its path:** NVIDIA's NGX on Linux loads DLSS's
  runtime only from a folder whose whole path is ASCII. A game in `~/Games/Zelda` needs nothing more. A game in a
  folder such as `~/Games/Zelda-Café` (the `é` is not ASCII) cannot use the runtime in its own `ngx` folder, and the
  log and the Insert menu say so: copy `libnvidia-ngx-dlss.so.*` from there into `$XDG_DATA_HOME/hookshot/ngx`
  (`~/.local/share/hookshot/ngx`), which the game also looks in, or into any other folder with an ASCII path and name
  that folder in `RX_VK_DLSS_DIR`.

The **DLSS model** row chooses DLSS's network, by NVIDIA's preset letter, for every mode: **Transformer (K)**, the
transformer model and the one tested most here (the default); **Transformer 2 (M)**, NVIDIA's second-generation
transformer as it picks it for Performance; **Transformer 2 (L)**, the same generation as tuned for Ultra Performance;
**NVIDIA default**, which lets DLSS's runtime pick for each mode, so it can change with a runtime update. The runtime
the SDK comes with (310.9.16) has all of them. A change applies at once, without a restart (`dlss_model=` in
`hookshot_options.ini`, 0 to 3 in that order). `RX_VK_DLSS_PRESET=<letter>` (or `0` for NVIDIA's default) chooses
the preset for one run and wins over the row; the log's `vk: NVIDIA DLSS: ... preset <letter>` line says which one was
asked for, and with `RX_VK_DLSS_LOG=1` NGX's own `Using App hint Preset <letter>` lines say which it took.

**8 GB cards:** DLAA draws the 3D at full size, and DLSS and DLAA add images of their own beside the game's. In busy
areas that can pass what the driver allows the game, and the game then stutters: Quality or Balanced is safer there.
The log's `vk: status:` line every 600 frames shows the video memory in use of the allowance, with DLSS's details
(its images' size, the jitter and the actor motion) appended while it draws, and a `vk: over the driver's
video-memory allowance` line says when the game passed it.

The game's log says whether DLSS started, and why not when it did not (`vk: NVIDIA DLSS:` lines in `hookshot_log.txt`).
NGX starts on demand: when the Insert menu first opens (to know whether the Upscaler can offer DLSS) or at the first
gameplay upscale with DLSS chosen, so on the title screen the log has no NGX lines yet.
With `RX_VK_DLSS_LOG=1` NGX's own lines go there too (`ngx:`). Some of them look like errors but are not:
`failed to load NGXCore: 126` (twice) is NVIDIA's library looking for `_nvngx.dll` and `nvngx.dll` beside the game, which
are not there on purpose, before it loads the driver's; `Feature ... failed to load from cache` is NGX finding no model
of its own downloads (`C:\ProgramData\NVIDIA\NGX\models`) before it uses the game folder's `nvngx_dlss.dll`.

</details>

<details>
<summary>AMD FSR 4 (optional, AMD's DLLs not included)</summary>

A game built with FSR 4 offers **AMD FSR 4** and **AMD FSR 4 Native AA** in the Graphics tab's **Upscaler** row: AMD's
machine-learned temporal upscaler in place of the game's own, on the same inputs as DLSS (the scene's colour and depth, the
camera's and the characters' motion, and the camera jitter). Native AA draws the 3D at full size and smooths its edges.
With HDR on **Native**, it takes the game's HDR colour as linear light, so highlights above the paper white are kept
(`RX_VK_FSR4_HDR=0`: cut at the paper white, as before).
Unlike FSR 3.1 (above), it runs AMD's own DLLs, so it needs them beside the game. Choose it in the menu (`dlss=4` or `5`
in `hookshot_options.ini`, `RX_VK_FSR4=1` or `2` for one run, `RX_VK_FSR4=0` to leave it out).

The Flutter builder's form does not offer FSR 4 yet, and the Rust Vulkan build has no FSR 4 flags; use the Python path
below for it. The no-FSR-4 Rust build compiles the same stubs as a build without AMD's SDK.

- **Windows, the Hookshot Builder:** tick **AMD FSR 4** before you build. It shows AMD's license first and stays ticked
  only if you accept; the build then fetches AMD's DLLs for you (below), into the builder's `ffx-sdk` folder and the
  game folder.
- **The command line:** `make_game.py --fsr` (or `HOOKSHOT_FSR=1` for `nx2/build.py`). Rebuilding a folder built with
  it (`--update`) keeps it; `--no-fsr` takes it out.
- **AMD's files:** Hookshot includes only AMD's MIT-licensed API headers (`third_party/fidelityfx`). Put
  `amd_fidelityfx_loader_vk.dll` and `amd_fidelityfx_upscaler_vk.dll` from the
  [FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) 2.4 release (`Kits/FidelityFX/signedbin`)
  beside the game's exe (`bin\hookshot.exe`) or in the game folder. (`RX_VK_FSR_DIR` names another folder, for testing:
  set it in the environment or a `HOS_ENV_FILE` file; `hookshot_env.txt` takes no paths.) Without them the Upscaler row
  says so and offers the rest. On Linux, a Vulkan FSR 4 library (`amd_fidelityfx_upscaler_vk.so`, such as fsr4-vk's)
  takes their place.
- **Or fetched for you (Windows):** `make_game.py --fsr --fetch-fsr-runtime --accept-amd-license` downloads those two
  DLLs and AMD's license from AMD's FidelityFX SDK repository at the v2.4.0 tag, checks each against its SHA-256, and
  puts them in the game folder (`HOOKSHOT_FFX_SDK=<folder>` keeps them between builds, default `workfx-sdk`).
  `python -I scripts/ffx_sdk.py --accept-amd-license --game <game folder>` does the same for a game built already.
  Without `--accept-amd-license` nothing is downloaded (the script asks when run by hand), as with NVIDIA's license for
  DLSS. AMD DLLs already in the game folder that are not v2.4.0's (a newer SDK put there by hand) are left alone, and
  the build says so. They are AMD's, under
  [AMD's FSR SDK license](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/blob/7539ae8b6568ad29ed78a753404351daf3b5cca7/Kits/FidelityFX/docs/license.md)
  (`LICENSE-AMD-FSR-SDK.md` beside them). On Linux the flag does nothing but say so.

The log's `vk: AMD FSR 4:` lines say which of AMD's upscalers it found for the card and which it uses (the newest FSR 4;
never an older FSR in its place), and why not when it did not start. Tested on an RX 6700 XT (RDNA 2).

</details>

<details>
<summary>Switches for testing (hookshot_env.txt)</summary>

Switches to test (`RX_VK_NOOPT=1` and the others) go in `hookshot_env.txt` in the game folder, one `NAME=VALUE` a
line, on Windows and Linux. The running game reads it, not the builder. A test run can name another file with
`HOS_ENV_FILE=<path>`; the game then reads that one instead and leaves `hookshot_env.txt` alone.
`hookshot_env.txt` sets only the game's own switches (names that start with `RX_`, `HOS_`, `HOOKSHOT_`, `EDEN_`,
`NVN_` or `SDL_`, but not SDL's library names such as `SDL_VULKAN_LIBRARY`), and no value that is a path except
`RX_VK_DLSS_DIR`'s (a folder on this PC, not a network share), and no value at all for the switches that name a file
or folder (`HOS_AUDIO_DUMP`, `RX_VK_SHOTS` and the like) or press buttons on a timer (`HOS_INPUT_SCRIPT`,
`RX_VK_MENU_SCRIPT`); other lines are left out, and the log says so. Switches that need a path, or other variables, go
in a file named by `HOS_ENV_FILE`.
A switch set in the environment the game is started with (a shell's `HOS_FULLSCREEN=0 ./hookshot`, a launcher's own)
goes over the same name in the file, and the log says so; within the file, a later line replaces an earlier one. A
switch that stands in for a menu row (`RX_VK_PACE`, `RX_VK_UPSCALE`, `RX_VK_FRAMEGEN_MULT`, `HOS_REALTIME_STEP`,
`HOS_DYNRES`, `HOS_OUTPUT_RES`) applies to that run only: `hookshot_options.ini` keeps the value chosen in the menu.

To enable the debug tools, turn **Debug tools** on in the menu's Debug tab (or add `HOS_OOT_CHEATS=1` to
`hookshot_env.txt` in the game folder). The tools then show below it on the same tab. The tab's Developer section,
always shown, has the Descriptor reuse, Sampler cache, Robust buffers and ASTC as RGBA8 rows.

A save whose minimap stopped showing after a Teleport on an earlier build (the area left had hidden it, and the flag
stayed set in the save) is mended by the Teleport itself: teleport to an outdoor area, wait until you can move, then
save from the game's System menu. The minimap then stays after a restart.

`RX_VK_UI_GAME_FONT=0` draws the menu with the included font (Libertinus) even when the game's files have the game's own
menu font. `RX_VK_UI_PROMPTS=pad` (or `keys`) shows the controller's (or the keyboard's) prompts whatever was used last,
for screenshots.

For a first run on a 4 GB card such as a GTX 1650, `RX_VK_LOW_SPEC=1` starts with the Minimum (4GB) preset, with
frame generation and upscaling off. It applies only when there is no `hookshot_options.ini` yet; after that, change
the settings in the in-game menu.

`RX_VK_MOVIE_UPSCALE=linear` replaces the movies' neural upscale with the plain linear one. The default, `auto`, keeps
the networks and falls back only when a movie frame takes the GPU more than 40 ms. That is on NVIDIA cards: on other
cards, or where the GPU cannot multiply the networks' matrices itself, the movies always get the linear upscale. It does
not change the gameplay upscaler, and movies can look softer. `RX_VK_NETWORK_KEEP=all` takes precedence and keeps the
movie networks.

`RX_VK_SKIP_CODE=<prefix,...>` leaves out the draws of every program with a stage whose native code key starts with one
of the prefixes (the keys the log prints after "its code:"). On a Steam Deck, `RX_VK_SKIP_CODE=2c56dc1006942f74`
avoids a GPU hang in Kokiri Forest and the Great Deku Tree: the GPU runs for seconds in the programs that share that
task shader (a culling pass) until the kernel resets it ([#817](https://github.com/NotRexed/Hookshot/issues/817)).
It is a workaround, not a fix: those are task + mesh programs, so the geometry they draw goes with them, and key models
are missing while it is set. Off unless set.

The switches for scripted test runs, screenshots, frame dumps and profiles are in the
[development guide](docs/development.md#switches-for-testing-and-debugging).

`RX_VK_WB_MAX_KB=<KB>` stops the write-back of written GPU bindings larger than that. OoT writes tens of MB of culling
lists and per-frame buffers through the GPU that the CPU never reads, and copying them back sat on every submit's
memory sync: in Hyrule Field on a 10 GB RTX 3080, about 66 MB and 4.4 ms a submit, holding the scene at 45-48 fps.
`RX_VK_WB_MAX_KB=1024` still copies the small results the CPU does read (the exposure meter and the like) and took the
same scene to 59-60 fps. `0` or unset copies every written binding, as before.

</details>

## How it works

1. On the Rust path, the Flutter builder sends a build request to the engine: `build.prepare` for a `.nup`, then
   `build.all` with the exefs and romfs. The same engine prints the progress lines the builder shows.
2. The engine builds the image, lifts the game's ARM64 code to C (jump tables and a per-function cache), applies the
   OoT patches and after-sites, and compiles the C with clang.
3. It links `host/`, which plays the part of the console's system software, into `hookshot`/`hookshot.exe`; on Linux
   the engine also builds `hookshot_vk.so` with `build.vk`. `host/nvn_native.c` records the game's NVN2 graphics calls,
   and `gpu/vk/` replays them on Vulkan.
4. Switch 2 Ampere shaders are lowered to Maxwell (`lower_to_maxwell.h`) and translated to SPIR-V by Eden's shader
   recompiler. The engine lists them and runs the built game with `--prepare-shaders` before the folder is ready.
5. Python still builds the Windows Vulkan DLL (and so the Windows DLSS build) and the release packages. The Linux
   NGX/DLSS and FSR 4 backend, the Windows host link and the prompt font and layouts are Rust now; the prompt textures
   are not generated yet. Those are the open items above.

More detail on the architecture is in the [design notes](docs/design.md). The [development guide](docs/development.md)
still describes the older Python pipeline and the runtime's testing switches; the build commands in this README are the
current ones.

## Testing

The checks are Rust, Flutter and C++ only. The Python source-reading suite and `tests/run_all.py` were removed. Every
check runs without the game or a dump.

| Command | What it runs |
|---|---|
| `cargo test --workspace` in `builder/rust` | Engine unit tests and the `--stream` CLI tests. |
| `flutter test` and `flutter analyze` in `builder/flutter` | Builder state, routing, updater, profiles and UI tests. |
| The C and C++ fixtures under `tests/` and `gpu/vk/tests` | Game-free checks, compiled with the host compiler. The three SPIR-V fixtures under `tests/global_load_16`, `tests/native_suatom` and `tests/f2i_immediate` have their own `CMakeLists.txt`. |
| A run in the game on your hardware | Runtime and renderer changes: what you did, what you saw, and the numbers from `hookshot_log.txt`. |

At this tree that is 208 Rust tests, 4 CLI tests and 59 Flutter tests, with a clean `flutter analyze`.

CI runs `cargo test --workspace` on Linux and Windows, plus `flutter test`, `flutter analyze` and the three SPIR-V
fixtures on Linux. Add a Rust or Flutter test for a fix where the behavior can be tested without the game. Never add
game files or generated game data to the repository or to CI. [CONTRIBUTING.md](CONTRIBUTING.md#testing) has the wider
rules.

## Want to help?

Testing on other graphics cards and reporting the result already helps a lot, and so does playing further into the
game than anyone has yet.

Bigger ways to help:

- **Rust and Flutter work.** The engine and the builder app are this fork's build system;
  [Still to come](#still-to-come) lists what is left to port.
- **Linux testing.** The Linux build (`host/posix`) has been tested on several distributions and cards: reports from
  others still help.
- **Fixes in your own fork.** Fork the project, fix what you find and send the change back. Keep game files, keys and
  anything generated from your game out of it.

[CONTRIBUTING.md](CONTRIBUTING.md) has the setup, the test commands, the code conventions and what a good pull request
looks like.

## Credits

Hookshot exists thanks to the work of many people:

- **NotRexed**: Hookshot, BlueShell (Mario Kart World), Skybound (Tears of the Kingdom) and the a64-kit tools.
- **[Eden](https://git.eden-emu.dev/eden-emu/eden)** and **Eglantine**: the shader recompiler and Switch 2 shader work.
- **suyu**: the graphics decoders and the AArch64 translator.
- **[seedds/nxnative](https://github.com/seedds/nxnative)**: reference for how task and mesh shaders work.
- **[EvilBoyce / evilmfkr](https://github.com/evilmfkr/Hookshot)** (EvilOOT): the GPU culling that brings back Kokiri's foliage
  (trees, ferns, grass, falling leaves, fireflies), the ambient light probes, the pond's water, the game's own fog, the freeze
  after Load (Horizon's mutex handoff) and the exposure flicker fix.
- **[zortos293/Lichii](https://github.com/zortos293/Lichii)**: reference for Ampere shader lowering, and its compare-exchange retry fix.
- **[mstorsjo/llvm-mingw](https://github.com/mstorsjo/llvm-mingw)**: the bundled Windows compiler (no Visual Studio needed).
- **NVIDIA DLSS**: builds made with NVIDIA's DLSS SDK (optional, downloaded by each build, never bundled) use NVIDIA DLSS
  Super Resolution, NVIDIA's technology. NVIDIA and DLSS are trademarks of NVIDIA Corporation.
- **AMD FSR 3.1 and FSR 4**: every build has AMD FidelityFX Super Resolution 3.1, built from AMD's MIT-licensed FidelityFX
  SDK shaders; builds made with FSR 4 (optional; AMD's DLLs are never bundled) use AMD FidelityFX Super Resolution 4
  through AMD's MIT-licensed FSR API headers. Both are AMD's technology. AMD, Radeon and FidelityFX are trademarks of Advanced Micro
  Devices, Inc.
- **[kinnay/zbic](https://github.com/kinnay/zbic)**, **Opus** and **SDL**: decompression, audio and controllers.
- **[sostenesfreitas/ocarina-recomp](https://github.com/sostenesfreitas/ocarina-recomp)**: this README's layout, and its notes
  confirmed the mesh stage's control section.
- **[MythosOfs/Orchid](https://github.com/MythosOfs/Orchid)**: the fog and exposure workaround Ocarina of Time used before
  its fog and lighting were fixed.
- **Mesa (NAK)**: the reference for Ampere's instruction encoding.
- Everyone who sent fixes: SkippySteve (Linux and the AppImage, and many more), monsterlag, klimovich008, hckrman101 (NVIDIA
  DLSS, DLAA and Reflex, the exposure flicker and fog fixes, build speed, CI), madebyjordan (AMD FSR 4.1), N3oTraX (FSR 4 on
  Linux), shayderrr, gab9281, Wpaif, nicholasgower, ZurielAtr, Joey-Magnus, Zorkats,
  d3zessete, chrispository, vzeol, leonidasmark, Segmentree, ShyVortex, hiKareeem and i900gaming, among others.

## License

Hookshot is free software under the [GNU General Public License, version 3 or later](LICENSE) (GPL-3.0-or-later).
It includes code from Eden and suyu, which are under the GPL too. Third-party parts under other licenses that
work with the GPL (Sirit, the Vulkan Memory Allocator, SDL, Opus, Dear ImGui, zstd, the Libertinus fonts, zbic and the
a64-kit tools) keep their own notices in their folders.

Hookshot is an unofficial fan project, not affiliated with or endorsed by Nintendo. The Legend of Zelda and Ocarina of
Time are trademarks of Nintendo.
