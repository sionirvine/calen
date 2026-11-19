# Local changes to iced_wgpu 0.14.0

Copied from crates.io (MIT, see the iced repository) and used through
`[patch.crates-io]` in the workspace `Cargo.toml`.

- `triangle::msaa::State::render` scissors its blit to the layer being drawn.
  With anti-aliasing on, each layer's meshes are drawn into a multisampled target
  and then blitted onto the frame. Unscissored, that full-screen blit put earlier
  layers' shapes back over their text. In Calen the week header's background
  covered its labels (weekday names, day numbers, all-day titles) wherever the
  scrollable week grid followed it. It happens with Vulkan, GL and llvmpipe, so
  it isn't a driver bug.
