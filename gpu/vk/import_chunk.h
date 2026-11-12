// The size of the game-memory imports (vk_memory.cpp, import_chunk): 4 MB by default where pools have VRAM mirrors, so a
// mirror fits the game's pool; 16 MB without them, where a chunk's padding costs no VRAM. RX_VK_IMPORT_CHUNK_MB sets it.
// Plain rules, no Vulkan and no game: tests/test_import_chunk.cpp runs them.
#pragma once

#include <algorithm>
#include <cstdint>
#include <cstdlib>

namespace mkwvk::import_chunk_size {

inline uint64_t bytes(const char* env, bool mirrors) {
    const int mb = env ? std::max(0, atoi(env)) : mirrors ? 4 : 16;
    return (uint64_t)mb << 20;
}

}  // namespace mkwvk::import_chunk_size
