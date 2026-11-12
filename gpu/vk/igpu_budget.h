// The texture budget of an integrated GPU, whose "VRAM" is the system's RAM (vk_texture.cpp). Upstream a435efd0
// (#2603): a hardcoded 2.5 GB was too much on an 8 GB laptop and too little on a 32 GB one.
//
// No Vulkan, no Windows, no environment here: the tiers are pure arithmetic, and tests/test_igpu_budget.cpp covers
// them. The caller reads the system's RAM once and hands in what the driver allows the process; RX_VK_VRAM_MB and a
// discrete GPU keep that allowance untouched.
#pragma once

#include <algorithm>
#include <cstdint>

namespace mkwvk::igpu_budget {

/// (a machine whose RAM could not be read: the smallest tier's, 8 GB)
inline constexpr uint64_t kUnknownSystemRam = 8ull << 30;
/// (never less than this, whatever the tiers say: below it the texture cache thrashes)
inline constexpr uint64_t kMinimum = 1024ull << 20;
/// (the most any card's textures may hold, discrete or integrated: a 16 GB card's 10.8 GB filled up, read as a leak)
inline constexpr uint64_t kCap = 6144ull << 20;

/// The hard cap by system-RAM tier: an 8 GB laptop gets 1.5 GB, a 16 GB one the 2.5 GB the old fixed budget gave,
/// a 48 GB one the 6 GB a discrete card is capped at. These are upper bounds only; what the rest of the PC needs and
/// what the driver allows are applied below.
inline constexpr uint64_t cap(uint64_t system_ram) {
    if (system_ram <= (8ull << 30)) return 1536ull << 20;    // 1.5 GB
    if (system_ram <= (12ull << 30)) return 2048ull << 20;   // 2 GB
    if (system_ram <= (16ull << 30)) return 2560ull << 20;   // 2.5 GB (the old fixed budget)
    if (system_ram <= (24ull << 30)) return 3584ull << 20;   // 3.5 GB
    if (system_ram <= (32ull << 30)) return 4096ull << 20;   // 4 GB
    return kCap;                                             // 6 GB
}

struct Plan {
    uint64_t budget;    // what the texture cache may hold
    uint64_t reserved;  // the system RAM kept for the game, the renderer's other allocations and the OS (0: no tier used)
};

/// The budget for a card that shares the system's RAM.
///
/// `driver_budget` is what the caller would use otherwise: RX_VK_VRAM_MB when set, else 70% of what the driver allows
/// the process at the first look (45% on a card of 8 GB or less), capped at 6 GB. `from_env` and a discrete GPU return
/// it unchanged, so RX_VK_VRAM_MB still wins over everything and discrete cards keep their behaviour.
///
/// Otherwise:
/// - reserved = min(system_ram / 3, 16 GB), floor 8 GB (the game's own ~10 GB, render targets and buffers, the OS)
/// - available = system_ram - reserved
/// - budget = min(available, driver_budget), capped by the RAM tier
/// - at least 1 GB, and never over the driver's allowance
///
/// On an 8 GB machine the 8 GB floor reserves all of it, so the 1 GB minimum is what is left, not the tier's 1.5 GB.
inline constexpr Plan choose(uint64_t system_ram, uint64_t driver_budget, bool from_env, bool integrated) {
    if (from_env || !integrated) return {driver_budget, 0};
    if (!system_ram) system_ram = kUnknownSystemRam;
    uint64_t reserved = std::min<uint64_t>(system_ram / 3, 16ull << 30);
    if (reserved < (8ull << 30)) reserved = 8ull << 30;
    const uint64_t available = system_ram > reserved ? system_ram - reserved : 0;
    uint64_t budget = std::min(available, driver_budget);
    budget = std::min(budget, cap(system_ram));
    budget = std::max(budget, kMinimum);
    budget = std::min(budget, driver_budget);   // (the driver's allowance wins even under the 1 GB floor)
    return {budget, reserved};
}

} // namespace mkwvk::igpu_budget
