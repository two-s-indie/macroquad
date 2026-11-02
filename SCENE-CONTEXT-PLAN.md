# Scene and context safety plan

Fix the remaining mutable-global warnings on the `game` branch by making
ownership and borrowing explicit. Complete `SCENE` first, then migrate `CONTEXT`
in small, independently verifiable changes.

The latest verified `cargo build` reports five `static_mut_refs` warnings:
one from `SCENE` and four from `CONTEXT`. The `THREAD_ID`, `STORAGE`, and
`PROFILER` changes are already implemented. Step 1 is complete; neither remaining
subsystem is yet declared safe. The warning count is unchanged.

## Working rules

- Keep each numbered step suitable for a separate commit and update
  `CHANGELOG.md` with its actual changes. Mark steps complete only after their
  acceptance checks pass; record compatibility decisions alongside the step.
- Preserve ordinary game-facing APIs where practical. Identify unavoidable
  signature or behavior changes before implementing them, and check usages in
  the sibling game repository.
- Do not treat warning suppression, raw-pointer casts, or an `UnsafeCell`
  wrapper around the existing `&'static mut` accessors as a completed fix.
- Do not hold a scene/context borrow across user callbacks, future polling,
  or destructors that may call back into Macroquad.
- Keep thread ownership explicit. Thread-local storage alone does not validate
  handles from another thread or from a previous engine session.
- Avoid expanding into unrelated warning cleanup or rendering changes.

## Part one SCENE

Relevant code: [scene.rs](src/experimental/scene.rs),
[arena.rs](src/experimental/scene/arena.rs), and scene-based tweens in
[coroutines.rs](src/experimental/coroutines.rs).

### 1 Establish scene behavior and ownership requirements

- [x] Map node creation, lookup, iteration, deletion, clearing, persistence,
  capability access, and `ready`/`update`/`fixed_update`/`draw` callbacks.
- [x] Make time inputs explicit where needed so ownership and scheduling tests
  can run without constructing a graphics context.
- [x] Add focused regression coverage for stale generations, slot reuse,
  simultaneous access to different nodes, conflicting access to one node,
  and additions/deletions during callbacks and iteration.
- [x] Establish the intended behavior of `clear()` with live node guards and
  persistent nodes. Failed operations must not leave a partially modified scene.
- [x] Audit pointers retained by `RefMut`, `RefMutAny`, and `Lens`: node data,
  borrow flags, capability vectors, type-erased callbacks, and arena storage.
  Include allocation alignment, vector relocation, and type validation.
- [x] Decide how to identify a scene/session in a handle. `Handle<T>` currently
  implements `Send`; preserve uses required by coroutine APIs while rejecting
  lookup in the wrong scene/thread. Do not silently reinterpret a foreign slot ID.

Acceptance: regression tests describe supported behavior, and the ownership and
cross-thread handle strategy is recorded before replacing global storage.
Tests that expose an existing defect become requirements for the next step.

Outcome: [SCENE-OWNERSHIP-AUDIT.md](SCENE-OWNERSHIP-AUDIT.md) records behavior,
pointer hazards, atomic rejection requirements, stable owned guards, and
sendable handles with scene/session identity and owner-thread checks. Nine
headless characterization tests pass; five ignored acceptance tests reproduce
known defects and all fail when explicitly run. Explicit clock/camera hooks keep
production ordering. These checks do not establish memory safety. See the audit
for deferred lens, alignment, callback reentry, and teardown validation.
Validation: `cargo test --lib` passes 33 tests with those five ignored;
`cargo build`, `cargo check --all-targets`, formatting, and whitespace checks pass.
The existing five main-library global-reference warnings and the tiled crate's
lifetime warning remain. Miri is unavailable; graphics paths were only compiled.

### 2 Make node guards and field projections retain valid storage

- [ ] Give each node stable ownership independent of the registry's backing
  vector. Prefer an owned node entry and checked typed access over raw pointers
  into movable registry entries.
- [ ] Make typed and untyped guards retain the node and its borrow state until
  release. Growing the registry, removing an entry, or reusing a slot must not
  invalidate live guards or their capability access.
- [ ] Enforce exclusive mutable access per node. Check node type and generation
  before producing typed access.
- [ ] Define deletion/clear behavior with outstanding guards: reject before
  modifying the registry or defer destruction until release. Preserve existing
  supported behavior and document any change.
