// A pipeline cache file's data, checked by its header before the rest is read (vk_draw.cpp's pcache_file_read;
// tests/pcache_read_test.cpp).
//
// After a driver update, or with a cache copied from another GPU, every start read the whole mkw_vk_pipelines.bin (hundreds of MB)
// into memory only to throw it away on the header check. This reads the 32-byte header first and drops a short, truncated or
// mismatched file there. A cache from this device and driver comes back byte for byte, as before.
#pragma once
#include <cstdint>
#include <cstring>
#include <fstream>
#include <string>
#include <vector>

// (the fixture in tests/ compiles this without the Vulkan SDK: only the cache header's constants are needed here)
#if defined(__has_include)
#  if __has_include(<vulkan/vulkan.h>)
#    include <vulkan/vulkan.h>
#  endif
#endif
#ifndef VK_UUID_SIZE
#define VK_UUID_SIZE 16
#endif
#ifndef VK_PIPELINE_CACHE_HEADER_VERSION_ONE
#define VK_PIPELINE_CACHE_HEADER_VERSION_ONE 1
#endif

namespace pcache_read {

/// The device a cache must have been made by: its vendor and device ids and its pipeline cache UUID.
struct Device {
    uint32_t vendor = 0, device = 0;
    uint8_t uuid[VK_UUID_SIZE] = {};
};

/// Whether a 32-byte cache header describes a file of `size` bytes made by this device and driver (vk_draw.cpp's
/// PipelineCacheFile::read uses this too, so the file-backed path checks the same header before mapping).
inline bool header_ok(const uint8_t* head, uint64_t size, const Device& at) {
    uint32_t head_size = 0, version = 0, vendor = 0, device = 0;
    std::memcpy(&head_size, head, 4);
    std::memcpy(&version, head + 4, 4);
    std::memcpy(&vendor, head + 8, 4);
    std::memcpy(&device, head + 12, 4);
    return head_size >= 32 && head_size <= size && version == VK_PIPELINE_CACHE_HEADER_VERSION_ONE &&   // (hookshot)
           vendor == at.vendor && device == at.device && !std::memcmp(head + 16, at.uuid, VK_UUID_SIZE);
}

/// A pipeline cache file's data, or nothing when it cannot be read, is short or truncated, or was made by another device or driver.
inline std::vector<uint8_t> read(const std::string& path, const Device& at) {
    std::vector<uint8_t> data;
    std::ifstream f(path, std::ios::binary);
    if (!f) return data;
    uint8_t head[32] = {};
    f.read(reinterpret_cast<char*>(head), sizeof head);
    if (f.gcount() != static_cast<std::streamsize>(sizeof head)) return data;
    f.seekg(0, std::ios::end);
    const std::streamoff size = f.tellg();
    if (size <= static_cast<std::streamoff>(sizeof head) || !header_ok(head, static_cast<uint64_t>(size), at)) return data;
    f.seekg(static_cast<std::streamoff>(sizeof head), std::ios::beg);
    data.resize(static_cast<size_t>(size));
    std::memcpy(data.data(), head, sizeof head);
    f.read(reinterpret_cast<char*>(data.data() + sizeof head), static_cast<std::streamsize>(data.size() - sizeof head));
    if (f.gcount() != static_cast<std::streamsize>(data.size() - sizeof head)) data.clear();
    return data;
}

} // namespace pcache_read
