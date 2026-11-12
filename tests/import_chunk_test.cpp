// import_chunk (gpu/vk/import_chunk.h): the size game memory is imported by. 4 MB where a pool's VRAM mirror would cover the
// import's padding, 16 MB without mirrors, and RX_VK_IMPORT_CHUNK_MB still chooses. No game, GPU or driver.
#include "../gpu/vk/import_chunk.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::printf("FAIL line %d: %s\n", __LINE__, #x); std::exit(1); } } while (0)

int main() {
    using mkwvk::import_chunk_size::bytes;
    constexpr uint64_t MB = 1ull << 20;
    // (the default: mirrors on -> 4 MB, off -> 16 MB)
    CHECK(bytes(true, false, 0) == 4 * MB);
    CHECK(bytes(false, false, 0) == 16 * MB);
    // (RX_VK_IMPORT_CHUNK_MB chooses on any card)
    CHECK(bytes(true, true, 16) == 16 * MB);
    CHECK(bytes(false, true, 4) == 4 * MB);
    CHECK(bytes(true, true, 2) == 2 * MB);
    // (0 and negative are 0, as atoi and std::max(0, ...) gave: no chunking)
    CHECK(bytes(true, true, 0) == 0);
    CHECK(bytes(false, true, -7) == 0);
    std::printf("import_chunk: ok\n");
    return 0;
}
