# Build speed: where the time goes

A profile of the builder on one PC, made to choose the next speed work rather than guess at it. The numbers
are from a Ryzen 9 5950X (16 cores, 32 threads), 15.5 GiB of RAM, an RTX 4070 Ti (12 GB) and f2fs on a
RAID 0, with clang/lld 23.1.4 and mold 3.0.0 (downloaded for the comparison, not shipped). The dump was the
real 25.6 GB package: 86 MB of exefs and 186,583 romfs files (25 GB), read where it lay.

Other agents were building on the same PC during most of the runs. **Absolute wall times are therefore upper
bounds**; every before/after below was measured by alternating the variants, and the syscall counts and the
file trees are the repeatable parts. A quiet PC is noticeably faster (the lift, for one, took 25.8 s idle
against 72 s under load).

## What a build spends its time on

Cold, from an empty work folder, in the order the engine runs:

| Stage | Seconds | What it is |
|---|---|---|
| Unpack the .nup (25.6 GB) | 50.5 s with 4 workers; 51.3 s with 6; 62.0 s with 8 | read, verify and write 186,588 members; f2fs is the limit, not the CPU |
| Lift (`lift.write`) | 25.8 s idle (72 s loaded) | discovery, translation, cache, chunks, patches, tables |
| Compile the 66 lifted/runtime objects | 412 s for 22 objects at the 3-job memory gate (work5); 1.84 GB peak RSS for one 26 MB chunk | clang -O1 on ~26 MB sources |
| Link `hookshot_game.so` | 2.7-4.8 s | 66 objects, 460 MB in, 428 MB out; unused by the game |
| Link the host executable | 0.7-2.1 s | 255 objects (66 lifted + 189 host), 412 MB out, 1.1 GB RSS |
| Link `hookshot_vk.so` | 0.08-0.18 s | 211 objects, 11.7 MB out |
| Package the game folder | 10.1 s cold, 0.43 s warm | 186,583 hard links, image and Vulkan copies, prompts |
| Shader census | 10.3-15.0 s | 1,445 archives in, 539 MB `hookshot_shaders.bin` out, 1.3 GB RSS |
| Shader preparation, first start | minutes (see below) | translate the seed, import the state list, compile pipelines |
| Shader preparation, prepared folder | 21.4 s reported with a 4.6 GB program cache and no pipeline cache; 5.3 s to build 15,330 + 71 pipelines from a warm cache | mostly opening caches and importing the state list |
| Rust engine start | 6-9 ms | the CLI answers `build.jobs` from 11 MB RSS |

The compile stage dominates a first build, then the shader preparation. Link and package together are under
10 s of a many-minute cold build. They matter most on updates, where the lift, compile and shader translation
are skipped and the remaining stages are the build.

## Ranked changes

Measured means this PC and these runs; estimated means the mechanism was measured but the total was not.
"Owner" marks files another agent had while this was written.

