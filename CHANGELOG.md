# Changelog

Changes in the [two-s-indie/macroquad](https://github.com/two-s-indie/macroquad)
fork's `game` branch, maintained for **Where's the Yokai?**.

This history starts after the branch's common ancestor with `master`,
[`8d602d3`](https://github.com/two-s-indie/macroquad/commit/8d602d3)
(`text: avoid spurious font atlas growth`). Earlier upstream changes are outside
this changelog's scope. Dates below are commit dates, not release dates.

## Unreleased

### Changed

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
