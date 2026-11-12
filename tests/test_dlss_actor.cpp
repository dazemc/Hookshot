// gpu/vk/dlss_actor.h and shaders/dlss_actor.vert: draw names and index addresses, the bone-model layouts (four
// influences, two, one, and a rigid model on one bone), their weights, and the motion they give a vertex. Authored
// synthetic vertices and bone palettes: no game, dump or GPU.
#include "../gpu/vk/dlss_actor.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>

#define CHECK(name, condition) do { if (!(condition)) { std::fprintf(stderr, "FAIL %s at line %d\n", name, __LINE__); return 1; } } while (0)
#define CHECK_NEAR(name, a, b, tol) do { \
    const double _a = (a), _b = (b); \
    if (!(std::fabs(_a - _b) <= (tol) * std::max(1.0, std::fabs(_b)))) { \
        std::fprintf(stderr, "FAIL %s at line %d: %.9f, want %.9f\n", name, __LINE__, _a, _b); return 1; \
    } \
} while (0)

namespace da = dlss_actor;

float last_motion[2];

namespace {
constexpr uint32_t kStride = 64;
constexpr uint32_t kWidth = 1920, kHeight = 1080;

void put16(uint8_t* v, uint32_t at, uint16_t x) { std::memcpy(v + at, &x, 2); }

da::SkinLayout layout_of(const da::SkinAttributes& a) { return da::classify(a); }

// A bone palette as the game gives it: 3 x 4 row-major matrices, 12 floats a bone
void translation(float* palette, uint32_t bone, float x, float y, float z) {
    float* b = palette + bone * 12;
    std::memset(b, 0, 12 * sizeof *b);
    b[0] = b[5] = b[10] = 1.0f;
    b[3] = x, b[7] = y, b[11] = z;
}

struct Clip { float x, y, w; };
Clip project(const float m[16], const float p[3]) {
    return {m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3],
            m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7],
            m[12] * p[0] + m[13] * p[1] + m[14] * p[2] + m[15]};
}

// dlss_actor.vert's skin(): sum(w * B[j] * p), p.w = 1
void skin(const float* palette, uint32_t nbones, const uint32_t* joints, const float* weights, int n, const float p[3],
          float out[3]) {
    out[0] = out[1] = out[2] = 0.0f;
    for (int k = 0; k < n; k++) {
        if (weights[k] <= 0.0f || joints[k] >= nbones) continue;
        const float* b = palette + joints[k] * 12;
        out[0] += weights[k] * (b[0] * p[0] + b[1] * p[1] + b[2] * p[2] + b[3]);
        out[1] += weights[k] * (b[4] * p[0] + b[5] * p[1] + b[6] * p[2] + b[7]);
        out[2] += weights[k] * (b[8] * p[0] + b[9] * p[1] + b[10] * p[2] + b[11]);
    }
}

// The vertex shader's world position: every set's influences skinned, then the draw's origin added
void skinned(const uint8_t* v, const da::SkinLayout& k, const float palettes[2][64 * 12], uint32_t nbones,
             const float origins[2][3], const float p[3], float world[2][3]) {
    for (int frame = 0; frame < 2; frame++) {
        world[frame][0] = world[frame][1] = world[frame][2] = 0.0f;
        for (int set = 0; set < k.sets; set++) {
            uint32_t joints[4]{};
            float weights[4]{}, acc[3];
            const int n = da::influences(v, k, set, joints, weights);
            skin(palettes[frame], nbones, joints, weights, n, p, acc);
            for (int i = 0; i < 3; i++) world[frame][i] += acc[i];
        }
        for (int i = 0; i < 3; i++) world[frame][i] += origins[frame][i];
    }
}

// dlss_actor.vert's and .frag's motion in render pixels: towards the frame before, y down
bool motion(const float world[2][3], const float cameras[2][16]) {
    const Clip now = project(cameras[0], world[0]), then = project(cameras[1], world[1]);
    if (!(now.w > 1e-6f && then.w > 1e-6f)) return false;
    const float nx = now.x / now.w, ny = now.y / now.w, tx = then.x / then.w, ty = then.y / then.w;
    last_motion[0] = (tx - nx) * 0.5f * kWidth;
    last_motion[1] = (ny - ty) * 0.5f * kHeight;
    return true;
}

