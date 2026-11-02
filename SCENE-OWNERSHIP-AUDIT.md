# Scene ownership audit

Steps 1 through 3 of [SCENE-CONTEXT-PLAN.md](SCENE-CONTEXT-PLAN.md), on
`feature/scene-context-safety`. The current outcome is recorded first; the
historical outcomes and step 1 baseline follow. Context migration remains open.

## Step 3 outcome

The scene registry is now `thread_local!` `RefCell<Option<Scene>>`. Private access
closures return owned values/guards, never registry references. Read-style APIs
return absence/defaults when inactive or during TLS destruction; mutations require
an initialized session on the calling thread. No lookup initializes a scene or
reads the graphics clock. Window setup explicitly installs a fresh scene after
the graphics context is ready. Session IDs still reject handles from another
thread or earlier session. The scene module contains no unsafe code.

Dispatch snapshots IDs once per frame. The deliberate compatibility correction
is that additions in any callback phase first run next frame, with ready before
other callbacks. A live guard blocking ready also blocks later callbacks for that
node during that frame. Ready is marked attempted before invocation, so a panicking
ready callback is not retried. Callback order, camera slot order, refresh snapping,
and the strict fixed-step boundary are retained. Deletions still take effect
immediately; reused slots cannot enter an existing frame snapshot.

Dispatch and fixed-step flags are restored by guards on unwind. Camera state is
popped if camera setup or drawing panics after a successful push. Recursive
dispatch is rejected without releasing the outer dispatch state. The engine now
calls scene update outside `Context::end_frame` and inside its user-code panic
recovery boundary, so scene callbacks can use camera/timing/drawing APIs without
retaining a whole-context borrow from the scene call site.

Clear validates before cancellation, calls cancellation outside registry access,
and revalidates afterward. Failed registry validation leaves membership unchanged;
side effects already performed by cancellation/destructors are not rolled back.
Explicit coroutine stopping detaches its snapshot before dropping futures, so
destructors may access the scene or create/stop coroutines. Newly created
coroutines survive that cancellation call. Coroutine polling still uses the old
context/scheduler access model; its reentrancy migration remains step 6.

### Session shutdown

On normal completion or accepted quit, the main future is released, coroutines
are canceled while the scene is still available, then the complete registry is
detached and nodes/cameras are dropped outside registry access. This includes
persistent nodes. The backend remains available on these normal exit paths.
Nested shutdown is a no-op, and cancellation panic still detaches the scene.
Outstanding node guards retain their data; their handles cannot address a later
session. During node teardown, optional scene lookup sees an inactive registry;
creating a new node requires a new engine session.

Stage destruction provides a fallback for unwinding or backend exits that bypass
the normal quit path. Its backend availability is platform-dependent; direct
low-level forced quit and graphics resources retained beyond shutdown are not
covered by the normal-exit guarantee. Full resource disposal and context shutdown
remain steps 5 and 8. Ordinary node data also drops at TLS destruction; optional
lookup tolerates the destroyed TLS key, while mutation is rejected.

### Step 3 verification

- `cargo test --lib`: 61 passed, none ignored, including 37 scene tests covering
  actual TLS isolation, repeated sessions, cancellation reentry/panics, shutdown,
  callback/setup unwinding, recursive dispatch, and readiness ordering.
- `cargo test --test scene_lifecycle -- --test-threads=1`: passed. Both child
  processes passed, verifying target pixels drawn by callbacks, time/camera access,
  cancellation reentry, completion and accepted-quit teardown, and GPU access
  from node destructors before backend teardown.
- Miniquad's process-global native display rejects a second window initialization
  with `NATIVE_DISPLAY already set`. This prevented running the existing three
  coroutine tests together in one process. Each scenario must run separately;
  the new integration test handles this process isolation itself. Repeated scene
  sessions are verified headlessly, not by reopening native windows.
- All three `coroutine_pause` tests passed when run in separate processes
  (`coroutine_execution_order`, `coroutine_manual_poll`, and
  `coroutine_manual_poll_delay`, selected with `--exact`).
- `cargo build`, `cargo check --all-targets`, and
  `cargo check --lib --target wasm32-unknown-unknown` passed. Examples were compiled,
  not run. Formatting and whitespace checks passed. The existing tiled lifetime
  warning remains in all-target checks.
- Miri remains unavailable. The four `CONTEXT` warnings remain; none originate
  from scene storage. This does not claim the remaining context code is alias-safe.

## Step 2 outcome (historical)

- Each node uses a correctly aligned `Box<T>` erased through `Any`, plus a stable
  `Rc<NodeEntry>`. A guard takes sole ownership of the boxed value and capabilities
  from the entry; the empty entry rejects competing borrows. Releasing a guard
  returns its value. A detached entry is destroyed when its last guard releases it.
  No guard contains a raw node pointer, an arena pointer, or a reference into the
  registry vector. The arena and unsupported `Sync`/`Send` implementations were
  removed; sendable handles use a non-owning type marker.
