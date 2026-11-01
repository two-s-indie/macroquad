# DPI diagnostic

This example pairs with the sibling Miniquad checkout's
[validation guide](../../miniquad/docs/windows-dpi-validation.md). It renders
an 800 x 600 logical-size diagnostic with a centered 200 x 120 rectangle,
text, a mouse marker, and a frame counter.

From the Macroquad repository root, explicitly select the local Miniquad fork:

```powershell
cargo tree --config 'patch.crates-io.miniquad.path="../miniquad"' -i miniquad
cargo run --example dpi_diagnostic --config 'patch.crates-io.miniquad.path="../miniquad"'
cargo run --example dpi_diagnostic --config 'patch.crates-io.miniquad.path="../miniquad"' -- --blocking
```

Confirm that the tree shows the sibling Miniquad directory. Without this
override, the example uses the registry dependency from Cargo.toml.

Optional flags can be combined:

- `--blocking`: redraw on configured input events and backend resize/DPI updates.
- `--low-dpi`: set Miniquad's high-DPI rendering preference to false.
- `--odd-size`: request 801 x 601 at startup to reveal rounding losses.

Keys: R requests 800 x 600 logical units; O requests 801 x 601; F toggles
fullscreen; C toggles cursor grab; P logs a snapshot; Escape exits.

The rectangle turns gold when the reported logical mouse position is inside
it. Logs capture scale, framebuffer dimensions, logical dimensions, mouse
position, and frame number. There is no application-side DPI polling or
automatic corrective resize. Runtime resize requests use Macroquad's existing
API so double-scaling and rounding differences remain observable.

The initial Windows run on 2026-11-01 used backend commits Miniquad
`2080ce3` and Macroquad `a28a268`. At a reported scale of 1.75, an 800 x 600
startup request produced a reported framebuffer of 799 x 598 and logical size
of approximately 456.571 x 341.714. This is a recorded failing baseline, not a
fixed result. The validation guide lists the checks still pending.

## Current Windows behavior

With the paired DPI forks, startup dimensions and Macroquad size requests are
logical client units. Miniquad reports framebuffer pixels and the current scale;
Macroquad divides by that scale for screen dimensions, drawing, and absolute
mouse coordinates. Windows runtime requests now round to the nearest pixel,
matching startup: 801 x 601 at 175% gives 1402 x 1052 pixels and approximately
801.1429 x 601.1429 reported logical units. The small fraction is expected pixel
rounding. Other backends retain their existing size conversion.

Text is rasterized/cached at the current DPI-dependent font size. The Windows
backend refreshes stationary absolute mouse coordinates after a DPI transition
or release of a cursor grab. Grabbed coordinates remain virtual raw-motion
accumulation. Clicks use their own client coordinates, even without a preceding
motion event.

For Windows IME placement through the re-exported Miniquad API, multiply a
logical insertion point by the current scale and round once. Resubmit it after
DPI or layout changes. Composition startup preserves that client-pixel anchor;
the IME may adjust the visible popup. See the paired validation guide for the
coordinate table, awareness fallbacks, and example code.

## Automated integration test

Run from this checkout with both feature branches checked out:

```powershell
cargo test --test windows_dpi_integration --config 'patch.crates-io.miniquad.path="../miniquad"' -- --ignored --nocapture --test-threads=1
```

This opt-in test requires the sibling Miniquad DPI fork, Windows 10 1703+,
an interactive desktop, OpenGL, and IMM support. It briefly clips the cursor
and restores its original position and clip rectangle. Set
`MACROQUAD_DPI_ARTIFACTS` to a directory to save framebuffer PNGs.

Four isolated modes cover high/low DPI with normal/blocking event loops. Checks
include native moves across available displays, injected 100-200% transitions,
actual GPU text and rectangle pixels, screen queries, even/odd runtime requests,
stationary pointer refresh, clicks without motion, signed release coordinates,
cursor clip rectangles, and IME anchor persistence through composition startup.
DPI awaits do not use a test timer or unconditional frame request. Native
monitor moves and explicit test commands request a frame separately.

The 2026-11-01 run passed on a 175% primary display and a 100% display to its left,
including moves out and back. All injected scales passed in all four modes;
IMM coordinate queries passed seven times per mode. GPU captures at 125% and
175% were visually inspected. Physical dragging, Windows Settings scale changes,
other real scale combinations, and visible IME popups remain interactive checks.
The Miniquad guide records full results and compatibility limits. The game's
Git dependency revisions have not been changed by this integration work.
