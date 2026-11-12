// pcache_read (gpu/vk/pcache_read.h): a pipeline cache file checked by its 32-byte header before the rest is read, so a cache
// from another device or driver (or a short, empty or truncated one) is dropped without reading hundreds of MB. No game, no GPU.
#include "../gpu/vk/pcache_read.h"

#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <vector>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using pcache_read::Device;

static constexpr uint32_t kVendor = 0x10de, kDevice = 0x2684;
static const uint8_t kUuid[VK_UUID_SIZE] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16};

// (a file of `size` bytes: a header naming the device, then a body of `size - 32` bytes that climb)
static std::vector<uint8_t> body(size_t size, uint32_t head_size = 32, uint32_t version = VK_PIPELINE_CACHE_HEADER_VERSION_ONE,
                                 uint32_t vendor = kVendor, uint32_t device = kDevice, uint8_t uuid_last = 16) {
    std::vector<uint8_t> data(size);
    std::memcpy(data.data(), &head_size, 4);
    std::memcpy(data.data() + 4, &version, 4);
    std::memcpy(data.data() + 8, &vendor, 4);
    std::memcpy(data.data() + 12, &device, 4);
    std::memcpy(data.data() + 16, kUuid, VK_UUID_SIZE);
    data[31] = uuid_last;
    for (size_t i = 32; i < size; i++) data[i] = (uint8_t)(i * 7);
    return data;
}
static void write(const std::filesystem::path& path, const std::vector<uint8_t>& data) {
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    f.write(reinterpret_cast<const char*>(data.data()), (std::streamsize)data.size());
}

int main() {
    const std::filesystem::path dir = std::filesystem::temp_directory_path() / "hookshot-pcache-read-test";
    std::filesystem::remove_all(dir);
    std::filesystem::create_directories(dir);
    Device at{kVendor, kDevice};
    std::memcpy(at.uuid, kUuid, VK_UUID_SIZE);

    CHECK(pcache_read::read((dir / "missing.bin").string(), at).empty());   // (no file)
    write(dir / "empty.bin", {});
    CHECK(pcache_read::read((dir / "empty.bin").string(), at).empty());
    write(dir / "short.bin", std::vector<uint8_t>(31, 0x5a));
    CHECK(pcache_read::read((dir / "short.bin").string(), at).empty());     // (no whole header)
    write(dir / "header-only.bin", body(32));
    CHECK(pcache_read::read((dir / "header-only.bin").string(), at).empty());   // (nothing after the header)

    // (this device's: byte for byte, as Vulkan would have had it)
    const std::vector<uint8_t> good = body(4096);
    write(dir / "good.bin", good);
    CHECK(pcache_read::read((dir / "good.bin").string(), at) == good);

    // (another driver's, GPU's or device's: dropped)
    write(dir / "vendor.bin", body(4096, 32, VK_PIPELINE_CACHE_HEADER_VERSION_ONE, kVendor + 1));
    CHECK(pcache_read::read((dir / "vendor.bin").string(), at).empty());
    write(dir / "device.bin", body(4096, 32, VK_PIPELINE_CACHE_HEADER_VERSION_ONE, kVendor, kDevice + 1));
    CHECK(pcache_read::read((dir / "device.bin").string(), at).empty());
    write(dir / "uuid.bin", body(4096, 32, VK_PIPELINE_CACHE_HEADER_VERSION_ONE, kVendor, kDevice, 17));
    CHECK(pcache_read::read((dir / "uuid.bin").string(), at).empty());

    // (a header that does not describe the file: an old or foreign version, a size below the header, a size past the file)
    write(dir / "version.bin", body(4096, 32, VK_PIPELINE_CACHE_HEADER_VERSION_ONE + 1));
    CHECK(pcache_read::read((dir / "version.bin").string(), at).empty());
    write(dir / "small-header.bin", body(4096, 16));
    CHECK(pcache_read::read((dir / "small-header.bin").string(), at).empty());
    write(dir / "truncated.bin", body(100, 4096));
    CHECK(pcache_read::read((dir / "truncated.bin").string(), at).empty());

    // (a header size past the 32 bytes it must contain is fine when the file holds them)
    const std::vector<uint8_t> big_header = body(4096, 64);
    write(dir / "big-header.bin", big_header);
    CHECK(pcache_read::read((dir / "big-header.bin").string(), at) == big_header);

    std::filesystem::remove_all(dir);
    std::printf("PASS: a pipeline cache is dropped by its header before the rest is read\n");
    return 0;
}
