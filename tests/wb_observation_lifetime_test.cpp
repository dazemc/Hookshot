// Completed GPU write-back observations (gpu/vk/wb_observe.h, vk_memory.cpp's WbdTable): table sizing, result bounds,
// validation of the shader's flags, and reading a result only once its submission is done and only once. No game, no GPU.
#include "../gpu/vk/wb_observe.h"

#include <cstdio>
#include <cstdlib>
#include <vector>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using wb_observe::Chunk;
using wb_observe::Result;

/// A table slot as the renderer keeps it: its input chunks, the optional flags after them, the submission's value, and
/// whether a result is pending. collect() consumes the pending result once, including a failed read.
struct Slot {
    uint64_t value = 0;
    bool pending = false;
    std::vector<Chunk> chunks;
    std::vector<uint32_t> flags;

    Result collect(uint64_t completed) {
        if (!pending || value > completed) return Result{};
        pending = false;
        return wb_observe::collect(chunks.data(), flags.data(), chunks.size());
    }
};

int main() {
    static_assert(sizeof(Chunk) == 32, "the shader's Chunk is three addresses, count and vecs");
    {   // (a result is one flag a chunk after the input, and only for a whole number of chunks of a real submission)
        const uint64_t input = 3 * sizeof(Chunk);
        CHECK(wb_observe::wanted_bytes(input, false, 100) == input);
        CHECK(wb_observe::wanted_bytes(input, true, 0) == input);
        CHECK(wb_observe::wanted_bytes(input, true, 100) == input + 3 * sizeof(uint32_t));
        CHECK(wb_observe::wanted_bytes(input + 1, true, 100) == input + 1);
        CHECK(wb_observe::wanted_bytes((wb_observe::kMaxChunks + 1) * sizeof(Chunk), true, 100) ==
              (wb_observe::kMaxChunks + 1) * sizeof(Chunk));
        // (a table holds the chunks and their flags; a count of 0 has no result)
        CHECK(!wb_observe::result_fits(0, 0));
        CHECK(!wb_observe::result_fits(3 * sizeof(Chunk) + 12, 0));
        CHECK(wb_observe::result_fits(3 * sizeof(Chunk) + 12, 3));
        CHECK(!wb_observe::result_fits(3 * sizeof(Chunk) + 11, 3));
        CHECK(!wb_observe::result_fits(3 * sizeof(Chunk), 3));
        CHECK(!wb_observe::result_fits(wb_observe::kMaxChunks * sizeof(Chunk) + 4 * wb_observe::kMaxChunks + 1,
                                       wb_observe::kMaxChunks + 1));
        CHECK(wb_observe::result_fits(wb_observe::kMaxChunks * sizeof(Chunk) + 4 * wb_observe::kMaxChunks,
                                      wb_observe::kMaxChunks));
    }
    {   // (clean and changed chunks sum the bytes they cover; a changed chunk is not an exact changed-byte count)
        Chunk chunks[3] = {
            {0, 0, 0, 4, 1},    // (4 x 16 = 64 bytes, changed)
            {0, 0, 0, 3, 0},    // (3 x 4 = 12 bytes, clean)
            {0, 0, 0, 1, 1},    // (16 bytes, changed)
        };
        uint32_t flags[3] = {1, 0, 1};
        Result r = wb_observe::collect(chunks, flags, 3);
        CHECK(r.ok && r.chunks == 3 && r.changed == 2 && r.bytes == 92 && r.changed_bytes == 80);
        flags[1] = 1;
        r = wb_observe::collect(chunks, flags, 3);
        CHECK(r.ok && r.changed == 3 && r.changed_bytes == 92);
        flags[0] = flags[1] = flags[2] = 0;
        r = wb_observe::collect(chunks, flags, 3);
        CHECK(r.ok && r.changed == 0 && r.changed_bytes == 0 && r.bytes == 92);
    }
    {   // (missing or invalid data rejects the whole batch: unknown is never clean)
        Chunk chunks[2] = {{0, 0, 0, 4, 1}, {0, 0, 0, 3, 0}};
        uint32_t flags[2] = {0, 0};
        flags[0] = 2;
        CHECK(!wb_observe::collect(chunks, flags, 2).ok);
        flags[0] = 0xffffffffu;   // (a flag the shader never wrote)
        CHECK(!wb_observe::collect(chunks, flags, 2).ok);
        flags[0] = 0;
        chunks[0].vecs = 2;
        CHECK(!wb_observe::collect(chunks, flags, 2).ok);
        chunks[0].vecs = 0;
        chunks[0].count = 0;
        CHECK(!wb_observe::collect(chunks, flags, 2).ok);
        chunks[0].count = 4097;   // (4097 x 16 > 64 KB)
        chunks[0].vecs = 1;
        CHECK(!wb_observe::collect(chunks, flags, 2).ok);
        chunks[0].count = 4096;   // (exactly 64 KB is the most a chunk is)
        CHECK(wb_observe::collect(chunks, flags, 2).ok);
        CHECK(!wb_observe::collect(chunks, flags, 0).ok);
        CHECK(!wb_observe::collect(nullptr, flags, 2).ok);
        CHECK(!wb_observe::collect(chunks, nullptr, 2).ok);
    }
    {   // (a result is read only after its submission is done, and consumed once, even when it is invalid)
        Slot slot;
        slot.value = 10;
        slot.pending = true;
        slot.chunks = {{0, 0, 0, 4, 1}};
        slot.flags = {1};
        CHECK(!slot.collect(9).ok);   // (the GPU has not done it: nothing is read)
        CHECK(slot.pending);
        Result r = slot.collect(10);
        CHECK(r.ok && r.chunks == 1 && r.changed == 1 && r.bytes == 64);
        CHECK(!slot.pending);
        CHECK(!slot.collect(11).ok);   // (the same result is not counted twice)
        slot.pending = true;
        slot.flags = {0xffffffffu};
        CHECK(!slot.collect(11).ok);
        CHECK(!slot.pending);   // (a failed read is consumed too: it is not retried as clean)
        slot.pending = true;
        slot.flags = {1};
        CHECK(slot.collect(11).ok);
    }
    {   // (a table slot is reused only after its submission is done; its old result is collected before the new input)
        Slot ring[2];
        for (int i = 0; i < 2; i++) {
            ring[i].value = 20 + i;
            ring[i].pending = true;
            ring[i].chunks = {{0, 0, 0, 1, 1}};
            ring[i].flags = {(uint32_t)i};
        }
        CHECK(ring[0].collect(20).ok);             // (the first slot is done at 20)
        CHECK(ring[0].collect(20).ok == false);    // (consumed)
        CHECK(ring[1].collect(20).ok == false);    // (the second slot is not done yet)
        CHECK(ring[1].pending);
        CHECK(ring[1].collect(21).ok);
        CHECK(!ring[1].pending);
        // (the slot may now take a new submission's input and flags)
        ring[0].value = 22;
        ring[0].pending = true;
        ring[0].flags = {1};
        CHECK(ring[0].collect(21).ok == false);
        CHECK(ring[0].collect(22).ok && ring[0].collect(22).ok == false);
    }
    std::printf("PASS: completed write-back observations\n");
    return 0;
}
