# Scene and context migration validation

Step 9 completed on 2026-11-03, testing engine revision `f610cd7` on
`feature/scene-context-safety`. This step adds validation records and repeatable
benchmark tooling; it requires no further production-code changes.

## Build and behavior checks

| Check | Result |
| --- | --- |
| `cargo build --locked` | Pass; no Rust compiler warnings |
| `cargo test --lib --all-features --locked` | 84 passed, none ignored |
| `cargo test --doc --all-features --locked` | 17 passed, 3 existing examples intentionally ignored |
| `cargo check --all-targets --all-features --locked` | Pass; no Rust compiler warnings |
| Same check with `--target wasm32-unknown-unknown` | Pass; no Rust compiler warnings |
| Native integration suite, all features | 12 binaries passed; see cases below |
| Coroutine execution/timing/value tests | Five cases passed in separate processes |
| `one_screenshot` configuration | Saves the third-frame screenshot and produces the expected diagnostic panic |
| Opt-in Windows DPI integration | All four high/low-DPI and blocking/nonblocking scenarios passed |
| Formatting and whitespace checks | Pass |

The twelve native binaries are `back_to_the_future`,
`back_to_the_future_coroutines`, `context_guards`, `context_lifecycle`,
`coroutine_reentry`, `image_codecs`, `input_reentry`, `material_lifetime`,
`material_textures`, `render_target_lifetime`, `rendering_context`, and
`scene_lifecycle`. They ran with `--all-features`, including real audio without
playback. The two multi-test coroutine binaries ran each case with `--exact`;
Miniquad cannot create a second native display in one process.

These runs exercise UI closures that reenter the engine, guard conflicts and
unwind, scene ready/update/fixed/draw callbacks, coroutine reentry/cancellation,
panic recovery, material changes, queued texture/render-target disposal,
telemetry capture, text atlases, particles, cube pixels, mixed raw/batched GL,
and screenshot readback. Lifecycle children cover completion, accepted quit,
uncaught main-future panic, and a destructor panic. Scene completion/quit and
late cross-thread resource drops also pass.

The DPI suite ran at a real startup scale of 175% on one monitor, with injected
96/120/144/168/192 DPI changes. All seven IME coordinate queries pass in each
scenario. Candidate-popup appearance and physical cross-monitor transitions
are not covered by these automated checks.

Source audit finds no `static mut SCENE`, `static mut CONTEXT`, or unrestricted
context/backend getters. No warning suppression was added. Rustup still emits
an environment path-canonicalization warning in the sandbox, and Git can report
an unreadable global ignore file there; neither is a Rust compiler warning.

## Game and performance comparison

The sibling game's revision is `cfa37bd`. Its current scene is an 800x600 logical
DPI/input prototype containing an outlined rectangle, a mouse marker, and text;
there is no completed hidden-object level in this checkout. Validation uses an
instrumented copy of that actual source and its Windows manifest, with no edits
to the sibling repository.

Compare pre-migration engine `53e0ac7` against `f610cd7` in release builds on the
same machine and toolchain: Windows x86-64, Intel i7-11700K, Rust 1.99.1. The
machine lists an NVIDIA GTX 1650 and a Microsoft Remote Display Adapter; these
measurements are specific to this desktop session. Both builds use identical
dependency lockfiles, including Miniquad's `game` revision `db2af9e4`.

The harness requests swap interval 0, retains the actual mouse query but fixes
the displayed marker at (400, 300), and terminates automatically. This makes
rendered frames comparable without injecting user input. The stress workload
adds 2,000 rectangles and 10,000 drawing/input/timing-related context reads per
frame. Existing native tests provide the scene/coroutine behavioral coverage;
this benchmark does not represent a large scene registry.

Each workload/version has five runs, alternating execution order. Each run
warms up for 1,000 frames and records 5,000 frames: 100,000 measured frames in
total. Sample buffers are preallocated. Screenshot capture, hashing, CSV output,
and termination occur in a separate unmeasured frame.