| # | Stage | Now | Change | Saving | Risk | Evidence |
|---|---|---|---|---|---|---|
| 1 | Shader preparation | 300-400 s first start | Keep and ship the program cache, pipeline cache and `hookshot_prepared.bin` across builds; the `build.all` update path already does | minutes; 5.3 s for the pipelines on a warm cache | medium: a translator change invalidates the program cache, a driver change its part | measured (first-run prediction 56,588 pipelines at ~142/s; warm precompile 5,330 ms with 2,739 ms opening caches) |
| 2 | Game link | 2.7-4.8 s + 0.5 s copy | Do not link and copy `hookshot_game.so`: nothing loads it, and the host executable already contains its objects | ~3.4 s and 428 MB a build | low: remove a dead artifact (`driver.rs`, owned) | measured |
| 3 | Compile | 412 s (22 objects, 3 at once) | The memory gate budgets 3.5 GB a job; one chunk peaked at 1.84 GB. Six jobs fit 12.8 GB of a 16 GB PC | ~2x on the compile, ~200 s cold (estimated) | medium: other work on the PC, and `compile.rs`/`toolchain.rs` are owned | peak RSS measured; total estimated |
| 4 | Package | 10.1 s cold | Stage folders on several threads (implemented) | ~2 s (10.1 -> ~2.0 s median under load) | low | measured, byte-identical tree |
| 5 | Package | 10.1 s cold | Link first, no stat per file (implemented) | 187,218 -> 635 `statx`, 1.7 s of syscall time | low | measured |
| 6 | Shader census | 10.3-15.0 s | Scan archives on several threads (in flight); also skip the 539 MB rewrite when the romfs is unchanged | 5-10 s cold; 10-15 s on every unchanged rebuild | low for the scan; a reuse needs a sound romfs stamp | scan in flight elsewhere; rewrite saving estimated |
| 7 | Lift | 5.4 s patches + 4.1 s cache store + 1.6 s chunks | Pre-size the chunk/cache buffers; share the emitted text instead of cloning it per function; store the cache in parallel | 2-4 s (estimated) | medium: `lift.rs` is owned | timings from the in-flight lift profile |
| 8 | CI | 1-3 min of cargo build | Cache `builder/rust/target` (or `Swatinem/rust-cache`) in the Rust job and the DLSS job; cache `~/.pub-cache` for Flutter | tens of seconds to minutes a warm run | low, workflows only (not edited here) | workflows read: the Rust job has no cargo cache, the DLSS job builds the engine cold, Flutter caches only the SDK |
| 9 | Link | 0.7-0.9 s | No change: lld already beats gold (3.2 s) and bfd (3.8 s); mold was 0.7-0.85 s, inside the noise, and is another dependency | 0 | - | measured |
| 10 | Any | 20.5 MB of the executable | `--strip-all` on the executable and shared object | 0.1 s and 20 MB; but crash logs and `scripts/prof.py` need the symbols | rejected | measured |

### Rejected or not worth it

- **GNU ld / gold in place of lld**: 3.8 s and 3.2 s against 0.7-0.9 s for the executable; the 428 MB shared
  object took 3.8-10.9 s with bfd and 3.6-6.0 s with gold, against 2.7-4.8 s with lld. lld is already the
  fast choice and needs no new dependency.
- **mold**: 0.71-0.85 s for the executable and 1.7-4.3 s for the shared object, level with lld within the
  noise on this loaded PC. Not worth a downloaded binary in the build.
- **`-Wl,--gc-sections`**: the objects have no `-ffunction-sections`, so it can only drop whole unused input
  sections; measured 0.59-0.69 s without it against 0.73-0.93 s with it. The flag is harmless but buys
  nothing; do not remove it for a win that is not there.
- **`-Wl,-Map`**: costs 2.5-3.9 s on the executable (41 MB map) and 10-17 s on the game shared object
  (18 MB map) here. The Rust engine does not write a Linux map; the Python route does, for crash
  symbolization. Keep the maps, but do not add them to the game object link.
- **`--strip-all`, `--strip-debug`, `--build-id=none`, `--icf=all`**: the objects are built `-g0`, so there are
  no debug sections to strip; `--strip-all` removes 20.5 MB (5%) of symbol tables in about the same time, and
  those tables are what names a crash offset. `--build-id=none` saves 144 bytes. `--icf=all` changes neither
  the size nor the time (there are no function sections to fold).
- **tmpfs for the link output**: `/tmp` is a 64 GB tmpfs here; linking there saved at most 0.1-0.2 s while the
  executable is written to its final folder anyway, and 412 MB does not belong in RAM on a 16 GB PC.
- **Compressing `hookshot_programs.bin` (7 GB)**: f2fs already compresses it on this PC, and every start
  reads it; a second decompression would cost more than the I/O it saves. Not measured further.
- **More NUP workers**: 6 was level with 4 (51.3 against 50.5 s) and 8 was worse (62.0 s), with more system
  time and 823 MB against 648 MB RSS. Four is the right default on a 16 GB PC; the in-flight code already
  caps at `min(cores, 4)`.
