//! The Massing & Site tab's site tools, after Revit's ([the site](../../../docs/areas/site.md)): Toposolid, Sub-divide and
//! Road beside it, Building Pad, Property Line, Site Component, Parking Component and Parking Area beside it. A tool that
//! draws an outline (a toposolid, a pad, a property line, a sub-region, a car park) or a path (a road, a hedge, a stream, a
//! row of parking) sketches it, as Floor does, with the Draw panel's lines and arcs; one that places a thing (a tree, a
//! bike rack, a play area, a pitch, a parking stall) takes a click, Space turning it. Each makes its element with the
//! core's commands (`toposolid`, `building_pad`, `property_line`, `site`); the Options Bar holds the choices. Here too:
//! the site elements' Properties (edited through `site_edit`), their grips (a path's points, an area's control points, a
//! tree's point) and Edit Boundary / Edit Path.

use std::cell::RefCell;

use bimcraft_core::geom::{self, P2};
use bimcraft_core::masterplan::{self, Site};
use bimcraft_core::model::{Element, Id, Model, TOL};
use bimcraft_core::site::Props;
use bimcraft_core::site::shapes::{self, Item};
use bimcraft_core::sketch::{self as solver, SketchLine};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{ComboBox, RichText, TextEdit, Ui};
use serde_json::{Value, json};

use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::properties::{combo, grid, group_header, number};
use crate::sketch::Of;
use crate::style::pal;
use crate::{BimApp, Status, Tool, big_button, panel, small_button};

/// The site tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteTool {
    Toposolid,
    /// Revit's Sub-divide / Graded Region: an area of the ground of the Options Bar's type (paving, landscape, water…).
    Subdivide,
    Road,
    BuildingPad,
    PropertyLine,
    /// Planting and site furniture: a tree, a hedge, a bike rack, a play area.
    Component,
    /// Revit's Parking Component: one stall.
    Stall,
    /// A row of stalls along a path, or a car park laid out over an area.
    ParkingArea,
}

/// How a tool takes its clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taking {
    /// An outline sketched, Finish making it.
    Outline,
    /// A path sketched, lines and arcs end to end.
    Path,
    /// A click where it stands.
    Point,
}

/// Sub-divide's types, the site's kinds of ground.
pub const SUBDIVIDE: [&str; 7] = ["paving", "terrace", "landscape", "water", "stream", "pitch", "playground"];
/// Site Component's kinds.
pub const COMPONENTS: [&str; 4] = ["tree", "hedge", "bike_rack", "play_area"];

/// What the site tools' Options Bars hold, kept between uses as the other tools' are.
#[derive(Clone, Debug, PartialEq)]
pub struct SiteChoices {
    pub subdivide: &'static str,
    pub subdivide_use: &'static str,
    pub road_use: &'static str,
    /// Typed in the project's units; blank, the figures' width for its use.
    pub road_width: String,
    /// A road's lanes; blank, as many as its width takes.
    pub road_lanes: String,
    pub component: &'static str,
    pub component_use: &'static str,
    /// A tree's crown, a hedge's width, a play area's radius (lengths) or a bike rack's spaces; blank, the figures'.
    pub component_size: String,
    pub stall_use: &'static str,
    /// Parking Area along a path (a row of stalls) rather than over an area (a car park).
    pub parking_row: bool,
    /// A building pad's top over its level; blank, at it.
    pub pad_offset: String,
    /// The way a placed thing turns, degrees: Space adds a quarter turn.
    pub angle: f64,
}

impl Default for SiteChoices {
    fn default() -> SiteChoices {
        SiteChoices {
            subdivide: "paving",
            subdivide_use: "forecourt",
            road_use: "road",
            road_width: String::new(),
            road_lanes: String::new(),
            component: "tree",
            component_use: "tree",
            component_size: String::new(),
            stall_use: "standard",
            parking_row: false,
            pad_offset: String::new(),
            angle: 0.0,
        }
    }
}

