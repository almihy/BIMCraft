//! Every site kind's shape from what was drawn: the port of Arqiflow's `normalise` (server/arqiflow/kernel/site_kinds.py,
//! roads.py; the browser's web/components/editor/geom/site.ts and roads.ts). The fixtures Arqiflow keeps its two
//! implementations within 5 mm by (tests/fixtures/site_kinds/, copied to `fixtures/`) keep this one too.
//!
//! [`normalise`] takes every site item of a level at once (roads that meet are worked out together) and gives each its
//! [`Shaped`]: its outline (what is selected, measured and kept clear), its parts, what it filled in from the figures
//! (`filled`, to be kept in its props) and what the person should be told (`notes`). Lengths in millimetres, angles in
//! degrees; [`catalog`] says which prop is which, so Arqiflow converts its metres from the engine's own list.
//!
//! The parts' roles:
//! - every path kind (lane, stream, hedge, ramp, parking band): `path`, its centre line (lines);
//! - a lane: `turning_head` and `fillet` (polys: its heads, its rounded corners where it ends on another lane);
//! - a road (a lane of use "road", [`roads`]): `carriageway`, `pavement`, `median`, `paint` (stop and give-way lines,
//!   one-way arrows) as polys; `kerb`, `pavement_edge` (the back of the pavements), `marking` (lane lines),
//!   `centre_line` and `double_centre_line` as lines;
//! - a tree and a bike rack: `point` (lines: one line of one point, where it stands), a tree's `crown`, a rack's `pad`;
//! - a pitch: `run_off` (its playing area with its margins) and `marking` (its painted lines);
//! - water: `bank` (the planted ring inside its outline);
//! - a ramp: `open` (its mouth) and `covered` (the rest);
//! - a parking band: `aisle`, `stall` and `accessible_stall` (one poly per bay, in [`band_bays`]' order).

#[path = "poly.rs"]
pub mod poly;
#[path = "roads.rs"]
pub mod roads;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use self::poly::{Ring, Xy};
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

// ====================================================================== the kinds and their figures
/// How finely an arc is cut into pieces: each within 20 mm of the true arc.
pub const ARC_TOLERANCE: f64 = 20.0;
/// A round play area's, turning head's or tree crown's sides.
pub const CIRCLE_SIDES: usize = 32;
/// The widest half road a lane's end meets: how far from its end a lane's rounded corner may lie.
pub const FILLET_REACH: f64 = 3500.0;
/// The most points a path, a ring or a shape worked out from them may have.
pub const MAX_POINTS: usize = 100_000;
/// The most items one call shapes.
pub const MAX_ITEMS: usize = 100_000;
/// A bike rack's pad across it, and its length per space along it.
pub const BIKE_RACK_DEPTH: f64 = 2000.0;
pub const BIKE_RACK_PER_SPACE: f64 = 400.0;

/// One kind of the site: its uses (the first the default), what it is drawn as, its label and Revit's equivalent.
pub struct Kind {
    pub kind: &'static str,
    pub uses: &'static [&'static str],
    /// "polygon", "point" or "path".
    pub shape: &'static str,
    pub label: &'static str,
    pub revit: &'static str,
}

pub const KINDS: &[Kind] = &[
    Kind { kind: "lane", uses: &["lane", "footpath", "road", "promenade"], shape: "path", label: "Lane", revit: "(roads: Civil's)" },
    Kind { kind: "parking_band", uses: &["parking"], shape: "path", label: "Parking", revit: "Parking Component" },
    Kind { kind: "car_park", uses: &["parking"], shape: "polygon", label: "Car park", revit: "Parking Component, laid out" },
    Kind { kind: "paving", uses: &["forecourt", "apron", "square", "court"], shape: "polygon", label: "Paving", revit: "Toposolid subdivision" },
    Kind { kind: "terrace", uses: &["terrace"], shape: "polygon", label: "Terrace", revit: "Toposolid subdivision" },
    Kind {
        kind: "landscape",
        uses: &["lawn", "meadow", "planting", "garden", "woodland", "orchard", "bank"],
        shape: "polygon",
        label: "Landscape",
        revit: "Toposolid subdivision",
    },
    Kind { kind: "tree", uses: &["tree"], shape: "point", label: "Tree", revit: "Planting" },
    Kind { kind: "hedge", uses: &["hedge"], shape: "path", label: "Hedge", revit: "Planting" },
    Kind { kind: "water", uses: &["pond", "lake", "wetland", "basin"], shape: "polygon", label: "Water", revit: "Site Component" },
    Kind { kind: "stream", uses: &["stream", "rill", "ditch"], shape: "path", label: "Stream", revit: "Site Component" },
    Kind {
        kind: "pitch",
        uses: &["five_a_side", "futsal", "football_7", "football_11", "basketball", "tennis", "padel", "multi_sport"],
        shape: "polygon",
        label: "Pitch",
        revit: "Site Component",
    },
    Kind { kind: "playground", uses: &["playground"], shape: "polygon", label: "Playground", revit: "Site Component" },
    Kind { kind: "play_area", uses: &["equipment", "toddlers", "juniors"], shape: "polygon", label: "Play area", revit: "Site Component" },
    Kind { kind: "bike_rack", uses: &["residents", "visitors"], shape: "point", label: "Bike rack", revit: "Site Component" },
    Kind { kind: "ramp", uses: &["cars"], shape: "path", label: "Ramp", revit: "(a site ramp)" },
];

/// A kind by its name.
pub fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.kind == name)
}

/// A kind's default use.
pub fn default_use(name: &str) -> Option<&'static str> {
    kind(name).and_then(|k| k.uses.first().copied())
}

/// Arqiflow's figures (site_kinds.FIGURES), lengths in mm, and whether each is a length.
pub const FIGURES: &[(&str, f64, &str)] = &[
    ("building.storeys", 2.0, "count"),
    ("building.attic", 1.0, "boolean"),
    ("building.floor_height", 3000.0, "length"),
    ("roof.pitch", 35.0, "angle"),
    ("roof.overhang", 400.0, "length"),
    ("roof.step_drop", 500.0, "length"),
    ("lane.width", 5500.0, "length"),
    ("path.width", 2500.0, "length"),
    ("road.width", 6000.0, "length"),
    ("lane.corner_radius", 3000.0, "length"),
    ("road.corner_radius", 6000.0, "length"),
    ("lane.turning_head_radius", 9000.0, "length"),
    ("lane.hammerhead_arm", 12000.0, "length"),
    ("water.level", -300.0, "length"),
    ("water.depth", 1000.0, "length"),
    ("water.smooth", 3.0, "count"),
    ("stream.width", 2000.0, "length"),
    ("rill.width", 600.0, "length"),
    ("ditch.width", 1000.0, "length"),
    ("tree.crown", 6000.0, "length"),
    ("tree.height", 8000.0, "length"),
    ("hedge.width", 1500.0, "length"),
    ("hedge.height", 2000.0, "length"),
    ("pitch.margin", 2500.0, "length"),
    ("bike_rack.spaces", 10.0, "count"),
    ("ramp.width", 3500.0, "length"),
    ("ramp.open_length", 12000.0, "length"),
    ("ramp.gradient", 15.0, "percent"),
    ("parking.aisle", 6000.0, "length"),
    ("parking.bay_depth", 5000.0, "length"),
    ("parking.bay_width", 2500.0, "length"),
    ("parking.level_height", 3200.0, "length"),
    ("area.attic_factor", 0.6, "number"),
];

