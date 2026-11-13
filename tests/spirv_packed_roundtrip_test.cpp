// Game-free round-trip check for the packed SPIR-V representation used by the program cache: several stages' SPIR-V
// words are packed into one contiguous buffer with a small header (count + per-stage offsets) and unpacked byte-for-byte.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>

static int failures = 0;
static void check(const char* name, bool v) { if (!v) { std::fprintf(stderr, "FAIL: %s\n", name); failures++; } }

// Packed layout: [count:u32][offset0:u32]...[offsetN:u32][size0:u32]...[sizeN:u32][words...]
// offsets are in words from the start of the words section; sizes in words.
struct Packed {
    std::vector<uint32_t> buf;
};

static Packed pack(const std::vector<std::vector<uint32_t>>& stages) {
    Packed p;
    const uint32_t n = (uint32_t)stages.size();
    p.buf.push_back(n);
    // placeholder offsets/sizes
    std::vector<uint32_t> off(n), sz(n);
    uint32_t cursor = 0;
    for (uint32_t i = 0; i < n; i++) { off[i] = cursor; sz[i] = (uint32_t)stages[i].size(); cursor += sz[i]; }
    for (uint32_t i = 0; i < n; i++) p.buf.push_back(off[i]);
    for (uint32_t i = 0; i < n; i++) p.buf.push_back(sz[i]);
    for (uint32_t i = 0; i < n; i++) for (uint32_t w : stages[i]) p.buf.push_back(w);
    return p;
}

static std::vector<std::vector<uint32_t>> unpack(const Packed& p) {
    std::vector<std::vector<uint32_t>> out;
    size_t i = 0;
    const uint32_t n = p.buf[i++];
    std::vector<uint32_t> off(n), sz(n);
    for (uint32_t k = 0; k < n; k++) off[k] = p.buf[i++];
    for (uint32_t k = 0; k < n; k++) sz[k] = p.buf[i++];
    const uint32_t words_base = (uint32_t)i;
    for (uint32_t k = 0; k < n; k++) {
        std::vector<uint32_t> stage;
        for (uint32_t w = 0; w < sz[k]; w++) stage.push_back(p.buf[words_base + off[k] + w]);
        out.push_back(stage);
    }
    return out;
}

int main() {
    // Build a few stages with distinct words.
    std::vector<std::vector<uint32_t>> stages;
    for (int s = 0; s < 3; s++) {
        std::vector<uint32_t> v;
        for (int w = 0; w < 5 + s * 7; w++) v.push_back((uint32_t)(0x1000 * (s + 1) + w));
        stages.push_back(v);
    }
    Packed p = pack(stages);
    auto rt = unpack(p);
    check("stage count round-trips", rt.size() == stages.size());
    for (size_t s = 0; s < stages.size(); s++) {
        check("stage size round-trips", rt[s].size() == stages[s].size());
        check("stage words round-trip", std::memcmp(rt[s].data(), stages[s].data(), stages[s].size() * 4) == 0);
    }
    // Empty stage list.
    std::vector<std::vector<uint32_t>> empty;
    auto rt2 = unpack(pack(empty));
    check("empty packs", rt2.empty());
    if (failures) return 1;
    std::printf("spirv packed round-trip: ok (%zu stages)\n", stages.size());
    return 0;
}