/// The words for the site's kinds, uses, surfaces and phases, as the interface shows them.
const NAMES: &[(&str, &str)] = &[
    ("paving", msg!("Paving")),
    ("terrace", msg!("Terrace")),
    ("landscape", msg!("Landscape")),
    ("water", msg!("Water")),
    ("stream", msg!("Stream")),
    ("pitch", msg!("Pitch")),
    ("playground", msg!("Playground")),
    ("tree", msg!("Tree")),
    ("hedge", msg!("Hedge")),
    ("bike_rack", msg!("Bike Rack")),
    ("play_area", msg!("Play Area")),
    ("lane", msg!("Lane")),
    ("parking_band", msg!("Parking Row")),
    ("car_park", msg!("Car Park")),
    ("parking_stall", msg!("Parking Stall")),
    ("ramp", msg!("Ramp")),
    ("forecourt", msg!("Forecourt")),
    ("apron", msg!("Apron")),
    ("square", msg!("Square")),
    ("court", msg!("Court")),
    ("lawn", msg!("Lawn")),
    ("meadow", msg!("Meadow")),
    ("planting", msg!("Planting")),
    ("garden", msg!("Garden")),
    ("woodland", msg!("Woodland")),
    ("orchard", msg!("Orchard")),
    ("bank", msg!("Bank")),
    ("pond", msg!("Pond")),
    ("lake", msg!("Lake")),
    ("wetland", msg!("Wetland")),
    ("basin", msg!("Basin")),
    ("rill", msg!("Rill")),
    ("ditch", msg!("Ditch")),
    ("five_a_side", msg!("Five-a-side")),
    ("futsal", msg!("Futsal")),
    ("football_7", msg!("Football 7-a-side")),
    ("football_11", msg!("Football 11-a-side")),
    ("basketball", msg!("Basketball")),
    ("tennis", msg!("Tennis")),
    ("padel", msg!("Padel")),
    ("multi_sport", msg!("Multi-sport")),
    ("equipment", msg!("Equipment")),
    ("toddlers", msg!("Toddlers")),
    ("juniors", msg!("Juniors")),
    ("residents", msg!("Residents")),
    ("visitors", msg!("Visitors")),
    ("footpath", msg!("Footpath")),
    ("road", msg!("Road")),
    ("promenade", msg!("Promenade")),
    ("parking", msg!("Parking")),
    ("standard", msg!("Standard")),
    ("accessible", msg!("Accessible")),
    ("ev", msg!("EV")),
    ("cars", msg!("Cars")),
    ("housing", msg!("Housing")),
    ("office", msg!("Office")),
    ("retail", msg!("Retail")),
    ("mixed", msg!("Mixed Use")),
    ("community", msg!("Community")),
    ("new", msg!("New")),
    ("existing", msg!("Existing")),
    ("demolished", msg!("Demolished")),
    ("planned", msg!("Planned")),
    ("block_paving", msg!("Block Paving")),
    ("asphalt", msg!("Asphalt")),
    ("gravel", msg!("Gravel")),
    ("resin", msg!("Resin")),
    ("stone", msg!("Stone")),
    ("grass_pavers", msg!("Grass Pavers")),
    ("concrete", msg!("Concrete")),
    ("grass", msg!("Grass")),
    ("natural", msg!("Natural")),
    ("synthetic", msg!("Synthetic")),
    ("hard", msg!("Hard")),
    ("safety", msg!("Safety Surface")),
    ("sand", msg!("Sand")),
];

/// A kind's, a use's, a surface's or a phase's name; one this list hasn't, its own words.
pub fn name(key: &str) -> String {
    match NAMES.iter().find(|(k, _)| *k == key) {
        Some((_, n)) => t(n).to_string(),
        None => {
            let words = key.replace('_', " ");
            let mut c = words.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

/// The props' labels in Properties.
const PROP_NAMES: &[(&str, &str)] = &[
    ("width", msg!("Width")),
    ("lanes", msg!("Lanes")),
    ("lane_width", msg!("Lane Width")),
    ("oneway", msg!("One Way")),
    ("one_way", msg!("One Way")),
    ("median", msg!("Median")),
    ("kerb", msg!("Kerb")),
    ("markings", msg!("Markings")),
    ("corner_radius", msg!("Corner Radius")),
    ("surface", msg!("Surface")),
    ("crown", msg!("Crown")),
    ("height", msg!("Height")),
    ("species", msg!("Species")),
    ("size", msg!("Size")),
    ("survey", msg!("Survey")),
    ("angle", msg!("Angle (°)")),
    ("length", msg!("Length")),
    ("margin", msg!("Run-off")),
    ("spaces", msg!("Spaces")),
    ("type", msg!("Type")),
    ("radius", msg!("Radius")),
    ("smooth", msg!("Smoothing")),
    ("level", msg!("Water Level")),
    ("depth", msg!("Depth")),
    ("bank", msg!("Bank")),
    ("aisle", msg!("Aisle")),
    ("bay_depth", msg!("Bay Depth")),
    ("bay_width", msg!("Bay Width")),
    ("sides", msg!("Sides")),
    ("standard", msg!("Standard")),
    ("stall_width", msg!("Stall Width")),
    ("stall_depth", msg!("Stall Depth")),
    ("setback", msg!("Setback")),
    ("end_island", msg!("End Island")),
    ("island_every", msg!("Island Every")),
    ("ev_share", msg!("EV Share")),
    ("column_every", msg!("Column Every")),
    ("column_size", msg!("Column Size")),
    ("floor_to_floor", msg!("Floor to Floor")),
    ("ramp_slope", msg!("Ramp Slope")),
    ("direction", msg!("Direction (°)")),
    ("open_length", msg!("Open Length")),
    ("to_level", msg!("To Level")),
    ("gradient", msg!("Gradient (%)")),
    ("from_lane", msg!("From Lane")),
    ("label", msg!("Label")),
    ("built", msg!("Built")),
    ("in", msg!("In")),
    ("storeys", msg!("Storeys")),
    ("attic", msg!("Attic")),
    ("floor_height", msg!("Floor Height")),
    ("roof", msg!("Roof")),
    ("pitch", msg!("Roof Pitch (°)")),
    ("ridge", msg!("Ridge (°)")),
    ("overhang", msg!("Overhang")),
    ("steps", msg!("Steps")),
    ("run", msg!("Run")),
    ("unit_type", msg!("Unit Type")),
    ("plates", msg!("Plates")),
    ("unit_floors", msg!("Unit Floors")),
    ("units", msg!("Units")),
];

fn prop_name(key: &str) -> String {
    PROP_NAMES.iter().find(|(k, _)| *k == key).map_or_else(|| name(key), |(_, n)| t(n).to_string())
}

/// The props Properties edits: what the catalogue gives the kind that is one value (a length, a number, yes or no, text),
/// not its points or paths (its grips and Edit Boundary / Edit Path), its phase (a row of its own) or what a reader keeps.
fn editable(kind: &str) -> Vec<(String, String)> {
    shapes::props_of(kind)
        .into_iter()
        .filter_map(|(k, v)| Some((k, v.as_str()?.to_string())))
        .filter(|(k, ty)| {
            !matches!(k.as_str(), "phase" | "ref" | "fetched" | "made" | "solved")
                && matches!(ty.as_str(), "length" | "count" | "angle" | "percent" | "number" | "fraction" | "boolean" | "string")
        })
        .collect()
}

/// A prop as Properties shows it: a length in the project's units, yes or no, a number, text.
fn shown(v: &Value, ty: &str, units: Units) -> String {
    match (v, ty) {
        (Value::Null, _) => String::new(),
        (Value::Number(n), "length") => n.as_f64().map(|x| units.length(x)).unwrap_or_default(),
        (Value::Number(n), _) => n.as_f64().map(|x| format!("{}", (x * 1000.0).round() / 1000.0)).unwrap_or_default(),
        (Value::String(s), _) => s.clone(),
        (v, _) => v.to_string(),
    }
}

/// A prop as typed, of its type: blank takes it back to its default (null).
fn parsed(text: &str, ty: &str, units: Units) -> Result<Value, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Value::Null);
    }
    match ty {
        "length" => units.parse(text).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not a length", &[text])),
        "count" => number(text).filter(|x| *x >= 0.0).map(|x| json!(x.round() as i64)).ok_or_else(|| tf("`{}` is not a count", &[text])),
        "string" => Ok(json!(text)),
        _ => number(text).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not a number", &[text])),
    }
}