`CPU work` measures the game body through drawing submission. `Whole frame`
also includes `next_frame().await`, engine frame completion, native event-loop
work, and the next frame's setup; it is not a GPU timer. Swap interval is a
driver hint, not a guarantee. Allocator instrumentation is identical in both
builds and counts successful Rust allocation/reallocation requests, requested
bytes, and corresponding deallocations. Native/GPU allocations are excluded.

Values below are medians of the five per-run percentiles, in microseconds.

| Workload/version | CPU work p50 | CPU work p95 | Whole frame p50 | Whole frame p95 |
| --- | ---: | ---: | ---: | ---: |
| Game baseline | 22.3 | 25.3 | 153.3 | 250.4 |
| Game migrated | 18.4 | 21.0 | 147.8 | 239.6 |
| Stress baseline | 324.6 | 347.2 | 355.8 | 428.9 |
| Stress migrated | 317.2 | 359.7 | 348.2 | 442.5 |

All twenty runs have the same allocation rates: 8.0322 requests and 1,206.2144
requested bytes per frame, with zero net retained Rust bytes over the measured
interval. All ten screenshots within each workload are byte-identical between
versions and repetitions. The 1400x1050 game and stress captures were also
visually inspected.

There is no measured median or allocation regression in these workloads.
Stress p95 CPU/frame values are about 3–4% higher; migrated stress per-run median
CPU work ranges from 313.4 to 345.5 microseconds, versus 322.6 to 325.8 for the
baseline. An initial shorter pilot showed substantial whole-frame variation,
so the comparison was repeated with longer warmup and samples. The longer runs
do not show a sustained median slowdown, but they do not establish a general
speedup or rule out tail-latency changes on other workloads. No unchecked access
or performance workaround was introduced.

The [measurement record](tools/context-migration/results-2026-11-03.json)
contains every run's summary, image hashes, revision identifiers, source hash,
and dependency-lock hash. Raw CSVs, screenshots, build logs, and generated
projects are under the ignored `target/context-migration/step9-bench/` directory.

## Reproduction

From this repository in PowerShell 7, choose a fresh output directory:

```powershell
./tools/context-migration/bench.ps1 -PrepareOnly -OutputDirectory target/context-migration/recheck
./tools/context-migration/bench.ps1 -RunOnly -OutputDirectory target/context-migration/recheck
./tools/context-migration/summarize.ps1 -Directory target/context-migration/recheck
```

Preparation uses Git/tar, the sibling game, the current local engine source,
and the cached Cargo dependencies. It archives the baseline without switching
branches, copies the game source into two isolated projects, injects the
[probe](tools/context-migration/probe.rs), and builds both with `--release
--offline`. It rejects differing dependency lockfiles. `-Runs`, `-Samples`,
`-Warmup`, and `-Baseline` are configurable. Native runs open short-lived windows;
avoid concurrent rendering/build loads while measuring. The summary command
rejects incomplete runs or differing screenshots.

Other checks can be reproduced with the commands above. Use
`cargo test --config 'build.rustflags=["--cfg","one_screenshot"]' --locked
--test screenshot_mode` for the diagnostic, and
`cargo test --all-features --locked --test windows_dpi_integration -- --ignored
--nocapture` for the Windows suite. The latter temporarily moves the cursor
and restores it afterward.

## Compatibility and limits

Ordinary game APIs compile and render unchanged. The deliberate low-level API
and typed-tween changes are documented in
[the plan](SCENE-CONTEXT-PLAN.md#7-resolve-apis-that-return-references-or-guards)
and [scene audit](SCENE-OWNERSHIP-AUDIT.md); examples, particles, and tests are
migrated. The sibling prototype has no affected calls.

`cargo +nightly miri --version` reports that Miri is not installed in the
available nightly toolchain. Isolated ownership tests passed normally, but
that is not Miri or an aliasing-safety proof. Browser WebAssembly execution,
Metal, Linux/macOS/Android native execution, and an asset-rich gameplay scene
were not tested. These are recorded coverage limits, not successful checks.
Sequential native windows remain a Miniquad limitation; repeated session
ownership is covered by headless tests.
