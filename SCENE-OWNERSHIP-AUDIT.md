# Scene ownership baseline

Step 1 of [SCENE-CONTEXT-PLAN.md](SCENE-CONTEXT-PLAN.md), on
`feature/scene-context-safety`. This audit records existing behavior and decisions
for the migration. It does not certify the existing unsafe implementation.

## Current behavior

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

`Scene::new_at` and `Scene::update_with` supply explicit time and camera hooks
for tests. Production wrappers retain the actual clock and camera operations.
The hooks preserve clock sampling after ready and do not change callback ownership.

## Retained pointers and safety requirements

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

## Selected ownership and handle strategy

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

## Consumer audit

- `tests/coroutine_pause.rs` captures scene handles in `start_coroutine` futures.
  Preserve this use while rejecting wrong-scene access at lookup.
- `src/experimental/coroutines.rs` uses `Lens` for `tweens::linear` and
  `follow_path`; both must migrate with guards.
- Searches of the checked-out sibling `Where-s-the-Yokai` Rust sources found no
  direct scene API usage. This is not evidence about other downstream consumers.

## Regression coverage and verification

Tests live in [scene/tests.rs](src/experimental/scene/tests.rs). A serialized fixture
installs a headless scene, restores the previous global on unwind, and explicitly
drops remaining test nodes. This fixture is not the production teardown solution.
Registry clearing is tested directly; public coroutine cancellation and actual
rendering remain later integration checks.

Nine passing tests cover independent/conflicting guards, stale generations and
slot reuse, persistence and destruction, type/capability queries, iterator changes,
clock sampling, fixed-step boundaries and snapping, camera pass ordering, callback
addition, and callback deletion of self/another node.

Five ignored acceptance tests are executable reproducers, each annotated with
its owning migration step:

| Test | Required correction |
| --- | --- |
| `rejected_clear_leaves_all_nodes_registered` | Preflight clearing before any removal. |
| `stale_delete_does_not_remove_reused_slot` | Validate generation before taking an entry. |
| `forged_typed_handle_is_rejected` | Check the stored type at lookup. |
| `foreign_scene_handle_is_rejected` | Validate scene/session identity. |
| `immediate_slot_reuse_does_not_duplicate_iteration` | Validate snapshot generations. |

Running `cargo test --lib experimental::scene::tests -- --ignored` currently
fails all five as expected. Remove each ignore when its correction is implemented;
do not invert the assertions to accept the defect. Alignment, lens lifetime,
destructor reentry, thread/session teardown, and panic recovery need additional
tests during steps 2 and 3 after their unsafe paths have been replaced.

Miri is unavailable in the installed nightly toolchain. No Miri or graphics
runtime validation is claimed for this step. The five existing main-library
`static_mut_refs` warnings are intentionally unchanged until global replacement.

Verification on Windows:

- `cargo test --lib`: 33 passed, 5 ignored (the known defects above).
- `cargo build`: passed, with the same five global-reference warnings.
- `cargo check --all-targets`: passed; the existing `macroquad-tiled` lifetime
  warning also remains. Graphics tests/examples were compiled, not executed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