std::string source(const char* rel) {
    const std::filesystem::path here = std::filesystem::path(__FILE__).parent_path();
    for (const std::filesystem::path& base : {here.parent_path(), here, here.parent_path().parent_path()}) {
        std::ifstream f(base / rel, std::ios::binary);
        if (f) return {std::istreambuf_iterator<char>(f), std::istreambuf_iterator<char>()};
    }
    return {};
}
} // namespace

int main() {
    // names: the same draw keeps its name, another draw of the same model is the next one, another model another name
    const uint64_t a0 = da::key(0x1000, 0x2000, 9702, 0, 0xabc, 0), a1 = da::key(0x1000, 0x2000, 9702, 0, 0xabc, 1);
    CHECK("the same draw, the same name", a0 == da::key(0x1000, 0x2000, 9702, 0, 0xabc, 0));
    CHECK("the second draw of a model is another", a0 != a1 && da::same_model(a0, a1));
    CHECK("another model", !da::same_model(a0, da::key(0x1000, 0x2000, 9702, 3, 0xabc, 0)) &&
                               !da::same_model(a0, da::key(0x1040, 0x2000, 9702, 0, 0xabc, 0)) &&
                               !da::same_model(a0, da::key(0x1000, 0x2000, 9702, 0, 0xabd, 0)));
    CHECK("many draws of one model", da::same_model(a0, da::key(0x1000, 0x2000, 9702, 0, 0xabc, 900)));

    // where a draw's indices start for the shader, which reads 32-bit words: the address 4-aligned down, the first index in it
    uint64_t at = 0;
    uint32_t first = 0;
    da::index_start(0x10002, 1, &at, &first);
    CHECK("16-bit at +2: the second half of the word", at == 0x10000 && first == 1);
    da::index_start(0x10003, 0, &at, &first);
    CHECK("8-bit at +3: the fourth byte", at == 0x10000 && first == 3);
    da::index_start(0x10008, 2, &at, &first);
    CHECK("32-bit, aligned", at == 0x10008 && first == 0);
    da::index_start(0x10004, 1, &at, &first);
    CHECK("16-bit, aligned", at == 0x10004 && first == 0);

    // ---- the bone-model layouts: what vk_draw.cpp's skin_check sets from a draw's vertex formats ----
    {   // four influences, the layout the actor motion has always taken
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        const da::SkinLayout k = layout_of(a);
        CHECK("four influences: one set", k.sets == 1 && k.formats == 0);
        CHECK("four influences: its offsets", k.weights[0] == 4 && k.joints[0] == 8);
    }
    {   // two sets of four, 8-bit joints in the second
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        a.weights[1] = da::kRGBA8, a.joints[1] = da::kRGBA8UI, a.weight_offset[1] = 12, a.joint_offset[1] = 16;
        const da::SkinLayout k = layout_of(a);
        CHECK("two sets of four", k.sets == 2 && k.formats == (da::kByteJoints << 1));
        CHECK("two sets of four: the second's offsets", k.weights[1] == 12 && k.joints[1] == 16);
    }
    {   // a fifth bone on Link's draws: one influence in the second set, an R8 weight and an R8UI or R16UI joint
        for (int r8ui = 0; r8ui < 2; r8ui++) {
            da::SkinAttributes a;
            a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
            a.weights[1] = da::kR8, a.joints[1] = r8ui ? da::kR8UI : da::kR16UI;
            a.weight_offset[1] = 17, a.joint_offset[1] = 21;
            const da::SkinLayout k = layout_of(a);
            CHECK("a single second influence", k.sets == 2 && (k.formats & (da::kSingle << 1)));
            CHECK("a single second influence: not a pair or a quad", !(k.formats & (da::kPair << 1)) &&
                                                                     (k.formats & (da::kByteJoints << 1)) == (r8ui ? 4u : 0u));
            CHECK("a single second influence: its offsets", k.weights[1] == 17 && k.joints[1] == 21);
        }
    }
    {   // a rigid model on one bone: a joint and no weight attribute (Navi's body, props, held items)
        for (int r8ui = 0; r8ui < 2; r8ui++) {
            da::SkinAttributes a;
            a.joints[0] = r8ui ? da::kR8UI : da::kR16UI;
            a.joint_offset[0] = 20;
            a.weight_stream[0] = true;   // (no weight is read: its stream does not matter)
            const da::SkinLayout k = layout_of(a);
            CHECK("a rigid model on one bone", k.sets == 1 && (k.formats & (da::kSingle | da::kNoWeight)) ==
                                                                  (da::kSingle | da::kNoWeight));
            CHECK("a rigid model's joint format", (k.formats & da::kByteJoints) == (r8ui ? 2u : 0u));
            CHECK("a rigid model's offsets", k.weights[0] == 0 && k.joints[0] == 20);
        }
    }
    {   // two influences: RG8 weights and RG8UI joints (Navi's wings among them)
        da::SkinAttributes a;
        a.weights[0] = da::kRG8, a.joints[0] = da::kRG8UI, a.weight_offset[0] = 13, a.joint_offset[0] = 17;
        const da::SkinLayout k = layout_of(a);
        CHECK("two influences", k.sets == 1 && (k.formats & da::kPair) && (k.formats & da::kByteJoints));
        CHECK("two influences: its offsets", k.weights[0] == 13 && k.joints[0] == 17);
    }
    {   // a pair in the second set, after four influences in the first
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        a.weights[1] = da::kRG8, a.joints[1] = da::kRG8UI, a.weight_offset[1] = 12, a.joint_offset[1] = 16;
        const da::SkinLayout k = layout_of(a);
        CHECK("a pair in the second set", k.sets == 2 && (k.formats & (da::kPair << 1)) &&
                                               (k.formats & (da::kByteJoints << 1)));
        CHECK("a pair in the second set: its offsets", k.weights[1] == 12 && k.joints[1] == 16);
    }
    {   // what is not a skinned draw, or a set the shader cannot read
        da::SkinAttributes a;
        CHECK("no attributes: no skin", layout_of(a).sets == 0);
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 6, a.joint_offset[0] = 8;
        CHECK("four influences at a weight offset not 4-aligned", layout_of(a).sets == 0);
        a.weight_offset[0] = 4, a.joint_offset[0] = 10;
        CHECK("four influences at a joint offset not 4-aligned", layout_of(a).sets == 0);
        a.joint_offset[0] = 8, a.weight_stream[0] = true;
        CHECK("weights in another stream", layout_of(a).sets == 0);
        a.weight_stream[0] = false, a.joint_stream[0] = true;
        CHECK("joints in another stream", layout_of(a).sets == 0);
        a.joint_stream[0] = false, a.weights[0] = da::kRG8, a.joints[0] = da::kRGBA16UI;
        CHECK("a weight pair with four joints", layout_of(a).sets == 0);
        a.weights[0] = da::kR8, a.joints[0] = da::kR16UI;
        CHECK("a single influence in the first set is not a rigid model", layout_of(a).sets == 0);
        a.weights[0] = da::kNone, a.joints[0] = da::kRGBA16UI;
        CHECK("a joint without a weight that is not one bone", layout_of(a).sets == 0);
        a.joints[0] = da::kRGBA16UI, a.joint_stream[0] = true;
        CHECK("a rigid model's joint in another stream", layout_of(a).sets == 0);
        da::SkinAttributes other;
        other.weights[0] = da::kOther, other.joints[0] = da::kR8UI;
        CHECK("a weight format the layouts do not take", layout_of(other).sets == 0);
        other.weights[0] = da::kRGBA8, other.joints[0] = da::kOther;
        CHECK("a joint format the layouts do not take", layout_of(other).sets == 0);
    }

    // ---- weights: a byte / 255, one, two or four influences, a set without weights weighing 1 ----
    uint8_t v[kStride];
    std::memset(v, 0, sizeof v);
    {   // four influences
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        const da::SkinLayout k = layout_of(a);
        v[4] = 255, v[5] = 0, v[6] = 128, v[7] = 1;
        put16(v, 8, 3), put16(v, 10, 9), put16(v, 12, 7), put16(v, 14, 11);
        uint32_t joints[4]{};
        float weights[4]{};
        CHECK("four influences read", da::influences(v, k, 0, joints, weights) == 4);
        CHECK_NEAR("a full weight", weights[0], 1.0, 1e-6);
        CHECK_NEAR("a zero weight", weights[1], 0.0, 0.0);
        CHECK_NEAR("128 is 128/255, not 1", weights[2], 128.0 / 255.0, 1e-6);
        CHECK_NEAR("the least weight", weights[3], 1.0 / 255.0, 1e-6);
        CHECK("four joints", joints[0] == 3 && joints[1] == 9 && joints[2] == 7 && joints[3] == 11);
    }
    {   // one influence in a second set, at an odd byte offset: its weight alone, not the bytes after it
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        a.weights[1] = da::kR8, a.joints[1] = da::kR16UI, a.weight_offset[1] = 17, a.joint_offset[1] = 23;
        const da::SkinLayout k = layout_of(a);
        v[17] = 200, v[18] = 99, v[19] = 99;
        put16(v, 23, 7);
        uint32_t joints[4]{};
        float weights[4]{};
        CHECK("one second influence read", da::influences(v, k, 1, joints, weights) == 1);
        CHECK_NEAR("its weight", weights[0], 200.0 / 255.0, 1e-6);
        CHECK("its joint at an odd offset", joints[0] == 7);
        CHECK("nothing after it", weights[1] == 0.0f && joints[1] == 0);
    }
    {   // a pair
        da::SkinAttributes a;
        a.weights[0] = da::kRG8, a.joints[0] = da::kRG8UI, a.weight_offset[0] = 12, a.joint_offset[0] = 16;
        const da::SkinLayout k = layout_of(a);
        v[12] = 51, v[13] = 204, v[14] = 99;
        v[16] = 3, v[17] = 4, v[18] = 99;
        uint32_t joints[4]{};
        float weights[4]{};
        CHECK("two influences read", da::influences(v, k, 0, joints, weights) == 2);
        CHECK_NEAR("the pair's first weight", weights[0], 51.0 / 255.0, 1e-6);
        CHECK_NEAR("the pair's second weight", weights[1], 204.0 / 255.0, 1e-6);
        CHECK("the pair's joints", joints[0] == 3 && joints[1] == 4);
        CHECK("nothing after the pair", weights[2] == 0.0f && joints[2] == 0);
    }
    {   // a rigid model on one bone: no weight byte is read, its joint weighs 1
        da::SkinAttributes a;
        a.joints[0] = da::kR8UI, a.joint_offset[0] = 20;
        const da::SkinLayout k = layout_of(a);
        v[0] = 0;   // (where a weight would have been, if there were one)
        v[20] = 9, v[21] = 99;
        uint32_t joints[4]{};
        float weights[4]{};
        CHECK("a rigid model reads one joint", da::influences(v, k, 0, joints, weights) == 1);
        CHECK_NEAR("its joint weighs 1", weights[0], 1.0, 0.0);
        CHECK("its joint", joints[0] == 9);
        CHECK("nothing after it", weights[1] == 0.0f && joints[1] == 0);
    }

    // ---- bones_used: the palette a draw needs (the largest joint with a weight, plus one) ----
    {   // four influences over two vertices, one vertex's zero weight skipped
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        const da::SkinLayout k = layout_of(a);
        uint8_t w[2 * kStride];
        std::memset(w, 0, sizeof w);
        w[4] = 255, w[5] = 0, w[6] = 0, w[7] = 0;
        put16(w, 8, 2), put16(w, 10, 30);
        w[kStride + 4] = 0, w[kStride + 5] = 255;
        put16(w + kStride, 8, 40), put16(w + kStride, 10, 5);
        CHECK("four influences: a weighted joint, not a zero-weighted one", da::bones_used(w, 2, kStride, k) == 6);
    }
    {   // one influence in the second set: only its first joint counts
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        a.weights[1] = da::kR8, a.joints[1] = da::kR16UI, a.weight_offset[1] = 17, a.joint_offset[1] = 23;
        const da::SkinLayout k = layout_of(a);
        uint8_t w[kStride];
        std::memset(w, 0, sizeof w);
        w[4] = 255;
        put16(w, 8, 1);
        w[17] = 255;
        put16(w, 23, 5), put16(w, 25, 50);
        CHECK("one second influence: its joint, not the one after", da::bones_used(w, 1, kStride, k) == 6);
    }
    {   // a pair: only its two joints count
        da::SkinAttributes a;
        a.weights[0] = da::kRG8, a.joints[0] = da::kRG8UI, a.weight_offset[0] = 12, a.joint_offset[0] = 16;
        const da::SkinLayout k = layout_of(a);
        uint8_t w[kStride];
        std::memset(w, 0, sizeof w);
        w[12] = 255, w[13] = 255;
        w[16] = 3, w[17] = 4, w[18] = 60;
        CHECK("two influences: two joints, not a third byte", da::bones_used(w, 1, kStride, k) == 5);
    }
    {   // a rigid model: its one joint counts without a weight
        da::SkinAttributes a;
        a.joints[0] = da::kR16UI, a.joint_offset[0] = 20;
        const da::SkinLayout k = layout_of(a);
        uint8_t w[kStride];
        std::memset(w, 0, sizeof w);
        put16(w, 20, 12), put16(w, 22, 60);
        CHECK("a rigid model: its joint counts", da::bones_used(w, 1, kStride, k) == 13);
    }
    {   // a pair and a single, two sets, and a vertex with no influence
        da::SkinAttributes a;
        a.weights[0] = da::kRG8, a.joints[0] = da::kRG8UI, a.weight_offset[0] = 12, a.joint_offset[0] = 16;
        a.weights[1] = da::kR8, a.joints[1] = da::kR8UI, a.weight_offset[1] = 20, a.joint_offset[1] = 21;
        const da::SkinLayout k = layout_of(a);
        uint8_t w[2 * kStride];
        std::memset(w, 0, sizeof w);
        w[12] = 255, w[13] = 0;
        w[16] = 2, w[17] = 40;
        w[20] = 255;
        w[21] = 7, w[22] = 60;
        CHECK("two sets: the largest weighted joint", da::bones_used(w, 2, kStride, k) == 8);
    }

    // ---- motion: authored palettes and cameras, every layout ----
    float palettes[2][64 * 12];
    std::memset(palettes, 0, sizeof palettes);
    float cameras[2][16];
    std::memset(cameras, 0, sizeof cameras);
    for (int i = 0; i < 2; i++) {
        cameras[i][0] = cameras[i][5] = cameras[i][10] = cameras[i][15] = 1.0f;
        for (uint32_t b = 0; b < 64; b++) translation(palettes[i], b, 0.0f, 0.0f, 0.0f);
    }
    const float p[3] = {0.0f, 0.0f, -2.0f};   // (in front of the camera, w = 1)
    const float no_origin[2][3] = {{0.0f, 0.0f, 0.0f}, {0.0f, 0.0f, 0.0f}};
    float world[2][3];

    {   // four influences, one bone: unchanged from the path before the other layouts
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        const da::SkinLayout k = layout_of(a);
        std::memset(v, 0, sizeof v);
        v[4] = 255;
        put16(v, 8, 5);
        translation(palettes[0], 5, 1.0f, 0.0f, 0.0f);
        skinned(v, k, palettes, 64, no_origin, p, world);
        CHECK("the vertex is in front of both cameras", motion(world, cameras));
        CHECK_NEAR("four influences: its bone's motion", last_motion[0], -0.5 * kWidth, 1e-5);
        CHECK_NEAR("four influences: no vertical motion", last_motion[1], 0.0, 0.0);
    }
    {   // one influence in the second set, added to the first set's four
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        a.weights[1] = da::kR8, a.joints[1] = da::kR16UI, a.weight_offset[1] = 17, a.joint_offset[1] = 23;
        const da::SkinLayout k = layout_of(a);
        std::memset(v, 0, sizeof v);
        v[4] = 255;
        put16(v, 8, 0);
        v[17] = 128;
        put16(v, 23, 1);
        translation(palettes[0], 0, 1.0f, 0.0f, 0.0f);
        translation(palettes[0], 1, 2.0f, 0.0f, 0.0f);
        skinned(v, k, palettes, 64, no_origin, p, world);
        CHECK("the vertex is in front of both cameras", motion(world, cameras));
        const float moved = 1.0f + (128.0f / 255.0f) * 2.0f;
        CHECK_NEAR("a single second influence moves its share", last_motion[0], -moved * 0.5 * kWidth, 1e-5);
    }
    {   // two influences: RG8 weights, RG8UI joints
        da::SkinAttributes a;
        a.weights[0] = da::kRG8, a.joints[0] = da::kRG8UI, a.weight_offset[0] = 12, a.joint_offset[0] = 16;
        const da::SkinLayout k = layout_of(a);
        std::memset(v, 0, sizeof v);
        v[12] = 51, v[13] = 204;
        v[16] = 3, v[17] = 4;
        translation(palettes[0], 3, 2.0f, 0.0f, 0.0f);
        translation(palettes[0], 4, -1.0f, 0.0f, 0.0f);
        skinned(v, k, palettes, 64, no_origin, p, world);
        CHECK("the vertex is in front of both cameras", motion(world, cameras));
        CHECK_NEAR("two influences blend their bones", last_motion[0], -(0.2 * 2.0 - 0.8) * 0.5 * kWidth, 1e-5);
    }
    {   // a rigid model on one bone: its joint weighs 1, whatever byte is where a weight would be
        da::SkinAttributes a;
        a.joints[0] = da::kR16UI, a.joint_offset[0] = 20;
        const da::SkinLayout k = layout_of(a);
        std::memset(v, 0, sizeof v);
        put16(v, 20, 7);
        translation(palettes[0], 7, 1.0f, 1.0f, 0.0f);
        skinned(v, k, palettes, 64, no_origin, p, world);
        CHECK("the vertex is in front of both cameras", motion(world, cameras));
        CHECK_NEAR("a rigid model moves by its bone", last_motion[0], -0.5 * kWidth, 1e-5);
        CHECK_NEAR("a rigid model moves by its bone: y towards the frame before", last_motion[1], 0.5 * kHeight, 1e-5);
    }
    {   // a rigid model's origin moves too
        da::SkinAttributes a;
        a.joints[0] = da::kR8UI, a.joint_offset[0] = 20;
        const da::SkinLayout k = layout_of(a);
        std::memset(v, 0, sizeof v);
        v[20] = 20;   // (a bone this fixture has not moved)
        float origins[2][3] = {{1.0f, 0.0f, 0.0f}, {0.0f, 0.0f, 0.0f}};
        skinned(v, k, palettes, 64, origins, p, world);
        CHECK("the vertex is in front of both cameras", motion(world, cameras));
        CHECK_NEAR("a rigid model's origin moves", last_motion[0], -0.5 * kWidth, 1e-5);
    }
    {   // a vertex with two sets: the pair's and the single's bones add
        da::SkinAttributes a;
        a.weights[0] = da::kRGBA8, a.joints[0] = da::kRGBA16UI, a.weight_offset[0] = 4, a.joint_offset[0] = 8;
        a.weights[1] = da::kRG8, a.joints[1] = da::kRG8UI, a.weight_offset[1] = 12, a.joint_offset[1] = 16;
        const da::SkinLayout k = layout_of(a);
        std::memset(v, 0, sizeof v);
        v[4] = 255;
        put16(v, 8, 0);
        v[12] = 255, v[13] = 0;
        v[16] = 1, v[17] = 2;
        translation(palettes[0], 0, 1.0f, 0.0f, 0.0f);
        translation(palettes[0], 1, 0.0f, 2.0f, 0.0f);
        translation(palettes[0], 2, 0.0f, 4.0f, 0.0f);
        skinned(v, k, palettes, 64, no_origin, p, world);
        CHECK("the vertex is in front of both cameras", motion(world, cameras));
        CHECK_NEAR("two sets add: x", last_motion[0], -0.5 * kWidth, 1e-5);
        CHECK_NEAR("two sets add: y", last_motion[1], 2.0 * 0.5 * kHeight, 1e-5);
    }
    {   // the frame before's palette, not this frame's, gives the motion: a bone still last frame and moved now
        da::SkinAttributes a;
        a.joints[0] = da::kR16UI, a.joint_offset[0] = 20;
        const da::SkinLayout k = layout_of(a);
        std::memset(v, 0, sizeof v);
        put16(v, 20, 9);
        translation(palettes[1], 9, -1.0f, 0.0f, 0.0f);
        skinned(v, k, palettes, 64, no_origin, p, world);
        CHECK("the vertex is in front of both cameras", motion(world, cameras));
        CHECK_NEAR("the frame before's pose", last_motion[0], -0.5 * kWidth, 1e-5);
    }

    // ---- the shaders read these layouts: the source's own words (the header beside each shader is glslang's) ----
    const std::string vert = source("gpu/vk/shaders/dlss_actor.vert");
    CHECK("the vertex shader is here", !vert.empty());
    CHECK("the shader reads one, two or four influences", vert.find("8u << s") != std::string::npos &&
                                                                 vert.find("32u << s") != std::string::npos &&
                                                                 vert.find("128u << s") != std::string::npos);
    CHECK("the shader normalizes a byte weight", vert.find("/ 255.0") != std::string::npos);
    CHECK("the shader weighs a rigid joint 1", vert.find("no_weight ? 1.0") != std::string::npos);
    CHECK("the shader reads a joint at any byte offset", vert.find("byte_at(vb, pc.joints[s] + k)") != std::string::npos &&
                                                             vert.find("short_at(vb, pc.joints[s] + 2u * k)") != std::string::npos);
    const std::string draw = source("gpu/vk/vk_draw.cpp");
    CHECK("skin_check classifies the draw's formats", draw.find("dlss_actor::classify") != std::string::npos);
    const std::string temporal = source("gpu/vk/vk_temporal.cpp");
    CHECK("bones_used reads the classified layout", temporal.find("dlss_actor::bones_used") != std::string::npos);

    std::puts("dlss_actor: ok");
    return 0;
}
