// gpu/vk/progcache_zstd.h with third_party/zbic's zstd, the exact helper the program cache reads its records through
// (RX_VK_PROGRAM_CACHE_ZSTD). A record is a zstd frame led by its unpacked size; the uncompressed path is unchanged and
// returns the stored bytes themselves. No game, dump or GPU.
#include "../gpu/vk/progcache_zstd.h"

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

static int failed;
#define CHECK(x)                                                \
    do {                                                        \
        if (!(x)) {                                             \
            std::printf("FAILED line %d: %s\n", __LINE__, #x);  \
            failed++;                                           \
        }                                                       \
    } while (0)

static std::vector<uint8_t> frame(const std::vector<uint8_t>& plain) {
    std::vector<uint8_t> stored(8 + ZSTD_compressBound(plain.size()));
    const uint64_t usize = plain.size();
    std::memcpy(stored.data(), &usize, 8);
    const size_t n = ZSTD_compress(stored.data() + 8, stored.size() - 8, plain.data(), plain.size(), 3);
    CHECK(!ZSTD_isError(n));
    stored.resize(8 + n);
    return stored;
}

int main() {
    using namespace mkwvk;
    // (a record with the shape of a kept program: repeated words, a long c1 block, a tail)
    std::vector<uint8_t> plain;
    for (uint32_t i = 0; i < 512; i++) {
        const uint32_t words[4] = {i, i * 3, 0x5a5a5a5a, i & 7};
        plain.insert(plain.end(), (const uint8_t*)words, (const uint8_t*)words + sizeof words);
    }
    plain.resize(plain.size() + 3000, 0x5a);

    // The uncompressed path hands the stored bytes back as they are (the old cache's 16-byte head and all).
    const uint8_t* payload = nullptr;
    size_t payload_size = 0;
    std::vector<uint8_t> spare(1, 0xee);
    CHECK(program_cache_payload(plain.data(), plain.size(), 0, spare, &payload, &payload_size));
    CHECK(payload == plain.data() && payload_size == plain.size() && spare.size() == 1);

    // A compressed record round-trips, and the helper's plain buffer holds the exact bytes.
    std::vector<uint8_t> stored = frame(plain);
    CHECK(stored.size() + 8 < plain.size());   // (the cache only stores it compressed when the frame is smaller)
    std::vector<uint8_t> unpacked;
    CHECK(program_cache_payload(stored.data(), stored.size(), kCompressedKind, unpacked, &payload, &payload_size));
    CHECK(payload == unpacked.data() && payload_size == plain.size() && unpacked == plain);
    std::printf("zstd: %zu bytes stored as %zu (%.2fx)\n", plain.size(), stored.size(), (double)plain.size() / stored.size());

    // Not a whole record, an unpacked size past the cap, and a frame that does not unpack: all refused.
    CHECK(!program_cache_payload(stored.data(), 7, kCompressedKind, unpacked, &payload, &payload_size));
    std::vector<uint8_t> huge = stored;
    const uint64_t too_big = (1ull << 31) + 1;
    std::memcpy(huge.data(), &too_big, 8);
    CHECK(!program_cache_payload(huge.data(), huge.size(), kCompressedKind, unpacked, &payload, &payload_size));
    std::vector<uint8_t> junk(64, 0x7f);
    const uint64_t want = 32;
    std::memcpy(junk.data(), &want, 8);
    CHECK(!program_cache_payload(junk.data(), junk.size(), kCompressedKind, unpacked, &payload, &payload_size));
    std::vector<uint8_t> wrong = frame(plain);
    const uint64_t shorter = plain.size() - 1;
    std::memcpy(wrong.data(), &shorter, 8);   // (the frame unpacks to plain.size(), not the head's)
    CHECK(!program_cache_payload(wrong.data(), wrong.size(), kCompressedKind, unpacked, &payload, &payload_size));

    // The uncompressed kind keeps working after all that, with a kind that has other flags too.
    CHECK(program_cache_payload(plain.data(), plain.size(), 0x7, spare, &payload, &payload_size));
    CHECK(payload == plain.data() && payload_size == plain.size());

    if (failed) return 1;
    std::printf("program_cache_zstd: ok\n");
    return 0;
}
