// (hookshot) The graphics pipeline state a mesh fallback draw needs (vk_draw.cpp build_graphics and draw).
// tests/fallback_pipeline_state_test.cpp
//
// The fallback's generated vertex stage reads the records by vertex index and binds no vertex buffers, so its pipeline
// must not require VK_DYNAMIC_STATE_VERTEX_INPUT_BINDING_STRIDE (VUID-vkCmdDrawIndirect-pStrides-04913). It still uses
// dynamic primitive topology and primitive restart. Ordinary vertex-input pipelines use all three; native mesh
// pipelines have no vertex input at all.
//
// Binding a fallback pipeline makes its vertex input static, which invalidates the dynamic strides set before it even
// though the fallback binds no vertex buffers. Both vertex-bind skip caches must then be dropped: an ordinary draw that
// follows with the same buffers would otherwise keep its old strides. Buffer handles, offsets and sizes are not
// dynamic state, and the resolved-buffer lookup cache stays valid.
#pragma once

namespace fallback_pipeline_state {

/// The pipeline's vertex input is static for a fallback program: it reads records, not vertex streams.
inline bool dynamic_vertex_stride(bool mesh, bool fallback) { return !mesh && !fallback; }

/// Primitive topology and primitive restart are dynamic for ordinary and fallback draws; native mesh sets them itself.
inline bool dynamic_input_assembly(bool mesh) { return !mesh; }

/// After binding a graphics pipeline: a fallback bind invalidates the dynamic vertex strides, so the next ordinary
/// draw must bind them again even with unchanged buffers. Native mesh and ordinary binds leave them valid.
inline void after_graphics_bind(bool fallback, bool& last_vb_valid, bool& vertex_set_on) {
    if (!fallback) return;
    last_vb_valid = false;
    vertex_set_on = false;
}

} // namespace fallback_pipeline_state