/// A path's points as sketch lines: each piece straight, or an arc by its bulge.
pub fn path_lines(path: &[[f64; 3]], closed: bool) -> Vec<SketchLine> {
    let n = path.len();
    let count = if closed { n } else { n.saturating_sub(1) };
    (0..count)
        .filter_map(|i| {
            let (a, b) = (path.get(i)?, path.get((i + 1) % n)?);
            let (p, q) = (P2(a[0], a[1]), P2(b[0], b[1]));
            let sweep = (4.0 * a[2].atan()).to_degrees();
            (p.dist(q) > TOL).then(|| bimcraft_core::curves::Curve::arc(p, q, sweep).into())
        })
        .collect()
}

/// `props[key]`, a path's or a ring's points `[x, y, bulge]` (a ring's bulges 0).
fn props_points(props: &Props, key: &str) -> Vec<[f64; 3]> {
    props.get(key).and_then(|v| shapes::path_points(v).ok()).unwrap_or_default()
}

/// One site element shaped on its own, as the plan would draw it: a tool's preview.
pub fn ghost(m: &Model, level: Id, kind: &str, use_: &str, props: Props) -> Option<Element> {
    let item = Item { key: "preview", kind, use_, props: &props, phase: "new", context: false };
    let shaped = shapes::normalise(&[item], &masterplan::figures(m)).pop()?;
    if shaped.outline.is_empty() {
        return None;
    }
    let mut props = props;
    props.extend(shaped.filled);
    Some(Element::Site(Box::new(Site {
        kind: kind.into(),
        use_: use_.into(),
        name: String::new(),
        level: m.guid(level),
        phase: "new".into(),
        source: String::new(),
        reference: String::new(),
        props,
        outline: shaped.outline,
        parts: shaped.parts,
        layout: None,
        notes: Vec::new(),
        params: Default::default(),
    })))
}

/// What the last frame's path preview was shaped from, and what it made: the same lines and choices make it again only once.
pub type GhostCache = RefCell<Option<(Vec<SketchLine>, Value, Option<Element>)>>;

impl SiteTool {
    /// What the tool's contextual tab and its sketch are called.
    pub fn title(self) -> &'static str {
        match self {
            SiteTool::Toposolid => msg!("Create Toposolid"),
            SiteTool::Subdivide => msg!("Create Sub-region"),
            SiteTool::Road => msg!("Create Road Path"),
            SiteTool::BuildingPad => msg!("Create Pad Boundary"),
            SiteTool::PropertyLine => msg!("Create Property Line"),
            SiteTool::Component => msg!("Place Site Component"),
            SiteTool::Stall => msg!("Place Parking Component"),
            SiteTool::ParkingArea => msg!("Create Parking Area"),
        }
    }

    /// The category of what it makes.
    pub fn category(self) -> &'static str {
        match self {
            SiteTool::Toposolid => "toposolid",
            SiteTool::BuildingPad => "buildingpad",
            SiteTool::PropertyLine => "propertyline",
            _ => "site",
        }
    }
}

impl SiteChoices {
    /// The site kind and use a tool makes now, for the tools that make a `site` element.
    pub fn kind(&self, tool: SiteTool) -> Option<(&'static str, &'static str)> {
        Some(match tool {
            SiteTool::Subdivide => (self.subdivide, self.subdivide_use),
            SiteTool::Road => ("lane", self.road_use),
            SiteTool::Component => (self.component, self.component_use),
            SiteTool::Stall => ("parking_stall", self.stall_use),
            SiteTool::ParkingArea if self.parking_row => ("parking_band", "parking"),
            SiteTool::ParkingArea => ("car_park", "parking"),
            _ => return None,
        })
    }

    /// How the tool takes its clicks now: by the shape of the kind it makes.
    pub fn taking(&self, tool: SiteTool) -> Taking {
        match self.kind(tool) {
            None => Taking::Outline,
            Some((kind, _)) => match shapes::kind(kind).map(|k| k.shape) {
                Some("path") => Taking::Path,
                Some("point") => Taking::Point,
                // A pitch has a standard size, set where its middle goes; a play area is a ring round a point.
                _ if matches!(kind, "pitch" | "play_area") => Taking::Point,
                _ => Taking::Outline,
            },
        }
    }
}

