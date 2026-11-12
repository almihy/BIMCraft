//! The site and masterplan as elements of the model ([docs/areas/site.md](../../../docs/areas/site.md)): a `site` element
//! (a lane, a lawn, a pond, a tree, a car park, a parking stall…), Revit's Property Line, Toposolid and Building Pad, and a
//! mass's masterplan figures. The shapes are the pure geometry of [`crate::site`]; this module holds what the model keeps
//! of them and keeps it up to date:
//!
//! - [`keep`] runs after every command that changes the model ([`crate::engine::Session`]): a site element whose truth
//!   (its props) changed, and its neighbours (the lanes it meets), are shaped again ([`shapes::normalise`]); a car park is
//!   laid out again ([`parking::lay_out`]) when it or what it keeps clear of changed; a site element's `phase` and its
//!   `Phase Created` and `Phase Demolished` are kept in step; a mass with storeys takes its height from them.
//! - [`transform_props`] moves a site element's truth with the Modify tools (move, copy, rotate, mirror, array).
//! - [`check_site`] and the other checks are `Model::check`'s rules for them.
//! - [`solids`] is what the 3D view builds of them; the plan's drawing is `render/site.rs`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::geom::{self, P2};
use crate::model::{Changed, Element, Floor, Guid, Id, MAX_COORD, MAX_TEXT, Model, ShapePoint};
use crate::phasing;
use crate::prim::Rgb;
use crate::site::{Poly, Props, parking, shapes};

/// Arqiflow's phases of a site element: what the design makes, what stands and stays, what stands and comes down, what
/// others will build.
pub const PHASES: [&str; 4] = ["new", "existing", "demolished", "planned"];
/// Where context comes from: the plot's own streets, OpenStreetMap, Overture. An existing element from one of them is
/// context, not the design (Arqiflow's `isContext`).
pub const CONTEXT_SOURCES: [&str; 3] = ["site", "osm", "overture"];
/// The uses of a building on the masterplan (a mass's `site.use`); the first is the default.
pub const MASS_USES: [&str; 5] = ["housing", "office", "retail", "mixed", "community"];
/// The longest text a prop or a note holds.
pub const MAX_PROP_TEXT: usize = 4096;
/// The most notes an element keeps, and the most points all its shapes hold together.
pub const MAX_NOTES: usize = 100;
pub const MAX_SHAPE_POINTS: usize = 1_000_000;
/// How far a lane's shape may reach for a neighbour's (its fillets, turning heads and junction corners): lanes nearer
/// than this to a changed one are shaped again with it.
const REACH: f64 = 30_000.0;

fn is_new(p: &str) -> bool {
    p == "new"
}

fn new_phase() -> String {
    "new".into()
}

/// One element of the masterplan (docs/areas/site.md): what it is (`kind`, `use`), what a person drew (`props`, its truth,
/// mm) and the shape worked out from it (`outline`, `parts`, a car park's `layout`), kept so a reader never works it out
/// again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub kind: String,
    #[serde(rename = "use", default)]
    pub use_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub level: Guid,
    /// Arqiflow's: `new`, `existing`, `demolished` or `planned`; kept in step with its `Phase Created` and `Phase
    /// Demolished` ([`keep`]).
    #[serde(default = "new_phase", skip_serializing_if = "is_new")]
    pub phase: String,
    /// Where context came from (`site`, `osm`, `overture`…) and its id there.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(rename = "ref", default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(default)]
    pub props: Props,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<Poly>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<shapes::Part>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<parking::Layout>,
    /// What the person should be told of its shape (a pitch outside its type's sizes, a path too short…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

impl Site {
    /// Its use, or its kind's default.
    pub fn use_or_default(&self) -> &str {
        if self.use_.is_empty() { shapes::default_use(&self.kind).unwrap_or("") } else { &self.use_ }
    }

    /// Whether it is context fetched round the plot (Arqiflow's `isContext`), not the design.
    pub fn context(&self) -> bool {
        CONTEXT_SOURCES.contains(&self.source.as_str()) && self.phase == "existing"
    }

    /// Its surface: its own, else the kind's default.
    pub fn surface(&self) -> &str {
        self.props.get("surface").and_then(Value::as_str).unwrap_or("")
    }
}

/// A mass's figures on the masterplan (docs/areas/site.md): its use, phase, where it came from, and its props (storeys,
/// floor height, attic, roof, pitch, label, entrance…, lengths in mm).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MassSite {
    #[serde(rename = "use", default, skip_serializing_if = "String::is_empty")]
    pub use_: String,
    #[serde(default = "new_phase", skip_serializing_if = "is_new")]
    pub phase: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(rename = "ref", default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

/// Revit's Property Line: the plot's boundary, closed, through `points` ([x, y, bulge]: the bulge, as DXF's, of the piece
/// from that point to the next, 0 straight).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyLine {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub level: Guid,
    pub points: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

impl PropertyLine {
    /// Its line, closed, its arcs in pieces within [`shapes::ARC_TOLERANCE`] of the true arc (the first point not
    /// repeated at the end).
    pub fn ring(&self) -> Vec<P2> {
        let n = self.points.len();
        let mut out = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            let q = self.points[(i + 1) % n];
            if out.is_empty() {
                out.push(P2(p[0], p[1]));
            }
            if out.len() > shapes::MAX_POINTS {
                break;
            }
            out.extend(shapes::arc_points([p[0], p[1]], [q[0], q[1]], p[2], shapes::ARC_TOLERANCE).into_iter().map(|a| P2(a[0], a[1])));
        }
        if out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-6) {
            out.pop();
        }
        out
    }

    /// Its area, mm², its arcs' exactly: the polygon of its points and each arc's segment.
    pub fn area(&self) -> f64 {
        let n = self.points.len();
        let mut signed = 0.0;
        for (i, p) in self.points.iter().enumerate() {
            let q = self.points[(i + 1) % n];
            signed += (p[0] * q[1] - q[0] * p[1]) / 2.0;
            let b = p[2];
            if b.abs() > 1e-12 {
                let c = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
                let theta = 4.0 * b.abs().atan();
                let r = c * (1.0 + b * b) / (4.0 * b.abs());
                signed += b.signum() * r * r / 2.0 * (theta - theta.sin());
            }
        }
        signed.abs()
    }
}