- Node types are checked on typed lookup and untyped conversion. Lookup returns
  `None` for mismatches; `to_typed` panics and releases its guard on a mismatch.
  IDs include a unique scene identity and slot generation. Scene identity exhaustion
  fails without wrapping; exhausted slot generations retire the slot. Nodes retain
  thread ownership checks. This does not make concurrent access to the remaining
  mutable global safe; that requires step 3.
- Clear preflights all affected nodes. Borrowed persistent nodes survive; any
  borrowed nonpersistent node rejects the entire registry operation. Public clear
  also preflights before coroutine cancellation and rechecks afterward. Coordination
  with cancellation side effects and engine shutdown remains later work.
  Delete validates before removing membership. Both operations return detached
  entries and destroy them after registry access ends. Destructors can access other
  nodes, add nodes, and unwind without losing unrelated membership or leaving a
  guard's borrow state stuck. Arbitrary destructor side effects are not rolled back.
- Iteration snapshots full IDs; clearing and immediate slot reuse cannot invalidate
  indexes, revive stale entries, or produce duplicates. Slot reuse allocates fresh
  typed data, resets metadata/capabilities, and no longer depends on the old size.
- Callback adapters accept owned untyped guards and check their types before
  invoking `Node` methods. Dispatch no longer holds a registry borrow across user
  code, a necessary prerequisite for deletion inside callbacks. Cameras are retained
  independently for each draw pass. The global accessor, panic recovery for fixed
  flags/camera state, and engine initialization/shutdown are still step 3 work.
  Per-pass ordering, strict fixed timing, and the legacy early-draw-before-ready
  behavior are preserved.
- `Lens::with` applies a retained typed projection to initialized data while owning
  the parent guard. The supplied field operation cannot return a reference tied to
  that temporary borrow. The guard is released on return or panic. There are no
  fabricated uninitialized references, byte offsets, or escaping field references.

### Compatibility and costs

Ordinary scene entry points and guard methods retain their signatures, including
the const handle accessors. A guard survives destruction of its originating registry;
its later `persist` or `delete` cannot affect another session's same-numbered slot.
Handles remain `Copy` and `Send`, including handles to non-`Send` nodes.

The explicit tween type is now `LinearTweenFuture<T, T1, F>` (value, node,
projection). Normal inferred `linear(...)` calls and borrowed projections in
`follow_path` continue to work. The closure is retained and evaluated each poll,
so it may read initialized node state and select a different field. Previously
it ran once against uninitialized memory. A future borrowing a projection cannot
outlive that borrow; a future submitted to a `Send`/`'static` scheduler must own a
projection satisfying those bounds. Projections capturing non-`Send` values may
still be polled locally. No explicit tween type uses were found in the checked-out
game. Headless tests exercise both tween APIs, including non-`Send` nodes and
borrowed closures, and ensure no node guard crosses a poll boundary.

Node allocation now uses a typed box and an entry allocation; each iterator owns
an ID snapshot. Performance measurement remains step 9. `scene_allocated_memory()`
now sums inline payload sizes of registered nodes, including borrowed nodes,
rather than reporting an arena offset. It excludes capabilities, node-owned heap
allocations, registry/guard overhead, and detached nodes from an old scene.

### Step 2 verification

- `cargo test --lib`: 51 passed, none ignored. The 27 scene tests include all five
  formerly ignored defect reproducers, registry growth, detached guards,
  exactly-once destruction, destructor reentry/panics, alignment/zero-size nodes,
  projection lifetimes/unwinding, tween polling, and foreign-thread scene IDs.
- `cargo build`, `cargo check --all-targets`, and
  `cargo check --lib --target wasm32-unknown-unknown`: passed. All five existing
  main-library global-reference warnings remain. The tiled lifetime warning also
  remains in all-target checks. Graphics tests/examples were compiled, not run.
- Formatting and whitespace checks passed. Miri is not installed in the nightly
  toolchain; these ordinary tests do not constitute an aliasing proof.

## Step 1 behavior baseline (before the ownership migration)

