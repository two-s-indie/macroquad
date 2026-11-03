# Changelog

Changes in the [two-s-indie/macroquad](https://github.com/two-s-indie/macroquad)
fork's `game` branch, maintained for **Where's the Yokai?**.

This history starts after the branch's common ancestor with `master`,
[`8d602d3`](https://github.com/two-s-indie/macroquad/commit/8d602d3)
(`text: avoid spurious font atlas growth`). Earlier upstream changes are outside
this changelog's scope. Dates below are commit dates, not release dates.

## Unreleased

### Fixed

- Make the `Map::tiles` return lifetime explicit, removing the tiled crate's
  `mismatched_lifetime_syntaxes` warning without changing behavior.
- Complete step 5 of the context safety plan: texture, render-pass, material,
  and font-atlas destructors enqueue disposal without borrowing the context or
  calling the backend. Queues belong to their original session and close at
  shutdown, making late drops harmless. Preserve queued material/attachment
  lifetimes and defer atlas deletion until queued text finishes. Add tests for
  nested disposal, exact-once queueing, session isolation, and late drops after
  completion/accepted quit. The four `CONTEXT` warnings remain for later steps.
- Complete step 4 of the context safety plan: pass backend and texture lookup
  data explicitly through frame rendering, shapes/models, textures, text,
  UI materials/atlases, and telemetry capture. Resolve camera inputs before
  borrowing the context. Preserve public drawing APIs and add a real-backend
  regression test for texture updates, atlas/text drawing, UI capture, and
  screenshots. Resource disposal and legacy low-level access remain separate
  migration steps; the four `CONTEXT` warnings remain.
- Complete step 3 of the scene safety plan: replace the scene mutable global
  with scoped thread-local storage, removing its `static_mut_refs` warning.
  Restore dispatch/fixed flags and camera state on unwind; run scene callbacks
  outside the whole-context frame borrow and within user-code panic recovery.
  Initialize scene sessions explicitly, reject foreign/stale handles, and clean
  up on completion and accepted quit before backend teardown. Add headless and
  real-rendering lifecycle tests; four `CONTEXT` warnings remain.
- Detach explicitly canceled coroutines before dropping their futures, allowing
  cancellation destructors to reenter the scene and schedule/stop coroutines.
- Complete step 2 of the scene safety plan: replace the raw arena and borrowed
  registry pointers with typed allocations and owned guards. Validate node type,
  scene identity, and generation; reject conflicting clear/delete operations
  before mutation. Snapshot iterator identities to prevent stale-slot reuse and
  duplicate visits. Drop removed nodes outside registry access, including deletion
  from callbacks, and replace unchecked callback casts with typed adapters.
  All five previously ignored scene defect tests now pass; add coverage for guard
  survival, capabilities, alignment, destructor reentry, and scoped projections.
- Replace tween field offsets derived from uninitialized nodes with retained
  typed projections under parent guards. Field access cannot outlive its guard,
  and polling releases the node on return or panic. Preserve borrowed projections
  in `follow_path` and sendable handles for non-`Send` nodes.
- Replace the profiler's mutable global with thread-local `RefCell` storage and
  scoped borrows, removing its `static_mut_refs` warning while preserving the
  public telemetry API. Avoid nested profiler access during frame reset and
  apply capture-state changes after releasing the profiler borrow. Add tests
  for nested zones, frame reset, capture transitions, and thread isolation.
- Replace the experimental storage's mutable global with thread-local storage
  and owned borrow guards, removing its six `static_mut_refs` warnings without
  adding unsafe code. Preserve the `store`/`get`/`get_mut` API and runtime borrow
  checks; guards now keep replaced values alive until released. Stored values
  are isolated per thread and owned by its storage. Drop replaced values
  outside the map borrow so their destructors can access storage.
- Protect the registered Macroquad thread ID with an `RwLock`, removing its
  `static_mut_refs` warning and unsafe global access while preserving thread
  reassignment during window/test initialization. Add regression coverage for
  uninitialized access, wrong-thread access, and reinitialization.
- Make borrowed return lifetimes explicit in UI and scene APIs and remove the
  unused `MiniquadTexture` import to resolve lifetime-syntax and unused-import
  warnings without changing runtime behavior.
- Declare `one_screenshot` as an expected custom cfg under `[lints.rust]` to
  eliminate its `unexpected_cfgs` warnings. Preserve the opt-in
  `RUSTFLAGS='--cfg one_screenshot'` debug mode, which saves `screenshot.png`
  on frame 3 and then panics; the lint declaration does not enable the mode.

### Changed

- Nodes created during scene callbacks now begin callbacks next frame, with
  `ready()` before update/fixed/draw. A guard blocking ready also blocks later
  callbacks that frame. Clearing still preserves persistent nodes; session
  shutdown removes all nodes. Optional access to an inactive scene returns
  absence/defaults instead of implicitly constructing a scene.
- Coroutine stopping cancels the current snapshot; futures created by its
  destructors remain scheduled. Full scheduler/context migration is still pending.
- The explicit tween type is now `LinearTweenFuture<T, T1, F>`; inferred
  `linear(...)` calls remain supported. Projection closures are retained and run
  each poll, so their captures determine the future's lifetime and `Send` bounds.
- Scene memory telemetry now reports the sum of registered nodes' inline sizes
  instead of an arena offset; it excludes registry overhead and nested allocations.
- Establish the scene safety baseline with internal clock/camera inputs for
  headless tests, preserving production callback ordering and timing. Add nine
  characterization tests and five ignored acceptance tests reproducing known
  ownership/identity defects for subsequent migration steps. The remaining
  scene/context globals and their warnings are unchanged.
- Update `image` from `0.24` to `0.25`, retaining disabled default features and
  PNG/TGA support. Validated with `0.25.10`, which requires Rust 1.88 or newer.
  Add regression tests for PNG export/reload (including the existing vertical
  flip) and TGA decoding, checking dimensions, pixel colors, and alpha values.
  Downstream code passing `image` types such as `ImageFormat` or `ImageError`
  across Macroquad's API must also use a compatible `image` version.
- Update `glam` from `0.27` to `0.34.2`, retaining `scalar-math` and the
  optional `glam-serde` feature. Migrate camera view and projection constructors
  to the new `glam::camera` API while preserving right-handed OpenGL conventions
  and the existing experimental `camera` module export in the prelude.
  The dependency now requires Rust 1.85 or newer. Downstream code using the
  re-exported math types must use a compatible `glam` version and migrate any
  removed constructors to the new camera API.

### Documentation

- Complete step 1 of the scene/context plan and add `SCENE-OWNERSHIP-AUDIT.md`
  with behavior, pointer hazards, clear/delete requirements, and the proposed
  owned-guard and scene/session handle strategy.
- Add `SCENE-CONTEXT-PLAN.md` with ordered ownership, callback, lifecycle, and
  validation steps for fixing the remaining scene and context globals.
- Add this changelog covering all 10 existing commits on `game` since its
  divergence from `master`, through `1420b0d`.
- Require changelog updates alongside changes committed in this fork in
  `AGENTS.md`.

## 2026-11-02

### Changed

- Enable the sibling `../miniquad` checkout as the Miniquad dependency patch,
  then replace it with the Git dependency patch targeting
  `https://github.com/two-s-indie/miniquad.git`, branch `game`. Keep the local
  path as a commented alternative.
  ([`56a0d8e`](https://github.com/two-s-indie/macroquad/commit/56a0d8e),
  [`1420b0d`](https://github.com/two-s-indie/macroquad/commit/1420b0d))

### Documentation

- Add `AGENTS.md` describing the fork's purpose, `game` as the default base,
  sibling repositories, and the game's use of the Macroquad and Miniquad
  `game` branches.
  ([`c7d7c22`](https://github.com/two-s-indie/macroquad/commit/c7d7c22))

## 2026-11-01

### Fixed

- Round Windows runtime window-size requests to the nearest framebuffer pixel
  after DPI scaling, matching the paired Miniquad fork's startup rounding and
  avoiding truncation at fractional scales. Other backends retain their
  existing conversion.
  ([`728ad33`](https://github.com/two-s-indie/macroquad/commit/728ad33))

### Added

- Add the `dpi_diagnostic` example showing logical dimensions, framebuffer
  dimensions, DPI scale, mouse hit testing, text, and frame updates. Include
  runtime resizing, fullscreen and cursor-grab controls, plus blocking,
  low-DPI, and odd-size startup modes. Document local Miniquad overrides and
  the initial failing 175% startup baseline.
  ([`3c706c5`](https://github.com/two-s-indie/macroquad/commit/3c706c5))
- Add an opt-in Windows DPI integration test and the Windows-only `winapi`
  development dependency. Cover high/low DPI with normal/blocking event loops,
  monitor round trips, injected 100-200% scales, GPU text and rectangle output,
  logical screen queries, even/odd resize requests, stationary pointer refresh,
  clicks without motion, signed cursor-release coordinates, cursor clipping,
  and IME anchor persistence. Support saving framebuffer PNGs.
  ([`728ad33`](https://github.com/two-s-indie/macroquad/commit/728ad33))

### Documentation and validation

- Clarify logical client units for screen dimensions, mouse position, and size
  requests; document dynamic DPI scale, virtual grabbed-mouse coordinates,
  Windows pixel rounding, and IME coordinate conversion in the diagnostic guide.
- Record successful integration runs in all four modes on real 100%/175%
  monitors and injected scales, plus Linux/Wasm compilation checks. Physical
  dragging, Windows Settings scale changes, other real scale combinations, and
  visible IME popups remain documented manual checks. See the
  [DPI diagnostic guide](examples/dpi_diagnostic.md) for historical results and
  requirements.
  ([`728ad33`](https://github.com/two-s-indie/macroquad/commit/728ad33))

## 2026-10-31

### Fixed

- Make cancellation and lookup of stale, completed, cleared, or repeatedly
  stopped coroutine handles harmless. Use bounds-checked storage access,
  preserve slot generations when clearing, and prevent duplicate free slots
  or old handles becoming valid again. Add four storage regression tests.
  ([`d1acd02`](https://github.com/two-s-indie/macroquad/commit/d1acd02))
- Keep material textures alive with owned `Texture2D` handles and snapshot
  bindings for each queued draw. Changing a texture breaks batching so earlier
  draws retain their original bindings; unset bindings use the white fallback.
  Release snapshots when draws finish or are cleared. Add regression tests for
  texture retention, replacement, disposal, and queued binding changes.
  ([`f5bc82e`](https://github.com/two-s-indie/macroquad/commit/f5bc82e))
- Defer material pipeline disposal until queued draws finish. Dropping the
  active material restores the default pipeline for subsequent draws; retired
  pipelines release their backend pipelines and owned shaders and make storage
  reusable. Add regression coverage for queued draws, shader deletion, and
  repeated material creation and disposal.
  ([`ea8c461`](https://github.com/two-s-indie/macroquad/commit/ea8c461))
- Share render-pass ownership with retained render-target texture handles so
  textures remain usable after their target is dropped. Coordinate attachment
  cleanup to avoid double deletion on OpenGL and explicitly release attachments
  on Metal. Add lifetime and cleanup coverage with and without depth buffers,
  including multisampled targets when supported.
  ([`167e8d3`](https://github.com/two-s-indie/macroquad/commit/167e8d3))
- Accumulate all horizontal and vertical mouse-wheel events within a frame
  instead of keeping only the last event. Document that `mouse_wheel()` returns
  the accumulated deltas, which reset at the end of each frame.
  ([`a28a268`](https://github.com/two-s-indie/macroquad/commit/a28a268))
