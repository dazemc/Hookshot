# Device loss during native presentation

The replay already treats `VK_ERROR_DEVICE_LOST` as terminal: `device_lost()`
writes its checkpoint report once, then `replay_fatal()` flushes the host log
before exiting. Continuing cannot complete GPU work and can retain captures and
staging allocations indefinitely.

Native presentation uses the same policy for the frame-fence wait/reset,
command-pool reset, command-buffer begin/end, image acquisition, blit submission
and presentation. The check precedes dependent work: a failed fence wait cannot
lead to readback, a failed submission cannot enqueue a present waiting for its
unsignaled semaphore, and a failed present cannot be counted as completed by
Reflex. Both the shared and dedicated presentation queues are covered.

This does not recover a lost device, change successful presentation, or replace
the existing out-of-date/suboptimal/Wayland-timeout handling. Other error classes
retain their existing behavior, including allocation errors; this is not a
general submission-recovery mechanism.

`tests/test_present_device_loss.py` executes the actual call/guard sites with
injected return values. It covers nine sites (two presentation queues), checks
that loss reports before the site continues, and supplies non-loss controls.
It tests routing rather than simulating an entire Vulkan implementation. The
existing `test_device_lost_exit.py` separately exercises the real concurrent
report-once and host-log-flush/exit contract. Hardware-reset recovery is not
claimed, and no physical driver reset is needed to run these tests.

## Hangs the driver never reports

A GPU hang is not always reported as `VK_ERROR_DEVICE_LOST`. On NVIDIA's Xid
109 (CTX SWITCH TIMEOUT) the context stops, the timeline stops short of what was
sent, and every later wait returns `VK_TIMEOUT` or never returns, but no Vulkan
call says the device is lost. Before the watchdog, `device_lost()` never ran:
the game froze and the log did not say which GPU work died.

`gpu_hang_watch_start()` starts a thread at the first present. It reads the
timeline once a second with `vkGetSemaphoreCounterValue`, which needs no
external synchronization. A lost device it sees goes straight to
`device_lost()`. Work already sent to the queue (`note_timeline_sent`, at both
submissions that signal the timeline) that finishes nothing for
`RX_VK_GPU_HANG_SEC` seconds (default 30) is logged and goes to `device_lost()`
too: the checkpoint report, the log flushed, exit code 3. `0` only logs the
stall (at 10 s, then every 30 s) and lets the game wait.

Only submissions that were actually sent count: a value held back on the CPU
side is not a GPU hang. The present's submission waits for its swapchain image,
so a window the system stops presenting could in principle hold the queue as
long; that is why the default is 30 s rather than a few seconds, and why `0`
exists.

`builder/rust/tests/gpu_hang_watch.rs` cuts `gpu_hang_watch_start()` into a
fixture with the timeline and sent value stubbed. A stuck timeline ends the
process after the limit, a moving one and `=0` do not, a lost device ends it at
once, and both timeline-signalling submissions are counted. It also compiles
the real `device_lost()` to check that the default `VK_ERROR_DEVICE_LOST`
message is unchanged and a hang reason is passed through.