impl BimApp {
    /// Whether site tool `tool` is the one in hand, placing or sketching.
    pub(crate) fn site_on(&self, tool: SiteTool) -> bool {
        self.tool == Tool::Site(tool) || matches!(self.sketch.as_ref().map(|s| s.of), Some(Of::Site { tool: t, .. }) if t == tool)
    }

    /// A site tool picked: a sketch begun at once for an outline or a path, as Floor's is, in a plan; else clicks place.
    pub(crate) fn begin_site(&mut self, tool: SiteTool) {
        if self.site.taking(tool) == Taking::Point || self.sketch.is_some() {
            return;
        }
        if matches!(self.sheet, crate::Sheet::Plan(_))
            && let Some(level) = self.plan_level()
        {
            self.start_sketch(Of::Site { level, tool }, Vec::new());
        } else {
            self.status = Status { text: t("Open a floor plan to draw on the site.").into(), error: true };
        }
    }

    /// An Options Bar choice changed how the tool takes its clicks (Sub-divide's Pitch places where Paving sketches): the
    /// tool begun again that way, a sketch with no lines yet dropped. A sketch with lines keeps them.
    fn site_retake(&mut self, tool: SiteTool, was: Taking) {
        let now = self.site.taking(tool);
        if now == was {
            return;
        }
        if self.sketch.as_ref().is_some_and(|s| !s.lines.is_empty()) {
            self.status = Status { text: t("Finish or cancel the sketch before changing what it draws.").into(), error: true };
            return;
        }
        self.sketch = None;
        self.set_tool(Tool::Site(tool));
    }

    /// The Massing & Site tab's site panels: Model Site, Property Line's, and the components.
    pub(crate) fn site_panels(&mut self, ui: &mut Ui) {
        let tools = [
            (
                msg!("Model Site"),
                vec![
                    (
                        Icon::Toposolid,
                        msg!("Toposolid"),
                        msg!("Toposolid (TS): sketch the ground's outline; its heights and contours are edited after"),
                        SiteTool::Toposolid,
                    ),
                    (
                        Icon::Subdivide,
                        msg!("Sub-divide"),
                        msg!("Sub-divide / Graded Region: an area of the ground, of the Options Bar's type: paving, a terrace, a lawn, water, a stream, a pitch, a playground"),
                        SiteTool::Subdivide,
                    ),
                    (
                        Icon::Road,
                        msg!("Road"),
                        msg!("Road: sketch its path with lines and arcs; the Options Bar sets its use, width and lanes"),
                        SiteTool::Road,
                    ),
                    (
                        Icon::BuildingPad,
                        msg!("Building Pad"),
                        msg!("Building Pad (BP): sketch a flat pad cut into the toposolid, its top at the Options Bar's offset"),
                        SiteTool::BuildingPad,
                    ),
                ],
            ),
            (
                msg!("Modify Site"),
                vec![(
                    Icon::PropertyLine,
                    msg!("Property Line"),
                    msg!("Property Line (PL): sketch the plot's boundary, lines and arcs closed into a loop"),
                    SiteTool::PropertyLine,
                )],
            ),
            (
                msg!("Components"),
                vec![
                    (
                        Icon::SiteComponent,
                        msg!("Site Component"),
                        msg!("Site Component: a tree, a bike rack or a play area where you click (Space turns it), or a hedge along a sketched path"),
                        SiteTool::Component,
                    ),
                    (
                        Icon::ParkingComponent,
                        msg!("Parking Component"),
                        msg!("Parking Component: one parking stall where you click; Space turns it"),
                        SiteTool::Stall,
                    ),
                    (
                        Icon::ParkingArea,
                        msg!("Parking Area"),
                        msg!("Parking Area: a row of stalls along a sketched path, or a car park laid out over a sketched area"),
                        SiteTool::ParkingArea,
                    ),
                ],
            ),
        ];
        for (name, row) in tools {
            panel(ui, name, |ui| {
                for &(icon, label, tip, tool) in &row {
                    // Road beside Sub-divide and Parking Area beside Parking Component, as Revit puts a tool's sibling.
                    let small = matches!(tool, SiteTool::Road | SiteTool::ParkingArea);
                    let on = self.site_on(tool);
                    let clicked = if small { small_button(ui, icon, label, on, tip) } else { big_button(ui, icon, label, on, tip) }.clicked();
                    if clicked {
                        if self.sketch.is_some() {
                            self.cancel_sketch();
                        }
                        self.set_tool(Tool::Site(tool));
                    }
                }
            });
        }
    }