/// A named figure: the firm's or the project's (`figures`), else Arqiflow's.
pub fn figure(figures: &Figures, name: &str) -> f64 {
    figures
        .0
        .get(name)
        .copied()
        .filter(|v| v.is_finite() && v.abs() <= poly::MAX_COORD)
        .or_else(|| FIGURES.iter().find(|f| f.0 == name).map(|f| f.1))
        .unwrap_or(0.0)
}

/// A pitch's standard size by its use: length × width, the range a person may type (None: fixed), its label and its
/// own run-off ([ends, sides]) where it has one. mm.
pub struct PitchSize {
    pub use_: &'static str,
    pub size: (f64, f64),
    pub range: Option<((f64, f64), (f64, f64))>,
    pub label: &'static str,
    pub margin: Option<(f64, f64)>,
}

pub const PITCH_SIZES: &[PitchSize] = &[
    PitchSize { use_: "five_a_side", size: (39000.0, 21700.0), range: Some(((25000.0, 42000.0), (16000.0, 25000.0))), label: "Five-a-side", margin: None },
    PitchSize { use_: "futsal", size: (40000.0, 20000.0), range: Some(((38000.0, 42000.0), (18000.0, 25000.0))), label: "Futsal", margin: None },
    PitchSize { use_: "football_7", size: (60000.0, 40000.0), range: Some(((50000.0, 65000.0), (30000.0, 45000.0))), label: "Football 7-a-side", margin: None },
    PitchSize { use_: "football_11", size: (105000.0, 68000.0), range: Some(((90000.0, 120000.0), (45000.0, 90000.0))), label: "Football 11-a-side", margin: None },
    PitchSize { use_: "basketball", size: (28000.0, 15000.0), range: None, label: "Basketball", margin: None },
    PitchSize { use_: "tennis", size: (23770.0, 10970.0), range: None, label: "Tennis", margin: Some((6400.0, 3660.0)) },
    PitchSize { use_: "padel", size: (20000.0, 10000.0), range: None, label: "Padel", margin: None },
    PitchSize { use_: "multi_sport", size: (30000.0, 15000.0), range: Some(((20000.0, 44000.0), (12000.0, 22000.0))), label: "Multi-sport", margin: None },
];

/// A pitch's size by its use; an unknown use is a multi-sport pitch.
pub fn pitch_size(use_: &str) -> &'static PitchSize {
    let u = if use_.is_empty() { "five_a_side" } else { use_ };
    PITCH_SIZES.iter().find(|s| s.use_ == u).unwrap_or(&PITCH_SIZES[PITCH_SIZES.len() - 1])
}

const STREAM_WIDTH: &[(&str, &str)] = &[("stream", "stream.width"), ("rill", "rill.width"), ("ditch", "ditch.width")];
const LANE_WIDTH: &[(&str, &str)] = &[("lane", "lane.width"), ("road", "road.width"), ("footpath", "path.width"), ("promenade", "path.width")];
const LANDSCAPE_SURFACE: &[(&str, &str)] = &[
    ("lawn", "grass"),
    ("meadow", "meadow"),
    ("planting", "planting"),
    ("garden", "planting"),
    ("woodland", "planting"),
    ("orchard", "grass"),
    ("bank", "planting"),
];
/// The surfaces a kind may have (site_kinds.SURFACES).
pub const SURFACES: &[(&str, &[&str])] = &[
    ("paving", &["block_paving", "asphalt", "gravel", "resin", "stone", "grass_pavers", "concrete"]),
    ("lane", &["block_paving", "asphalt", "gravel", "resin", "stone", "grass_pavers", "concrete"]),
    ("landscape", &["grass", "meadow", "planting"]),
    ("stream", &["natural", "stone"]),
    ("pitch", &["grass", "synthetic", "hard"]),
    ("playground", &["safety", "sand", "grass"]),
];

fn lookup(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

// ====================================================================== reading props
fn num(p: &Props, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() <= poly::MAX_COORD)
}

/// A number the way Arqiflow's `p.get(k) or default` reads it: missing, null and 0 take the default.
fn num_or(p: &Props, k: &str, d: f64) -> f64 {
    num(p, k).filter(|v| *v != 0.0).unwrap_or(d)
}

fn missing(p: &Props, k: &str) -> bool {
    p.get(k).is_none_or(Value::is_null)
}

fn xy(v: &Value) -> Option<Xy> {
    let a = v.as_array()?;
    let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
    let p = [x, y];
    poly::ok(p).then_some(p)
}

fn point(p: &Props, k: &str) -> Option<Xy> {
    p.get(k).and_then(xy)
}

/// A ring of points (a pond's `control`): at most MAX_POINTS, every one finite.
fn points(p: &Props, k: &str) -> Result<Option<Ring>, String> {
    let Some(v) = p.get(k).filter(|v| !v.is_null()) else { return Ok(None) };
    let a = v.as_array().ok_or_else(|| format!("`{k}` is a list of points [x, y] in mm."))?;
    if a.len() > MAX_POINTS {
        return Err(format!("`{k}` has more than {MAX_POINTS} points."));
    }
    let r: Option<Ring> = a.iter().map(xy).collect();
    r.map(Some).ok_or_else(|| format!("`{k}` is a list of points [x, y] in mm."))
}

const PATH_HELP: &str = "A path is a list of points [x, y] or [x, y, bulge] in mm.";

/// A path as [x, y, bulge] points, repeated points dropped. Err says what is wrong.
pub fn path_points(v: &Value) -> Result<Vec<[f64; 3]>, String> {
    let a = v.as_array().ok_or_else(|| PATH_HELP.to_string())?;
    if a.len() > MAX_POINTS {
        return Err(format!("A path has at most {MAX_POINTS} points."));
    }
    let mut out: Vec<[f64; 3]> = Vec::new();
    for q in a {
        let q = q.as_array().ok_or_else(|| PATH_HELP.to_string())?;
        let x = q.first().and_then(Value::as_f64).ok_or_else(|| PATH_HELP.to_string())?;
        let y = q.get(1).and_then(Value::as_f64).ok_or_else(|| PATH_HELP.to_string())?;
        let b = match q.get(2) {
            None | Some(Value::Null) => 0.0,
            Some(b) => b.as_f64().ok_or_else(|| PATH_HELP.to_string())?,
        };
        if !poly::ok([x, y]) || !b.is_finite() || b.abs() > 1e6 {
            return Err(PATH_HELP.into());
        }
        if out.last().is_some_and(|l| poly::dist([l[0], l[1]], [x, y]) < 1e-3) {
            continue;
        }
        out.push([x, y, b]);
    }
    if out.len() < 2 {
        return Err("A path needs at least two points apart.".into());
    }
    Ok(out)
}

/// The points along the arc from p0 to p1 of that bulge, p1 included and p0 not; [p1] for a straight piece.
pub fn arc_points(p0: Xy, p1: Xy, bulge: f64, tol: f64) -> Vec<Xy> {
    if bulge.abs() < 1e-9 {
        return vec![p1];
    }
    let c = poly::dist(p0, p1);
    let theta = 4.0 * bulge.atan();
    let r = c * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    let (nx, ny) = (-(p1[1] - p0[1]) / c, (p1[0] - p0[0]) / c);
    let s = c * (1.0 - bulge * bulge) / (4.0 * bulge);
    let (cx, cy) = ((p0[0] + p1[0]) / 2.0 + nx * s, (p0[1] + p1[1]) / 2.0 + ny * s);
    let a0 = (p0[1] - cy).atan2(p0[0] - cx);
    let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { std::f64::consts::FRAC_PI_2 };
    let n = ((theta.abs() / step - 1e-9).ceil().max(1.0)).min(10_000.0) as usize;
    let mut out: Vec<Xy> = (1..n)
        .map(|k| {
            let t = a0 + theta * k as f64 / n as f64;
            [cx + r * t.cos(), cy + r * t.sin()]
        })
        .collect();
    out.push(p1);
    out
}