/// Revit's Building Pad: a flat pad along `boundary` with its top `offset` over its level, a floor type's thickness
/// under that, which cuts the toposolid it lies in ([`Model::toposolid_cut`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingPad {
    #[serde(rename = "type")]
    pub type_name: String,
    pub level: Guid,
    pub boundary: Vec<P2>,
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

/// The project's figures, a firm's own (`Model::site_figures`), over Arqiflow's.
pub fn figures(m: &Model) -> shapes::Figures {
    shapes::Figures(m.site_figures.clone())
}

// ====================================================================== phases
/// What `Phase Created` and `Phase Demolished` an Arqiflow phase is among the project's `phases`: what stands (existing,
/// demolished) is made in the first; what the design makes (new, planned) in the last; what comes down, in the last.
pub fn phase_params(phases: &[String], phase: &str) -> (Option<String>, Option<String>) {
    let (first, last) = (phases.first().cloned(), phases.last().cloned());
    match phase {
        "existing" => (first, None),
        "demolished" => (first, last),
        _ => (last, None),
    }
}

/// `params` with the `Phase Created` and `Phase Demolished` of `phase`.
pub fn put_phase(phases: &[String], params: &mut BTreeMap<String, Value>, phase: &str) {
    let (created, demolished) = phase_params(phases, phase);
    for (key, v) in [(phasing::CREATED, created), (phasing::DEMOLISHED, demolished)] {
        match v {
            Some(v) => {
                params.insert(key.into(), Value::String(v));
            }
            None => {
                params.remove(key);
            }
        }
    }
}

/// The Arqiflow phase `params` say, `was` kept where they don't say otherwise (planned stays planned while new).
fn phase_of(phases: &[String], params: &BTreeMap<String, Value>, was: &str) -> String {
    let at = |k: &str| params.get(k).and_then(Value::as_str).and_then(|n| phases.iter().position(|p| p == n));
    let created = at(phasing::CREATED).unwrap_or(0);
    if at(phasing::DEMOLISHED).is_some() {
        return "demolished".into();
    }
    if created == 0 && phases.len() > 1 {
        return "existing".into();
    }
    if was == "planned" { "planned".into() } else { "new".into() }
}

/// The phase to keep and the params to keep with it: the phase when it changed (or the element is new), else what its
/// params say when they changed.
fn phase_in_step(
    phases: &[String],
    phase: &str,
    params: &BTreeMap<String, Value>,
    before: Option<(&str, &BTreeMap<String, Value>)>,
) -> (String, BTreeMap<String, Value>) {
    let mut params = params.clone();
    let phase = match before {
        Some((was, old)) if was == phase && *old != params => phase_of(phases, &params, phase),
        _ => phase.to_string(),
    };
    put_phase(phases, &mut params, &phase);
    (phase, params)
}

// ====================================================================== the mass
/// A mass's height from its storeys and floor height (the figure `building.floor_height` where it has none), when its
/// masterplan figures give storeys.
pub fn mass_height(site: &MassSite, figures: &shapes::Figures) -> Option<f64> {
    let storeys = site.props.get("storeys").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0 && *s <= 1000.0)?;
    let fh = site
        .props
        .get("floor_height")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0 && *v <= MAX_COORD)
        .unwrap_or_else(|| shapes::figure(figures, "building.floor_height"));
    let h = storeys * fh;
    (h > 0.0 && h <= MAX_COORD).then_some(h)
}

// ====================================================================== moving the truth
/// The props a kind holds as a direction in the world, which turn with it: the rest of its angles are measured from its
/// own line (a parking band's stalls from its path).
fn world_angle(kind: &str, key: &str) -> bool {
    matches!((kind, key), ("pitch" | "bike_rack" | "parking_stall", "angle") | ("car_park", "direction") | ("mass", "ridge"))
}

