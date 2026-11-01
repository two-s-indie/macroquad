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
