# Write-back observations

`RX_VK_WB_OBSERVE=1` measures actual mirror/shadow differences in the existing GPU write-back pass. It is off by default. It adds observations, not a write-back optimization: copy decisions, CPU/GPU invalidation and synchronization policy remain unchanged. It needs the ordinary diff path to be enabled and available.

## Reading the output

The existing 600-submission logging cadence reports completed batches, compared chunks, changed chunks, compared MiB, MiB covered by changed chunks, unavailable chunks and the latest consumed submission value. A chunk is at most 64 KiB; it need not align to a physical 64 KiB tile. One changed word marks the entire chunk changed, so the changed-chunk volume is not an exact changed-byte count.

Results are collected when a completed table slot is reused. They may lag the current recording by the table ring's lifetime. The report contains observations consumed since the previous report, not necessarily the same submissions as other interval counters. Pending results are never read or forced to complete. Final pending results need not appear before shutdown.

Unavailable results are unknown, not clean. Allocation, mapping, address, capacity, flush or invalidation failure keeps the ordinary write-back path. An unwritten or invalid flag discards the entire observation batch. Feedback is bounded to 262144 chunks per slot (1 MiB of flags), appended to the existing mapped input table and reused only after its submission retires. There is no extra submission, helper thread or wait.

A clean observation at submission N says nothing about a later GPU write at N+1. These counters alone cannot justify skipping comparisons or write-backs. Future culling needs an independent freshness and invalidation proof.

## Tests without game files

- `tests/wb_observation_lifetime_test.cpp` (compiled and run by `builder/rust/tests/vk_memory_fixtures.rs`, also directly with `c++ -std=c++20 -Wall -Wextra -Werror tests/wb_observation_lifetime_test.cpp`) exercises table sizing, result bounds, flag validation, completion, reuse and a failed read consumed once, with authored memory and no GPU.
- `tests/wb_diff_observations_gpu.cpp` is an authored Vulkan fixture: replace `@SHADER_HEADER@` with the include of `gpu/vk/shaders/wb_diff_comp_spv.h` and build it against Vulkan 1.3 headers. It needs a GPU with buffer device address and compute subgroup votes to run; its compile does not.
- Run it on such a device to execute the generated shader. It checks copy parity, mixed vector/word chunks, reused clean results, zero count, output guards and the 65535-wide 2D dispatch boundary. Host-coherent fixture memory does not prove noncoherent host visibility in the game.
- Build the same fixture against an older `wb_diff_comp_spv.h` as a causal control: the old shader preserves copy parity but cannot produce the expected observations.

Native overhead comparisons must use matching host, renderer base, scene, effective resolution and settings; compare both opt-out and opt-in, with counterbalanced runs after warmup. Report observed overhead and limits without treating software Vulkan or CPU lifecycle tests as gameplay validation.