| Operation | Observed implementation and compatibility requirement |
| --- | --- |
| Creation | Append a dense ID immediately. Reuse the first free allocation large enough, incrementing its generation and resetting persistence, initialization, and capabilities. Otherwise allocate node bytes and a borrow flag in the arena. |
| Lookup | Null, absent, stale, or borrowed nodes return `None`; `get_node` panics instead. Different nodes can have simultaneous mutable guards. Typed lookup does not validate the actual stored type. |
| Untyped access | `Any` supports inspection. `handle::<T>()` and `to_typed::<T>()` accept arbitrary types; only the type-filtering query helpers currently check the type. |
| Iteration | Snapshot the dense length, then read live entries. Skip missing/borrowed nodes. Later additions are normally excluded from that iterator, but reused slots bypass generation checks and can be visited twice. Clearing during iteration can invalidate indexes. |
| Capabilities | Copy the first capability of the requested type; skip borrowed nodes. Reuse clears capabilities. Guards retain a pointer into a registry entry rather than owning the capability vector. |
| Deletion | Consume the guard, release its flag, remove its cell, run its destructor immediately, and defer dense removal until frame end. Stale internal deletion removes the current cell before checking generation. |
| Clearing | Public `clear()` first cancels all coroutines. Registry clearing removes nonpersistent nodes and preserves persistent nodes, including borrowed persistent nodes. A borrowed nonpersistent node causes a panic after earlier nodes and its own entry may already have been removed. |
| Persistence | A guard marks its slot permanent. The operation does not independently validate generation. Persistence survives ordinary clearing. |
| Callbacks | Separate ready, update, fixed, and camera draw passes, each with its own iterator. Ready runs once, with the initialized flag set before the callback. Borrowed nodes are skipped. Deletion takes effect for subsequent visits. |
| Additions during callbacks | A node added during update can draw that frame, then receive ready next frame. Similarly, additions during ready can receive later callbacks before ready. This is characterized, not endorsed as a lifecycle guarantee. |
| Timing | Sample the clock after ready. Snap deltas within 0.0002 seconds of 1/120, 1/60, or 1/30. Accumulate time and run fixed steps while the accumulator is strictly greater than 1/60. Preserve this boundary and ordering during refactoring. |
| Cameras | Draw once per enabled camera in slot order; set camera position and push/set/pop camera state around each pass. Default: one camera. The current bounds assertion also allows invalid slot 4; leave unrelated API cleanup separate. |

Step 1 introduced `Scene::new_at` and `Scene::update_with` with explicit time and
camera hooks for tests. Step 2 moved dispatch to the module-level `update_with`
to release registry access around callbacks. Production wrappers retain the
actual clock and camera operations, including clock sampling after ready.

### Retained pointers and safety requirements at step 1

| Storage/access | Finding | Migration requirement |
| --- | --- | --- |
| Node bytes | Arena page buffers remain allocated as the outer vectors move, but `Vec<u8>` only guarantees byte alignment. Rounding allocation sizes to pointer width does not establish alignment for arbitrary `T`. Reuse checks size alone. | Use correctly aligned typed allocations; ownership must survive registry relocation/removal. Test over-aligned and zero-sized nodes after replacement. |
| Borrow flag | Arena pointer survives registry growth, but has no owning guard or session lifetime. | Store borrow state in the same stable owned entry as node data. |
| Capabilities | `RefMut` and `RefMutAny` point into `Cell` inside `nodes: Vec<_>`. Growth relocates cells; removal/reuse replaces them. | Guards must own the stable entry containing capabilities. Test capability access through a live guard while the registry grows. |
| Typed data | `RefMutAny::to_typed` casts without type validation; a forged typed handle also reaches unchecked typed lookup. | Check stored `TypeId` before constructing typed access. Failed conversions must release the borrow normally. |
| Trait objects/callbacks | Manually split/rebuild trait objects and cast `fn(RefMut<T>)` pointers to `fn(RefMut<()>)`; this relies on representation/calling assumptions. | Use checked erasure and typed callback adapters, not transmuted trait-object layouts or incompatible function-pointer types. |
| `Lens` | Constructs `&mut T` from uninitialized storage to compute an offset. `get()` then drops its temporary parent guard before returning a field reference. | Project only from an initialized node; retain the parent guard for the entire field borrow. No fake references or unchecked offsets. |
| Registry borrows | Callbacks and destructors run while a whole `&mut Scene` is held, and can reacquire the global. Iterators also reacquire it. | Short registry operations, then release the registry borrow before user code or destruction. Merely wrapping the current methods in `RefCell` would panic on reentry. |
| Unwinding | Fixed callback panic leaves `in_fixed_update` set; draw panic bypasses camera pop. Delete can unwind before forgetting its already-released guard. | Restore flags/camera state and guard bookkeeping on unwind. Test panicking callbacks and destructors after ownership/dispatch changes. |
| Scene lifetime | Scene has no destructor that drops live node values; freeing arena bytes is insufficient. Blanket `unsafe impl Sync for Scene` does not establish safety for arbitrary nodes. | Explicit teardown, exactly-once destruction outside registry borrows, and no unsupported `Sync`. Guards must remain valid through removal/shutdown. |

Tests do not intentionally create misaligned typed nodes, dereference wrongly
typed guards, access relocated capability pointers, or construct a legacy lens.
Those paths require structural fixes before meaningful memory-safety validation.
The normal callback tests exercise the existing unsafe dispatcher; ordinary test
success does not validate its aliasing or function-pointer assumptions.

