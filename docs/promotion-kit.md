# Promotion kit

Ready-to-paste posts for the Rust/Flutter builder. The GitHub announcement is live and the upstream reference PR is
open:

- Announcement: https://github.com/dazemc/Hookshot-rs/discussions/8
- Upstream reference PR: https://github.com/NotRexed/Hookshot/pull/2951
- Benchmark: https://github.com/dazemc/Hookshot-rs/blob/main/docs/build-performance-report.md
- Good first issues: https://github.com/dazemc/Hookshot-rs/blob/main/docs/good-first-issues.md
- Current release: [v2.2.0-rs.10](https://github.com/dazemc/Hookshot-rs/releases/tag/v2.2.0-rs.10) (pre-release,
  2026-11-12)

**Rules for every post:** no game files, keys, ROM links or download instructions; say "bring your own supported
dump"; do not claim Nintendo affiliation; do not mention DLSS 5. Static recompilation is not emulation, but some
communities treat both as piracy-adjacent, so read each community's rules before posting. If you post after a newer
tag, replace `v2.2.0-rs.10` with it.

## One-line pitch

> Hookshot-rs is a Rust + Flutter builder for the Ocarina of Time (Switch 2) static recompilation: every stage the
> app runs is Rust, the app never invokes Python, and a first build is 15–30 minutes instead of 45–60.

## r/rust

**Title:** `Hookshot-rs: a Rust builder for a Switch 2 static recompilation, at parity with the Python pipeline`

**Body:**

```text
Hookshot recompiles The Legend of Zelda: Ocarina of Time (Switch 2) to native Windows/Linux code. The original
builder is Python. This fork replaces every stage the app runs with Rust: NUP extraction, the AArch64-to-C lift,
chunking and patches, compile planning, host and Vulkan links, packaging, button-prompt generation, shader census and
prepare.

The Flutter desktop app calls only the Rust engine. The remaining Python is release tooling, the legacy `nx2/` and
`builder/py` builders and the pipeline-state asset merge; none is on the app's path.

Build-time numbers on the reference PC (Ryzen 9 5950X, 32 threads, 15.5 GiB, f2fs RAID 0):
- first build: 15-30 min typical, 12-16 min on this PC (Python path: 36:17 measured, 45-60 min reported)
- cold compile: 13.8 min for 335 units (Python: 20.3 min)
- full lift: 57 s cold / 19.8 s cached (Python: 10 min 30 s)
- 25.6 GB .nup read: 64 s (Python: 94 s)
- forced prompt generation: 3.25 s -> 1.60 s

The interesting Rust bits: per-function lift caching, stat-keyed compile objects, memory-sized compile waves, a
vendored ASTC encoder without bindgen, BC4 prompt textures, and a game-free fixture suite that cuts C/C++ functions
out of the source and compiles them with stubs.

Code: https://github.com/dazemc/Hookshot-rs
Benchmark: https://github.com/dazemc/Hookshot-rs/blob/main/docs/build-performance-report.md
Upstream reference PR: https://github.com/NotRexed/Hookshot/pull/2951

No game files or keys are in the repo. You need your own supported dump to build the game; CI runs without one.
```

## r/FlutterDev

**Title:** `A Flutter desktop builder for a native game recompilation (BLoC, Fluent-style UI, Windows + Linux)`

**Body:**

```text
This is the builder for Hookshot-rs, a static recompilation of Ocarina of Time (Switch 2). The app is Flutter on
Windows and Linux, with flutter_bloc for the build/launcher/update state machines, a Fluent-style desktop UI, and a
Rust engine behind it.

The app never invokes Python. It streams one JSON command per stage from the Rust engine, shows read/lift/compile
progress, manages profiles and saves, pins releases, and offers a game launcher with gamescope options on Linux.

What might be useful to other Flutter desktop projects:
- one BLoC state machine for a long-running native process, with cancellation and progress;
- a desktop launcher/updater flow with version pinning and save import/export;
- widget tests for the build, launcher and update states;
- a Windows setup and Linux AppImage built without Python.

Code: https://github.com/dazemc/Hookshot-rs/tree/main/builder/flutter
Benchmark and architecture: https://github.com/dazemc/Hookshot-rs/blob/main/docs/build-performance-report.md
Good first issues (Flutter keyboard navigation is one): https://github.com/dazemc/Hookshot-rs/blob/main/docs/good-first-issues.md

No game files or keys are in the repo; the builder reads the user's own dump.
```

## r/Emulation

**Title:** `Hookshot-rs: Rust + Flutter builder for the Ocarina of Time (Switch 2) static recompilation`

**Body:**

```text
Hookshot statically recompiles Ocarina of Time (Switch 2) to native Windows/Linux code; it does not emulate the
console. This fork replaces the Python builder with a Rust engine and a Flutter desktop app, and ports the recent
renderer/runtime fixes.

Measured build time: 15-30 minutes typical, 12-16 minutes on the 32-core reference PC, against 36:17 measured and
45-60 minutes reported on the Python path. Cold compile 13.8 min vs 20.3 min; lift 57 s vs 10 min 30 s.

No game files, keys or download links are in the repo or this post. You need your own supported dump; the builder
reads it where it lies and writes only to your PC.

Code: https://github.com/dazemc/Hookshot-rs
Benchmark: https://github.com/dazemc/Hookshot-rs/blob/main/docs/build-performance-report.md
Upstream reference PR: https://github.com/NotRexed/Hookshot/pull/2951
```

