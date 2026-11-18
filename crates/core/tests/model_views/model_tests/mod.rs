//! The model's drawings driven through the Session: plans, sections, the 3D view, graphics, sheets.

mod background_grid;
mod dims3d;
mod fills;
mod gltf;
mod graphics;
mod graphics_category_tests;
mod graphics_fill_tests;
mod inquiry;
mod lines;
mod lines_pattern_tests;
mod pat;
mod pdf;
mod pen;
mod render;
mod render_regions_tests;
mod schedule;
mod sheet;
mod snap;
mod styles;
mod triad;
mod view3d;
mod view3d_fragments;
mod view3d_gpu;
mod workplane;

#[allow(dead_code)]
#[path = "../../common/graphics_testing.rs"]
pub(crate) mod graphics_testing;