    /// A drop-down of `keys` by their names: the one picked, when one is.
    fn site_combo(ui: &mut Ui, salt: &str, label: &str, now: &'static str, keys: &[&'static str]) -> Option<&'static str> {
        let l = ui.label(t(label));
        let mut picked = None;
        let c = ComboBox::from_id_salt(salt).selected_text(name(now)).width(130.0).show_ui(ui, |ui| {
            for k in keys {
                if ui.selectable_label(*k == now, name(k)).clicked() && *k != now {
                    picked = Some(*k);
                }
            }
        });
        c.response.labelled_by(l.id);
        picked
    }

    /// The Options Bar of the site tool in hand, placing or sketching.
    pub(crate) fn site_options(&mut self, ui: &mut Ui, tool: SiteTool) {
        let was = self.site.taking(tool);
        let hint = self.session.model.units.name();
        let uses = |kind: &str| shapes::kind(kind).map(|k| k.uses).unwrap_or(&[]);
        match tool {
            SiteTool::Subdivide => {
                if let Some(k) = Self::site_combo(ui, "site-subdivide", msg!("Type:"), self.site.subdivide, &SUBDIVIDE) {
                    self.site.subdivide = k;
                    self.site.subdivide_use = uses(k).first().copied().unwrap_or(k);
                }
                let u = uses(self.site.subdivide);
                if u.len() > 1
                    && let Some(k) = Self::site_combo(ui, "site-subdivide-use", msg!("Use:"), self.site.subdivide_use, u)
                {
                    self.site.subdivide_use = k;
                }
            }
            SiteTool::Road => {
                if let Some(k) = Self::site_combo(ui, "site-road-use", msg!("Use:"), self.site.road_use, uses("lane")) {
                    self.site.road_use = k;
                }
                let l = ui.label(t("Width:"));
                ui.add(TextEdit::singleline(&mut self.site.road_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                if self.site.road_use == "road" {
                    let l = ui.label(t("Lanes:"));
                    ui.add(TextEdit::singleline(&mut self.site.road_lanes).desired_width(40.0)).labelled_by(l.id);
                }
            }
            SiteTool::Component => {
                if let Some(k) = Self::site_combo(ui, "site-component", msg!("Type:"), self.site.component, &COMPONENTS) {
                    self.site.component = k;
                    self.site.component_use = uses(k).first().copied().unwrap_or(k);
                    self.site.component_size.clear();
                }
                let u = uses(self.site.component);
                if u.len() > 1
                    && let Some(k) = Self::site_combo(ui, "site-component-use", msg!("Use:"), self.site.component_use, u)
                {
                    self.site.component_use = k;
                }
                let (label, hint) = match self.site.component {
                    "tree" => (msg!("Crown:"), hint),
                    "hedge" => (msg!("Width:"), hint),
                    "play_area" => (msg!("Radius:"), hint),
                    _ => (msg!("Spaces:"), ""),
                };
                let l = ui.label(t(label));
                ui.add(TextEdit::singleline(&mut self.site.component_size).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
            }
            SiteTool::Stall => {
                if let Some(k) = Self::site_combo(ui, "site-stall", msg!("Use:"), self.site.stall_use, uses("parking_stall")) {
                    self.site.stall_use = k;
                }
            }
            SiteTool::ParkingArea => {
                ui.radio_value(&mut self.site.parking_row, false, t("Area"));
                ui.radio_value(&mut self.site.parking_row, true, t("Row"));
            }
            SiteTool::BuildingPad => {
                let l = ui.label(t("Height Offset:"));
                ui.add(TextEdit::singleline(&mut self.site.pad_offset).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
            }
            SiteTool::Toposolid | SiteTool::PropertyLine => {}
        }
        if self.site.taking(tool) == Taking::Point {
            let l = ui.label(t("Angle:"));
            let mut a = format!("{}", self.site.angle);
            if ui.add(TextEdit::singleline(&mut a).desired_width(40.0).hint_text("°")).labelled_by(l.id).changed()
                && let Some(x) = number(&a).filter(|x| x.is_finite())
            {
                self.site.angle = x.rem_euclid(360.0);
            }
        }
        self.site_retake(tool, was);
    }

    /// The props a tool's choices give what it makes, before what is clicked or drawn.
    fn site_props(&self, tool: SiteTool) -> Props {
        let units = self.session.model.units;
        let mut p = Props::new();
        let length = |s: &str| units.parse(s.trim()).filter(|x| *x > 0.0);
        match tool {
            SiteTool::Road => {
                if let Some(w) = length(&self.site.road_width) {
                    p.insert("width".into(), json!(w));
                }
                if self.site.road_use == "road"
                    && let Some(n) = number(&self.site.road_lanes).filter(|n| *n >= 1.0)
                {
                    p.insert("lanes".into(), json!(n.round() as i64));
                }
            }
            SiteTool::Component => {
                let size = &self.site.component_size;
                match self.site.component {
                    "tree" => length(size).map(|x| p.insert("crown".into(), json!(x))),
                    "hedge" => length(size).map(|x| p.insert("width".into(), json!(x))),
                    "play_area" => length(size).map(|x| p.insert("radius".into(), json!(x))),
                    _ => number(size).filter(|n| *n >= 1.0).map(|n| p.insert("spaces".into(), json!(n.round() as i64))),
                };
            }
            _ => {}
        }
        p
    }

    /// What a click of a placing site tool makes at `at` on `level`: its kind, use and props.
    pub(crate) fn site_placed(&self, tool: SiteTool, at: P2) -> Option<(&'static str, &'static str, Props)> {
        let (kind, use_) = self.site.kind(tool)?;
        let mut p = self.site_props(tool);
        p.insert("at".into(), json!([at.0, at.1]));
        if shapes::props_of(kind).contains_key("angle") {
            p.insert("angle".into(), json!(self.site.angle));
        }
        Some((kind, use_, p))
    }

    /// A placing site tool's click: its element where clicked.
    pub(crate) fn site_click(&mut self, tool: SiteTool, level: Id, at: P2) {
        if let Some((kind, use_, props)) = self.site_placed(tool, at) {
            self.run("site", json!({"kind": kind, "use": use_, "level": level, "props": props}));
        }
    }

    /// What a placing site tool would make with the cursor at `at`: drawn under it.
    pub(crate) fn site_click_ghost(&self, m: &Model, tool: SiteTool, level: Id, at: P2) -> Option<Element> {
        let (kind, use_, props) = self.site_placed(tool, at)?;
        ghost(m, level, kind, use_, props)
    }

    /// While a site path is sketched: the element its lines make so far, drawn under them (a road with its kerbs and
    /// markings, a hedge, a row of stalls).
    pub(crate) fn site_sketch_ghost(&self, m: &Model) -> Option<Element> {
        let sk = self.sketch.as_ref()?;
        let Of::Site { level, tool } = sk.of else { return None };
        if self.site.taking(tool) != Taking::Path || sk.lines.is_empty() {
            return None;
        }
        let (kind, use_) = self.site.kind(tool)?;
        let key = json!([kind, use_, self.site_props(tool)]);
        if let Some((lines, k, e)) = self.site_ghost.borrow().as_ref()
            && *lines == sk.lines
            && *k == key
        {
            return e.clone();
        }
        let made = solver::chain(&sk.lines).ok().filter(|(_, closed)| !closed).and_then(|(path, _)| {
            let mut props = self.site_props(tool);
            props.insert("path".into(), json!(path));
            ghost(m, level, kind, use_, props)
        });
        *self.site_ghost.borrow_mut() = Some((sk.lines.clone(), key, made.clone()));
        made
    }

    /// Finish Edit Mode on a site tool's sketch: the outline or the path made into the tool's element. Whether it was made.
    pub(crate) fn finish_site_sketch(&mut self, level: Id, tool: SiteTool) -> Option<Value> {
        let lines = self.sketch.as_ref()?.lines.clone();
        let m = &self.session.model;
        let ring = |lines: &[SketchLine]| -> Result<Vec<P2>, String> {
            let r = solver::solve_one(lines).map_err(|p| crate::sketch::friendly(&p, lines, m.units))?;
            if !r.holes.is_empty() {
                return Err(t("Sketch one outline with no holes in it.").into());
            }
            Ok(r.outer)
        };
        let points = |r: &[P2]| json!(r.iter().map(|q| [q.0, q.1]).collect::<Vec<_>>());
        let made = match (tool, self.site.taking(tool)) {
            (SiteTool::Toposolid, _) => ring(&lines).map(|r| ("toposolid", json!({"boundary": points(&r), "level": level}))),
            (SiteTool::BuildingPad, _) => ring(&lines).map(|r| {
                let mut p = json!({"boundary": points(&r), "level": level});
                if let Some(o) = m.units.parse(self.site.pad_offset.trim()) {
                    p["offset"] = json!(o);
                }
                ("building_pad", p)
            }),
            (SiteTool::PropertyLine, _) => match solver::chain(&lines) {
                Ok((pts, true)) => Ok(("property_line", json!({"points": pts, "level": level}))),
                Ok(_) => Err(t("A property line is closed: end it where it began.").into()),
                Err(e) => Err(e),
            },
            (_, Taking::Path) => match solver::chain(&lines) {
                Ok((path, false)) => {
                    let (kind, use_) = self.site.kind(tool)?;
                    let mut props = self.site_props(tool);
                    props.insert("path".into(), json!(path));
                    Ok(("site", json!({"kind": kind, "use": use_, "level": level, "props": props})))
                }
                Ok(_) => Err(t("A path has two ends: leave it open.").into()),
                Err(e) => Err(e),
            },
            (_, Taking::Outline) => ring(&lines).map(|r| {
                let (kind, use_) = self.site.kind(tool).unwrap_or(("paving", "forecourt"));
                let mut props = self.site_props(tool);
                props.insert("control".into(), points(&r));
                ("site", json!({"kind": kind, "use": use_, "level": level, "props": props}))
            }),
            (_, Taking::Point) => return None,
        };
        match made {
            Ok((command, p)) => self.run(command, p),
            Err(e) => {
                self.status = Status { text: e, error: true };
                None
            }
        }
    }

    /// The site element, pad or property line selected alone, whose outline or path Edit Boundary / Edit Path sketches
    /// again (a toposolid's is the core's `edit_boundary`, as a floor's): whether its line is a path.
    pub(crate) fn site_boundary(&self) -> Option<(Id, bool)> {
        let &[id] = self.selection.as_slice() else { return None };
        match self.session.model.elements.get(id)? {
            Element::Site(st) if st.props.contains_key("path") => Some((id, true)),
            Element::Site(st) if st.props.contains_key("control") => Some((id, false)),
            Element::BuildingPad(_) | Element::PropertyLine(_) => Some((id, false)),
            _ => None,
        }
    }

    /// Edit Boundary / Edit Path on the selected site element: its line as sketch lines, to change and Finish.
    pub(crate) fn edit_site_boundary(&mut self, id: Id) {
        let m = &self.session.model;
        let lines = match m.elements.get(id) {
            Some(Element::Site(st)) if st.props.contains_key("path") => path_lines(&props_points(&st.props, "path"), false),
            Some(Element::Site(st)) => path_lines(&props_points(&st.props, "control"), true),
            Some(Element::BuildingPad(bp)) => solver::lines_of(&bp.boundary, &[]),
            Some(Element::PropertyLine(pl)) => path_lines(&pl.points, true),
            _ => return,
        };
        let path = matches!(m.elements.get(id), Some(Element::Site(st)) if st.props.contains_key("path"));
        self.start_sketch(Of::EditSite { id, path }, lines);
    }

    /// Finish Edit Mode on a site element's line: the element changed to it.
    pub(crate) fn finish_site_edit(&mut self, id: Id) -> Option<Value> {
        let lines = self.sketch.as_ref()?.lines.clone();
        let m = &self.session.model;
        let ring = |lines: &[SketchLine]| -> Result<Vec<[f64; 2]>, String> {
            let r = solver::solve_one(lines).map_err(|p| crate::sketch::friendly(&p, lines, m.units))?;
            if !r.holes.is_empty() {
                return Err(t("Sketch one outline with no holes in it.").into());
            }
            Ok(r.outer.iter().map(|q| [q.0, q.1]).collect())
        };
        let made = match m.elements.get(id) {
            Some(Element::Site(st)) if st.props.contains_key("path") => match solver::chain(&lines) {
                Ok((path, false)) => Ok(("site_edit", json!({"id": id, "props": {"path": path}}))),
                Ok(_) => Err(t("A path has two ends: leave it open.").into()),
                Err(e) => Err(e),
            },
            Some(Element::Site(_)) => ring(&lines).map(|r| ("site_edit", json!({"id": id, "props": {"control": r}}))),
            Some(Element::BuildingPad(_)) => ring(&lines).map(|r| ("modify", json!({"id": id, "boundary": r}))),
            Some(Element::PropertyLine(_)) => match solver::chain(&lines) {
                Ok((pts, true)) => {
                    // Counter-clockwise, as the property line holds it: the other way round, its arcs turned.
                    let ring: Vec<P2> = pts.iter().map(|q| P2(q[0], q[1])).collect();
                    let pts = if geom::area(&ring) < 0.0 {
                        let n = pts.len();
                        (0..n).map(|i| [pts[n - 1 - i][0], pts[n - 1 - i][1], -pts[(2 * n - 2 - i) % n][2]]).collect()
                    } else {
                        pts
                    };
                    Ok(("modify", json!({"id": id, "points": pts})))
                }
                Ok(_) => Err(t("A property line is closed: end it where it began.").into()),
                Err(e) => Err(e),
            },
            _ => return None,
        };
        match made {
            Ok((command, p)) => self.run(command, p).map(|_| json!({"id": id})),
            Err(e) => {
                self.status = Status { text: e, error: true };
                None
            }
        }
    }

    /// The site element selected alone, with Modify: its grips, each `(prop, index, where)`: a path's points, an area's
    /// control points, a placed thing's point.
    pub(crate) fn site_grips(&self) -> Vec<(&'static str, usize, P2)> {
        let (&[id], Tool::Modify) = (self.selection.as_slice(), &self.tool) else { return Vec::new() };
        if self.sketch.is_some() {
            return Vec::new();
        }
        let Some(Element::Site(st)) = self.session.model.elements.get(id) else { return Vec::new() };
        let mut out = Vec::new();
        for key in ["path", "control"] {
            for (i, q) in props_points(&st.props, key).into_iter().enumerate() {
                out.push((key, i, P2(q[0], q[1])));
            }
        }
        if let Some(q) = st.props.get("at").and_then(|v| shapes::path_points(&json!([v])).ok()).and_then(|v| v.first().copied()) {
            out.push(("at", 0, P2(q[0], q[1])));
        }
        out
    }

    /// The selected site element's props with grip `(key, i)` moved to `to`.
    pub(crate) fn site_grip_moved(&self, key: &str, i: usize, to: P2) -> Option<Value> {
        let &[id] = self.selection.as_slice() else { return None };
        let Some(Element::Site(st)) = self.session.model.elements.get(id) else { return None };
        if key == "at" {
            return Some(json!({"at": [to.0, to.1]}));
        }
        let mut pts = props_points(&st.props, key);
        let q = pts.get_mut(i)?;
        (q[0], q[1]) = (to.0, to.1);
        let v = if key == "control" { json!(pts.iter().map(|q| [q[0], q[1]]).collect::<Vec<_>>()) } else { json!(pts) };
        Some(json!({ key: v }))
    }

    /// A grip let go at `to`: the element's prop changed, its shape worked out again.
    pub(crate) fn site_grip_drop(&mut self, key: &str, i: usize, to: P2) {
        let &[id] = self.selection.as_slice() else { return };
        if let Some(props) = self.site_grip_moved(key, i, to) {
            self.run("site_edit", json!({"id": id, "props": props}));
        }
    }

    /// While a grip is dragged: the element as it would be, shaped with the grip at `to`.
    pub(crate) fn site_grip_ghost(&self, m: &Model, key: &str, i: usize, to: P2) -> Option<Element> {
        let &[id] = self.selection.as_slice() else { return None };
        let Some(Element::Site(st)) = m.elements.get(id) else { return None };
        let mut props = st.props.clone();
        if let Some(Value::Object(o)) = self.site_grip_moved(key, i, to) {
            props.extend(o);
        }
        let level = m.elements.id(st.level)?;
        ghost(m, level, &st.kind, &st.use_, props)
    }

    /// Properties of the site element selected alone: its kind, use, name and phase, the props its kind has, and what it
    /// measures; of a mass, its masterplan figures. Each edit is one `site_edit`.
    pub(crate) fn site_properties(&mut self, ui: &mut Ui, model: &Model, ids: &[Id]) {
        let &[id] = ids else { return };
        let units = model.units;
        let (kind, use_, uses, phase, props, figures): (String, String, Vec<&'static str>, String, Props, Vec<(String, String)>) =
            match model.elements.get(id) {
                Some(Element::Site(st)) => {
                    let uses = shapes::kind(&st.kind).map(|k| k.uses.to_vec()).unwrap_or_default();
                    (st.kind.clone(), st.use_.clone(), uses, st.phase.clone(), st.props.clone(), site_figures(model, st))
                }
                Some(Element::Mass(ms)) => {
                    let s = ms.site.clone().unwrap_or_default();
                    ("mass".into(), s.use_, masterplan::MASS_USES.to_vec(), s.phase, s.props, Vec::new())
                }
                _ => return,
            };
        let mut change: Option<Result<Value, String>> = None;
        let mass = kind == "mass";
        // One group: what it is, then the props its kind is drawn with, by the catalogue.
        let group = if mass { msg!("Masterplan") } else { msg!("Site") };
        let open = !self.collapsed.contains(group);
        if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if open {
            grid(ui, ("site-identity", id), |ui| {
                if !mass {
                    ui.label(t("Kind"));
                    ui.label(RichText::new(name(&kind)).color(pal().muted));
                    ui.end_row();
                }
                let l = ui.label(t("Use"));
                let mut options: Vec<String> = uses.iter().map(|u| name(u)).collect();
                if mass {
                    options.insert(0, String::new());
                }
                let now = if use_.is_empty() { String::new() } else { name(&use_) };
                if let Some(picked) = combo(ui, (id, "site-use"), l.id, &now, &options) {
                    let key = uses.iter().find(|u| name(u) == picked).copied().unwrap_or_default();
                    change = Some(Ok(json!({"use": key})));
                }
                ui.end_row();
                let l = ui.label(t("Phase"));
                let phases: Vec<String> = masterplan::PHASES.iter().map(|p| name(p)).collect();
                let now = name(if phase.is_empty() { "new" } else { &phase });
                if let Some(picked) = combo(ui, (id, "site-phase"), l.id, &now, &phases) {
                    let key = masterplan::PHASES.iter().find(|p| name(p) == picked).copied().unwrap_or("new");
                    change = Some(Ok(json!({"phase": key})));
                }
                ui.end_row();
            });
            let surfaces: Vec<String> =
                shapes::SURFACES.iter().find(|(k, _)| *k == kind).map(|(_, s)| s.iter().map(|x| name(x)).collect()).unwrap_or_default();
            let mut typed: Option<(String, String, String)> = None;
            let mut flag: Option<(String, bool)> = None;
            let mut picked_surface: Option<String> = None;
            grid(ui, ("site-props", id), |ui| {
                for (key, ty) in editable(&kind) {
                    let v = props.get(&key).cloned().unwrap_or(Value::Null);
                    let l = ui.label(prop_name(&key));
                    match ty.as_str() {
                        "boolean" => {
                            let mut on = v.as_bool().unwrap_or(false);
                            if ui.checkbox(&mut on, "").labelled_by(l.id).changed() {
                                flag = Some((key.clone(), on));
                            }
                        }
                        "string" if key == "surface" && !surfaces.is_empty() => {
                            let now = v.as_str().map(name).unwrap_or_default();
                            if let Some(p) = combo(ui, (id, "site-surface"), l.id, &now, &surfaces) {
                                picked_surface = Some(p);
                            }
                        }
                        _ => {
                            let hint = if ty == "length" { units.name() } else { "" };
                            if let Some(text) = self.field(ui, id, &format!("site.{key}"), shown(&v, &ty, units), l.id, hint) {
                                typed = Some((key.clone(), ty.clone(), text));
                            }
                        }
                    }
                    ui.end_row();
                }
            });
            if let Some((key, ty, text)) = typed {
                change = Some(parsed(&text, &ty, units).map(|v| json!({"props": {key: v}})));
            }
            if let Some((key, on)) = flag {
                change = Some(Ok(json!({"props": {key: on}})));
            }
            if let Some(p) = picked_surface {
                let all = shapes::SURFACES.iter().find(|(k, _)| *k == kind).map(|(_, s)| *s).unwrap_or(&[]);
                let key = all.iter().find(|s| name(s) == p).copied().unwrap_or_default();
                change = Some(Ok(json!({"props": {"surface": key}})));
            }
        }
        // What it measures, read only.
        if !figures.is_empty() {
            let group = "Dimensions";
            let open = !self.collapsed.contains(group);
            if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
                self.collapsed.insert(group);
            }
            if open {
                grid(ui, ("site-figures", id), |ui| {
                    for (label, value) in &figures {
                        ui.label(t(label));
                        ui.label(RichText::new(value).color(pal().muted));
                        ui.end_row();
                    }
                });
            }
        }
        match change {
            Some(Ok(Value::Object(mut p))) => {
                p.insert("id".into(), json!(id));
                self.run("site_edit", Value::Object(p));
            }
            Some(Err(e)) => self.status = Status { text: e, error: true },
            _ => {}
        }
    }
}

/// What a site element measures, read only, by label: its area; a car park's stalls, accessible stalls and area per stall; a
/// road's carriageway and pavements.
fn site_figures(m: &Model, st: &Site) -> Vec<(String, String)> {
    let units = m.units;
    let mut out = Vec::new();
    let area: f64 = st.outline.iter().map(|p| geom::area(&p.outer).abs() - p.holes.iter().map(|h| geom::area(h).abs()).sum::<f64>()).sum();
    if area > 0.0 {
        let (a, unit) = units.area(area);
        out.push((msg!("Area").to_string(), format!("{a:.2} {unit}")));
    }
    if let Some(r) = st.layout.as_ref().map(|l| &l.result) {
        let count = |k: &str| r.get(k).and_then(Value::as_f64).map(|n| format!("{n:.0}"));
        if let Some(n) = count("stalls") {
            out.push((msg!("Stalls").to_string(), n));
        }
        if let Some(n) = count("accessible") {
            out.push((msg!("Accessible Stalls").to_string(), n));
        }
        if let Some(n) = r.get("area_per_stall").and_then(Value::as_f64) {
            let (a, unit) = units.area(n * 1.0e6);
            out.push((msg!("Area per Stall").to_string(), format!("{a:.1} {unit}")));
        }
    }
    if st.kind == "lane" && st.use_ == "road" {
        let s = shapes::roads::section_of(&st.props);
        out.push((msg!("Carriageway").to_string(), units.length(s.carriageway())));
        out.push((msg!("Pavements").to_string(), units.length(s.pavement())));
    }
    out
}
