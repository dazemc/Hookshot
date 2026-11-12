// Optional observations of completed GPU write-back differences (vk_memory.cpp's WbdTable, shaders/wb_diff.comp).
// RX_VK_WB_OBSERVE=1 exports one flag a chunk; the CPU reads them only once the submission that wrote them is done,
// before the table slot is reused. Missing or invalid results are unknown, never clean. No Vulkan here.
// tests/wb_observation_lifetime_test.cpp
#pragma once

#include <cstdint>

namespace wb_observe {

/// The shader's Chunk (32 bytes: three 64-bit addresses, the count, the vector flag).
struct Chunk {
    uint64_t mirror, shadow, guest;
    uint32_t count, vecs;   // (16-byte vectors when vecs is 1, else words)
};
static_assert(sizeof(Chunk) == 32, "the write-back shader's Chunk layout");

/// At most 1 MiB of optional flags per table (one uint a chunk).
constexpr uint64_t kMaxChunks = 256u << 10;
/// A chunk is at most 64 KB of the run (the pass's split, vk_memory.cpp's wbd_split).
constexpr uint64_t kMaxChunkBytes = 64u << 10;

/// Whether a table of table_bytes holds count chunks and their flags after them. A count of 0 has no result to read.
inline bool result_fits(uint64_t table_bytes, uint64_t count) {
    if (!count || count > kMaxChunks) return false;
    const uint64_t offset = count * sizeof(Chunk);
    return offset <= table_bytes && count * sizeof(uint32_t) <= table_bytes - offset;
}

/// The table's size for an input of input_bytes: the chunks, and when observations are on for a submission, one flag a
/// chunk after them. An input not a whole number of chunks, no value, or more chunks than the cap gets no flags.
inline uint64_t wanted_bytes(uint64_t input_bytes, bool observe, uint64_t value) {
    const uint64_t count = input_bytes / sizeof(Chunk);
    const uint64_t extra = observe && value && input_bytes % sizeof(Chunk) == 0 && count <= kMaxChunks
                               ? count * sizeof(uint32_t)
                               : 0;
    return input_bytes <= UINT64_MAX - extra ? input_bytes + extra : input_bytes;
}

struct Result {
    bool ok = false;   // (false: the batch is unknown, the whole write-back stays ordinary)
    uint64_t chunks = 0, changed = 0, bytes = 0, changed_bytes = 0;
};

/// Validate and sum one completed batch. A flag over 1 (the 0xffffffff sentinel of a store that did not happen), a
/// vector flag over 1, a zero-size chunk or one over 64 KB rejects the whole batch: missing data is not clean data.
inline Result collect(const Chunk* chunks, const uint32_t* flags, uint64_t count) {
    Result r;
    if (!chunks || !flags || !count || count > kMaxChunks) return r;
    for (uint64_t i = 0; i < count; ++i) {
        const uint64_t size = (uint64_t)chunks[i].count * (chunks[i].vecs ? 16u : 4u);
        if (flags[i] > 1 || chunks[i].vecs > 1 || !size || size > kMaxChunkBytes) return r;
        r.bytes += size;
        if (flags[i]) r.changed++, r.changed_bytes += size;
    }
    r.chunks = count;
    r.ok = true;
    return r;
}

} // namespace wb_observe
