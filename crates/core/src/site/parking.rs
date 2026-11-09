//! A car park laid out over an area: its aisles, bays (stalls), accessible bays, columns and ramps, level by level. The
//! port of Arqiflow's web/lib/parking (carpark.ts, solver.ts, geom.ts, standards.ts), which lays out a car park the
//! person drags and the one the agent makes the same way.
//!
//! STUB: the signatures below are the contract the element is built against. The bodies are filled in by the parking
//! port; it may add to these types, never change what is here.

use super::{Poly, Props};
use crate::geom::P2;

/// A level the car park may stand on: its name and elevation, mm.
#[derive(Clone, Debug)]
pub struct LevelIn {
    pub name: String,
    pub elevation: f64,
}

/// Something already on the site the layout keeps clear of (a building, a core, a ramp of another car park): its
/// outline and the level it stands on.
#[derive(Clone, Debug)]
pub struct Obstacle {
    pub kind: String,
    pub level: String,
    pub outline: Vec<Poly>,
}

/// One piece of a laid-out car park: an "aisle", "stall", "column", "ramp", … (carpark.ts `carParkChildren`), on its
/// level, with its own props (a stall's `accessible`, its angle…) and outline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Piece {
    pub kind: String,
    pub level: String,
    pub props: Props,
    pub outline: Vec<Poly>,
}

/// The layout: its pieces and its figures (carpark.ts `carParkResult`: stalls, accessible stalls, levels, area per
/// stall…).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub pieces: Vec<Piece>,
    pub result: Props,
}

/// The car park whose area and choices are `props` (its `control` ring, mm, and the person's choices), laid out on
/// `level` among `levels`, keeping clear of `obstacles`, its way in towards the nearest of `streets` (street edges of the
/// plot, mm).
pub fn lay_out(props: &Props, level: &str, levels: &[LevelIn], obstacles: &[Obstacle], streets: &[(P2, P2)]) -> Layout {
    let _ = (props, level, levels, obstacles, streets);
    Layout::default()
}
