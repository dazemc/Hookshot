// How long a readback waits for its write-back (vk_replay.cpp's readback_wait_value; tests/readback_wait_test.cpp).
//
// invalidate() waits for a GPU result's write-back before the CPU reads it. When the recordings' timeline does not know the
// value it falls back to a wait on the main timeline, which used to have no limit. On a Steam Deck (RADV, no buffer mirrors,
// queues replayed in submit order) the game's main thread and one of its jobs waited there right after a save loaded, for a
// value the timeline never reached while they waited, and the game stopped for good: 2 of 8 starts, 4 of 4 with
// RX_VK_CAPTURE_WAIT=0. The wait now gives up after RX_VK_READBACK_WAIT_MS (250 by default) and the CPU reads what memory
// holds, as the capture's wait already did (vk_timeline_wait); 0 or less keeps the unlimited wait, as before.
#pragma once
#include <cstdint>
#include <cstdlib>

namespace readback_wait {

/// RX_VK_READBACK_WAIT_MS: the wait's limit in ms, 250 by default (0 or less: no limit).
inline int64_t limit_ms(const char* env) { return env ? std::atoll(env) : 250; }

/// The wait's timeout for a limit: UINT64_MAX for the unlimited wait, its milliseconds in nanoseconds otherwise.
inline uint64_t timeout_ns(int64_t limit) {
    return limit <= 0 ? UINT64_MAX : static_cast<uint64_t>(limit) * 1000000ull;
}

} // namespace readback_wait
