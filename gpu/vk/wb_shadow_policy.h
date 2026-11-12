// When a pool's write-back is worth a VRAM shadow, and when a shadow given back under VRAM pressure is made again
// (vk_memory.cpp's memory_end_submission and shadows_check_room). No Vulkan here. tests/wb_shadow_policy_test.cpp
#pragma once

#include <cstdint>

namespace wb_shadow {

/// A write-back worth a shadow: enough bytes in one submission, the pool has none, and the diff pass is on. The mark
/// stays after VRAM takes the shadow (Pool::shadow_wanted), so a pool whose own write-backs stay under this size for
/// good after a scene change still gets one made again when there is room.
constexpr uint64_t kMakeBytes = 256u << 10;
constexpr bool worth_shadow(bool diff, bool has_shadow, uint64_t run_bytes) {
    return diff && !has_shadow && run_bytes >= kMakeBytes;
}

/// A pool to make a shadow for again: it had one worth making, VRAM took it, and its mirror is still there (a
/// mirror_free'd pool cannot be shadowed; not a try).
constexpr bool remake_candidate(bool shadow_wanted, bool has_shadow, bool has_mirror) {
    return shadow_wanted && !has_shadow && has_mirror;
}

/// How often the room check runs, in write-back submissions, and how many shadows it may make in one check (each is an
/// allocation of the pool's whole size).
constexpr int kCheckEvery = 120;
constexpr int kMakePerCheck = 8;

/// Whether this submission is a check (every kCheckEvery write-back submissions).
constexpr bool check_period(int64_t submissions) { return submissions % kCheckEvery == 0; }

/// Whether anything asks for the check: shadows still held, or one wanted and given back. With neither, the check does
/// nothing and does not read VRAM usage.
constexpr bool check_worth(bool shadow_bytes, bool any_wanted) { return shadow_bytes || any_wanted; }

/// The attempt cap: a candidate too big to fit now is skipped without spending one (a later check has room).
constexpr bool may_try(int tries) { return tries < kMakePerCheck; }

} // namespace wb_shadow
