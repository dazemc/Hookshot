// RX_VK_GAME_ORDER's rule (gpu/vk/barrier_order.h), with no Vulkan and no game: an authored sequence of draws, dispatches,
// game barriers and the replay's own work is replayed against the same predicates pass_start_barrier and dispatch use.
// It checks the back-to-back shortcuts and every conservative path: the switch off, another vendor's default, a parallel
// replay, a command buffer or queue change, an indirect dispatch, a switch between draws and compute, and any command the
// replay records between two dispatches or passes (a copy, clear, fill, uniform update or a game barrier).
#include <cstdint>
#include <cstdio>

#include "barrier_order.h"

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #x); return 1; } } while (0)

namespace {

/// A tiny replay: work() is a command that does GPU work (vk_work.h's count), state() one that only binds or marks.
struct Replay {
    uint64_t epoch = 0;
    uint64_t cmd = 1;
    uint64_t last_dispatch_cmd = 0, last_dispatch_epoch = 0;
    uint64_t last_pass_end_cmd = 0, last_pass_end_epoch = 0;
    bool order = true, parallel = false, pass_open = false;
    int full = 0, fragments = 0, unordered = 0;

    void work() { ++epoch; }
    void barrier() { ++full, work(); }

    /// end_pass (vk_draw.cpp): the open pass ends and note_pass_end() records the count right after it.
    void end_pass() {
        if (!pass_open) return;
        pass_open = false;
        work();
        last_pass_end_cmd = cmd, last_pass_end_epoch = epoch;
    }

    void begin_pass() {
        end_pass();
        if (barrier_order::pass_after_pass(cmd == last_pass_end_cmd, epoch == last_pass_end_epoch, order, parallel))
            ++fragments;
        else
            ++full;
        work();
        pass_open = true;
    }

    void dispatch(bool indirect = false) {
        end_pass();   // (vk_draw.cpp dispatch's end_pass, before its barrier)
        if (barrier_order::dispatch_after_dispatch(indirect, cmd == last_dispatch_cmd, epoch == last_dispatch_epoch,
                                                   order, parallel))
            ++unordered;
        else
            ++full;
        work();
        last_dispatch_cmd = cmd, last_dispatch_epoch = epoch;
    }

    void state() {}   // (a bind, a viewport, a push constant: in the command buffer, but not GPU work)

    /// The replay's own copy, fill, clear or uniform update: a full barrier before and after it.
    void replay_work() { end_pass(), barrier(), barrier(); }

    /// The game's nvnCommandBufferBarrier: end_pass, then a full barrier the replay keeps.
    void game_barrier() { end_pass(), barrier(); }

    void other_command_buffer(uint64_t next) { end_pass(), cmd = next; }

