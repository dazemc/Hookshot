# Controller rest detection and camera inversion

SDL stick axes initially read as held remain suppressed until they actually rest inside the selected dead zone or
cross their resting position. This protects against a trigger or an unused axis mapped onto a stick, including a
second device whose held axis would otherwise dominate the player's controller.

The dead zone is circular for a stick's two axes (#1600): inside the circle both components are zero, and outside it
both pass through with their original magnitude and direction. A diagonal such as SDL's `(12000, -4000)` at the
default cutoff no longer snaps toward horizontal, while each axis still has the same threshold as before.

The rest detector uses a stable axis orientation. Camera X/Y inversion applies to the admitted output, so changing
an inversion setting cannot impersonate physical movement and permanently admit a phantom axis (#2273). An axis
that already rested continues to respond immediately to inversion; real neutral and worn-stick crossing recovery,
the selected dead zone, and independent axes/devices retain their behavior.

`cargo test --manifest-path builder/rust/Cargo.toml -p hookshot_engine --test input_parity` compiles the actual
filter and SDL conversion/rest/output code over authored device samples. It covers all six dead zones, diagonals in
every quadrant, the exact cutoff, repeated settings changes with held axes, both right-stick directions, independent
devices, genuine crossings, neutral recovery and the unchanged left-stick mapping. It needs a C compiler, not a
controller or game data; physical-controller validation is a separate test.
