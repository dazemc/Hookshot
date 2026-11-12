// The import chunk's default (gpu/vk/import_chunk.h): 4 MB where pools have VRAM mirrors, 16 MB without, and the switch
// still sets it. No game, no GPU.
#include "../gpu/vk/import_chunk.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::printf("FAIL line %d: %s\n", __LINE__, #x); std::exit(1); } } while (0)

int main() {
    using mkwvk::import_chunk_size::bytes;
    const uint64_t MB = 1ull << 20;
    CHECK(bytes(nullptr, true) == 4 * MB);     // (a mirror is its import's whole range: 4 MB chunks fit the game's pools)
    CHECK(bytes("", true) == 0);               // (set but empty: 0, as the switch's getenv read always meant)
    CHECK(bytes(nullptr, false) == 16 * MB);   // (no mirror: a chunk's padding costs no VRAM)
    CHECK(bytes("16", true) == 16 * MB);       // (the old size, as before)
    CHECK(bytes("4", false) == 4 * MB);
    CHECK(bytes("0", true) == 0);              // (imports by whole pool)
    CHECK(bytes("8", true) == 8 * MB);
    CHECK(bytes("-1", true) == 0);             // (never a negative chunk)
    std::printf("import chunk: 4 MB with mirrors, 16 MB without, switch honoured\n");
    return 0;
}
