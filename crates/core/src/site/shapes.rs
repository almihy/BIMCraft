//! Every site kind's shape from what was drawn: the port of Arqiflow's `normalise` (server/arqiflow/kernel/site_kinds.py,
//! roads.py; the browser's web/components/editor/geom/site.ts and roads.ts). The fixtures Arqiflow keeps its two
//! implementations within 5 mm by (tests/fixtures/site_kinds/) keep this one too.
//!
//! STUB: the signatures below are the contract the element (model.rs, commands.rs) is built against. The bodies are
//! filled in by the shapes port; it may add to these types, never change what is here.

#[path = "poly.rs"]
pub mod poly;

use std::collections::BTreeMap;

use super::{Poly, Props};
use crate::geom::P2;

/// One site element as the shapes read it.
#[derive(Clone, Debug)]
pub struct Item<'a> {
    /// What it is known by, stable across edits (its GUID as text): a junction's corner is kept by the roads it joins.
    pub key: &'a str,
    /// Its kind: "lane", "water", "stream", "tree", "hedge", "pitch", "playground", "play_area", "bike_rack",
    /// "parking_band", "car_park", "landscape", "paving", "terrace", "ramp".
    pub kind: &'a str,
    /// Its use (a lane's "lane", "footpath", "road", "promenade"; a pitch's "tennis"…); empty for the kind's default.
    pub use_: &'a str,
    pub props: &'a Props,
    /// Its phase, as Arqiflow's: "new", "existing", "demolished" or "planned".
    pub phase: &'a str,
    /// Context: what was fetched around the plot (a street of the survey, OpenStreetMap's roads), not the design.
    pub context: bool,
}

/// What a part of an element's shape is, for drawing and building it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    /// "carriageway", "pavement", "median", "kerb", "marking", "centre_line", "turning_head", "fillet", "crown",
    /// "covered", "open", "pad", "run_off", … (docs/areas/site.md).
    pub role: String,
    pub polys: Vec<Poly>,
    pub lines: Vec<Vec<P2>>,
}

/// An element's shape: its outline (what is selected, measured and kept clear) and its parts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shaped {
    pub outline: Vec<Poly>,
    pub parts: Vec<Part>,
    /// What was not given and was filled in from the figures (a lane's width, a tree's crown), to be kept in its props.
    pub filled: Props,
    /// What the person should be told (the carriageway is wider than the right of way…).
    pub notes: Vec<String>,
}

/// Arqiflow's figures (site_kinds.FIGURES), by the name a firm sets its own by, in mm for lengths.
#[derive(Clone, Debug, PartialEq)]
pub struct Figures(pub BTreeMap<String, f64>);

impl Default for Figures {
    fn default() -> Self {
        Figures(BTreeMap::new())
    }
}

/// Every item's shape, in the order given. Roads that meet are worked out together (their junctions' corners), so the
/// whole site's items are given at once.
pub fn normalise(items: &[Item<'_>], figures: &Figures) -> Vec<Shaped> {
    let _ = figures;
    items.iter().map(|_| Shaped::default()).collect()
}
