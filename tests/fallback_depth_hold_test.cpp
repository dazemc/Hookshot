// fallback_hold (gpu/vk/fallback_depth_hold.h): a mesh fallback's depth-only draws held while the colour draw of the same
// culled list waits for its build, its 120-frame release and the switch's caller. No game, no GPU.
#include "../gpu/vk/fallback_depth_hold.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using fallback_hold::Held;
using fallback_hold::Role;

int main() {
    // (the roles: colour writes a colour target, depth alone writes depth, and neither for a direct draw or a program that writes memory)
    CHECK(fallback_hold::role_of(true, false, 0xf, true, true) == Role::Colour);
    CHECK(fallback_hold::role_of(true, false, 0, true, true) == Role::DepthOnly);
    CHECK(fallback_hold::role_of(true, false, 0, false, true) == Role::None);   // (colour to nothing and no depth target)
    CHECK(fallback_hold::role_of(true, false, 0, true, false) == Role::None);   // (a depth target the program does not write)
    CHECK(fallback_hold::role_of(false, false, 0xf, true, true) == Role::None); // (not indirect: no list to hold it by)
    // (a program that stores global memory or writes a storage buffer or image is never held: it can be skipped, its writes would be lost)
    CHECK(fallback_hold::role_of(true, true, 0xf, true, true) == Role::None);
    CHECK(fallback_hold::role_of(true, true, 0, true, true) == Role::None);

    Held held;
    CHECK(!held.held(0x1000, 10));
    held.mark(0x1000, 10, true);
    CHECK(held.held(0x1000, 10) && held.held(0x1000, 130));   // (120 frames after the last wait)
    CHECK(!held.held(0x2000, 10));                            // (another list is not held by it)
    CHECK(held.held(0x1000, 130) && !held.held(0x1000, 131)); // (one past 120 releases it)
    held.mark(0x1000, 200, true);
    held.mark(0x1000, 200, false);                            // (the colour draw ran)
    CHECK(!held.held(0x1000, 200));
    held.mark(0x3000, 10, true);
    held.mark(0x3000, 20, true);                              // (left out again: the wait counts from the latest frame)
    CHECK(held.held(0x3000, 140) && !held.held(0x3000, 141));

    std::printf("PASS: a fallback list's depth-only draws are held while its colour draw builds\n");
    return 0;
}