## r/linux_gaming

**Title:** `Hookshot-rs: native Linux builder for the Ocarina of Time (Switch 2) recompilation, now Rust + Flutter`

**Body:**

```text
The Linux side used to be a Tk builder. This fork is one Flutter desktop app for Linux and Windows, with a Rust engine
for every build stage. The Linux release is an AppImage; the builder can also build from source with the distribution's
clang and Vulkan/SPIR-V/VMA headers.

Build time on the reference Linux PC: 15-30 min typical, 12-16 min on a 32-thread Ryzen 9 5950X, against 45-60 min
reported on the Python path. The lift is 57 s cold / 19.8 s cached; a 25.6 GB .nup reads in 64 s.

No game files or keys are in the repo. You need your own supported dump.

Code: https://github.com/dazemc/Hookshot-rs
Benchmark: https://github.com/dazemc/Hookshot-rs/blob/main/docs/build-performance-report.md
```

## r/vulkan

**Title:** `Vulkan backend for a Switch 2 recompilation: FSR 3.1 on the temporal chain, pipeline caches, and a game-free fixture suite`

**Body:**

```text
Hookshot-rs replays the game's NVN2 GPU work on Vulkan. Recent work includes FSR 3.1 as the temporal chain's third
engine (shared colour/depth/motion with DLSS and FSR 4), a GPU hang watch that exits with the checkpoint report,
texture-shadow staging in cached RAM, pipeline-cache persistence without full-size heap copies, and a 5120x2160
widescreen path.

The renderer is C++ with SPIR-V shaders; the builder is Rust. The tests cut functions out of the renderer source and
compile them with stubs, so CI needs no game or GPU.

Code: https://github.com/dazemc/Hookshot-rs/tree/main/gpu/vk
Performance ledger: https://github.com/dazemc/Hookshot-rs/blob/main/docs/upstream-performance-log.md
Upstream reference PR: https://github.com/NotRexed/Hookshot/pull/2951

No game files or keys are in the repo.
```

## Hacker News (Show HN)

**Title:** `Show HN: Hookshot-rs – a Rust and Flutter builder for a Switch 2 game recompilation`

**Body:**

```text
Hookshot statically recompiles Ocarina of Time (Switch 2) to native Windows/Linux code. The original builder is
Python; this fork replaces every stage the app runs with Rust and adds a Flutter desktop app.

The Rust engine does NUP extraction, the AArch64-to-C lift with per-function caching, chunking and patches, compile
planning, host/Vulkan links, packaging, prompt generation, shader census and prepare. The Flutter app never invokes
Python. A first build is 15-30 minutes typical (12-16 on the 32-core reference PC), against 36:17 measured and 45-60
minutes reported on the Python path; cold compile is 13.8 min vs 20.3 min.

No game files, keys or download links are in the repo. You need your own supported dump.

https://github.com/dazemc/Hookshot-rs
https://github.com/dazemc/Hookshot-rs/blob/main/docs/build-performance-report.md
```

## Discord

**Upstream Hookshot Discord, #hookshot-discussion:**

```text
I opened a reference PR with the whole Rust/Flutter builder state, including the measured build times:
https://github.com/NotRexed/Hookshot/pull/2951

Short version: every stage the Flutter app runs is Rust and the app never invokes Python. First build 15-30 min typical
(12-16 on a 32-thread PC), cold compile 13.8 min vs 20.3 min, lift 57 s vs 10 min 30 s. Benchmark:
https://github.com/dazemc/Hookshot-rs/blob/main/docs/build-performance-report.md

It is a snapshot, not a request for a 776-file review; close it if it is not useful. The fork is at
https://github.com/dazemc/Hookshot-rs and the good first issues are here:
https://github.com/dazemc/Hookshot-rs/blob/main/docs/good-first-issues.md
```

**Rust Discord, #showcase (or the project-showcase channel):**

```text
Hookshot-rs: a Rust builder for a Switch 2 static recompilation, at parity with the Python pipeline. Every stage the
Flutter app runs is Rust: NUP, lift, compile planning, host/Vulkan links, packaging, prompts, shader prepare. Cold
compile 13.8 min for 335 units, full lift 57 s cold / 19.8 s cached, first build 15-30 min.
https://github.com/dazemc/Hookshot-rs
```

**Flutter Discord, #showcase:**

```text
A Flutter desktop builder for a native game recompilation: flutter_bloc state machines for a long-running Rust build,
profiles/saves, release pinning, updater, and a game launcher. Windows and Linux, no Python in the app.
https://github.com/dazemc/Hookshot-rs/tree/main/builder/flutter
```

## What each platform needs to post directly

| Platform | What is missing here | What would let an agent post |
|---|---|---|
| GitHub | nothing (already posted) | `gh` auth |
| Reddit | account, OAuth app/token, subreddit posting permission | `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_USERNAME`, `REDDIT_PASSWORD`, or a user-provided token |
| Hacker News | account login cookie or API credentials; HN has no official write API | a user-posted link; automation is not recommended |
| Discord | bot token or webhook for each server/channel, plus membership | `DISCORD_WEBHOOK_<NAME>` or a bot token with message permission |
| Matrix | homeserver, access token, room IDs | `MATRIX_HOMESERVER`, `MATRIX_ACCESS_TOKEN`, room IDs |

Do not put any of these in the repository. Set them in the environment for one run, or post manually from the drafts
above.