/// A path's line as points: straight pieces as they are, arcs in pieces no more than `tol` off the true arc.
pub fn densify(v: &Value, tol: f64) -> Result<Vec<Xy>, String> {
    let pts = path_points(v)?;
    let mut out = vec![[pts[0][0], pts[0][1]]];
    for w in pts.windows(2) {
        out.extend(arc_points([w[0][0], w[0][1]], [w[1][0], w[1][1]], w[0][2], tol));
        if out.len() > MAX_POINTS {
            return Err(format!("A path's arcs come to more than {MAX_POINTS} points."));
        }
    }
    Ok(out)
}

/// The part of a line from `s0` to `s1` along it.
pub fn substring(line: &[Xy], s0: f64, s1: f64) -> Vec<Xy> {
    let mut out: Vec<Xy> = Vec::new();
    let mut at = 0.0;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let l = poly::dist(a, b);
        let (lo, hi) = (s0.max(at), s1.min(at + l));
        if hi >= lo && l > 0.0 {
            let pa = [a[0] + (b[0] - a[0]) * (lo - at) / l, a[1] + (b[1] - a[1]) * (lo - at) / l];
            let pb = [a[0] + (b[0] - a[0]) * (hi - at) / l, a[1] + (b[1] - a[1]) * (hi - at) / l];
            if out.last().is_none_or(|q| poly::dist(*q, pa) > 1e-6) {
                out.push(pa);
            }
            if out.last().is_some_and(|q| poly::dist(*q, pb) > 1e-6) {
                out.push(pb);
            }
        }
        at += l;
    }
    out
}

// ====================================================================== defaults
/// What a kind's item does not give and the figures fill in (site_kinds.fill_defaults), in mm: the props to keep.
pub fn fill_defaults(kind_: &str, use_: &str, p: &Props, figures: &Figures) -> Props {
    let mut out = Props::new();
    let f = |n: &str| figure(figures, n);
    let mut set = |k: &str, v: Value| {
        if missing(p, k) {
            out.insert(k.into(), v);
        }
    };
    let use_ = if use_.is_empty() { default_use(kind_).unwrap_or("") } else { use_ };
    match kind_ {
        "lane" => {
            set("width", f(lookup(LANE_WIDTH, use_).unwrap_or("lane.width")).into());
            set("surface", "block_paving".into());
            set("heads", json!({"start": null, "end": null}));
            if use_ == "road" {
                set("corner_radius", f("road.corner_radius").into());
                // The cross-section from the right of way (given, or the one just filled in).
                let mut with = p.clone();
                if missing(p, "width") {
                    with.insert("width".into(), f("road.width").into());
                }
                for (k, v) in roads::fill_road(&with) {
                    set(&k, v);
                }
            } else {
                set("corner_radius", f("lane.corner_radius").into());
            }
        }
        "water" => {
            set("smooth", (f("water.smooth").round() as i64).into());
            set("level", f("water.level").into());
            set("depth", f("water.depth").into());
            set("bank", 0.0.into());
        }
        "stream" => {
            set("width", f(lookup(STREAM_WIDTH, use_).unwrap_or("stream.width")).into());
            set("surface", "natural".into());
        }
        "tree" => {
            set("crown", f("tree.crown").into());
            set("height", f("tree.height").into());
        }
        "hedge" => {
            set("width", f("hedge.width").into());
            set("height", f("hedge.height").into());
        }
        "pitch" => {
            let s = pitch_size(use_);
            set("length", s.size.0.into());
            set("width", s.size.1.into());
            set("angle", 0.0.into());
            set("margin", s.margin.map_or_else(|| f("pitch.margin").into(), |(a, b)| json!([a, b])));
            set("surface", "grass".into());
        }
        "playground" => {
            set("smooth", 0.into());
            set("surface", "safety".into());
        }
        "bike_rack" => {
            set("angle", 0.0.into());
            set("spaces", (f("bike_rack.spaces").round() as i64).into());
            set("type", "hoops".into());
        }
        "ramp" if !missing(p, "path") => {
            set("width", f("ramp.width").into());
            set("open_length", f("ramp.open_length").into());
            set("gradient", f("ramp.gradient").into());
        }
        "parking_band" => {
            set("aisle", f("parking.aisle").into());
            set("bay_depth", f("parking.bay_depth").into());
            set("bay_width", f("parking.bay_width").into());
            set("angle", 90.0.into());
            set("sides", "both".into());
            set("omit", json!([]));
        }
        "landscape" if !missing(p, "control") => {
            set("smooth", 0.into());
            set("surface", lookup(LANDSCAPE_SURFACE, use_).unwrap_or("grass").into());
        }
        "paving" if !missing(p, "control") => {
            set("smooth", 0.into());
            set("surface", "block_paving".into());
        }
        _ => {}
    }
    out
}

// ====================================================================== lanes
/// A turning head at a lane's `end` (0 its start, 1 its end): {shape: "round", radius, side} or {shape: "hammerhead",
/// arm, side}. `side` "left" or "right" (looking out of the lane) bulges that way only, the lane's edge on the other side
/// running straight on; "both" either way.
pub fn turning_head(line: &[Xy], width: f64, end: usize, head: &Value, figures: &Figures) -> Option<Ring> {
    let h = head.as_object()?;
    let (p, d) = roads::end_of(line, end);
    let n = poly::left_of(d);
    let k = match h.get("side").and_then(Value::as_str) {
        Some("left") => 1.0,
        Some("right") => -1.0,
        _ => 0.0,
    };
    let w2 = width / 2.0;
    if h.get("shape").and_then(Value::as_str) == Some("hammerhead") {
        let arm = num_or(h, "arm", figure(figures, "lane.hammerhead_arm"));
        let (lo, hi) = if k > 0.0 { (-w2, arm) } else if k < 0.0 { (-arm, w2) } else { (-arm, arm) };
        let r: Ring = [(lo, -width), (hi, -width), (hi, 0.0), (lo, 0.0)]
            .iter()
            .map(|(s, t)| [p[0] + n[0] * s + d[0] * t, p[1] + n[1] * s + d[1] * t])
            .collect();
        return Some(poly::ccw(r));
    }
    let radius = num_or(h, "radius", figure(figures, "lane.turning_head_radius"));
    let off = (radius - w2).max(0.0) * k;
    Some(poly::circle(poly::at(p, n, off), radius, CIRCLE_SIDES))
}

/// A lane's own outline, before it joins others: its centre line at one width, and its turning heads (also given
/// apart).
pub fn lane_own(line: &[Xy], width: f64, heads: Option<&Value>, figures: &Figures) -> (Vec<Ring>, Vec<Ring>) {
    let mut parts = poly::band(line, width);
    let mut hs = Vec::new();
    for (end, name) in [(0, "start"), (1, "end")] {
        if let Some(h) = heads.and_then(|h| h.get(name)).and_then(|h| turning_head(line, width, end, h, figures)) {
            hs.push(h.clone());
            parts.push(h);
        }
    }
    (poly::union(&parts), hs)
}

