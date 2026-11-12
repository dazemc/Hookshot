// The texture budget's arithmetic (gpu/vk/texture_budget.h): BC/ASTC whole blocks, BC7's 16-byte 4x4 blocks, decoded
// ASTC as RGBA8, partial blocks and 1x1 tails, skipped top levels, layers and samples. No game, no GPU.
#include "../gpu/vk/texture_budget.h"

#include <algorithm>
#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::printf("FAIL line %d: %s\n", __LINE__, #x); std::exit(1); } } while (0)

using namespace mkwvk::texture_budget;

namespace {

constexpr uint64_t kMB = 1ull << 20;

/// The independent block arithmetic the production helper must agree with: each level rounded up on its own.
uint64_t expected_units(uint32_t w, uint32_t h, uint32_t d, uint32_t levels, uint32_t skip, uint32_t bw, uint32_t bh) {
    uint64_t n = 0;
    for (uint32_t l = skip; l < levels; l++) {
        const uint64_t lw = (uint64_t)((std::max(1u, w >> l) + bw - 1) / bw);
        const uint64_t lh = (uint64_t)((std::max(1u, h >> l) + bh - 1) / bh);
        n += lw * lh * std::max(1u, d >> l);
    }
    return n;
}

}  // namespace

int main() {
    // one level: blocks round up, even a 1x1 tail is a whole block
    CHECK(level_units(1, 1, 1) == 1 && level_units(4, 4, 1, 4, 4) == 1 && level_units(5, 5, 1, 4, 4) == 4);
    CHECK(level_units(1365, 1365, 1, 4, 4) == 342 * 342);
    CHECK(level_units(0, 0, 0) == 1);

    // the 14 ASTC block sizes (NVN 0x79.. as vk_formats.cpp has them)
    const uint32_t astc[][2] = {{4, 4}, {5, 4}, {5, 5}, {6, 5}, {6, 6}, {8, 5}, {8, 6},
                                {8, 8}, {10, 5}, {10, 6}, {10, 8}, {10, 10}, {12, 10}, {12, 12}};
    for (const auto& b : astc) {
        CHECK(level_units(13, 13, 1, b[0], b[1]) == ((13 + b[0] - 1) / b[0]) * ((13 + b[1] - 1) / b[1]));
        CHECK(level_units(1365, 1365, 1, b[0], b[1]) == ((1365 + b[0] - 1) / b[0]) * ((1365 + b[1] - 1) / b[1]));
    }

    // the mip chain from a skipped level: a huge texture leaving its top levels out
    const uint64_t w = 1365, h = 1365, d = 1, levels = 11, layers = 90;
    CHECK(chain_units(w, h, d, levels, 0, 8, 8) == expected_units(w, h, d, levels, 0, 8, 8));
    CHECK(chain_units(w, h, d, levels, 3, 8, 8) == expected_units(w, h, d, levels, 3, 8, 8));
    CHECK(chain_units(w, h, d, levels, 3, 8, 8) < chain_units(w, h, d, levels, 0, 8, 8));

    // native ASTC: 16 bytes a block; the 1365 x 1365 x 90 ASTC 8x8 array is 53.8 MiB of blocks, not 3410 MiB of texels
    const uint64_t astc_blocks = chain_units(w, h, d, levels, 0, 8, 8);
    const uint64_t astc_bytes = bytes(astc_blocks, layers, 1, 16);
    CHECK(astc_blocks * layers * 16 == 56365920);
    CHECK(astc_bytes / kMB == 53);
    CHECK(bytes(chain_units(w, h, d, levels, 0, 1, 1), layers, 1, 16) == 3575649600);   // (the old texel count)

    // native BC: 8 bytes a 4x4 block for BC1/BC4, 16 for the rest; the same image counts half as BC1, not twice
    const uint64_t bc_units = chain_units(w, h, d, levels, 0, 4, 4);
    CHECK(bytes(bc_units, layers, 1, 8) * 2 == bytes(bc_units, layers, 1, 16));
    CHECK(bc_units == expected_units(w, h, d, levels, 0, 4, 4));
    // (on a 4x4-aligned level BC3 is exactly one byte a texel; 1365's partial blocks count their whole 16 bytes)
    CHECK(level_units(1364, 1364, 1, 4, 4) * 16 == 1364ull * 1364);

    // decoded ASTC as RGBA8: four bytes a texel, 16 times an 8x8 ASTC block (less the blocks' rounding)
    const uint64_t decoded = bytes(chain_units(w, h, d, levels, 0, 1, 1), layers, 1, 4);
    CHECK(decoded > astc_bytes * 15 && decoded < astc_bytes * 16);

    // re-encoded BC3/BC7: 4x4 16-byte blocks, not the source ASTC's block size; BC7 is the same size as BC3
    CHECK(counted_unit_bytes(true, false, 0, 16) == 16);
    CHECK(counted_unit_bytes(false, true, 0, 16) == 4);
    CHECK(counted_unit_bytes(false, false, 3, 16) == 4);
    CHECK(counted_unit_bytes(false, false, 6, 16) == 8);
    CHECK(counted_unit_bytes(false, false, 0, 2) == 2);
    CHECK(counted_unit_bytes(false, false, 0, 0) == 1);
    const uint64_t reencoded = bytes(chain_units(w, h, d, levels, 0, 4, 4), layers, 1, counted_unit_bytes(true, false, 0, 16));
    CHECK(reencoded == bytes(chain_units(w, h, d, levels, 0, 4, 4), layers, 1, 16));   // (BC7 mode 6 and BC3: the same VRAM)
    CHECK(level_units(1364, 1364, 1, 4, 4) * 16 * 4 == 1364ull * 1364 * 4);   // (a quarter of RGBA8, one level)
    CHECK(reencoded < bytes(chain_units(w, h, d, levels, 0, 1, 1), layers, 1, 4));     // (1365's partial blocks add a little)

    // the estimate before creation: decoded ASTC is RGBA8, a native BC/ASTC format its whole blocks, the rest texels
    CHECK(estimate_unit_bytes(true, false, 16, 8, 8) == 4);
    CHECK(estimate_unit_bytes(false, true, 8, 4, 4) == 8);    // (BC1/BC4)
    CHECK(estimate_unit_bytes(false, true, 16, 8, 8) == 16);  // (ASTC)
    CHECK(estimate_unit_bytes(false, false, 4, 1, 1) == 4);
    CHECK(estimate_unit_bytes(false, false, 2, 2, 1) == 1);   // (the bytes a texel, not less than one)
    CHECK(estimate_unit_bytes(false, false, 0, 1, 1) == 1);

    // layers, depth, samples and sparse images
    CHECK(bytes(level_units(64, 64, 8, 4, 4), 6, 4, 16) == 16 * 16 * 8 * 6 * 4 * 16);
    CHECK(bytes(level_units(32, 17, 9, 4, 4), 1, 1, 16) == 8 * 5 * 9 * 16);
    const uint64_t sparse_count = 0;   // (create charges a sparse image nothing until its layers are bound)
    CHECK(sparse_count == 0 && bytes(level_units(1, 1, 1, 1, 1), 1, 1, 1) == 1);

    // every level's own block grid: 1365 -> level 2 is 86 blocks across, not 342 >> 2 = 85
    CHECK(level_units(1365 >> 2, 1365 >> 2, 1, 4, 4) == 86 * 86);
    CHECK(level_units(1365 >> 2, 1365 >> 2, 1, 4, 4) != (level_units(1365, 1365, 1, 4, 4) >> 4));

    std::printf("texture budget: BC/ASTC blocks, BC7 16-byte blocks, decoded ASTC, partial blocks and mip chains pass\n");
    return 0;
}
