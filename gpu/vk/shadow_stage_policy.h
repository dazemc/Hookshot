// The texture shadow's staging blocks (vk_memory.cpp: stage_get and stage_put; tests/shadow_stage_test.cpp).
//
// take_capture's texture shadow used to read the pages it copies from the capture buffer, which on some drivers is
// write-combined memory (host-visible, not cached): the CPU reads that at ~0.3 GB/s, and a frame streaming thousands of
// texture pages spent milliseconds in those copies. capture_make now copies the pages that are texture storage into a
// block of ordinary cached RAM as well (the staging), and take_capture reads the shadow's copies from there; the block
// goes back to a small free list after the copies, so the next capture does not fault in a fresh one on the game's
// thread. At most four blocks and 96 MB are kept; RX_VK_LOW_SPEC keeps none, as RAM is the tight resource there.
// RX_VK_SHADOW_STAGE=0 turns the staging off (the shadow reads the capture buffer, as before).
#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <new>
#include <utility>
#include <vector>

#include "compat/hookshot_env.h"

namespace shadow_stage {

constexpr size_t kAlign = 1u << 20;   // (blocks rounded to 1 MB: blocks of near sizes fit each other)
constexpr size_t kMaxBlocks = 4;      // (kept after a take, for the next capture)
constexpr size_t kMaxBytes = 96ull << 20;

/// A block of cached RAM: a capture's staging for its texture pages.
struct Block {
    std::unique_ptr<uint8_t[]> mem;
    size_t bytes = 0;
    explicit operator bool() const { return mem != nullptr; }
    uint8_t* get() const { return mem.get(); }
};

/// The block size for need bytes: rounded up to 1 MB (0 for 0, which asks for no block).
inline size_t block_bytes(size_t need) { return need ? (need + kAlign - 1) & ~(kAlign - 1) : 0; }

/// A new block of block_bytes(need) bytes: empty when the memory is not there.
inline Block make(size_t need) {
    Block b;
    b.bytes = block_bytes(need);
    b.mem.reset(new (std::nothrow) uint8_t[b.bytes]);
    if (!b.mem) b.bytes = 0;
    return b;
}

/// RX_VK_SHADOW_STAGE's value: on unless an off value (0, off, false or no; unset is on, as everywhere).
inline bool enabled(const char* value) { return !value || !env_value_off(value); }

/// The kept blocks, for the next capture. Not thread-safe: the caller's lock is the capture staging's (a leaf lock).
class FreeList {
public:
    /// The smallest kept block of at least need bytes, taken off the list; empty when none fits.
    Block take_kept(size_t need) {
        size_t best = free_.size();
        for (size_t i = 0; i < free_.size(); i++)
            if (free_[i].bytes >= need && (best == free_.size() || free_[i].bytes < free_[best].bytes)) best = i;
        if (best == free_.size()) return {};
        Block b = std::move(free_[best]);
        bytes_ -= b.bytes;
        free_.erase(free_.begin() + best);
        return b;
    }
    /// A block of at least need bytes: the smallest kept one that fits, else a new one rounded up to 1 MB. Empty when
    /// the memory is not there.
    Block take(size_t need) {
        Block b = take_kept(need);
        return b ? std::move(b) : make(need);
    }
    /// The block back to the kept ones: at most four and 96 MB in all, none with low_spec; else it is freed.
    void put(Block b, bool low_spec) {
        if (!b) return;
        if (!low_spec && free_.size() < kMaxBlocks && bytes_ + b.bytes <= kMaxBytes) {
            bytes_ += b.bytes;
            free_.push_back(std::move(b));
        }
    }
    size_t blocks() const { return free_.size(); }
    size_t bytes() const { return bytes_; }
private:
    std::vector<Block> free_;
    size_t bytes_ = 0;
};

} // namespace shadow_stage