- [ ] Repair `Lens` ownership as part of this guard migration: a field borrow
  must retain its parent-node guard. Avoid deriving field offsets by creating a
  reference to an uninitialized `T`.
- [ ] Drop removed node data outside registry borrows so destructors can safely
  access other nodes or engine resources.

Acceptance: tests cover guard survival during registry growth, stale-handle
rejection, exact-once destruction, capabilities, and tween/field-projection
lifetimes. No guard depends on a pointer into a relocatable registry entry.

### 3 Separate scene callbacks and replace the global

- [ ] Refactor scene update into short registry operations and callback dispatch.
  Acquire the node guard and callback information, release the registry borrow,
  invoke user code, then reconcile pending changes.
- [ ] Preserve callback ordering, fixed-step timing, and camera selection. Review
  the audit's early-draw-before-ready behavior and explicitly document any
  correction to callback eligibility for newly added nodes.
- [ ] Ensure coroutine cancellation from scene clearing and node destructors
  happens outside scene registry borrows.
- [ ] Replace `static mut SCENE` and `get_scene() -> &'static mut Scene` with
  thread-local, scoped access using the ownership model from step 2. Remove the
  unsupported blanket `unsafe impl Sync for Scene` if it is no longer needed.
- [ ] Keep time/context reads outside scene borrows where they could re-enter
  the scene. Define initialization and shutdown order explicitly.

Acceptance: scene callbacks can access other nodes, spawn/delete nodes, use
camera and timing APIs, and unwind without leaving borrowing state stuck.
Scene tests and relevant examples pass; the `SCENE` warning is gone. Four
`CONTEXT` warnings may still remain.

## Part two CONTEXT

Relevant code: [lib.rs](src/lib.rs), [window.rs](src/window.rs),
[quad_gl.rs](src/quad_gl.rs), [ui.rs](src/ui.rs),
[texture.rs](src/texture.rs), [material.rs](src/material.rs), and
[coroutines.rs](src/experimental/coroutines.rs).

There are approximately 180 context/backend accessor uses across 17 source
files at the planning baseline. A direct replacement with one
`RefCell<Context>` would encounter existing nested borrows.

### 4 Remove nested renderer and backend access

- [ ] Borrow the renderer and backend together from one context access, passing
  them explicitly into internal rendering helpers instead of calling
  `get_quad_context()` again.
- [ ] Start with `clear_background`, draw-buffer resizing,
  `perform_render_passes`, and frame drawing/commit operations.
- [ ] Pass the already-borrowed backend into telemetry draw-call capture.
- [ ] Supply texture lookup data to rendering helpers without re-entering the
  entire context through `Texture2D::raw_miniquad_id()`.
- [ ] Update internal UI rendering to use the renderer/backend it receives,
  including material initialization and material selection.

Acceptance: the affected rendering paths do not reacquire the context/backend
while already borrowing them. Rendering, material, and render-target lifetime
tests pass. Public drawing APIs remain unchanged.

### 5 Move resource disposal into independent queues

- [ ] Give texture, render-pass, and material guards access to a narrowly owned
  disposal queue rather than requiring the whole context in `Drop`.
- [ ] Drain queues at defined frame boundaries while borrowing the renderer and
  backend explicitly. Preserve queued-draw lifetimes and attachment deletion order.
- [ ] Associate queues with their originating engine session so late resource
  drops cannot delete resources belonging to a newer context.
- [ ] Define behavior after shutdown. Resource destructors must not attempt to
  access an already-destroyed thread-local context or unavailable graphics backend.
- [ ] Preserve currently supported resource thread-affinity behavior; do not
  make graphics operations cross-thread as a side effect of queue changes.

Acceptance: existing material/texture/render-target tests pass, including
dropping resources before queued draws finish. Add targeted checks for nested
destruction, exact-once disposal, and shutdown/session boundaries.

### 6 Move callbacks and future polling outside context borrows

- [ ] Invoke scene updates outside any whole-context borrow.
- [ ] Refactor coroutine scheduling so polling a future holds neither the whole
  context nor an exclusive scheduler borrow that callbacks need to reacquire.
- [ ] Preserve coroutine insertion, cancellation, completion, and manual timing
  semantics while callbacks create or stop coroutines.
- [ ] Gather input/timing snapshots before UI processing, and pass the required
  data into internal UI functions instead of reading globals mid-borrow.
