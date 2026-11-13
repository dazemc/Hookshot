// (hookshot, upstream #2559) Back-to-back draw passes and compute dispatches ordered by the game's barriers
// (RX_VK_GAME_ORDER).
//
// Every pass begin and every compute dispatch recorded a full barrier, which makes the GPU finish everything before each
// of them. The game records its own barrier (nvnCommandBufferBarrier, kept as a full barrier) wherever a pass samples what
// an earlier one drew or a dispatch reads what another wrote. With RX_VK_GAME_ORDER a draw pass right after another waits
// in its fragment stages only, and a direct dispatch right after another, nothing recorded between, gets no barrier. A
// switch between draws and compute, the replay's own copies, clears, fills and uniform updates, indirect dispatches, a
// parallel replay and the game's barriers keep the full one. On NVIDIA by default (the game's barriers were placed for an
// NVIDIA GPU), off on others: =1 to try it there, =0 off. RX_VK_PASS_BARRIER=game / full still sets the pass part, and
// RX_VK_NO_DISPATCH_ORDER keeps every dispatch's barrier.
//
// The rule alone, with no Vulkan: tests/barrier_order_test.cpp replays authored pass and dispatch sequences.
#pragma once

#include <cstdint>

#include "compat/hookshot_env.h"

namespace barrier_order {

constexpr uint32_t kNvidia = 0x10de;   // (the GPU the game's own barriers were placed for)

/// RX_VK_GAME_ORDER's value or null: the vendor's default when unset (NVIDIA on, every other vendor off).
inline bool enabled(const char* value, uint32_t vendor_id) {
    return value ? !env_value_off(value) : vendor_id == kNvidia;
}

/// Whether a direct dispatch goes on without the replay's full barrier: the last command counted (vk_work.h) was another
/// dispatch on the same command buffer, with nothing recorded since it, the order on, and no parallel replay. An indirect
/// dispatch reads arguments the GPU wrote: it always keeps its full barrier.
inline bool dispatch_after_dispatch(bool indirect, bool same_command_buffer, bool nothing_recorded_since, bool on,
                                    bool parallel) {
    return !indirect && same_command_buffer && nothing_recorded_since && on && !parallel;
}

/// Whether a draw pass right after another starts with pass_start_barrier's fragment-side barrier: the last thing on the
/// command buffer was a draw pass with nothing recorded since it, the order on, and no parallel replay.
inline bool pass_after_pass(bool same_command_buffer, bool nothing_recorded_since, bool on, bool parallel) {
    return same_command_buffer && nothing_recorded_since && on && !parallel;
}

}  // namespace barrier_order
