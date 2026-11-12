// (hookshot) The temporal chain's motion input: the bones a skinned draw uses, read from its own vertex and index streams
// (vk_temporal.cpp bones_used). builder/rust/tests/fsr3_hdr.rs cuts the function out of the renderer's source and compiles
// it with these game-free buffers: four influences, two (kPair), one (kSingle) and a rigid model on one bone
// (kSingle | kNoWeight), as vk_draw.cpp's skin check now recognises them. No game, dump or GPU.
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

#include "gpu/vk/dlss_actor.h"

struct SkinnedDraw {
    uint64_t vbuf = 0, vsize = 0;
    uint32_t stride = 0;
    uint32_t joints[2]{}, weights[2]{};
    int sets = 0;
    uint32_t formats = 0;
    uint64_t ibuf = 0;
    int itype = 0, count = 0, base_vertex = 0;
};

static constexpr uint64_t kIndexAddress = 0x1000, kVertexAddress = 0x2000;
static std::vector<uint8_t> indices, vertices;

bool memory_read_as_submitted(uint64_t gpu, void* dst, uint64_t n) {
    const bool vertex = gpu >= kVertexAddress;
    const std::vector<uint8_t>& source = vertex ? vertices : indices;
    const uint64_t at = gpu - (vertex ? kVertexAddress : kIndexAddress);
    if (at + n > source.size()) return false;
    std::memcpy(dst, source.data() + at, (size_t)n);
    return true;
}

/* BONES_USED */

static int failures = 0;
static void check(const char* name, bool value) {
    if (!value) {
        std::fprintf(stderr, "FAIL: %s\n", name);
        failures++;
    }
}

struct Set {
    uint32_t joint = 0, weight = 0;
    uint8_t j[4]{}, w[4]{};
};

/// Four vertices, each 16 bytes: every set's joint and weight bytes at its own offsets
static void setup(const Set* sets, int count) {
    indices.assign(8, 0);
    for (int i = 0; i < 4; i++) {
        indices[(size_t)i * 2] = (uint8_t)i;
        indices[(size_t)i * 2 + 1] = 0;
    }
    vertices.assign(4 * 16, 0);
    for (int i = 0; i < 4; i++) {
        uint8_t* v = vertices.data() + (size_t)i * 16;
        for (int s = 0; s < count; s++)
            for (int k = 0; k < 4; k++) {
                v[sets[s].joint + (size_t)k] = sets[s].j[k];
                v[sets[s].weight + (size_t)k] = sets[s].w[k];
            }
    }
}

static uint32_t used(uint32_t formats, int set_count, const uint32_t joints[2], const uint32_t weights[2]) {
    SkinnedDraw k;
    k.vbuf = kVertexAddress, k.vsize = vertices.size(), k.stride = 16;
    k.ibuf = kIndexAddress, k.itype = 1, k.count = 4, k.base_vertex = 0;
    k.formats = formats, k.sets = set_count;
    for (int i = 0; i < 2; i++) k.joints[i] = joints[i], k.weights[i] = weights[i];
    return bones_used(k);
}

int main() {
    const uint32_t joints[2] = {0, 8}, weights[2] = {4, 12};
    {   // four influences, RGBA8UI joints and RGBA8 weights
        const Set set{0, 4, {3, 1, 2, 0}, {255, 128, 0, 64}};
        setup(&set, 1);
        check("four influences: the largest joint with a weight",
              used(dlss_actor::kByteJoints, 1, joints, weights) == 4);
    }
    {   // two influences (kPair): RG8 weights and RG8UI joints
        const Set set{0, 4, {4, 7, 0xff, 0xff}, {255, 1, 0xff, 0xff}};
        setup(&set, 1);
        check("two influences: both joints read", used(dlss_actor::kPair | dlss_actor::kByteJoints, 1, joints, weights) == 8);
    }
    {   // one influence in a second set (kSingle): R8 weight, R8UI joint
        const uint32_t two_joints[2] = {0, 8}, two_weights[2] = {4, 12};
        const Set sets[2] = {{0, 4, {0, 0, 0, 0}, {255, 0, 0, 0}}, {8, 12, {20, 0, 0, 0}, {255, 0, 0, 0}}};
        setup(sets, 2);
        check("one influence in a second set: its joint is read",
              used(dlss_actor::kSingle << 1 | dlss_actor::kByteJoints << 1, 2, two_joints, two_weights) == 21);
    }
    {   // a rigid model on one bone (kSingle | kNoWeight): a joint and no weight, which weighs 1
        const Set set{0, 4, {11, 0xff, 0xff, 0xff}, {0, 0xff, 0xff, 0xff}};
        setup(&set, 1);
        check("a rigid model's joint counts though its weight is 0",
              used(dlss_actor::kSingle | dlss_actor::kNoWeight | dlss_actor::kByteJoints, 1, joints, weights) == 12);
    }
    {   // a zero weight in a four-influence set is not a bone (unless kNoWeight says so)
        const Set set{0, 4, {9, 8, 7, 6}, {0, 0, 0, 0}};
        setup(&set, 1);
        check("unweighted four-influence joints do not count", used(0, 1, joints, weights) == 0);
    }
    if (failures) return 1;
    std::printf("fsr3 motion: all checks passed\n");
    return 0;
}
