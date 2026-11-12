// (hookshot) NVIDIA DLSS's actor motion (vk_dlss.cpp): what is worked out on the CPU for a skinned draw drawn again, kept
// apart so tests/test_dlss_actor.cpp checks it without a GPU. Header only, no Vulkan.
#pragma once

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <vector>

namespace dlss_actor {

/// A draw's name across frames: its model (vertex and index buffers, count, base vertex, program) and which of the frame's
/// draws of that model it is (the same model drawn twice: the n-th draw of it is matched with the n-th the frame before)
inline uint64_t key(uint64_t vbuf, uint64_t ibuf, int count, int base_vertex, uint64_t program, int nth) {
    uint64_t h = 0x5bd1e995;
    for (uint64_t v : {vbuf, ibuf, (uint64_t)(uint32_t)count, (uint64_t)(uint32_t)base_vertex, program}) {
        h ^= v + 0x9e3779b97f4a7c15ull + (h << 6) + (h >> 2);
        h *= 0xff51afd7ed558ccdull;
    }
    return (h & ~0xffull) | (uint64_t)std::clamp(nth, 0, 255);
}
inline bool same_model(uint64_t a, uint64_t b) { return a >> 8 == b >> 8; }

/// shaders/dlss_actor.vert's push constants (the fragment stage's the same)
struct Push {
    uint64_t vertices;   // (the vertex stream's device address)
    uint64_t indices;    // (the index buffer's, 4-aligned down: first says where the draw's indices start in it)
    uint64_t origins[2]; // (the draw's origin, xyz: this frame's and the frame before's, copied into the pose buffers)
    uint64_t bones[2];   // (its palettes: 3 x 4 row-major matrices, 12 floats a bone)
    uint32_t nbones;
    uint32_t stride;     // (in words)
    uint32_t count;      // (the vertices in the stream: an index past them draws nothing)
    uint32_t first;      // (in indices, from the address)
    uint32_t itype;      // (0: 8-bit, 1: 16-bit, 2: 32-bit indices)
    int32_t base_vertex;
    uint32_t pos, sets;  // (bytes in a vertex: the position; the joint and weight sets, 1 or 2)
    uint32_t joints[2], weights[2];
    uint32_t formats;    // (kHalfPosition, kByteJoints << set)
    uint32_t tolerance;  // (depth steps from the scene's within which a pixel is the draw's)
};
static_assert(sizeof(Push) == 6 * 8 + 14 * 4 && sizeof(Push) <= 128, "dlss_actor.vert's push constants");

/// Where a draw's indices are for the shader: the address 4-aligned down, and its first index from there
inline void index_start(uint64_t address, int itype, uint64_t* aligned, uint32_t* first) {
    *aligned = address & ~3ull;
    *first = (uint32_t)((address & 3) >> (itype < 0 ? 0 : itype > 2 ? 2 : itype));
}

/// Push::formats: the position is RGBA16F (else RGB32F); a set's joints are RGBA8UI (else RGBA16UI)
constexpr uint32_t kHalfPosition = 1, kByteJoints = 2;
/// Push::formats: set n is a single influence (an R8 weight; its joint R8UI with kByteJoints << n, else R16UI)
constexpr uint32_t kSingle = 8;
/// Push::formats: set n has two influences (an RG8 weight pair and an RG8UI joint pair, any byte offset)
constexpr uint32_t kPair = 32;
/// Push::formats: set n has no weights, its one joint weighs 1 (rigid models bound to one bone: Navi's body,
/// props, held items; with kSingle)
constexpr uint32_t kNoWeight = 128;

/// A joint or weight attribute's format, as vk_draw.cpp's skin_check maps NVN's to it (kNone: no attribute)
enum AttrFormat : uint8_t {
    kNone = 0,
    kR8,       // (an R8 weight: one influence)
    kR8UI,     // (an R8UI joint)
    kR16UI,    // (an R16UI joint)
    kRG8,      // (an RG8 weight pair)
    kRG8UI,    // (an RG8UI joint pair)
    kRGBA8,    // (four weights)
    kRGBA8UI,  // (four 8-bit joints)
    kRGBA16UI, // (four 16-bit joints)
    kOther,    // (a format the layouts do not take)
};

/// A draw's joint and weight attributes, as skin_check reads them from its vertex attributes: each set's formats, their byte
/// offsets in a vertex, and whether they are in another stream (only stream 0's are taken)
struct SkinAttributes {
    AttrFormat weights[2]{kNone, kNone}, joints[2]{kNone, kNone};
    uint32_t weight_offset[2]{}, joint_offset[2]{};
    bool weight_stream[2]{}, joint_stream[2]{};
};

/// What the sets are, from their attributes: Push::formats's bits, the sets' byte offsets, and how many sets (0: not a
/// skinned draw). A set is four influences (RGBA8 weights, RGBA8UI or RGBA16UI joints, 4-aligned), two (kPair: RG8
/// weights and RG8UI joints), one (kSingle: an R8 weight and an R8UI or R16UI joint), or a rigid model on one bone
/// (kSingle | kNoWeight: a joint and no weight attribute)
struct SkinLayout {
    uint32_t formats = 0;
    uint32_t weights[2]{}, joints[2]{};
    int sets = 0;
};
inline SkinLayout classify(const SkinAttributes& a) {
    SkinLayout k;
    for (int set = 0; set < 2; set++) {
        const AttrFormat w = a.weights[set], j = a.joints[set];
        const bool w_other = a.weight_stream[set], j_other = a.joint_stream[set];
        // (a second set of one influence: an R8 weight and an R8UI or R16UI joint; Link's draws' fifth bone)
        if (set == 1 && w == kR8 && (j == kR8UI || j == kR16UI) && !w_other && !j_other) {
            k.formats |= kSingle << set;
            if (j == kR8UI) k.formats |= kByteJoints << set;
            k.weights[set] = a.weight_offset[set], k.joints[set] = a.joint_offset[set], k.sets = set + 1;
            break;
        }
        // (a rigid model on one bone: a joint and no weight; and two-bone skinning, RG8 weights with RG8UI joints)
        if (set == 0 && w == kNone && (j == kR8UI || j == kR16UI) && !j_other) {
            k.formats |= (kSingle | kNoWeight) << set;
            if (j == kR8UI) k.formats |= kByteJoints << set;
            k.weights[set] = 0, k.joints[set] = a.joint_offset[set], k.sets = set + 1;
            break;
        }
        if (w == kRG8 && j == kRG8UI && !w_other && !j_other) {
            k.formats |= (kPair | kByteJoints) << set;
            k.weights[set] = a.weight_offset[set], k.joints[set] = a.joint_offset[set], k.sets = set + 1;
            continue;
        }
        if (w != kRGBA8 || (j != kRGBA16UI && j != kRGBA8UI) || w_other || j_other ||
            a.weight_offset[set] % 4 || a.joint_offset[set] % 4)
            break;
        if (j == kRGBA8UI) k.formats |= kByteJoints << set;
        k.weights[set] = a.weight_offset[set], k.joints[set] = a.joint_offset[set], k.sets = set + 1;
    }
    return k;
}

/// A vertex's influences for one set, read at the layout's byte offsets (the shader's dlss_actor.vert loop): its joints and
/// weights (a byte / 255, or 1 for a set without weights). At most four; returns how many.
inline int influences(const uint8_t* vertex, const SkinLayout& k, int set, uint32_t* joints, float* weights) {
    const int n = (k.formats & (kSingle << set)) ? 1 : (k.formats & (kPair << set)) ? 2 : 4;
    const bool byte_joints = (k.formats & (kByteJoints << set)) != 0, no_weight = (k.formats & (kNoWeight << set)) != 0;
    for (int i = 0; i < n; i++) {
        if (byte_joints) joints[i] = vertex[k.joints[set] + i];
        else {
            uint16_t j = 0;
            std::memcpy(&j, vertex + k.joints[set] + 2 * i, 2);
            joints[i] = j;
        }
        weights[i] = no_weight ? 1.0f : (float)vertex[k.weights[set] + i] / 255.0f;
    }
    return n;
}

/// The bones a draw's vertices use (the largest joint with a weight, plus one) over `count` vertices of `stride` bytes; 0
/// when none. The shader reads one, two or four joints a set, and a set without weights counts its joint
inline uint32_t bones_used(const uint8_t* vertices, std::size_t count, uint32_t stride, const SkinLayout& k) {
    uint32_t most = 0;
    for (std::size_t i = 0; i < count; i++) {
        const uint8_t* q = vertices + i * stride;
        for (int set = 0; set < k.sets; set++) {
            uint32_t joints[4]{};
            float weights[4]{};
            const int n = influences(q, k, set, joints, weights);
            for (int j = 0; j < n; j++)
                if (weights[j] > 0.0f) most = std::max(most, joints[j] + 1);
        }
    }
    return most;
}

/// The cameras' buffer, 36 words: this frame's camera (as drawn, jittered), the frame before's (its jitter taken off), this
/// frame's jitter in clip space, the render size
constexpr uint32_t kHeadWords = 36;

} // namespace dlss_actor
