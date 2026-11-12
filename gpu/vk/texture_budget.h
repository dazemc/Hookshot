// The texture budget's arithmetic (vk_texture.cpp, create): whole blocks for a compressed image, texels for the rest.
// Plain functions, no Vulkan and no game: tests/test_texture_budget.cpp runs them.
#pragma once

#include <algorithm>
#include <cstdint>

namespace mkwvk::texture_budget {

/// One mip level in texels (bw = bh = 1), or in whole bw x bh blocks. Each extent is rounded up on its own: even a 1x1
/// BC/ASTC level occupies one block.
inline uint64_t level_units(uint32_t w, uint32_t h, uint32_t d, uint32_t bw = 1, uint32_t bh = 1) {
    w = std::max(1u, w), h = std::max(1u, h), d = std::max(1u, d), bw = std::max(1u, bw), bh = std::max(1u, bh);
    return (uint64_t)((w + bw - 1) / bw) * ((h + bh - 1) / bh) * d;
}

/// A mip chain's units, from level `skip` on (a huge texture leaves its top levels out).
inline uint64_t chain_units(uint32_t width, uint32_t height, uint32_t depth, uint32_t levels, uint32_t skip,
                            uint32_t bw = 1, uint32_t bh = 1) {
    uint64_t px = 0;
    for (uint32_t l = skip; l < levels; l++)
        px += level_units(width >> l, height >> l, depth >> l, bw, bh);
    return px;
}

/// What an image counts: its units x layers x bytes a unit x samples. A sparse image counts nothing until its layers are
/// bound (create's caller adds them).
inline uint64_t bytes(uint64_t units, uint32_t layers, uint32_t samples, uint32_t unit_bytes) {
    return units * layers * unit_bytes * std::max(1u, samples);
}

/// Bytes a unit of the estimate before creation: decoded ASTC (astc) is RGBA8; a native BC/ASTC format counts its whole
/// blocks at `bpb` bytes; anything else counts texels at the bytes a texel `bpb / (bw * bh)` (at least one).
inline uint32_t estimate_unit_bytes(bool astc, bool blocks, int bpb, int bw, int bh) {
    if (astc) return 4;
    if (blocks) return (uint32_t)std::max(0, bpb);
    return (uint32_t)std::max(1, bpb / std::max(1, bw * bh));
}

/// Bytes a unit of the final count: a re-encoded BC3/BC7 image is 4x4 16-byte blocks (not its source ASTC's block size);
/// decoded ASTC is RGBA8; a native format counts its own blocks (bpb) or texels (from, the source's bytes a texel).
inline uint32_t counted_unit_bytes(bool bc, bool astc, int from, int bpb) {
    if (bc) return 16;
    if (astc) return 4;
    return (uint32_t)std::max(1, from ? (from == 3 ? 4 : from == 6 ? 8 : 16) : bpb);
}

}  // namespace mkwvk::texture_budget
