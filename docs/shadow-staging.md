# The texture shadow's staging

The texture shadow copies a capture's texture pages on the replay thread (`take_capture`, `gpu/vk/vk_memory.cpp`), so a
later submission reads the page as it was when it was submitted. It used to read every page from the capture buffer. That
buffer comes from `create_buffer_cached`, but VMA can land it in host-visible memory without `HOST_CACHED` (on the driver
that reported this, only the first ~30 MB of cached allocations succeed): the CPU reads that write-combined memory at
~0.3 GB/s, and a frame streaming thousands of texture pages spent up to 16 ms in those copies.

`capture_make` now copies the pages that are texture storage (the merged `tex_cap_ranges`, which it already takes on the
game's thread) into a block of ordinary heap RAM as well, before copying them into the capture buffer. Both hold the same
bytes, and `take_capture` builds the shadow's `CopyPair`s from the staging block; a page without a slot there (not
staged, or a late page that `take_capture` copies again after its GPU wait) still comes from the capture buffer. A page
`guarded_copy` cannot read is marked the same way as before in both places.

The game thread pays one extra cached copy per texture page (about 0.4 µs) to save the replay thread's uncached read
(3-14 µs). The staging block goes back to a small free list after the copies, so a big block is not faulted in again on
the game thread: at most four blocks and 96 MB are kept, and `RX_VK_LOW_SPEC` keeps none. A capture whose buffer is
already `HOST_CACHED` is not staged, as the extra copy would buy nothing.

`RX_VK_SHADOW_STAGE=0` (or `off`, `false`, `no`) turns the staging off: the shadow reads the capture buffer, as before.
Every 600 takes the log's `vk: shadow staging` line counts the shadow copies read from the staging and from the capture
buffer, and how many capture buffers were host-cached.

The staging block's size, the free list's limits and the switch are in `gpu/vk/shadow_stage_policy.h`, and
`tests/shadow_stage_test.cpp` checks them without the game or a GPU:

```
c++ -std=c++20 -Wall -Wextra -Werror tests/shadow_stage_test.cpp -o shadow-stage-test
./shadow-stage-test
```
