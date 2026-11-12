//! The site and masterplan, drawn in the same model as the building (Arqiflow's "site option B";
//! [docs/areas/site.md](../../../../docs/areas/site.md)): roads and lanes, parking, paving, landscape, water, planting,
//! pitches and play, hedges, streams and ramps, each a `site` element holding what a person drew (`props`, its truth) and
//! the shape everything reads (`outline`, worked out from the truth by [`shapes::normalise`]). A car park's aisles, bays and
//! columns are laid out by [`parking::lay_out`].
//!
//! This module is pure geometry: it knows nothing of the model's elements, so the shapes can be tested against Arqiflow's
//! fixtures on their own. The element, its commands and its drawing live where every element's do (`model.rs`,
//! `commands.rs`, `render.rs`, `view3d.rs`).
//!
//! Units: millimetres, x east, y north; angles degrees counter-clockwise from x. Points are `[x, y]`, a path's points
//! `[x, y]` or `[x, y, bulge]` (the bulge as DXF has it, on the piece that starts at that point: 0 straight, the tangent of
//! a quarter of the arc's angle, positive counter-clockwise).

pub mod parking;
pub mod shapes;

use serde::{Deserialize, Serialize};

use crate::geom::P2;

/// What a person drew, by name, in millimetres (lengths and points), degrees (angles) and plain numbers (counts, a
/// gradient in percent, a bulge). The names and what each kind takes are in docs/areas/site.md, the same as Arqiflow's
/// masterplan elements' `props` with lengths in millimetres instead of metres.
pub type Props = serde_json::Map<String, serde_json::Value>;

/// A polygon of the plan: its outline counter-clockwise, its holes clockwise, mm.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Poly {
    pub outer: Vec<P2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<P2>>,
}
