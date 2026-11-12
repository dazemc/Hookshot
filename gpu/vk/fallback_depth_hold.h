// A mesh fallback's depth-only draws held while the colour draw of the same culled list waits for its build (vk_draw.cpp;
// tests/fallback_depth_hold_test.cpp).
//
// Without mesh shaders, grass and foliage go through the mesh fallback. Its colour programs take long to build (minutes each on
// an Iris Xe), and until they are ready their draws are left out. The depth pass's programs for the same objects are small and
// already built, so the depth pass still draws: depth with no colour lets the sky show through the grass, which bloom blows up
// into specks. A depth-only draw is held while its list's colour draw is, and for up to 120 frames after its last wait, so the
// objects are missing while they build instead of being holes. RX_VK_FALLBACK_DEPTH_HOLD=0 draws them as before.
//
// Draws whose colour draw ran, programs that write memory, and permanent fallback-record failures are not held.
#pragma once
#include <cstdint>
#include <unordered_map>

namespace fallback_hold {

enum class Role { None, DepthOnly, Colour };

/// Which fallback draw this is: colour (it writes a colour target) or depth alone (it writes depth into a depth target), for an
/// indirect draw of a program that writes no memory. The switch itself is read by the caller: RX_VK_FALLBACK_DEPTH_HOLD.
inline Role role_of(bool indirect, bool writes_memory, uint32_t colour_written, bool depth_target, bool depth_written) {
    if (!indirect || writes_memory) return Role::None;
    if (colour_written) return Role::Colour;
    return depth_target && depth_written ? Role::DepthOnly : Role::None;
}

/// The culled lists whose colour draw was left out, by their indirect-argument address (an address is assumed to stay with one
/// list for the hold's lifetime), and the frame it last was.
class Held {
public:
    /// Marks the list's colour draw as left out (held) or recorded (!held) this frame.
    void mark(uint64_t list, uint64_t frame, bool held) {
        if (held) lists[list] = frame;
        else lists.erase(list);
    }
    /// True while the list's colour draw was left out in the last 120 frames (so a list no colour draw uses any more is released).
    bool held(uint64_t list, uint64_t frame) {
        const auto it = lists.find(list);
        if (it == lists.end()) return false;
        if (frame - it->second <= 120) return true;
        lists.erase(it);
        return false;
    }

private:
    std::unordered_map<uint64_t, uint64_t> lists;
};

} // namespace fallback_hold