/// A site element's (or a mass's, `kind` "mass") truth taken by `f`, a mirror image when `mirror`: its points and paths
/// (a mirrored arc's bulge turns the other way), its directions in the world, and what is on one side of its path
/// (a parking band's `sides` and its bays, a lane's turning heads) to the other side.
pub fn transform_props(kind: &str, props: &mut Props, f: &dyn Fn(P2) -> P2, mirror: bool) {
    let spec = shapes::props_of(kind);
    let keys: Vec<String> = props.keys().cloned().collect();
    for key in keys {
        let Some(v) = props.get_mut(&key) else { continue };
        if world_angle(kind, &key) {
            if let Some(a) = v.as_f64().filter(|a| a.is_finite()) {
                *v = json!(turned(f, a));
            }
            continue;
        }
        let t = spec.get(&key).cloned().unwrap_or(Value::Null);
        move_value(&t, v, f, mirror);
        if mirror {
            match (kind, key.as_str()) {
                ("parking_band", "sides") => {
                    if let Some(s) = v.as_str() {
                        *v = json!(match s {
                            "left" => "right",
                            "right" => "left",
                            other => other,
                        });
                    }
                }
                ("parking_band", "omit" | "accessible") => {
                    for b in v.as_array_mut().into_iter().flatten() {
                        if let Some(side) = b.get_mut("side") {
                            *side = json!(if side.as_str() == Some("left") { "right" } else { "left" });
                        }
                    }
                }
                ("lane", "heads") => {
                    for end in ["start", "end"] {
                        if let Some(side) = v.get_mut(end).and_then(|h| h.get_mut("side")) {
                            match side.as_str() {
                                Some("left") => *side = json!("right"),
                                Some("right") => *side = json!("left"),
                                _ => {}
                            }
                        }
                    }
                }
                ("mass", "arc") => {
                    // Its sweep runs the other way round: its ends swap.
                    if let Some(o) = v.as_object_mut() {
                        let (a0, a1) = (o.get("a0").cloned(), o.get("a1").cloned());
                        if let (Some(a0), Some(a1)) = (a0, a1) {
                            o.insert("a0".into(), a1);
                            o.insert("a1".into(), a0);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Where `f` takes the direction `angle` degrees: as `commands::turned` does.
fn turned(f: &dyn Fn(P2) -> P2, angle: f64) -> f64 {
    let o = f(P2(0.0, 0.0));
    let r = angle.to_radians();
    let b = f(P2(1000.0 * r.cos(), 1000.0 * r.sin()));
    let a = (b.1 - o.1).atan2(b.0 - o.0).to_degrees();
    (a * 1e6).round() / 1e6
}

/// A value of type `t` (as [`shapes::props_of`] says) taken by `f`.
fn move_value(t: &Value, v: &mut Value, f: &dyn Fn(P2) -> P2, mirror: bool) {
    let pt = |q: &mut Value| {
        if let Some(a) = q.as_array_mut()
            && let (Some(x), Some(y)) = (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64))
        {
            let p = f(P2(x, y));
            a[0] = json!(round(p.0));
            a[1] = json!(round(p.1));
            if mirror && let Some(b) = a.get_mut(2).and_then(|b| b.as_f64().map(|x| (b, x))) {
                *b.0 = json!(-b.1);
            }
        }
    };
    match t {
        Value::String(s) => match s.as_str() {
            "point" => pt(v),
            "points" | "path" => v.as_array_mut().into_iter().flatten().for_each(pt),
            _ => {}
        },
        Value::Object(o) => match (o.get("type").and_then(Value::as_str), v) {
            (Some("object"), Value::Object(m)) => {
                let empty = serde_json::Map::new();
                let ps = o.get("props").and_then(Value::as_object).unwrap_or(&empty);
                for (k, x) in m.iter_mut() {
                    if k == "a0" || k == "a1" {
                        // An arc's ends turn as directions from its centre.
                        if let Some(a) = x.as_f64() {
                            *x = json!(turned(f, a));
                        }
                        continue;
                    }
                    move_value(ps.get(k).unwrap_or(&Value::Null), x, f, mirror);
                }
            }
            (Some("list"), Value::Array(a)) => {
                let of = o.get("of").cloned().unwrap_or(Value::Null);
                a.iter_mut().for_each(|x| move_value(&of, x, f, mirror));
            }
            _ => {}
        },
        _ => {}
    }
}

/// To a hundredth of a millimetre, as the model keeps points.
fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ====================================================================== the checks
fn finite(v: f64, cap: f64) -> bool {
    v.is_finite() && v.abs() <= cap
}

fn check_point(v: &Value, bulge: bool) -> bool {
    let Some(a) = v.as_array() else { return false };
    let n = if bulge { 2..=3 } else { 2..=2 };
    n.contains(&a.len())
        && a.iter().take(2).all(|c| c.as_f64().is_some_and(|c| finite(c, MAX_COORD)))
        && a.get(2).is_none_or(|b| b.is_null() || b.as_f64().is_some_and(|b| finite(b, 1e6)))
}

/// Whether `v` is a value of type `t` ([`shapes::props_of`]): what it is not, said.
fn check_value(key: &str, t: &Value, v: &Value) -> Result<(), String> {
    if v.is_null() {
        return Ok(());
    }
    let number = |cap: f64| v.as_f64().is_some_and(|x| finite(x, cap));
    let ok = match t {
        Value::String(s) => match s.as_str() {
            "length" => {
                number(MAX_COORD)
                    || v.as_array().is_some_and(|a| a.len() <= 16 && a.iter().all(|x| x.as_f64().is_some_and(|x| finite(x, MAX_COORD))))
            }
            "area" => number(1e18),
            "angle" | "percent" | "number" => number(1e9),
            "fraction" => v.as_f64().is_some_and(|x| (0.0..=1.0).contains(&x)),
            "count" => v.as_f64().is_some_and(|x| (0.0..=1e6).contains(&x)),
            "point" => check_point(v, false),
            "points" => v.as_array().is_some_and(|a| a.len() <= shapes::MAX_POINTS && a.iter().all(|q| check_point(q, false))),
            "path" => v.as_array().is_some_and(|a| a.len() <= shapes::MAX_POINTS && a.iter().all(|q| check_point(q, true))),
            "string" => v.as_str().is_some_and(|s| s.chars().count() <= MAX_PROP_TEXT),
            "boolean" => v.is_boolean(),
            _ => true,
        },
        Value::Object(o) => match o.get("type").and_then(Value::as_str) {
            Some("object") => {
                let Some(m) = v.as_object() else { return Err(format!("`{key}` is an object of props")) };
                let empty = serde_json::Map::new();
                let ps = o.get("props").and_then(Value::as_object).unwrap_or(&empty);
                for (k, x) in m {
                    let tk = ps.get(k).ok_or_else(|| format!("`{key}.{k}` isn't known; `{key}` takes: {}", names(ps)))?;
                    check_value(&format!("{key}.{k}"), tk, x)?;
                }
                true
            }
            Some("list") => {
                let Some(a) = v.as_array() else { return Err(format!("`{key}` is a list")) };
                if a.len() > shapes::MAX_POINTS {
                    return Err(format!("`{key}` holds more than {} items", shapes::MAX_POINTS));
                }
                let of = o.get("of").cloned().unwrap_or(Value::Null);
                for (i, x) in a.iter().enumerate() {
                    check_value(&format!("{key}[{i}]"), &of, x)?;
                }
                true
            }
            _ => true,
        },
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        let what = match t {
            Value::String(s) => match s.as_str() {
                "length" => "a length in mm (a finite number within 1000 km)",
                "area" => "an area in mm²",
                "angle" => "an angle in degrees",
                "percent" => "a percentage",
                "number" => "a number",
                "fraction" => "a fraction from 0 to 1",
                "count" => "a count, 0 or more",
                "point" => "a point [x, y] in mm",
                "points" => "a list of points [x, y] in mm",
                "path" => "a list of points [x, y] or [x, y, bulge] in mm",
                "string" => "text (up to 4096 characters)",
                "boolean" => "true or false",
                _ => "a value",
            },
            _ => "a value",
        };
        Err(format!("`{key}` is {what}, not {}", short(v)))
    }
}

fn names(m: &serde_json::Map<String, Value>) -> String {
    m.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 60 { format!("{}…", s.chars().take(60).collect::<String>()) } else { s }
}

/// A site kind's (or "mass") props, each of a prop the catalogue gives it and of its type.
pub fn check_props(kind: &str, props: &Props) -> Result<(), String> {
    let spec = shapes::props_of(kind);
    for (k, v) in props {
        let t = spec.get(k).ok_or_else(|| {
            format!("`{k}` isn't a prop of a {}; it takes: {} (site_kinds lists them)", kind.replace('_', " "), names(&spec))
        })?;
        check_value(k, t, v)?;
    }
    Ok(())
}

fn text(who: &str, what: &str, s: &str, cap: usize) -> Result<(), String> {
    if s.chars().count() > cap {
        return Err(format!("{who}: its {what} is longer than {cap} characters"));
    }
    Ok(())
}

fn polys_ok(who: &str, what: &str, polys: &[Poly], count: &mut usize) -> Result<(), String> {
    for p in polys {
        for r in std::iter::once(&p.outer).chain(&p.holes) {
            *count += r.len();
            if r.iter().any(|q| !finite(q.0, MAX_COORD) || !finite(q.1, MAX_COORD)) {
                return Err(format!("{who}: a point of its {what} is not finite or reaches past 1000 km"));
            }
        }
    }
    if *count > MAX_SHAPE_POINTS {
        return Err(format!("{who}: its shapes hold more than {MAX_SHAPE_POINTS} points; draw it in parts"));
    }
    Ok(())
}

/// `Model::check`'s rules for a site element: its kind and use known, its phase one of Arqiflow's, its props the
/// catalogue's and of their types (finite, within the model's reach), its shapes finite and not too big.
pub fn check_site(who: &str, s: &Site) -> Result<(), String> {
    let kind = shapes::kind(&s.kind).ok_or_else(|| {
        format!(
            "{who}: `{}` isn't a site kind; it is one of: {}",
            s.kind,
            shapes::KINDS.iter().map(|k| k.kind).collect::<Vec<_>>().join(", ")
        )
    })?;
    if !s.use_.is_empty() && !kind.uses.contains(&s.use_.as_str()) {
        return Err(format!("{who}: a {} is used as one of: {}; not `{}`", s.kind.replace('_', " "), kind.uses.join(", "), s.use_));
    }
    if !PHASES.contains(&s.phase.as_str()) {
        return Err(format!("{who}: phase `{}` is one of: {}", s.phase, PHASES.join(", ")));
    }
    text(who, "name", &s.name, MAX_TEXT)?;
    text(who, "source", &s.source, MAX_TEXT)?;
    text(who, "ref", &s.reference, MAX_PROP_TEXT)?;
    check_props(&s.kind, &s.props).map_err(|e| format!("{who}: {e}"))?;
    let mut count = 0;
    polys_ok(who, "outline", &s.outline, &mut count)?;
    for p in &s.parts {
        text(who, "part's role", &p.role, MAX_TEXT)?;
        polys_ok(who, "parts", &p.polys, &mut count)?;
        for l in &p.lines {
            count += l.len();
            if l.iter().any(|q| !finite(q.0, MAX_COORD) || !finite(q.1, MAX_COORD)) {
                return Err(format!("{who}: a point of its parts is not finite or reaches past 1000 km"));
            }
        }
    }
    if let Some(l) = &s.layout {
        if s.kind != "car_park" {
            return Err(format!("{who}: only a car park is laid out; a {} has no layout", s.kind.replace('_', " ")));
        }
        for piece in &l.pieces {
            polys_ok(who, "layout", &piece.outline, &mut count)?;
        }
        if serde_json::to_string(&l.result).map_or(true, |t| t.len() > 1 << 20) {
            return Err(format!("{who}: its layout's figures are too big"));
        }
    }
    if count > MAX_SHAPE_POINTS {
        return Err(format!("{who}: its shapes hold more than {MAX_SHAPE_POINTS} points; draw it in parts"));
    }
    if s.notes.len() > MAX_NOTES || s.notes.iter().any(|n| n.chars().count() > MAX_PROP_TEXT) {
        return Err(format!("{who}: it keeps at most {MAX_NOTES} notes of up to {MAX_PROP_TEXT} characters"));
    }
    Ok(())
}

/// `Model::check`'s rules for a mass's masterplan figures.
pub fn check_mass_site(who: &str, s: &MassSite) -> Result<(), String> {
    if !s.use_.is_empty() && !MASS_USES.contains(&s.use_.as_str()) {
        return Err(format!("{who}: a building on the masterplan is used as one of: {}; not `{}`", MASS_USES.join(", "), s.use_));
    }
    if !PHASES.contains(&s.phase.as_str()) {
        return Err(format!("{who}: phase `{}` is one of: {}", s.phase, PHASES.join(", ")));
    }
    text(who, "source", &s.source, MAX_TEXT)?;
    text(who, "ref", &s.reference, MAX_PROP_TEXT)?;
    check_props("mass", &s.props).map_err(|e| format!("{who}: {e}"))
}

/// `Model::check`'s rules for a property line: three points or more (up to the model's most), finite, bulges finite,
/// enclosing an area.
pub fn check_property_line(who: &str, p: &PropertyLine) -> Result<(), String> {
    text(who, "name", &p.name, MAX_TEXT)?;
    if !(3..=crate::model::MAX_POINTS).contains(&p.points.len()) {
        return Err(format!("{who}: a property line runs through 3 to {} points", crate::model::MAX_POINTS));
    }
    if p.points.iter().any(|q| !finite(q[0], MAX_COORD) || !finite(q[1], MAX_COORD) || !finite(q[2], 1e6)) {
        return Err(format!("{who}: a point of it is not finite or reaches past 1000 km, or a bulge isn't a finite number"));
    }
    if p.area() < 1.0 {
        return Err(format!("{who}: its points enclose no area; give the plot's corners in order round it"));
    }
    Ok(())
}

// ====================================================================== keeping the shapes
/// What a command left that [`keep`] works out again: an element's new shape, props, phase and params, a car park's
/// layout, a mass's height.
pub enum Update {
    Site { id: Id, site: Box<Site> },
    Mass { id: Id, height: f64, phase: String, params: BTreeMap<String, Value> },
}

/// The commands that shape what they make or change themselves (to answer with it), which [`keep`] then leaves alone.
pub const SHAPES_ITSELF: &[&str] = &["site", "site_edit"];

/// After a command: the site elements it changed shaped again, with the lanes they meet; the car parks it touched laid
/// out again; phases and params in step; masses with storeys their height. Only what changed and its neighbours: what
/// [`apply`] then changes, none when nothing needs it (so the engine copies the model only when it must).
pub fn keep(m: &Model, before: &Model) -> Vec<Update> {
    let changed = what_changed(m, before);
    plan(m, Some(before), &changed)
}

/// [`keep`] of the model alone, as a command that shapes itself runs it: `before` is the model before the command.
pub fn keep_model(m: &mut Model, before: &Model) {
    let changed = what_changed(m, before);
    let updates = plan(m, Some(before), &changed);
    if !updates.is_empty() {
        apply(m, updates);
    }
}

/// What changed of what shapes the site: the elements changed; or all of it, when its figures, its phases or a level did
/// (a level's elevation says what is below ground, its name what a car park's pieces stand on).
fn what_changed(m: &Model, before: &Model) -> Changed {
    if m.site_figures != before.site_figures || m.phases != before.phases {
        return Changed::All;
    }
    match m.elements.changed_since(&before.elements) {
        Changed::Ids(ids) if ids.iter().any(|id| [m, before].iter().any(|x| matches!(x.elements.get(*id), Some(Element::Level(_))))) => {
            Changed::All
        }
        c => c,
    }
}

/// Every site element and mass shaped again, as a file read from elsewhere may need.
pub fn reshape_all(m: &mut Model) {
    let updates = plan(m, None, &Changed::All);
    if !updates.is_empty() {
        apply(m, updates);
    }
}

/// What shapes the site: its elements, and the masses (buildings) its car parks keep clear of.
fn relevant(e: Option<&Element>) -> bool {
    matches!(e, Some(Element::Site(_) | Element::Mass(_)))
}

/// A box [x0, y0, x1, y1].
type Bx = [f64; 4];

fn grow(b: Bx, by: f64) -> Bx {
    [b[0] - by, b[1] - by, b[2] + by, b[3] + by]
}

fn meet(a: &Bx, b: &Bx) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

fn add_point(b: &mut Option<Bx>, x: f64, y: f64) {
    if !(x.is_finite() && y.is_finite()) {
        return;
    }
    *b = Some(match *b {
        None => [x, y, x, y],
        Some(c) => [c[0].min(x), c[1].min(y), c[2].max(x), c[3].max(y)],
    });
}

fn polys_box(polys: &[Poly]) -> Option<Bx> {
    let mut b = None;
    for p in polys {
        for q in &p.outer {
            add_point(&mut b, q.0, q.1);
        }
    }
    b
}

/// The box of what a person drew: its points, paths and control ring, grown by half its width.
fn props_box(p: &Props) -> Option<Bx> {
    let mut b = None;
    let mut visit = |v: &Value| {
        if let Some(a) = v.as_array() {
            if let (Some(x), Some(y)) = (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)) {
                add_point(&mut b, x, y);
            } else {
                for q in a {
                    if let Some(c) = q.as_array()
                        && let (Some(x), Some(y)) = (c.first().and_then(Value::as_f64), c.get(1).and_then(Value::as_f64))
                    {
                        add_point(&mut b, x, y);
                    }
                }
            }
        }
    };
    for k in ["path", "control", "at"] {
        if let Some(v) = p.get(k) {
            visit(v);
        }
    }
    let half = ["width", "aisle", "crown", "length"]
        .iter()
        .filter_map(|k| p.get(*k).and_then(Value::as_f64))
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    b.map(|b| grow(b, half.min(MAX_COORD)))
}

fn site_box(s: &Site) -> Option<Bx> {
    match (polys_box(&s.outline), props_box(&s.props)) {
        (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
        (a, b) => a.or(b),
    }
}

fn element_box(e: &Element) -> Option<Bx> {
    match e {
        Element::Site(s) => site_box(s),
        Element::Mass(ms) => {
            let mut b = None;
            for q in &ms.boundary {
                add_point(&mut b, q.0, q.1);
            }
            b
        }
        _ => None,
    }
}

fn level_of(e: &Element) -> Option<Guid> {
    match e {
        Element::Site(s) => Some(s.level),
        Element::Mass(ms) => Some(ms.level),
        _ => None,
    }
}

/// What [`keep`] will change, worked out from the model as the command left it.
fn plan(m: &Model, before: Option<&Model>, changed: &Changed) -> Vec<Update> {
    let all = matches!(changed, Changed::All);
    let ids: BTreeSet<Id> = match changed {
        Changed::Ids(ids) => ids
            .iter()
            .copied()
            .filter(|id| relevant(m.elements.get(*id)) || before.is_some_and(|b| relevant(b.elements.get(*id))))
            .collect(),
        Changed::All => BTreeSet::new(),
    };
    if !all && ids.is_empty() {
        return Vec::new();
    }
    // Every site element by level, and the masses with their figures.
    let mut by_level: BTreeMap<Guid, Vec<(Id, &Site)>> = BTreeMap::new();
    let mut masses: Vec<(Id, &crate::model::Mass)> = Vec::new();
    for (id, e) in m.elements.iter() {
        match e {
            Element::Site(s) => by_level.entry(s.level).or_default().push((*id, s)),
            Element::Mass(ms) => masses.push((*id, ms)),
            _ => {}
        }
    }
    if by_level.is_empty() && !masses.iter().any(|(_, ms)| ms.site.is_some()) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let figures = figures(m);
    // The masses: their phases and params in step, their heights from their storeys.
    for (id, ms) in &masses {
        let Some(site) = &ms.site else { continue };
        if !all && !ids.contains(id) {
            continue;
        }
        let was = before.and_then(|b| match b.elements.get(*id) {
            Some(Element::Mass(old)) => old.site.as_ref().map(|s| (s.phase.as_str(), &old.params)),
            _ => None,
        });
        let (phase, params) = phase_in_step(&m.phases, &site.phase, &ms.params, was);
        let height = if ms.top_level.is_none() { mass_height(site, &figures).unwrap_or(ms.height) } else { ms.height };
        if phase != site.phase || params != ms.params || height != ms.height {
            out.push(Update::Mass { id: *id, height, phase, params });
        }
    }
    // The levels whose site elements change, and the boxes of what changed there (as it is and as it was).
    let mut dirty: BTreeMap<Guid, Vec<(Id, Option<Bx>, bool)>> = BTreeMap::new();
    if all {
        for level in by_level.keys() {
            dirty.entry(*level).or_default();
        }
    } else {
        for id in &ids {
            for (e, now) in [(m.elements.get(*id), true), (before.and_then(|b| b.elements.get(*id)), false)] {
                let Some(e) = e else { continue };
                if let Some(level) = level_of(e) {
                    dirty.entry(level).or_default().push((*id, element_box(e), now && matches!(e, Element::Site(_))));
                }
            }
        }
    }
    let levels: Vec<parking::LevelIn> =
        m.levels().iter().map(|(_, l)| parking::LevelIn { name: l.name.clone(), elevation: l.elevation }).collect();
    for (level, touched) in dirty {
        let Some(sites) = by_level.get(&level) else { continue };
        let kind_of = |id: Id| sites.iter().find(|(i, _)| *i == id).map(|(_, s)| s.kind.as_str());
        let was_kind = |id: Id| match before.and_then(|b| b.elements.get(id)) {
            Some(Element::Site(s)) => Some(s.kind.clone()),
            _ => None,
        };
        let changed_here: BTreeSet<Id> = if all {
            sites.iter().map(|(id, _)| *id).collect()
        } else {
            touched.iter().filter(|(_, _, site_now)| *site_now).map(|(id, _, _)| *id).collect()
        };
        // Lanes meet lanes: those within reach of a changed lane (as it is, or was) are shaped again with it, and theirs
        // given to them so their own corners stay as they were.
        let is_lane = |id: Id| kind_of(id) == Some("lane") || was_kind(id).as_deref() == Some("lane");
        let lane_boxes: Vec<Bx> =
            touched.iter().filter(|(id, _, _)| is_lane(*id)).filter_map(|(_, b, _)| b.map(|b| grow(b, REACH))).collect();
        let lanes: Vec<(Id, Option<Bx>)> = sites.iter().filter(|(_, s)| s.kind == "lane").map(|(id, s)| (*id, site_box(s))).collect();
        let near = |boxes: &[Bx], skip: &BTreeSet<Id>| -> BTreeSet<Id> {
            lanes
                .iter()
                .filter(|(id, b)| !skip.contains(id) && b.is_some_and(|b| boxes.iter().any(|c| meet(&b, c))))
                .map(|(id, _)| *id)
                .collect()
        };
        let mut write: BTreeSet<Id> = changed_here.clone();
        let n1 = if all { BTreeSet::new() } else { near(&lane_boxes, &write) };
        write.extend(&n1);
        let n1_boxes: Vec<Bx> = lanes.iter().filter(|(id, _)| n1.contains(id)).filter_map(|(_, b)| b.map(|b| grow(b, REACH))).collect();
        let n2 = if all { BTreeSet::new() } else { near(&n1_boxes, &write) };
        // A ramp changed: the parking bands keep clear of the ramps, so they are shaped again; and a band, with every ramp.
        let ramp_changed = touched.iter().any(|(id, _, _)| kind_of(*id) == Some("ramp") || was_kind(*id).as_deref() == Some("ramp"));
        if ramp_changed {
            write.extend(sites.iter().filter(|(_, s)| s.kind == "parking_band").map(|(id, _)| *id));
        }
        let with_bands = write.iter().any(|id| kind_of(*id) == Some("parking_band"));
        let shape: Vec<(Id, &Site)> = sites
            .iter()
            .filter(|(id, s)| write.contains(id) || n2.contains(id) || (with_bands && s.kind == "ramp"))
            .map(|(id, s)| (*id, *s))
            .collect();
        let keys: Vec<String> = shape.iter().map(|(id, _)| m.guid(*id).to_string()).collect();
        let items: Vec<shapes::Item> = shape
            .iter()
            .zip(&keys)
            .map(|((_, s), key)| shapes::Item {
                key,
                kind: &s.kind,
                use_: s.use_or_default(),
                props: &s.props,
                phase: &s.phase,
                context: s.context(),
            })
            .collect();
        let shaped = shapes::normalise(&items, &figures);
        let mut now: BTreeMap<Id, Site> = BTreeMap::new();
        for ((id, s), sh) in shape.iter().zip(shaped) {
            if !write.contains(id) {
                continue;
            }
            let mut site = (*s).clone();
            for (k, v) in sh.filled {
                site.props.insert(k, v);
            }
            site.outline = sh.outline;
            site.parts = sh.parts;
            site.notes = sh.notes;
            site.use_ = s.use_or_default().to_string();
            let was = before.and_then(|b| match b.elements.get(*id) {
                Some(Element::Site(old)) => Some((old.phase.as_str(), &old.params)),
                _ => None,
            });
            let (phase, params) = phase_in_step(&m.phases, &site.phase, &site.params, was);
            (site.phase, site.params) = (phase, params);
            if site.kind != "car_park" {
                site.layout = None;
            }
            now.insert(*id, site);
        }
        // Car parks: laid out again when they changed, or what they keep clear of did, near them.
        let level_name = m.level(level).map(|l| l.name.clone()).unwrap_or_default();
        let changed_boxes: Vec<(Id, Bx)> = touched.iter().filter_map(|(id, b, _)| b.map(|b| (*id, grow(b, 10_000.0)))).collect();
        let parks: Vec<Id> = sites
            .iter()
            .filter(|(_, s)| s.kind == "car_park")
            .filter(|(id, s)| {
                all || write.contains(id)
                    || site_box(now.get(id).unwrap_or(s)).is_some_and(|b| changed_boxes.iter().any(|(other, c)| other != id && meet(&b, c)))
            })
            .map(|(id, _)| *id)
            .collect();
        if !parks.is_empty() {
            // What stands on the level: the other site elements, as they now are, and the masses.
            let mut obstacles: Vec<(Id, parking::Obstacle)> = sites
                .iter()
                .map(|(id, s)| {
                    let s = now.get(id).unwrap_or(s);
                    let kind = if s.kind == "lane" { format!("lane/{}", s.use_or_default()) } else { s.kind.clone() };
                    (*id, parking::Obstacle { kind, level: level_name.clone(), outline: s.outline.clone() })
                })
                .collect();
            obstacles.extend(masses.iter().filter(|(_, ms)| ms.level == level).map(|(id, ms)| {
                (
                    *id,
                    parking::Obstacle {
                        kind: "mass".into(),
                        level: level_name.clone(),
                        outline: vec![Poly { outer: ms.footprint(), holes: Vec::new() }],
                    },
                )
            }));
            for id in parks {
                let Some(s) = now.get(&id).cloned().or_else(|| sites.iter().find(|(i, _)| *i == id).map(|(_, s)| (*s).clone())) else {
                    continue;
                };
                let others: Vec<parking::Obstacle> = obstacles.iter().filter(|(o, _)| *o != id).map(|(_, o)| o.clone()).collect();
                let mut layout = parking::lay_out(&s.props, &level_name, &levels, &others, &[]);
                let band = m.guid(id).to_string();
                for piece in &mut layout.pieces {
                    piece.props.insert("band".into(), json!(band));
                }
                let mut site = s;
                site.layout = Some(layout);
                now.insert(id, site);
            }
        }
        for (id, site) in now {
            if !matches!(m.elements.get(id), Some(Element::Site(old)) if **old == site) {
                out.push(Update::Site { id, site: Box::new(site) });
            }
        }
    }
    out
}

pub fn apply(m: &mut Model, updates: Vec<Update>) {
    for u in updates {
        match u {
            Update::Site { id, site } => {
                if let Some(Element::Site(s)) = m.elements.get_mut(id) {
                    *s = site;
                }
            }
            Update::Mass { id, height, phase, params } => {
                if let Some(Element::Mass(ms)) = m.elements.get_mut(id) {
                    ms.height = height;
                    ms.params = params;
                    if let Some(site) = &mut ms.site {
                        site.phase = phase;
                    }
                }
            }
        }
    }
}

// ====================================================================== the toposolid and its pads
impl Model {
    /// The building pads that cut toposolid `f`: on its level, each wholly inside its outline and outside its openings
    /// and the other pads taken.
    pub fn pads_in(&self, f: &Floor) -> Vec<(Id, &BuildingPad)> {
        let mut out: Vec<(Id, &BuildingPad)> = Vec::new();
        for (id, e) in self.elements.iter() {
            let Element::BuildingPad(p) = e else { continue };
            if p.level != f.level || p.boundary.len() < 3 {
                continue;
            }
            let apart =
                |ring: &[P2]| !p.boundary.iter().any(|q| geom::contains(ring, *q)) && !ring.iter().any(|q| geom::contains(&p.boundary, *q));
            let inside = p.boundary.iter().all(|q| geom::contains(&f.boundary, *q))
                && !f.boundary.iter().any(|q| geom::contains(&p.boundary, *q))
                && f.openings.iter().all(|o| apart(o))
                && out.iter().all(|(_, t)| apart(&t.boundary));
            if inside {
                out.push((*id, p));
            }
        }
        out
    }

    /// Toposolid `f` as its building pads cut it: each pad's outline an opening of it, its corners on the terrain where it
    /// is shaped, so the ground round a pad stays as it was. The pad fills the hole at its own height (Revit's pad cuts
    /// the toposolid down, or fills it up, to its bottom; here the hole runs through the toposolid's whole depth and the
    /// pad is a slab in it).
    pub fn toposolid_cut(&self, f: &Floor) -> Floor {
        let pads = self.pads_in(f);
        if pads.is_empty() {
            return f.clone();
        }
        let mut cut = f.clone();
        for (_, p) in pads {
            if !f.shape.is_empty() {
                for q in &p.boundary {
                    if !cut.shape.points.iter().any(|s| s.at.dist(*q) <= crate::model::TOL) {
                        cut.shape.points.push(ShapePoint { at: *q, z: self.shape_height(f, *q), ..Default::default() });
                    }
                }
            }
            let mut ring = p.boundary.clone();
            if geom::area(&ring) > 0.0 {
                ring.reverse();
            }
            cut.openings.push(ring);
        }
        cut
    }

    /// A building pad's top and bottom, mm: its level's elevation and offset, and its type's thickness under that.
    pub fn pad_span(&self, p: &BuildingPad) -> (f64, f64) {
        let top = self.elevation(p.level) + p.offset;
        let t = self.types.floor.get(&p.type_name).map_or(300.0, |t| t.thickness);
        (top - t, top)
    }
}

// ====================================================================== colours
fn hex(s: &str) -> Rgb {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0x98a2b3);
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// A kind's [fill, stroke] on the plan, as Arqiflow's (palette.ts SITE_COLORS, draw-site.ts); a road's by its parts.
pub fn plan_colours(kind: &str, use_: &str, survey: bool) -> (Rgb, Rgb) {
    let (f, s) = match kind {
        "landscape" if use_ == "meadow" => ("#e4eec4", "#98ad5f"),
        "landscape" if use_ == "lawn" || use_ == "orchard" => ("#d6ebc6", "#7fae63"),
        "landscape" => ("#c4dfae", "#6f9a52"),
        "paving" => ("#ebe6dd", "#a39882"),
        "terrace" => ("#ece4d6", "#a38c6a"),
        "ramp" => ("#e7e2d8", "#7a6f5c"),
        "aisle" => ("#eef0f3", "#98a2b3"),
        "stall" | "parking_stall" => ("#f7f8fa", "#98a2b3"),
        "accessible_stall" => ("#dbe7fb", "#2f6fd6"),
        "ev_stall" => ("#dcf5e6", "#12b76a"),
        "column" => ("#344054", "#101828"),
        "car_park" => ("#f7f8fa", "#667085"),
        "lane" => ("#ece6da", "#a39882"),
        "water" | "stream" => ("#cfe6f3", "#4f97c4"),
        "hedge" => ("#b9d8a6", "#5f8f4a"),
        "pitch" => ("#cfe8c0", "#4f8f3a"),
        "playground" => ("#f6e3c4", "#c9913d"),
        "play_area" => ("#fbeed8", "#c9913d"),
        "bike_rack" => ("#e4e7ec", "#667085"),
        "parking_band" => ("#f2f4f7", "#98a2b3"),
        "tree" if survey => ("#6f9f55", "#3c6b2a"),
        "tree" => ("#a9d18e", "#4f8f3a"),
        "mass" => ("#e4e7ec", "#344054"),
        _ => ("#eef0f3", "#98a2b3"),
    };
    (hex(f), hex(s))
}

/// A road's colours, as Arqiflow's draw-roads.ts: [asphalt, pavement, kerb, back of the pavement, planted median, marking];
/// context's paler.
pub fn road_colours(context: bool) -> [Rgb; 6] {
    let c = if context {
        ["#ccd0d5", "#ecebe7", "#9aa1ab", "#cfcbc3", "#dbe6d1", "#ffffff"]
    } else {
        ["#a4aab2", "#e6e1d7", "#6f7782", "#b5ac9b", "#c9dcb6", "#ffffff"]
    };
    c.map(hex)
}

/// What a surface looks like in 3D (Arqiflow's view3d-look.ts, its base colour).
pub fn surface_colour(surface: &str) -> Option<Rgb> {
    Some(hex(match surface {
        "block_paving" => "#8f8371",
        "asphalt" => "#5c6065",
        "gravel" => "#cdc3ae",
        "resin" => "#b59a6e",
        "stone" => "#a7a39b",
        "grass_pavers" => "#8aa067",
        "concrete" => "#b8b6b0",
        "grass" => "#7ea458",
        "meadow" => "#a2b46a",
        "planting" => "#5b8743",
        "natural" => "#4d8dbb",
        "synthetic" => "#4f9a4a",
        "hard" => "#9a7c62",
        "safety" => "#c9913d",
        "sand" => "#dcc79a",
        _ => return None,
    }))
}

// ====================================================================== 3D
/// A piece of the 3D view: a ring in plan from one height up to another, in a colour.
pub type Piece3 = (Vec<P2>, f64, f64, Rgb);

fn num(p: &Props, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

/// The rings of `polys` with no holes in them: as they are, or in pieces where a hole cuts one.
fn rings_of(polys: &[Poly]) -> Vec<Vec<P2>> {
    polys
        .iter()
        .flat_map(|p| {
            if p.holes.is_empty() || p.outer.len() < 3 {
                vec![p.outer.clone()]
            } else {
                crate::model::solid(std::slice::from_ref(&p.outer), &p.holes)
            }
        })
        .filter(|r| r.len() >= 3)
        .collect()
}

/// What the 3D view builds of a site element: each area a thin slab over its level (lawns 20 mm, paving 40, lanes 50, a
/// road's carriageway 30 and its pavements and median a kerb's 150 over it, water at its level under the ground), a
/// tree a trunk and a crown, a hedge its height, a car park its aisles, bays and columns at their levels.
pub fn solids(m: &Model, s: &Site) -> Vec<Piece3> {
    let z = m.elevation(s.level);
    let mut out: Vec<Piece3> = Vec::new();
    let surface = surface_colour(s.surface());
    let use_ = s.use_or_default();
    let mut slab = |polys: &[Poly], lo: f64, hi: f64, c: Rgb| {
        for r in rings_of(polys) {
            out.push((r, z + lo, z + hi, c));
        }
    };
    let part = |role: &str| s.parts.iter().filter(|p| p.role == role).flat_map(|p| p.polys.clone()).collect::<Vec<Poly>>();
    match s.kind.as_str() {
        "lane" if use_ == "road" => {
            let c = road_colours(s.context());
            slab(&part("carriageway"), 0.0, 30.0, hex("#5c6065"));
            slab(&part("pavement"), 0.0, 150.0, surface.unwrap_or(c[1]));
            slab(&part("median"), 0.0, 150.0, hex("#5b8743"));
        }
        "lane" => slab(&s.outline, 0.0, 50.0, surface.unwrap_or_else(|| hex("#8f8371"))),
        "landscape" => slab(
            &s.outline,
            0.0,
            20.0,
            surface.unwrap_or_else(|| {
                hex(if use_ == "meadow" {
                    "#a2b46a"
                } else if use_ == "lawn" || use_ == "orchard" {
                    "#7ea458"
                } else {
                    "#5b8743"
                })
            }),
        ),
        "water" | "stream" => {
            let level = if s.kind == "water" { num(&s.props, "level").unwrap_or(-300.0) } else { -200.0 };
            slab(&s.outline, level - 20.0, level, hex("#4d8dbb"));
            slab(&part("bank"), 0.0, 20.0, hex("#5b8743"));
        }
        "hedge" => slab(&s.outline, 0.0, num(&s.props, "height").unwrap_or(2000.0).max(1.0), hex("#4f7a3a")),
        "tree" => {
            let h = num(&s.props, "height").unwrap_or(8000.0).max(1.0);
            let crown = part("crown");
            if let Some(at) = s.parts.iter().find(|p| p.role == "point").and_then(|p| p.lines.first()).and_then(|l| l.first()) {
                let t = 150.0;
                let trunk = vec![*at + P2(-t, -t), *at + P2(t, -t), *at + P2(t, t), *at + P2(-t, t)];
                out.push((trunk, z, z + h * 0.35, hex("#6b4f3a")));
            }
            for r in rings_of(&crown) {
                out.push((r, z + h * 0.35, z + h, hex("#5f8f4a")));
            }
        }
        "car_park" => {
            for piece in s.layout.iter().flat_map(|l| &l.pieces) {
                let at = m.levels().iter().find(|(_, l)| l.name == piece.level).map_or(z, |(_, l)| l.elevation);
                let (lo, hi, c) = match piece.kind.as_str() {
                    "column" => (0.0, num(&s.props, "floor_to_floor").unwrap_or(3500.0).max(1.0), hex("#9aa3b1")),
                    "stall" => (0.0, 40.0, hex("#a4aab2")),
                    _ => (0.0, 30.0, hex("#5c6065")),
                };
                for r in rings_of(&piece.outline) {
                    out.push((r, at + lo, at + hi, c));
                }
            }
        }
        "parking_band" => {
            slab(&part("aisle"), 0.0, 30.0, hex("#5c6065"));
            slab(&part("stall"), 0.0, 40.0, hex("#a4aab2"));
            slab(&part("accessible_stall"), 0.0, 40.0, hex("#2f6fd6"));
        }
        "parking_stall" => slab(
            &s.outline,
            0.0,
            40.0,
            if use_ == "accessible" {
                hex("#2f6fd6")
            } else if use_ == "ev" {
                hex("#12b76a")
            } else {
                hex("#a4aab2")
            },
        ),
        "pitch" => {
            slab(&part("run_off"), 0.0, 30.0, hex("#6f9a52"));
            slab(&s.outline, 0.0, 40.0, surface.unwrap_or_else(|| hex("#7ea458")));
        }
        "playground" | "play_area" => slab(&s.outline, 0.0, 40.0, surface.unwrap_or_else(|| hex("#c9913d"))),
        "bike_rack" => slab(&s.outline, 0.0, 40.0, hex("#b8b6b0")),
        "ramp" => slab(&s.outline, 0.0, 50.0, hex("#b8b6b0")),
        _ => slab(&s.outline, 0.0, 40.0, surface.unwrap_or_else(|| hex("#8f8371"))),
    }
    out
}

/// A copy of `ms`'s masterplan figures given back by `site_edit` and `elements`.
pub fn mass_json(site: &MassSite) -> Value {
    serde_json::to_value(site).unwrap_or(Value::Null)
}
