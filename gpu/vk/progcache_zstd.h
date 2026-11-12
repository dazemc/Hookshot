// (hookshot) The program cache's compressed records (RX_VK_PROGRAM_CACHE_ZSTD, vk_progcache.cpp): the record's stored bytes
// are a zstd frame when the kind has kCompressedKind, led by the unpacked size. Apart from the cache so that
// tests/program_cache_zstd_test.cpp can run the exact helper with third_party/zbic's zstd.
#pragma once

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <vector>

#include "zstd.h"

namespace mkwvk {

// (the index's kind word is the record's first flags word: bits 0-3 are compute/mesh/fallback/fallback_task)
constexpr uint32_t kCompressedKind = 16;   // (a zstd record: 8-byte unpacked size, then the frame)
constexpr size_t kZstdHead = 20;           // (the zstd cache's record head: key, stored size, kind; the old cache's is 16)

/// The payload of a record: the stored bytes the file has at d.at (d.size of them), unpacked when the kind says it is
/// compressed (RX_VK_PROGRAM_CACHE_ZSTD). On true, payload/payload_size are the program's bytes (plain, when it was
/// compressed); false when the record is not whole or the frame does not unpack.
inline bool program_cache_payload(const uint8_t* stored, size_t stored_size, uint32_t kind, std::vector<uint8_t>& plain,
                                  const uint8_t** payload, size_t* payload_size) {
    *payload = stored;
    *payload_size = stored_size;
    if (!(kind & kCompressedKind)) return true;
    if (stored_size < 8) return false;
    uint64_t usize;
    std::memcpy(&usize, stored, 8);
    if (usize < 4 || usize > (1ull << 31)) return false;
    plain.resize((size_t)usize);
    const size_t n = ZSTD_decompress(plain.data(), plain.size(), stored + 8, stored_size - 8);
    if (ZSTD_isError(n) || n != plain.size()) return false;
    *payload = plain.data();
    *payload_size = plain.size();
    return true;
}

} // namespace mkwvk
