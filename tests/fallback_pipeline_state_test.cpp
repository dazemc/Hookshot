/* fallback_pipeline_state.h: the mesh fallback's graphics pipeline state and the vertex-stride caches a fallback bind
   invalidates. No game, no GPU: the Vulkan state lifetime is modeled. */
#include "../gpu/vk/fallback_pipeline_state.h"

#include <cstdio>
#include <cstdlib>
#include <vector>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

namespace state = fallback_pipeline_state;

// What build_graphics pushes for a program: native mesh, ordinary vertex input, or the compute fallback.
static std::vector<int> dynamic_states(bool mesh, bool fallback) {
    std::vector<int> dyn;
    if (state::dynamic_input_assembly(mesh)) {
        dyn.push_back(1);   // VK_DYNAMIC_STATE_PRIMITIVE_TOPOLOGY
        dyn.push_back(2);   // VK_DYNAMIC_STATE_PRIMITIVE_RESTART_ENABLE
        if (state::dynamic_vertex_stride(mesh, fallback)) dyn.push_back(3);   // VK_DYNAMIC_STATE_VERTEX_INPUT_BINDING_STRIDE
    }
    return dyn;
}

static bool has(const std::vector<int>& v, int x) {
    for (int y : v)
        if (y == x) return true;
    return false;
}

// The draw's vertex-bind decision, after the pipeline bind. last_vb_valid is the whole-set bind cache; vertex_set.on is
// the guest-stream cache. The buffers here never change, so only a bind cache that was dropped asks for a rebind.
struct DrawState {
    bool last_vb_valid = false, vertex_set_on = false;
    void bind_pipeline(bool fallback) { state::after_graphics_bind(fallback, last_vb_valid, vertex_set_on); }
    // an ordinary draw: binds the whole set when its cache was dropped, then marks both caches valid
    bool ordinary_draw() {
        const bool rebound = !last_vb_valid;
        last_vb_valid = true, vertex_set_on = true;
        return rebound;
    }
};

int main() {
    // ordinary: dynamic topology, restart and binding stride
    const auto ordinary = dynamic_states(false, false);
    CHECK(has(ordinary, 1) && has(ordinary, 2) && has(ordinary, 3));
    // fallback: dynamic topology and restart, but no dynamic stride (VUID-vkCmdDrawIndirect-pStrides-04913)
    const auto fallback = dynamic_states(false, true);
    CHECK(has(fallback, 1) && has(fallback, 2) && !has(fallback, 3));
    // native mesh: none of the three (its pipeline holds them, and it has no vertex input)
    const auto mesh = dynamic_states(true, false);
    CHECK(mesh.empty());

    // ordinary -> fallback -> ordinary, same buffers: the fallback bind drops both caches, so the third draw rebinds
    DrawState d;
    CHECK(d.ordinary_draw());   // first bind
    CHECK(!d.ordinary_draw());  // same buffers: the caches stand
    d.bind_pipeline(false);
    CHECK(d.last_vb_valid && d.vertex_set_on);   // an ordinary bind changes no dynamic stride
    CHECK(!d.ordinary_draw());
    d.bind_pipeline(true);
    CHECK(!d.last_vb_valid && !d.vertex_set_on);   // the fallback's static stride invalidated them
    CHECK(d.ordinary_draw());                      // the next ordinary draw restores its strides

    // a native mesh bind between ordinary draws does not disturb the dynamic strides
    d.bind_pipeline(false);
    CHECK(!d.ordinary_draw());
    d.bind_pipeline(false);
    CHECK(d.last_vb_valid && d.vertex_set_on);
    CHECK(!d.ordinary_draw());

    // a fallback bind with no ordinary draw after it leaves both dropped; a second fallback bind keeps them dropped
    d.bind_pipeline(true);
    CHECK(!d.last_vb_valid && !d.vertex_set_on);
    d.bind_pipeline(true);
    CHECK(!d.last_vb_valid && !d.vertex_set_on);
    CHECK(d.ordinary_draw());

    std::printf("PASS: fallback draws need no dynamic vertex stride, and their bind restores ordinary strides\n");
    return 0;
}
