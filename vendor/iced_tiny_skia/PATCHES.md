# Local changes to iced_tiny_skia 0.14.1

Copied from crates.io (MIT, see the iced repository) and used through
`[patch.crates-io]` in the workspace `Cargo.toml`.

- `engine::adjust_clip_mask` skips rebuilding the clip mask when it already holds
  the requested rectangle. Unpatched, every layer, primitive group and partly
  clipped text clears and refills a window-sized mask. At 2560×1600 that was
  about 40% of Calen's CPU time while redrawing.

Remove this copy when Calen moves to an iced release whose CPU renderer doesn't
rebuild the mask per draw call. (iced's main branch has replaced tiny-skia with
`vello_cpu`.)
