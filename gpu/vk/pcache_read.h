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

#include <vulkan/vulkan.h>

namespace pcache_read {

/// The device a cache must have been made by: its vendor and device ids and its pipeline cache UUID.
struct Device {
    uint32_t vendor = 0, device = 0;
    uint8_t uuid[VK_UUID_SIZE] = {};
};

/// A pipeline cache file's data, or nothing when it cannot be read, is short or truncated, or was made by another device or driver.
inline std::vector<uint8_t> read(const std::string& path, const Device& at) {
    std::vector<uint8_t> data;
    std::ifstream f(path, std::ios::binary);
    if (!f) return data;
    uint8_t head[32] = {};
    f.read(reinterpret_cast<char*>(head), sizeof head);
    if (f.gcount() != static_cast<std::streamsize>(sizeof head)) return data;
    uint32_t head_size = 0, version = 0, vendor = 0, device = 0;
    std::memcpy(&head_size, head, 4);
    std::memcpy(&version, head + 4, 4);
    std::memcpy(&vendor, head + 8, 4);
    std::memcpy(&device, head + 12, 4);
    if (head_size < 32 || version != VK_PIPELINE_CACHE_HEADER_VERSION_ONE ||   // (hookshot)
        vendor != at.vendor || device != at.device || std::memcmp(head + 16, at.uuid, VK_UUID_SIZE))
        return data;
    f.seekg(0, std::ios::end);
    const std::streamoff size = f.tellg();
    if (size <= static_cast<std::streamoff>(sizeof head) || static_cast<uint64_t>(head_size) > static_cast<uint64_t>(size))
        return data;
    f.seekg(static_cast<std::streamoff>(sizeof head), std::ios::beg);
    data.resize(static_cast<size_t>(size));
    std::memcpy(data.data(), head, sizeof head);
    f.read(reinterpret_cast<char*>(data.data() + sizeof head), static_cast<std::streamsize>(data.size() - sizeof head));
    if (f.gcount() != static_cast<std::streamsize>(data.size() - sizeof head)) data.clear();
    return data;
}

} // namespace pcache_read