- **Rust engine start-up**: 6-9 ms, nothing to shave.

## The shader preparation, in detail

The preparation is two very different things:

1. **Translate** the 38,809 programs of the seed (three parts each: float, one and two integer bone inputs)
   and write `hookshot_programs.bin`. First run: minutes and a 7 GB cache. With the cache and the same
   translator, `hookshot_prepared.bin`'s stamp makes it a **21.4 s** read (0 new programs), of which 3.9 s is
   importing the 25,251-pipeline state list and 0.25-0.33 s is hashing the 539 MB seed.
2. **Compile pipelines** for the state list and the programs the notes predict. With an empty driver cache,
   56,588 predicted pipelines ran at ~142 a second (about 400 s), and the log says how far it got every
   10 s. With the driver's cache and `mkw_vk_pipelines.bin` in place, the same step was 5,330 ms for 15,330
   graphics and 71 compute pipelines in 5,260 ms, 2,739 ms of it opening the caches and reading the notes.

So the 36.8 s figure is a **warm** preparation (or one whose pipelines the driver already has), not a first
start. The mechanism that skips unchanged work is there and works: `build.all` on an existing folder keeps
`hookshot_prepared.bin`, `hookshot_programs.bin`, `mkw_vk_pipelines.bin` and the pipeline caches, and the
stamps (translator, seed contents, driver and libraries) decide whether each half may be skipped. A source
change to the translator, a game update that changes the seed, or a driver update each legitimately throws
one half away.

One gap: a fresh Rust build does not seed the shipped pipeline state list. `package` copies
`hookshot_pipeline_states_shipped.bin` only from the `assets` folder it is given, and the fresh-build
request does not pass one; the merge of `assets/pipeline_states/*.xz` is still Python's. Such a build starts
without the 25,251 shipped pipelines and compiles them in play instead of before the title.

## Commands used

The engine is `builder/rust/target/release/hookshot_engine`; `rusage` is a 30-line C wrapper around
`wait4` that prints wall, user, sys, peak RSS and block I/O of the child (a stand-in for `/usr/bin/time -v`,
which this PC lacks).

```sh
# one stage at a time
printf '%s' '{"command":"lift.write","exefs":"<dump>/exefs","out":"<work>/gen","standalone":true}' |
  rusage hookshot_engine > lift.json
printf '%s' '{"command":"build.shaders","romfs":"<dump>/romfs","out":"<out>/hookshot_shaders.bin"}' |
  rusage hookshot_engine > census.json
printf '%s' '{"command":"build.package","work":"<work>","out":"<out>","romfs":"<dump>/romfs","assets":"","language":"en-US"}' |
  rusage hookshot_engine > package.json

# where the package stage's syscalls go
strace -c -f hookshot_engine < package.json

# the links, from the response files the engine writes beside them
rusage clang -shared -fPIC -o hookshot_game.so @hookshot_game.so.rsp
rusage clang -fuse-ld=lld -o hookshot -Wl,--gc-sections \
  -Wl,--export-dynamic-symbol=hos_posix_* -Wl,--no-as-needed @hookshot.rsp \
  -lavcodec -lswscale -lswresample -lavutil -lpthread -ldl -lm

# NUP extraction at a chosen worker count (the in-flight engine's switch)
HOOKSHOT_NUP_JOBS=4 rusage hookshot_engine < nup.extract.json

# the preparation, from the built folder
cd <out> && HOS_HEADLESS=1 HOS_AUDIO=0 ./bin/hookshot --prepare-shaders
```

`cargo test --workspace` in `builder/rust` covers both package changes. The staged tree was compared by
name, size and inode: 186,583 files, every one the same inode as its romfs source, and the image and Vulkan
copies byte-identical. The prompts' `External.bfotf` is not byte-identical between two runs of any build: its
font records its build time, so two runs differ in five bytes. That is older than this work and not a
package regression.
