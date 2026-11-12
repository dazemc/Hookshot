// The page table behind memory_writeback_value (vk_memory.cpp): which submission, and which part of it, last wrote a
// guest page back, and whether that write came from a mirror (part) or was made in place (whole). No Vulkan here.
// tests/wb_pages_test.cpp
#pragma once

#include <algorithm>
#include <cstdint>
#include <memory>
#include <vector>

namespace wb_page_table {

template <uint64_t PageShift, uint64_t Pages, uint64_t ChunkPages>
struct Table {
    static constexpr uint64_t kPageShift = PageShift, kPages = Pages, kChunkPages = ChunkPages;
    // (a chunk is made when a page of it is first written back; write-backs are of imported pools only, which lie in
    // the GPU's windows, so few chunks are ever made)
    std::vector<std::unique_ptr<uint64_t[]>> chunks = std::vector<std::unique_ptr<uint64_t[]>>(Pages / ChunkPages);
    bool any = false;

    void set(uint64_t page, uint64_t value) {
        if (page >= kPages) return;
        auto& c = chunks[page / kChunkPages];
        if (!c) {
            c.reset(new uint64_t[kChunkPages]);
            std::fill(c.get(), c.get() + kChunkPages, 0ull);
        }
        c[page % kChunkPages] = value;
        any = true;
    }

    uint64_t get(uint64_t page) const {
        if (page >= kPages) return 0;
        const auto& c = chunks[page / kChunkPages];
        return c ? c[page % kChunkPages] : 0;
    }

    /// the largest value of pages first..last (0: none), and whether any had one; a chunk never made is skipped whole
    uint64_t max_in(uint64_t first, uint64_t last, bool* ever) const {
        uint64_t v = 0;
        for (uint64_t p = first; p <= last && p < kPages;) {
            const uint64_t end = (p / kChunkPages + 1) * kChunkPages;   // (the first page past p's chunk)
            const auto& c = chunks[p / kChunkPages];
            if (!c) {
                p = end;
                continue;
            }
            for (; p <= last && p < end; p++)
                if (const uint64_t pv = c[p % kChunkPages]) {
                    if (ever) *ever = true;
                    v = std::max(v, pv);
                }
        }
        return v;
    }

    bool empty() const { return !any; }
};

struct Value {
    uint64_t value = 0;   // (the newest write-back of any page in the range; 0: none)
    uint64_t part = 0;    // (the newest part that wrote any page back from a mirror; 0: none)
    uint64_t whole = 0;   // (the newest value among the pages written in place: their whole submission is waited for)
    bool ever = false;    // (any page in the range was ever written back)
};

/// The range's newest write-back, and the part/whole split (vk_memory.cpp's memory_writeback_value). A page's part is
/// replaced with its value by every note: the two are of the same write, so a later in-place write clears an older part.
template <class Table>
Value range(const Table& pages, const Table& parts, uint64_t first, uint64_t last) {
    Value r;
    r.value = pages.max_in(first, last, &r.ever);
    if (!r.value) return r;
    for (uint64_t p = first; p <= last && p < Table::kPages;) {
        if (!pages.chunks[p / Table::kChunkPages]) {   // (a chunk never made: none of its pages, as max_in)
            p = (p / Table::kChunkPages + 1) * Table::kChunkPages;
            continue;
        }
        const uint64_t pv = pages.get(p), pp = pv ? parts.get(p) : 0;
        if (pp) r.part = std::max(r.part, pp);
        else if (pv) r.whole = std::max(r.whole, pv);
        p++;
    }
    return r;
}

} // namespace wb_page_table