/// Where this lane's ends meet other lanes: the inside corners rounded by `radius` (the pieces a closing of the two
/// leaves near this lane's end; the lane that ends at a junction takes its corners).
pub fn lane_fillets(own: &[Ring], line: &[Xy], width: f64, others: &[&Vec<Ring>], radius: f64) -> Vec<Ring> {
    if !(radius > 0.0) || others.is_empty() || line.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for p in [line[0], line[line.len() - 1]] {
        let near: Vec<&&Vec<Ring>> = others.iter().filter(|o| poly::rings_point_distance(o, p) <= width / 2.0 + 10.0).collect();
        if near.is_empty() {
            continue;
        }
        let mut all: Vec<Ring> = own.to_vec();
        for o in &near {
            all.extend(o.iter().cloned());
        }
        let both = poly::union(&all);
        for g in poly::closing_pieces(&both, radius, 100.0) {
            let gs = std::slice::from_ref(&g);
            if poly::rings_distance(gs, own) < 1.0 && poly::rings_point_distance(gs, p) <= width / 2.0 + radius + FILLET_REACH {
                out.push(g);
            }
        }
    }
    out
}

// ====================================================================== pitches
/// A pitch's run-off at its ends and at its sides.
pub fn pitch_margins(p: &Props) -> (f64, f64) {
    match p.get("margin") {
        Some(Value::Array(a)) if a.len() == 2 => {
            let g = |i: usize| a[i].as_f64().filter(|v| v.is_finite() && v.abs() <= poly::MAX_COORD).unwrap_or(0.0);
            (g(0), g(1))
        }
        Some(v) => {
            let m = v.as_f64().filter(|v| v.is_finite() && v.abs() <= poly::MAX_COORD).unwrap_or(2500.0);
            (m, m)
        }
        None => (2500.0, 2500.0),
    }
}