- [ ] Review panic recovery and future destruction. Restore bookkeeping on
  unwind without holding a context borrow while user values are dropped.
- [ ] Move the `one_screenshot` diagnostic's screenshot operation outside the
  context borrow used to increment/check its frame counter.

Acceptance: coroutine and scene callbacks can use drawing, input, timing, and
resource APIs without nested whole-context access. Existing coroutine tests,
panic recovery, and the screenshot configuration remain functional.

### 7 Resolve APIs that return references or guards

- [ ] Keep `root_ui()` usable without retaining a whole-context mutable borrow.
  Use independently owned UI state and a guard with a valid owner/lifetime.
- [ ] Audit other returned references and guards before removing the old
  accessors; references must not escape a temporary context borrow.
- [ ] Decide the compatibility strategy for `get_internal_gl()` and
  `InternalGlContext`. Prefer a scoped closure or explicit guard; if the existing
  unsafe API is retained, specify and enforce its lifetime/exclusivity boundary
  as far as its signature permits.
- [ ] Make `InternalGlContext::flush()` operate through its existing access
  rather than reacquiring the global context.

Acceptance: common UI usage remains valid; low-level GL examples and tests use
the selected access model. Any public API migration is documented and checked
against the sibling game before proceeding.

### 8 Replace CONTEXT storage and define its lifecycle

- [ ] Select the final grouping from the access patterns established above:
  scoped `RefCell` access for independently borrowed subsystems, and explicit
  borrowing of multiple fields for operations that need them together.
- [ ] Replace `static mut CONTEXT`, unrestricted `&'static mut` getters, direct
  initialization, and completion cleanup with the new scoped API.
- [ ] Retain main-thread validation and deliberate support for sequential
  window/test initialization. Do not make the rendering context freely
  accessible from arbitrary threads.
- [ ] Shut down explicitly while the graphics backend remains valid. Release
  user futures, scene/UI resources, disposal queues, and backend resources in a
  tested order instead of relying on thread-local destructor order.
- [ ] Check whether the current `#[no_mangle] CONTEXT` symbol has any external
  consumers before removing or changing its representation.

Acceptance: the four `CONTEXT` warnings are gone without suppressing them;
there are no unrestricted context/backend getters returning `&'static mut`.
Initialization, normal frames, panic recovery, and shutdown tests pass.

### 9 Verify the complete migration

- [ ] Run `cargo build` and confirm no `static_mut_refs` warnings remain in the
  main library. Report unrelated warnings separately rather than hiding them.
- [ ] Run the library tests, relevant graphics integration tests, and checks for
  all targets/features on Windows and `wasm32-unknown-unknown`.
- [ ] Exercise nested UI, scene callbacks, coroutines, material changes,
  render-target disposal, telemetry capture, and screenshot mode in actual
  rendering runs. Compilation alone is insufficient for these paths.
- [ ] Use Miri on isolated ownership/borrowing tests where supported; keep GPU
  calls outside those tests. If unavailable, record that limitation rather than
  claiming aliasing validation from ordinary test success.
- [ ] Run a representative game scene and compare frame time/allocation behavior
  with the baseline. Investigate regressions in hot drawing/input paths before
  adding unchecked fast paths.

Acceptance: documented checks pass, the game-facing compatibility decisions
are resolved, and the changelog describes the final behavior and any limitations.

## Verification for each step

Run formatting and whitespace checks for every change. Run focused regression
tests for the affected subsystem, then the relevant broader build checks once
those pass. Graphics tests require a working backend; report which tests were
executed and which were only compiled. Update this plan with outcomes as work
progresses without marking later steps complete early.

The next implementation task is step 2: make node guards and field projections
retain valid storage. Some preparatory steps will deliberately leave warning counts
unchanged until the corresponding global can be replaced safely.

## Rust reference guidance

- [Mutable static migration](https://doc.rust-lang.org/edition-guide/rust-2024/static-mut-references.html)
  explains why short-lived, controlled access is preferable to global references.
- [UnsafeCell aliasing rules](https://doc.rust-lang.org/std/cell/struct.UnsafeCell.html#aliasing-rules)
  still require mutable references to be exclusive.
- [Thread-local RefCell access](https://doc.rust-lang.org/std/thread/struct.LocalKey.html#impl-LocalKey%3CRefCell%3CT%3E%3E)
  provides scoped borrowing but rejects conflicting reentrant access.