    /// A new recording: its state is fresh, and its start barrier is recorded before any work.
    void new_recording() {
        end_pass();
        barrier();
        last_dispatch_cmd = last_dispatch_epoch = 0;
        last_pass_end_cmd = last_pass_end_epoch = 0;
    }
};

int switches() {
    CHECK(barrier_order::enabled(nullptr, barrier_order::kNvidia));   // (NVIDIA by default)
    CHECK(!barrier_order::enabled(nullptr, 0x1002));                  // (off on AMD)
    CHECK(!barrier_order::enabled(nullptr, 0x8086));                  // (off on Intel)
    CHECK(!barrier_order::enabled(nullptr, 0));                       // (and with no device)
    CHECK(barrier_order::enabled("1", 0x1002));                       // (=1: try it there)
    CHECK(barrier_order::enabled("yes", 0x1002));
    CHECK(!barrier_order::enabled("0", barrier_order::kNvidia));      // (=0: off)
    CHECK(!barrier_order::enabled("off", barrier_order::kNvidia));
    CHECK(!barrier_order::enabled("false", barrier_order::kNvidia));
    CHECK(!barrier_order::enabled("no", barrier_order::kNvidia));
    CHECK(!barrier_order::enabled(" 0 ", barrier_order::kNvidia));
    CHECK(!barrier_order::enabled("", barrier_order::kNvidia));
    return 0;
}

int authored_sequences() {
    {   // three direct dispatches, one full barrier: the game's barriers order the chain
        Replay r;
        r.dispatch(), r.dispatch(), r.dispatch();
        CHECK(r.full == 1 && r.unordered == 2 && r.fragments == 0);
    }
    {   // three passes: the first starts after other work with a full barrier, the next two in their fragment stages
        Replay r;
        r.begin_pass(), r.end_pass(), r.begin_pass(), r.end_pass(), r.begin_pass();
        CHECK(r.full == 1 && r.fragments == 2 && r.unordered == 0);
    }
    {   // a game barrier between passes, and between dispatches, keeps the full one
        Replay r;
        r.begin_pass(), r.end_pass(), r.game_barrier(), r.begin_pass();
        CHECK(r.full == 3 && r.fragments == 0);
        Replay d;
        d.dispatch(), d.game_barrier(), d.dispatch();
        CHECK(d.full == 3 && d.unordered == 0);
    }
    {   // a switch between draws and compute: the pass begins full, the dispatch after it full
        Replay r;
        r.begin_pass(), r.dispatch();
        CHECK(r.full == 2 && r.fragments == 0 && r.unordered == 0);
    }
    {   // and compute to draws
        Replay r;
        r.dispatch(), r.begin_pass();
        CHECK(r.full == 2 && r.fragments == 0 && r.unordered == 0);
    }
    {   // binds and marks record nothing: the dispatch after them still follows the one before
        Replay r;
        r.dispatch(), r.state(), r.state(), r.dispatch();
        CHECK(r.full == 1 && r.unordered == 1);
    }
    {   // the replay's own copy/fill/update between them keeps both full barriers
        Replay r;
        r.dispatch(), r.replay_work(), r.dispatch();
        CHECK(r.full == 4 && r.unordered == 0);
    }
    {   // an indirect dispatch keeps its own barrier; a direct one after it may follow (its arguments were read after that barrier)
        Replay r;
        r.dispatch(), r.dispatch(true), r.dispatch();
        CHECK(r.full == 2 && r.unordered == 1);
    }
    {   // another command buffer or queue: full
        Replay r;
        r.dispatch(), r.other_command_buffer(2), r.dispatch();
        CHECK(r.full == 2 && r.unordered == 0);
    }
    {   // a pass right after another on another command buffer: full
        Replay r;
        r.begin_pass(), r.end_pass(), r.other_command_buffer(2), r.begin_pass();
        CHECK(r.full == 2 && r.fragments == 0);
    }
    {   // work recorded between the passes: full
        Replay r;
        r.begin_pass(), r.end_pass(), r.replay_work(), r.begin_pass();
        CHECK(r.full == 4 && r.fragments == 0);
    }
    {   // state-only between passes: fragment barrier
        Replay r;
        r.begin_pass(), r.end_pass(), r.state(), r.begin_pass();
        CHECK(r.full == 1 && r.fragments == 1);
    }
    {   // a new recording starts afresh
        Replay r;
        r.dispatch(), r.new_recording(), r.dispatch();
        CHECK(r.full == 3 && r.unordered == 0);
    }
    return 0;
}

int conservative_fallback() {
    {   // RX_VK_GAME_ORDER=0 / another vendor: every dispatch and pass begin gets its full barrier
        Replay r;
        r.order = false;
        r.dispatch(), r.dispatch(), r.begin_pass(), r.end_pass(), r.begin_pass();
        CHECK(r.full == 4 && r.unordered == 0 && r.fragments == 0);
    }
    {   // a parallel replay: as before, whatever the switch says
        Replay r;
        r.parallel = true;
        r.dispatch(), r.dispatch(), r.begin_pass(), r.end_pass(), r.begin_pass();
        CHECK(r.full == 4 && r.unordered == 0 && r.fragments == 0);
    }
    {   // the replay's own work and every game barrier: full in both modes
        Replay r;
        r.order = false;
        r.begin_pass(), r.end_pass(), r.game_barrier(), r.begin_pass(), r.replay_work(), r.dispatch(), r.replay_work(),
            r.dispatch();
        CHECK(r.full == 9 && r.fragments == 0 && r.unordered == 0);
    }
    return 0;
}

}  // namespace

int main() {
    if (switches() || authored_sequences() || conservative_fallback()) return 1;
    std::puts("PASS: back-to-back passes and dispatches ordered by the game's barriers, with the conservative path");
    return 0;
}