### Ownership and handle strategy selected at step 1

For step 2, use independently owned, stable node entries, preferably `Rc` entries
with per-node checked borrowing and checked type erasure. Both typed and untyped
guards must keep the entry alive. The registry owns membership, not the lifetime
of a guard's allocation. Node values may remain non-`Send`; rendering and node
access stay on the owning thread.

Keep handles as copyable, sendable identifiers so they can be captured in
`Send` coroutine futures. Add a process-unique scene/session identity to the slot
and generation, and record the owner thread in scene state. Validate identity,
owner thread, slot, generation, and type before access. A new engine session gets
a new identity even if it uses the same thread and slot numbers. Sending a handle
does not permit access to its node from another thread. Reject foreign/stale
handles through optional lookup; retain the panicking convenience lookup. Do not
solve this by requiring all nodes to be `Send` or by silently using the receiving
thread's same-numbered slot. Identity allocation must fail rather than wrap into
a previously used identity.

Preserve rejection of clearing borrowed nonpersistent nodes, but preflight all
affected entries before changing registry state. Persistent nodes remain,
including borrowed persistent nodes. Invalid deletion must validate before taking
an entry. A consuming delete releases its own guard deliberately, then removes
membership and drops the value outside the registry borrow. Coordinate public
clear with coroutine cancellation in step 3: rejection must occur before canceling
coroutines, and cancellation/destruction must run outside registry borrows.
Destructors may reenter; do not promise rollback of arbitrary destructor side
effects or panics, but keep internal state valid.

Iteration should snapshot full identities, skip deleted/borrowed/stale entries,
and never reinterpret a reused slot. Preserve per-pass ordering and fixed timing.
For step 3, review the lifecycle correction explicitly: recommended behavior is
to make nodes added during dispatch eligible starting next frame, with ready
before any other callback. That would change the characterized early-draw behavior;
do not silently introduce it during the step 2 guard migration. Also define
eligibility if a guard prevents ready but is released before a later pass.

Replace lens offsets with a typed projection retained by the tween and applied
under an owned parent guard. This can affect projection closure lifetimes and the
public `LinearTweenFuture` type; work through compatibility in step 2 rather than
adding unchecked lifetime extensions. `follow_path` currently borrows its closure
when constructing each linear tween, so an indiscriminate `'static` bound is not
a compatible solution.

### Consumer audit

- `tests/coroutine_pause.rs` captures scene handles in `start_coroutine` futures.
  Preserve this use while rejecting wrong-scene access at lookup.
- `src/experimental/coroutines.rs` uses `Lens` for `tweens::linear` and
  `follow_path`; both must migrate with guards.
- Searches of the checked-out sibling `Where-s-the-Yokai` Rust sources found no
  direct scene API usage. This is not evidence about other downstream consumers.

### Step 1 regression coverage and verification

Tests live in [scene/tests.rs](src/experimental/scene/tests.rs). A serialized fixture
installs a headless scene and restores the previous global on unwind. At step 1
it manually dropped remaining node values; step 2 lets owned entries drop them.
This fixture is not the production teardown solution.
Registry clearing is tested directly; public coroutine cancellation and actual
rendering remain later integration checks.

Nine passing tests cover independent/conflicting guards, stale generations and
slot reuse, persistence and destruction, type/capability queries, iterator changes,
clock sampling, fixed-step boundaries and snapping, camera pass ordering, callback
addition, and callback deletion of self/another node.

Five initially ignored acceptance tests supplied executable reproducers:

| Test | Required correction |
| --- | --- |
| `rejected_clear_leaves_all_nodes_registered` | Preflight clearing before any removal. |
| `stale_delete_does_not_remove_reused_slot` | Validate generation before taking an entry. |
| `forged_typed_handle_is_rejected` | Check the stored type at lookup. |
| `foreign_scene_handle_is_rejected` | Validate scene/session identity. |
| `immediate_slot_reuse_does_not_duplicate_iteration` | Validate snapshot generations. |

At step 1, running `cargo test --lib experimental::scene::tests -- --ignored`
failed all five as expected. Step 2 removed their ignores without inverting the
assertions, and added alignment, projection lifetime, destructor reentry, and
ownership tests. Engine teardown and callback panic recovery remain step 3 work.

Miri is unavailable in the installed nightly toolchain. No Miri or graphics
runtime validation is claimed for this step. The five existing main-library
`static_mut_refs` warnings are intentionally unchanged until global replacement.

Step 1 verification on Windows (superseded by the step 2 results above):

- `cargo test --lib`: 33 passed, 5 ignored (the known defects above).
- `cargo build`: passed, with the same five global-reference warnings.
- `cargo check --all-targets`: passed; the existing `macroquad-tiled` lifetime
  warning also remains. Graphics tests/examples were compiled, not executed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
