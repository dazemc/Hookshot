# Pipeline and program cache storage

The pipeline cache (`mkw_vk_pipelines.bin`, the play generations and their saves) and the translated-program cache
(`hookshot_programs.bin`) keep their formats and switches. Two storage changes are worth knowing when reading or
changing their code.

## Pipeline caches are file-backed

Vulkan pipeline-cache persistence uses file-backed contiguous storage (`gpu/vk/pipeline_cache_file.h`) rather than a
temporary byte vector the size of the cache. This applies to the initial base cache, play-generation files, startup
compaction and subsequent base-cache saves. The Vulkan cache format, generation retention, growth threshold and
`RX_VK_PCACHE_LOAD` / `RX_VK_PLAY_CACHE` switches are unchanged.

The small device/driver header is checked before acquiring the complete payload. Windows holds a read-only mapping
with write sharing denied. Linux maps the opened source directly with read-only `MAP_PRIVATE`: reads create no
temporary file and require no extra cache-sized disk write or free space. Both retain the opened file/inode across the
renderer's atomic replacements. Regular symlink targets and Windows cloud files follow normal filesystem resolution;
directories and non-regular inputs are refused. External in-place truncation while a Linux view is in use is
unsupported; a private mapping does not make that safe.

Saving creates a unique owned sibling temporary, reserves the queried capacity (`posix_fallocate` on Linux, before any
mapped driver write) and lets Vulkan write directly into its mapped bytes. Only `VK_SUCCESS` with a valid actual length
permits flush, unmap, truncation, close and atomic replacement. `VK_INCOMPLETE`, failed I/O or denied replacement leaves
the old published cache and saved-size accounting intact, so a later save can retry. There is no in-place overwrite
fallback. On Windows a mapped destination can prevent replacement; the old cache is retained until the reader releases
it and a later save succeeds.

At cache initialization, recognized base/play temporary names older than 24 hours are collected. Published caches,
other names, links and recent temporaries are left alone. A POSIX writer holds an exclusive advisory lock; Windows
write handles deny sharing. An active writer is retained even if its temporary has an old timestamp. Publication
refreshes the timestamp before closing the file, so a writer paused longer than 24 hours is also fresh during its
close/rename gap. Linux replacements preserve an existing owned regular cache's permission bits; new files retain
0600, without overriding a restrictive umask.

This removes the temporary bulk private/anonymous allocation, not the Vulkan driver's retained cache. File-backed pages
still consume physical memory and I/O. Filesystem-dependent acquisition timings are not a game startup or frame-rate
promise. Published files must be replaced atomically, and source bytes must not be modified in place while the driver
reads them. Mappings do not protect against media failures: mapped I/O can raise native in-page faults, which are not
ordinary recoverable C++ I/O exceptions.

## Files from another translator are dropped

`mkw_vk_pipelines.stamp` holds the `program_cache_stamp()` the pipeline cache's files were made for
(`gpu/vk/pcache_stamp.h`). A new translator gives every program new SPIR-V, so the old pipelines can never be hit
again; the file would only grow and be read whole at every start. At the first start after a change, `pcache_init`
drops the base file, the play files and their temporaries before reading them, and `prepare_run` makes the stage
libraries the warm pass had kept in them again. `RX_VK_PCACHE_STAMP=0` keeps them whatever made them, as before;
`RX_VK_PCACHE_LOAD=0` is unchanged.

## The program cache can be compressed

`RX_VK_PROGRAM_CACHE_ZSTD=1` keeps each translated program record in `hookshot_programs.zst.bin`, compressed with zstd
level 3 and unpacked when it is read (`gpu/vk/progcache_zstd.h`). It is a separate file: an existing
`hookshot_programs.bin` is untouched, the uncompressed format is unchanged, and a build without the switch never reads
the compressed file. The Vulkan back end builds `third_party/zbic/zstd.c` once as `vk_zstd.o` for this and the game's
font files, with `ZSTD_ZBIC_SUPPORT` off (stock zstd only).

## Game-free checks

```sh
cargo test --manifest-path builder/rust/Cargo.toml --workspace
```

`builder/rust/tests/pipeline_cache.rs` compiles the fixtures under `tests/` (file-backed persistence without a
cache-sized allocation, the translator stamp, strict translate-wait parsing, zstd round-trip and the uncompressed
path) and checks the wiring in `vk_draw.cpp`, `vk_jobs.cpp`, `vk_progcache.cpp`, `vk_shader.cpp` and
`nx2/build.py`. A machine with no C/C++ compiler skips the compiled fixtures. Actual driver and game runs remain
separate local validation; no generated cache files belong in the repository.