/// A warning when a size is outside what the type allows (a person may still keep it).
pub fn pitch_outside_range(use_: &str, length: f64, width: f64) -> Option<String> {
    let s = pitch_size(use_);
    let m = |v: f64| {
        let t = format!("{:.3}", v / 1000.0);
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    match s.range {
        None => ((length - s.size.0).abs() > 10.0 || (width - s.size.1).abs() > 10.0)
            .then(|| format!("A {} court is {} × {} m.", s.label.to_lowercase(), m(s.size.0), m(s.size.1))),
        Some(((l0, l1), (w0, w1))) => (!(l0 <= length && length <= l1 && w0 <= width && width <= w1)).then(|| {
            format!("A {} pitch is {}–{} m long and {}–{} m wide.", s.label.to_lowercase(), m(l0), m(l1), m(w0), m(w1))
        }),
    }
}

/// A pitch's painted lines, worked out from its type and size: its outline, the halfway line, the centre circle and
/// the areas at each end (football), the service lines (tennis, padel).
pub fn pitch_markings(use_: &str, c: Xy, angle: f64, l: f64, w: f64) -> Vec<Vec<Xy>> {
    let kind_ = if use_.is_empty() { "five_a_side" } else { use_ };
    let a = angle.to_radians();
    let (d, n) = ([a.cos(), a.sin()], [-a.sin(), a.cos()]);
    let at = |u: f64, v: f64| [c[0] + d[0] * u + n[0] * v, c[1] + d[1] * u + n[1] * v];
    let mut lines = vec![
        vec![at(-l / 2.0, -w / 2.0), at(l / 2.0, -w / 2.0), at(l / 2.0, w / 2.0), at(-l / 2.0, w / 2.0), at(-l / 2.0, -w / 2.0)],
        vec![at(0.0, -w / 2.0), at(0.0, w / 2.0)],
    ];
    let circle_r = match kind_ {
        "five_a_side" | "futsal" | "multi_sport" => Some(3000.0),
        "football_7" => Some(6000.0),
        "football_11" => Some(9150.0),
        "basketball" => Some(1800.0),
        _ => None,
    };
    if let Some(r) = circle_r {
        lines.push(
            (0..=32)
                .map(|k| {
                    let t = 2.0 * std::f64::consts::PI * k as f64 / 32.0;
                    at(r * t.cos(), r * t.sin())
                })
                .collect(),
        );
    }
    let boxes = match kind_ {
        "five_a_side" => Some((6000.0, 10000.0)),
        "futsal" => Some((6000.0, 12000.0)),
        "football_7" => Some((9000.0, 24000.0)),
        "football_11" => Some((16500.0, 40320.0)),
        "basketball" => Some((5800.0, 4900.0)),
        "multi_sport" => Some((5000.0, 8000.0)),
        _ => None,
    };
    if let Some((depth, wide)) = boxes {
        let wide: f64 = f64::min(wide, w);
        for s in [-1.0, 1.0] {
            let (u0, u1) = (s * l / 2.0, s * (l / 2.0 - depth));
            lines.push(vec![at(u0, -wide / 2.0), at(u1, -wide / 2.0), at(u1, wide / 2.0), at(u0, wide / 2.0)]);
        }
    }
    if kind_ == "tennis" {
        for s in [-1.0, 1.0] {
            lines.push(vec![at(s * 6400.0, -4115.0), at(s * 6400.0, 4115.0)]);
            lines.push(vec![at(-l / 2.0, s * 4115.0), at(l / 2.0, s * 4115.0)]);
        }
        lines.push(vec![at(-6400.0, 0.0), at(6400.0, 0.0)]);
    }
    if kind_ == "padel" {
        for s in [-1.0, 1.0] {
            lines.push(vec![at(s * 6950.0, -w / 2.0), at(s * 6950.0, w / 2.0)]);
        }
        lines.push(vec![at(-6950.0, 0.0), at(6950.0, 0.0)]);
    }
    lines
}

// ====================================================================== buildings
/// A curved building from its arc {centre, radius, depth, a0, a1, segments} (mm, degrees): the band of that depth
/// about the arc, and its parts, each with its ridge's direction, tangent to the arc. None when it is not one.
pub fn arc_footprint(arc: &Props) -> Option<(Ring, Vec<(Ring, f64)>)> {
    let c = point(arc, "centre")?;
    let (r, depth, a0, a1) = (num(arc, "radius")?, num(arc, "depth")?, num(arc, "a0")?, num(arc, "a1")?);
    let segs = num(arc, "segments")
        .filter(|s| *s >= 1.0)
        .map(|s| s.trunc())
        .unwrap_or_else(|| ((a1 - a0).abs() / 10.0).round().max(1.0))
        .clamp(1.0, 3600.0) as usize;
    let (ri, ro) = (r - depth / 2.0, r + depth / 2.0);
    let angles: Vec<f64> = (0..=segs).map(|k| (a0 + (a1 - a0) * k as f64 / segs as f64).to_radians()).collect();
    let outer: Vec<Xy> = angles.iter().map(|t| [c[0] + ro * t.cos(), c[1] + ro * t.sin()]).collect();
    let inner: Vec<Xy> = angles.iter().map(|t| [c[0] + ri * t.cos(), c[1] + ri * t.sin()]).collect();
    let parts = (0..segs)
        .map(|k| {
            let mid = (angles[k] + angles[k + 1]) / 2.0;
            (poly::ccw(vec![inner[k], outer[k], outer[k + 1], inner[k + 1]]), (mid.to_degrees() + 90.0).rem_euclid(180.0))
        })
        .collect();
    let mut ring = inner;
    ring.extend(outer.into_iter().rev());
    Some((poly::ccw(ring), parts))
}

// ====================================================================== the parking band
/// Parking stalls in a row on one side of a straight line a–b (Arqiflow's geo.stalls_along): `angle` between the
/// stall's depth and the line (90 square to it), `start` and `end` kept clear at each end.
pub fn stalls_along(a: Xy, b: Xy, width: f64, depth: f64, left: bool, angle: f64, start: f64, end: f64) -> Vec<Ring> {
    let length = poly::dist(a, b);
    if !(length > 0.0) || !(width > 0.0) || !(depth > 0.0) {
        return Vec::new();
    }
    let d = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
    let n = if left { [-d[1], d[0]] } else { [d[1], -d[0]] };
    let t = angle.clamp(1.0, 90.0).to_radians();
    let (sn, cs) = (t.sin(), t.cos());
    let axis = [cs * d[0] + sn * n[0], cs * d[1] + sn * n[1]];
    let across = [sn * d[0] - cs * n[0], sn * d[1] - cs * n[1]];
    let spacing = width / sn;
    let footprint = width * sn + depth * cs;
    let usable = length - start - end;
    if usable < footprint - 1e-6 {
        return Vec::new();
    }
    let count = (((usable - footprint) / spacing + 1e-9).trunc() as usize + 1).min(MAX_POINTS);
    let lift = width * cs;
    (0..count)
        .map(|i| {
            let s = start + i as f64 * spacing;
            let (ox, oy) = (a[0] + d[0] * s + n[0] * lift, a[1] + d[1] * s + n[1] * lift);
            vec![
                [ox, oy],
                [ox + across[0] * width, oy + across[1] * width],
                [ox + across[0] * width + axis[0] * depth, oy + across[1] * width + axis[1] * depth],
                [ox + axis[0] * depth, oy + axis[1] * depth],
            ]
        })
        .collect()
}

/// One bay of a parking band: which piece of its path it is along, on which side, its index along that side, whether
/// it is accessible, its id as Arqiflow writes it ("k" + the band's key + piece + "l"/"r" + index) and its outline.
#[derive(Clone, Debug, PartialEq)]
pub struct Bay {
    pub id: String,
    pub piece: usize,
    pub side: &'static str,
    pub index: usize,
    pub accessible: bool,
    pub outline: Poly,
}

fn listed(p: &Props, k: &str) -> Vec<(i64, String, i64)> {
    p.get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_object)
                .map(|o| {
                    let n = |k: &str, d: i64| o.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).map_or(d, |v| v as i64);
                    (n("piece", 0), o.get("side").and_then(Value::as_str).unwrap_or("").to_string(), n("index", -1))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A parking band's aisles (one per straight piece of its path) and bays (both sides, or one), none within a bay's
/// depth of a corner, none on what is `blocked`, none a person removed (`omit`), those in `accessible` marked so
/// (site_kinds.band_layout). `key` names the bays (Arqiflow's ids: the band's id without its "pb").
pub fn band_bays(key: &str, p: &Props, blocked: &[Ring]) -> Result<(Vec<Ring>, Vec<Bay>), String> {
    let pts: Vec<Xy> = path_points(p.get("path").unwrap_or(&Value::Null))?.iter().map(|q| [q[0], q[1]]).collect();
    let aisle = num(p, "aisle").unwrap_or(6000.0);
    let depth = num(p, "bay_depth").unwrap_or(5000.0);
    let width = num(p, "bay_width").unwrap_or(2500.0);
    let angle = num_or(p, "angle", 90.0);
    if !(angle > 0.0 && angle <= 90.0) {
        return Err("A parking band's angle is more than 0° and at most 90°.".into());
    }
    let sides: &[&'static str] = match p.get("sides").and_then(Value::as_str) {
        Some("left") => &["left"],
        Some("right") => &["right"],
        _ => &["left", "right"],
    };
    let omit = listed(p, "omit");
    let accessible = listed(p, "accessible");
    let has = |l: &[(i64, String, i64)], i: usize, s: &str, j: usize| l.iter().any(|(a, b, c)| *a == i as i64 && b == s && *c == j as i64);
    let mut aisles = Vec::new();
    let mut bays = Vec::new();
    for i in 0..pts.len() - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        let l = poly::dist(a, b);
        if l < 10.0 {
            continue;
        }
        aisles.extend(poly::band(&[a, b], aisle));
        let start = if i > 0 { depth } else { 0.0 };
        let end = if i + 2 < pts.len() { depth } else { 0.0 };
        let (dx, dy) = ((b[0] - a[0]) / l, (b[1] - a[1]) / l);
        for &side in sides {
            let k = if side == "left" { 1.0 } else { -1.0 };
            let o = [-dy * k * aisle / 2.0, dx * k * aisle / 2.0];
            let stalls = stalls_along([a[0] + o[0], a[1] + o[1]], [b[0] + o[0], b[1] + o[1]], width, depth, side == "left", angle, start, end);
            for (j, s) in stalls.into_iter().enumerate() {
                if has(&omit, i, side, j) {
                    continue;
                }
                let s = poly::ccw(s);
                if !blocked.is_empty() && poly::area(&poly::intersection(std::slice::from_ref(&s), blocked)) > 10_000.0 {
                    continue;
                }
                bays.push(Bay {
                    id: format!("k{key}{i}{}{j}", &side[..1]),
                    piece: i,
                    side,
                    index: j,
                    accessible: has(&accessible, i, side, j),
                    outline: Poly { outer: s.into_iter().map(poly::to_p2).collect(), holes: Vec::new() },
                });
                if bays.len() > MAX_POINTS {
                    return Err("A parking band of more than 100 000 bays is too long.".into());
                }
            }
        }
    }
    Ok((aisles, bays))
}

// ====================================================================== the shape of one item
fn rings_to(polys_: &[Ring]) -> Vec<Poly> {
    poly::polys_of(polys_)
}

fn part(role: &str, rings: &[Ring]) -> Part {
    Part { role: role.into(), polys: rings_to(rings), lines: Vec::new() }
}

fn line_part(role: &str, lines: Vec<Vec<Xy>>) -> Part {
    Part { role: role.into(), polys: Vec::new(), lines: lines.into_iter().map(|l| l.into_iter().map(poly::to_p2).collect()).collect() }
}

/// A ring made whole: counter-clockwise, its crossings resolved.
fn area_of(ring: Ring) -> Vec<Ring> {
    if ring.len() < 3 {
        return Vec::new();
    }
    poly::union(&[poly::ccw(ring)])
}

/// What one item's truth gives before it meets its neighbours, into `out`; a lane's own outline is returned for the
/// roads and lanes to join.
fn shape_one(it: &Item<'_>, use_: &str, p: &Props, figures: &Figures, out: &mut Shaped) -> Result<Option<(Vec<Xy>, Vec<Ring>)>, String> {
    let path = |p: &Props| densify(p.get("path").unwrap_or(&Value::Null), ARC_TOLERANCE);
    match it.kind {
        "lane" => {
            if missing(p, "path") {
                return Err("A lane is drawn from its `path`.".into());
            }
            let line = path(p)?;
            let width = if use_ == "road" { roads::section_of(p).outer() } else { num(p, "width").unwrap_or(0.0) };
            let (own, heads) = lane_own(&line, width, p.get("heads"), figures);
            if !heads.is_empty() {
                out.parts.push(part("turning_head", &heads));
            }
            out.parts.push(line_part("path", vec![line.clone()]));
            return Ok(Some((line, own)));
        }
        "stream" | "hedge" => {
            if missing(p, "path") {
                return Err(format!("A {} is drawn from its `path`.", it.kind));
            }
            let line = path(p)?;
            out.outline = rings_to(&poly::band(&line, num(p, "width").unwrap_or(0.0)));
            out.parts.push(line_part("path", vec![line]));
        }
        "ramp" => {
            if missing(p, "path") {
                return Err("A ramp is drawn from its `path`.".into());
            }
            let line = path(p)?;
            let width = num_or(p, "width", 3500.0);
            out.outline = rings_to(&poly::band(&line, width));
            let l = roads::length_of(&line);
            let k = num(p, "open_length").unwrap_or(0.0).clamp(0.0, l);
            if k > 10.0 {
                out.parts.push(part("open", &poly::band(&substring(&line, 0.0, k), width)));
            }
            if l - k > 10.0 {
                out.parts.push(part("covered", &poly::band(&substring(&line, k, l), width)));
            }
            out.parts.push(line_part("path", vec![line]));
        }
        "car_park" => {
            // As carpark.ts writes it: to the millimetre, counter-clockwise, from the same first point.
            let ring = points(p, "control")?.ok_or("A car park is drawn from its `control` ring.")?;
            let mut r: Ring = poly::open(&ring.iter().map(|q| [q[0].round(), q[1].round()]).collect::<Vec<_>>());
            if r.len() < 3 {
                return Err("A car park's area needs three points or more.".into());
            }
            if poly::ring_area(&r) < 0.0 {
                r.reverse();
            }
            out.outline = vec![Poly { outer: r.into_iter().map(poly::to_p2).collect(), holes: Vec::new() }];
        }
        "water" | "landscape" | "paving" | "playground" | "terrace" => {
            let ring = points(p, "control")?.ok_or_else(|| format!("A {} is drawn from its `control` points.", it.kind.replace('_', " ")))?;
            let passes = smooth_passes(p, ring.len());
            let rings = area_of(poly::chaikin(&ring, passes));
            if it.kind == "water" {
                let bank = num(p, "bank").unwrap_or(0.0);
                if bank > 0.0 {
                    let inner = poly::grow(&rings, -bank, poly::ARC_TOL);
                    out.parts.push(part("bank", &poly::difference(&rings, &inner)));
                }
            }
            out.outline = rings_to(&rings);
        }
        "play_area" => {
            let radius = num(p, "radius").unwrap_or(0.0);
            if let (Some(c), true) = (point(p, "at"), radius > 0.0) {
                out.outline = rings_to(&[poly::circle(c, radius, CIRCLE_SIDES)]);
            } else if let Some(ring) = points(p, "control")? {
                let passes = smooth_passes(p, ring.len());
                out.outline = rings_to(&area_of(poly::chaikin(&ring, passes)));
            } else {
                return Err("A play area is drawn from its `at` and `radius`, or its `control` points.".into());
            }
        }
        "pitch" => {
            let c = point(p, "at").ok_or("A pitch is placed by its `at`.")?;
            let angle = num(p, "angle").unwrap_or(0.0);
            let (l, w) = (num(p, "length").unwrap_or(0.0), num(p, "width").unwrap_or(0.0));
            out.outline = rings_to(&[poly::oriented_rect(c, angle, l, w)]);
            let (ends, sides) = pitch_margins(p);
            out.parts.push(part("run_off", &[poly::oriented_rect(c, angle, l + 2.0 * ends, w + 2.0 * sides)]));
            out.parts.push(line_part("marking", pitch_markings(use_, c, angle, l, w)));
            out.notes.extend(pitch_outside_range(use_, l, w));
        }
        "bike_rack" => {
            let c = point(p, "at").ok_or("A bike rack is placed by its `at`.")?;
            let spaces = num_or(p, "spaces", 10.0);
            let pad = poly::oriented_rect(c, num(p, "angle").unwrap_or(0.0), (BIKE_RACK_PER_SPACE * spaces).max(800.0), BIKE_RACK_DEPTH);
            out.outline = rings_to(&[pad.clone()]);
            out.parts.push(part("pad", &[pad]));
            out.parts.push(line_part("point", vec![vec![c]]));
        }
        "tree" => {
            let c = point(p, "at").ok_or("A tree is placed by its `at` (a survey's tree by the survey).")?;
            let crown = num(p, "crown").unwrap_or(6000.0).abs();
            let ring = poly::circle(c, crown / 2.0, CIRCLE_SIDES);
            if crown > 0.0 {
                out.outline = rings_to(&[ring.clone()]);
                out.parts.push(part("crown", &[ring]));
            }
            out.parts.push(line_part("point", vec![vec![c]]));
        }
        "parking_band" => {
            // Its bays keep clear of the ramps given with it (the cores of the level are the building's).
            let (aisles, bays) = band_bays(it.key, p, &[])?;
            out.parts.push(part("aisle", &aisles));
            let mut all = aisles.clone();
            for acc in [false, true] {
                let polys: Vec<Poly> = bays.iter().filter(|b| b.accessible == acc).map(|b| b.outline.clone()).collect();
                if !polys.is_empty() {
                    out.parts.push(Part { role: if acc { "accessible_stall" } else { "stall" }.into(), polys, lines: Vec::new() });
                }
            }
            all.extend(bays.iter().map(|b| b.outline.outer.iter().map(|q| poly::of_p2(*q)).collect::<Ring>()));
            let whole = poly::union(&all);
            let line = path(p)?;
            out.outline = rings_to(&if whole.is_empty() { poly::band(&line, num(p, "aisle").unwrap_or(6000.0)) } else { whole });
            out.parts.push(line_part("path", vec![line]));
        }
        other => return Err(format!("A site element is one of: {}; not \"{other}\".", KINDS.iter().map(|k| k.kind).collect::<Vec<_>>().join(", "))),
    }
    Ok(None)
}

/// Chaikin's passes as given (0 to 5), fewer where the points would come to more than MAX_POINTS.
fn smooth_passes(p: &Props, n: usize) -> i64 {
    let mut passes = num(p, "smooth").unwrap_or(0.0).clamp(0.0, 5.0) as i64;
    while passes > 0 && n.saturating_mul(1 << passes) > MAX_POINTS {
        passes -= 1;
    }
    passes
}

/// Every item's shape, in the order given. Roads that meet are worked out together (their junctions' corners), so the
/// whole site's items are given at once.
pub fn normalise(items: &[Item<'_>], figures: &Figures) -> Vec<Shaped> {
    let mut out: Vec<Shaped> = vec![Shaped::default(); items.len()];
    if items.len() > MAX_ITEMS {
        for s in &mut out {
            s.notes.push(format!("More than {MAX_ITEMS} site elements are shaped at once; none was."));
        }
        return out;
    }
    // Each item's truth with what the figures fill in.
    let mut props: Vec<Props> = Vec::with_capacity(items.len());
    let mut uses: Vec<&str> = Vec::with_capacity(items.len());
    for (it, s) in items.iter().zip(out.iter_mut()) {
        let use_ = if it.use_.is_empty() { default_use(it.kind).unwrap_or("") } else { it.use_ };
        s.filled = fill_defaults(it.kind, use_, it.props, figures);
        let mut p = it.props.clone();
        for (k, v) in &s.filled {
            p.insert(k.clone(), v.clone());
        }
        props.push(p);
        uses.push(use_);
    }
    // Each one's own shape; the lanes kept to join.
    let mut lanes: Vec<(usize, Vec<Xy>, Vec<Ring>)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        match shape_one(it, uses[i], &props[i], figures, &mut out[i]) {
            Ok(Some((line, own))) => lanes.push((i, line, own)),
            Ok(None) => {}
            Err(e) => out[i].notes.push(e),
        }
    }
    // Roads that meet make one paved area: each road's outline takes the rounded corners where it meets another road;
    // the other lanes' ends take the rounded inside corners where they meet a lane.
    let road_at: Vec<usize> = (0..lanes.len()).filter(|&n| uses[lanes[n].0] == "road").collect();
    if !road_at.is_empty() {
        let rs: Vec<roads::RoadIn> = road_at
            .iter()
            .map(|&n| {
                let (i, line, own) = &lanes[n];
                roads::RoadIn { id: items[*i].key.into(), line: line.clone(), s: roads::section_of(&props[*i]), own: Some(own.clone()) }
            })
            .collect();
        let lay = roads::layout(&rs);
        let outs = roads::outlines(&rs, &lay);
        let parts = roads::net(&rs, &lay, &outs);
        for ((&n, o), rp) in road_at.iter().zip(&outs).zip(parts) {
            let i = lanes[n].0;
            lanes[n].2 = o.clone();
            let s = &mut out[i];
            let sec = roads::section_of(&props[i]);
            if sec.paved() > sec.row + 1e-6 {
                s.notes.push(format!(
                    "The carriageway ({:.0} mm kerb to kerb) is wider than the right of way ({:.0} mm): the road is drawn at the carriageway's width.",
                    sec.paved(),
                    sec.row
                ));
            }
            s.parts.push(part("carriageway", &rp.carriageway));
            s.parts.push(part("pavement", &rp.pavement));
            if !rp.median.is_empty() {
                s.parts.push(part("median", &rp.median));
            }
            if !rp.paint.is_empty() {
                s.parts.push(part("paint", &rp.paint));
            }
            s.parts.push(line_part("kerb", rp.kerbs));
            s.parts.push(line_part("pavement_edge", rp.backs));
            for (role, kind_) in [("marking", roads::MarkKind::Lane), ("centre_line", roads::MarkKind::Centre), ("double_centre_line", roads::MarkKind::Double)] {
                let lines: Vec<Vec<Xy>> = rp.marks.iter().filter(|m| m.kind == kind_).map(|m| m.line.clone()).collect();
                if !lines.is_empty() {
                    s.parts.push(line_part(role, lines));
                }
            }
        }
    }
    for n in 0..lanes.len() {
        let i = lanes[n].0;
        if uses[i] == "road" {
            out[i].outline = rings_to(&lanes[n].2);
            continue;
        }
        let others: Vec<&Vec<Ring>> = lanes.iter().enumerate().filter(|(m, _)| *m != n).map(|(_, l)| &l.2).collect();
        let (line, own) = (&lanes[n].1, &lanes[n].2);
        let fil = lane_fillets(own, line, num(&props[i], "width").unwrap_or(0.0), &others, num(&props[i], "corner_radius").unwrap_or(0.0));
        if fil.is_empty() {
            out[i].outline = rings_to(own);
        } else {
            out[i].outline = rings_to(&poly::join(own, &fil));
            out[i].parts.push(part("fillet", &fil));
        }
    }
    // Kept to a hundredth of a millimetre, as the model keeps them.
    for s in &mut out {
        s.outline = poly::rounded(std::mem::take(&mut s.outline), 0.01);
    }
    out
}

// ====================================================================== the catalogue
/// Converts a kind's props between units: every length and point multiplied by `k` (1000 from Arqiflow's metres to
/// millimetres, 0.001 back), areas by k²; angles, gradients, counts, bulges, strings and yes-or-no as they are. The
/// props it does not know are kept as they are.
pub fn convert(kind_: &str, props: &Props, k: f64) -> Props {
    let spec = props_of(kind_);
    let mut out = Props::new();
    for (key, v) in props {
        let t = spec.get(key).cloned().unwrap_or(Value::Null);
        out.insert(key.clone(), scale_value(&t, v, k));
    }
    out
}

fn scale_value(t: &Value, v: &Value, k: f64) -> Value {
    let num_k = |v: &Value, k: f64| v.as_f64().map_or_else(|| v.clone(), |x| json!(x * k));
    match t {
        Value::String(s) => match s.as_str() {
            "length" => match v {
                Value::Array(a) => Value::Array(a.iter().map(|x| num_k(x, k)).collect()),
                _ => num_k(v, k),
            },
            "area" => num_k(v, k * k),
            "point" => match v {
                Value::Array(a) => Value::Array(a.iter().take(2).map(|x| num_k(x, k)).chain(a.iter().skip(2).cloned()).collect()),
                _ => v.clone(),
            },
            "points" | "path" => match v {
                Value::Array(a) => Value::Array(
                    a.iter()
                        .map(|q| match q {
                            Value::Array(c) => Value::Array(c.iter().take(2).map(|x| num_k(x, k)).chain(c.iter().skip(2).cloned()).collect()),
                            _ => q.clone(),
                        })
                        .collect(),
                ),
                _ => v.clone(),
            },
            _ => v.clone(),
        },
        Value::Object(o) => match (o.get("type").and_then(Value::as_str), v) {
            (Some("object"), Value::Object(m)) => {
                let empty = serde_json::Map::new();
                let ps = o.get("props").and_then(Value::as_object).unwrap_or(&empty);
                Value::Object(m.iter().map(|(key, x)| (key.clone(), scale_value(ps.get(key).unwrap_or(&Value::Null), x, k))).collect())
            }
            (Some("list"), Value::Array(a)) => {
                let of = o.get("of").cloned().unwrap_or(Value::Null);
                Value::Array(a.iter().map(|x| scale_value(&of, x, k)).collect())
            }
            _ => v.clone(),
        },
        _ => v.clone(),
    }
}

fn obj(props: Value) -> Value {
    json!({"type": "object", "props": props})
}

fn list(of: Value) -> Value {
    json!({"type": "list", "of": of})
}

/// What a kind's props are: "length" (mm; a number, or a list of numbers each a length), "point" ([x, y] mm),
/// "points" (a list of points), "path" (a list of [x, y] or [x, y, bulge]: x and y mm, the bulge a plain number),
/// "angle" (degrees), "percent", "fraction" (0 to 1), "number", "count", "string", "boolean", "area" (mm²), or
/// {"type": "object", "props"} and {"type": "list", "of"}.
pub fn props_of(kind_: &str) -> serde_json::Map<String, Value> {
    let head = obj(json!({"shape": "string", "radius": "length", "arm": "length", "side": "string"}));
    let bay = obj(json!({"piece": "count", "side": "string", "index": "count"}));
    let mut p = match kind_ {
        "lane" => json!({
            "path": "path", "width": "length", "surface": "string",
            "heads": obj(json!({"start": head, "end": head})), "corner_radius": "length",
            "lanes": "count", "lane_width": "length", "oneway": "boolean", "median": "length", "kerb": "length",
            "markings": "boolean",
        }),
        "water" => json!({"control": "points", "smooth": "count", "level": "length", "depth": "length", "bank": "length"}),
        "stream" => json!({"path": "path", "width": "length", "surface": "string"}),
        "tree" => json!({"at": "point", "crown": "length", "height": "length", "species": "string", "size": "string", "survey": "string"}),
        "hedge" => json!({"path": "path", "width": "length", "height": "length", "species": "string"}),
        "pitch" => json!({"at": "point", "angle": "angle", "length": "length", "width": "length", "margin": "length", "surface": "string"}),
        "playground" | "landscape" | "paving" | "terrace" => json!({"control": "points", "smooth": "count", "surface": "string"}),
        "play_area" => json!({"at": "point", "radius": "length", "control": "points", "smooth": "count", "in": "string"}),
        "bike_rack" => json!({"at": "point", "angle": "angle", "spaces": "count", "type": "string"}),
        "ramp" => json!({
            "path": "path", "width": "length", "open_length": "length", "to_level": "string", "gradient": "percent",
            "from_lane": "string",
        }),
        "parking_band" => json!({
            "path": "path", "aisle": "length", "bay_depth": "length", "bay_width": "length", "angle": "angle",
            "sides": "string", "omit": list(bay.clone()), "accessible": list(bay),
        }),
        "car_park" => json!({
            "control": "points", "standard": "string", "angle": "angle", "one_way": "boolean",
            "stall_width": "length", "stall_depth": "length", "aisle": "length", "setback": "length",
            "end_island": "length", "island_every": "count", "ev_share": "fraction", "column_every": "count",
            "column_size": "length", "floor_to_floor": "length", "ramp_slope": "fraction", "direction": "angle",
            "access": list(obj(json!({"at": "point", "kind": "string"}))), "removed": "points", "target": "point",
            "solved": "string",
            "result": obj(json!({
                "stalls": "count", "accessible": "count", "accessible_needed": "count", "ev": "count", "columns": "count",
                "arrangement": "string", "directions": list(json!("angle")), "basement": "boolean",
                "area_per_stall": "area", "ways": list(obj(json!({"at": "point", "kind": "string"}))),
                "notes": list(json!("string")),
            })),
        }),
        "mass" => json!({
            "storeys": "count", "attic": "boolean", "floor_height": "length", "roof": "string", "pitch": "angle",
            "ridge": "angle", "overhang": "length", "steps": "count",
            "arc": obj(json!({"centre": "point", "radius": "length", "depth": "length", "a0": "angle", "a1": "angle", "segments": "count"})),
            "run": "string",
            "dormers": obj(json!({"spacing": "length", "width": "length", "height": "length", "sides": "string"})),
            "windows": obj(json!({"bay": "length", "width": "length", "height": "length", "sill": "length"})),
            "entrance": "point", "materials": obj(json!({"walls": "string", "roof": "string"})), "unit_type": "string",
            "plates": "count", "unit_floors": "count", "units": "count",
        }),
        _ => json!({}),
    };
    // What every site element may hold: its phase, surface and label, where context came from (`ref`, `fetched`),
    // the print of what it was made from (`made`) and whether it counts as built.
    if let Value::Object(m) = &mut p {
        for (k, t) in [("phase", "string"), ("surface", "string"), ("label", "string"), ("ref", "string"), ("fetched", "string"), ("made", "string"), ("built", "boolean")] {
            m.entry(k).or_insert_with(|| json!(t));
        }
    }
    match p {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    }
}

fn figures_of(names: &[&str]) -> Value {
    Value::Object(names.iter().map(|n| ((*n).to_string(), json!(figure(&Figures::default(), n)))).collect())
}

/// Every site kind: its label, Revit's equivalent, its uses (the first the default), what it is drawn as, what each of
/// its props is (see [`props_of`]), its figures by Arqiflow's names (mm for lengths) and the defaults it fills in; and
/// every figure with what it is. The `site_kinds` command answers it, and Arqiflow converts its metres from it.
pub fn catalog() -> Value {
    let mut kinds = serde_json::Map::new();
    let mut all: Vec<(&str, &[&str], &str, &str, &str)> =
        KINDS.iter().map(|k| (k.kind, k.uses, k.shape, k.label, k.revit)).collect();
    all.push(("mass", &["housing", "office", "retail", "mixed", "community"], "polygon", "Building", "Mass"));
    for (name, uses, shape, label, revit) in all {
        let names: &[&str] = match name {
            "lane" => &["lane.width", "path.width", "road.width", "lane.corner_radius", "road.corner_radius", "lane.turning_head_radius", "lane.hammerhead_arm"],
            "water" => &["water.level", "water.depth", "water.smooth"],
            "stream" => &["stream.width", "rill.width", "ditch.width"],
            "tree" => &["tree.crown", "tree.height"],
            "hedge" => &["hedge.width", "hedge.height"],
            "pitch" => &["pitch.margin"],
            "bike_rack" => &["bike_rack.spaces"],
            "ramp" => &["ramp.width", "ramp.open_length", "ramp.gradient"],
            "parking_band" => &["parking.aisle", "parking.bay_depth", "parking.bay_width"],
            "car_park" => &["parking.level_height"],
            "mass" => &["building.storeys", "building.attic", "building.floor_height", "roof.pitch", "roof.overhang", "roof.step_drop", "area.attic_factor"],
            _ => &[],
        };
        // The defaults a new one is drawn with (site_kinds.defaults): from a path and a control ring, both left out.
        let mut probe = Props::new();
        probe.insert("path".into(), json!([[0, 0], [1000, 0]]));
        probe.insert("control".into(), json!([[0, 0]]));
        let defaults = fill_defaults(name, uses.first().copied().unwrap_or(""), &probe, &Figures::default());
        let mut k = json!({
            "label": label, "revit": revit, "uses": uses, "use": uses.first(), "shape": shape,
            "props": Value::Object(props_of(name)), "figures": figures_of(names), "defaults": Value::Object(defaults),
        });
        let surfaces = SURFACES.iter().find(|s| s.0 == name).map(|s| s.1);
        if let (Some(s), Value::Object(m)) = (surfaces, &mut k) {
            m.insert("surfaces".into(), json!(s));
        }
        if let Value::Object(m) = &mut k {
            match name {
                "lane" => {
                    m.insert("width_by_use".into(), Value::Object(LANE_WIDTH.iter().map(|(u, n)| ((*u).into(), json!(figure(&Figures::default(), n)))).collect()));
                    m.insert(
                        "corner_radius_by_use".into(),
                        Value::Object(LANE_WIDTH.iter().map(|(u, _)| ((*u).into(), json!(figure(&Figures::default(), if *u == "road" { "road.corner_radius" } else { "lane.corner_radius" })))).collect()),
                    );
                }
                "stream" => {
                    m.insert("width_by_use".into(), Value::Object(STREAM_WIDTH.iter().map(|(u, n)| ((*u).into(), json!(figure(&Figures::default(), n)))).collect()));
                }
                "pitch" => {
                    m.insert(
                        "sizes".into(),
                        Value::Object(
                            PITCH_SIZES
                                .iter()
                                .map(|s| {
                                    let mut v = json!({"length": s.size.0, "width": s.size.1, "label": s.label,
                                        "range": s.range.map(|((a, b), (c, d))| json!([[a, b], [c, d]]))});
                                    if let (Some((a, b)), Value::Object(o)) = (s.margin, &mut v) {
                                        o.insert("margin".into(), json!([a, b]));
                                    }
                                    (s.use_.to_string(), v)
                                })
                                .collect(),
                        ),
                    );
                }
                "car_park" => {
                    // The solver's own figures where the standard says nothing (lib/parking/standards.ts settingsOf).
                    m.insert(
                        "defaults".into(),
                        json!({"standard": "saudi", "angle": 90.0, "ev_share": 0.0, "end_island": 1500.0, "island_every": 0,
                               "setback": 0.0, "column_every": 0, "column_size": 600.0, "floor_to_floor": 3500.0, "ramp_slope": 0.15}),
                    );
                }
                _ => {}
            }
        }
        kinds.insert(name.into(), k);
    }
    json!({
        "units": {"length": "mm", "point": "[x, y] mm", "points": "[[x, y], …] mm",
                  "path": "[[x, y] or [x, y, bulge], …]: x, y mm, the bulge plain", "angle": "degrees counter-clockwise from x",
                  "percent": "%", "fraction": "0 to 1", "area": "mm²"},
        "kinds": Value::Object(kinds),
        "figures": Value::Object(FIGURES.iter().map(|(n, v, t)| ((*n).to_string(), json!({"value": v, "type": t}))).collect()),
    })
}

#[cfg(test)]
#[path = "shapes_tests.rs"]
mod tests;
