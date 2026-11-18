# bimcraft-render

Everything that draws the model: plans, sections and elevations (`render`, with `render/` for dimensions, hatches, regions,
the section cut and the site), the 3D view (`view3d`: the scene, the camera, the picture, picking, the GPU mesh and its
updates, the cursor plane its tools draw on), sheets and their title blocks drawn (`sheet`), 3D dimensions drawn and picked
(`dims3d`), the view's axes (`triad`), snapping (`snap`), what is under the cursor (`inquiry`), and what a drawing is
written as (`pdf`, `dxf_out`, `gltf`).

- **Above the model, beside IFC.** It reads `bimcraft-model` (the elements, and what each is in 3D: `solids`), never the
  commands. Nothing in the model draws: what the model needs of a drawing's numbers is the model's (`prim::DENSITY`,
  `prim::INK`, `Dimension3D::length`, `titleblock::sheet_values`), and what it would need of a drawing itself is added
  above (a viewport's size on its sheet, by the core's `element_json`). A change here rebuilds this crate and the core;
  the model and IFC are reused.
- The model's modules are re-exported in `lib.rs` under their usual paths (`crate::model`, `crate::walls`), so code moved
  here kept its `use` lines; `bimcraft-core` re-exports this crate's modules under theirs.
- Tests that need only the model and this crate stay beside their modules (`render_tests.rs`, `view3d_tests.rs`); those
  that drive the Session are the core's (`tests/model_views/model_tests/render.rs`, `view3d.rs`).
