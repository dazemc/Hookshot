// The size game memory is imported by (vk_memory.cpp's import_chunk; tests/import_chunk_test.cpp).
//
// Imports are made in chunks around what was bound, not by pool: each import cost 5-45 ms in the driver, more as their number
// grew, and the game makes ~13 pools a second. A pool's VRAM mirror is its import's whole range, so with 16 MB chunks 28-39% of
// the mirrors' bytes held memory outside every game pool (upstream #2685: 459 of 1,666 MB in a Hyrule Field walk, up to 925 MB
// after Castle Town to the field). With mirrors, 4 MB chunks cut the mirrors by roughly a quarter at the same frame rate; without
// mirrors a chunk's padding costs no VRAM, so iGPUs and GCN keep 16 MB and take no extra imports. RX_VK_IMPORT_CHUNK_MB still
// chooses (0: no chunking, as before).
#pragma once

#include <cstdint>

namespace import_chunk_size {

inline constexpr int kWithMirrorsMb = 4;      // (a mirror covers the whole import: its padding is VRAM)
inline constexpr int kWithoutMirrorsMb = 16;  // (no mirror: padding costs no VRAM, fewer imports is better)

/// The chunk in bytes. `set`: RX_VK_IMPORT_CHUNK_MB has a value; `env_mb` its number (0 or negative: 0, no chunking).
/// Unset: 4 MB where the pools have VRAM mirrors, 16 MB without them, as before.
constexpr uint64_t bytes(bool mirrors, bool set, int env_mb) {
    const int mb = set ? (env_mb > 0 ? env_mb : 0) : (mirrors ? kWithMirrorsMb : kWithoutMirrorsMb);
    return static_cast<uint64_t>(mb) << 20;
}

}  // namespace import_chunk_size
