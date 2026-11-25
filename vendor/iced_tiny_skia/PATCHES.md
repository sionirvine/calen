# Local changes to iced_tiny_skia 0.14.1

Copied from crates.io (MIT, see the iced repository) and used through
`[patch.crates-io]` in the workspace `Cargo.toml`.

- `engine::adjust_clip_mask` skips rebuilding the clip mask when it already holds
  the requested rectangle. Unpatched, every layer, primitive group and partly
  clipped text clears and refills a window-sized mask. At 2560×1600 that was
  about 40% of Calen's CPU time while redrawing.

- `engine::draw_text` always clips editor text (`text_editor`) to its clip bounds.
  Unpatched, it skipped the mask whenever the editor's nominal bounds fitted, but
  the half-scrolled first and last lines are drawn in full past those bounds;
  the damage tracker never repainted them, so scrolling notes left smeared lines
  above and below the box.
- `engine::draw_image` honours an image's own `clip_bounds` (and SVGs'), which
  `ContentFit::Cover` relies on to crop, and `raster::Pipeline::draw` rounds the
  corners (`border_radius`, applied to that clip) by filling a rounded path with
  the image as a pattern. Unpatched, both were dropped: thumbnails spilled over
  their neighbours and had square corners. The GPU renderer already did both.

Remove this copy when Calen moves to an iced release whose CPU renderer doesn't
rebuild the mask per draw call. (iced's main branch has replaced tiny-skia with
`vello_cpu`.)
