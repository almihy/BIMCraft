//! A drawn view as a DXF file (AutoCAD 2018, plain text): what [`crate::render::viewport`] draws of a plan, a section or an
//! elevation, written in the model's own coordinates, not pixels, so it lies on a survey or on another drawing of the same
//! project and is measured in a CAD program as it is built.
//!
//! Each line is a LINE or a LWPOLYLINE in the pen's colour and (where the view dashes it) its own linetype, at the pen's
//! weight on the paper the view stands for; each fill a solid HATCH in its colour and opacity. Every entity is on the layer
//! its element's category names, as the US National CAD Standard names them (A-WALL, A-DOOR, A-GLAZ, A-FLOR, A-ROOF,
//! C-PROP for the plot, C-ROAD, L-PAVE, L-PLNT-LAWN for a site's surfaces and so on), with `-PATT` after it for a hatch
//! pattern's own graphics, so an office sees the layers its own drawings use. Lettering is on A-ANNO-TEXT (a room's name on A-AREA-IDEN, a
//! dimension's on A-ANNO-DIMS). Lettering is
//! outlined: a drawing holds each letter as the lines the font strokes it with, not the string, so it comes out as lines.
//! Pictures (a section's picture beyond its cut, an image placed in the view) are left out, and said.
//!
//! Ink, the near black the drawing is made in, is written as colour 7 (black on a light screen, white on a dark one), as a
//! CAD drawing is; every other colour is exact.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::dxf;
use crate::family::Category;
use crate::geom::P2;
use crate::model::{Element, Id, Model};
use crate::prim::{Part, Prim, Rgb, Rule};
use crate::render::Drawing;

/// The unit a file's numbers are in (`$INSUNITS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unit {
    #[default]
    Millimetre,
    Centimetre,
    Metre,
    Inch,
    Foot,
}

impl Unit {
    /// The unit a name says: `mm`, `cm`, `m`, `in` or `ft`.
    pub fn parse(name: &str) -> Option<Unit> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "mm" | "millimetre" | "millimetres" | "millimeter" | "millimeters" => Unit::Millimetre,
            "cm" | "centimetre" | "centimetres" | "centimeter" | "centimeters" => Unit::Centimetre,
            "m" | "metre" | "metres" | "meter" | "meters" => Unit::Metre,
            "in" | "inch" | "inches" => Unit::Inch,
            "ft" | "foot" | "feet" => Unit::Foot,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Unit::Millimetre => "mm",
            Unit::Centimetre => "cm",
            Unit::Metre => "m",
            Unit::Inch => "in",
            Unit::Foot => "ft",
        }
    }

    /// Millimetres to the unit.
    fn mm(self) -> f64 {
        match self {
            Unit::Millimetre => 1.0,
            Unit::Centimetre => 10.0,
            Unit::Metre => 1000.0,
            Unit::Inch => 25.4,
            Unit::Foot => 304.8,
        }
    }

    /// `$INSUNITS`.
    fn code(self) -> i32 {
        match self {
            Unit::Inch => 1,
            Unit::Foot => 2,
            Unit::Millimetre => 4,
            Unit::Centimetre => 5,
            Unit::Metre => 6,
        }
    }

    /// How many decimals keep a thousandth of a millimetre.
    fn places(self) -> usize {
        match self {
            Unit::Millimetre => 3,
            Unit::Centimetre => 4,
            Unit::Metre => 6,
            Unit::Inch => 5,
            Unit::Foot => 6,
        }
    }
}

/// Which coordinates a plan's points are written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    /// The project's own, from its internal origin.
    #[default]
    Project,
    /// The survey's grid the project is drawn in: the project's axes (x east, y north) with the survey point's east and
    /// north added, so the drawing lies on the survey it was traced from.
    Survey,
    /// Revit's shared coordinates ([`Model::shared`]): the survey point's, turned by the angle to true north.
    Shared,
}

impl Place {
    /// The coordinates a name says: `project`, `survey` or `shared`.
    pub fn parse(name: &str) -> Option<Place> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "project" | "internal" => Place::Project,
            "survey" => Place::Survey,
            "shared" => Place::Shared,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Place::Project => "project",
            Place::Survey => "survey",
            Place::Shared => "shared",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub unit: Unit,
    pub place: Place,
    /// The names a firm gives layers instead of the ones written, by the name written: `A-WALL` to `A-WALLS-NEW`.
    pub rename: BTreeMap<String, String>,
}

/// A DXF file written, and what it holds.
pub struct Written {
    pub text: String,
    /// The layers it uses, by name.
    pub layers: Vec<String>,
    /// How many entities (lines, polylines, hatches) it holds.
    pub entities: usize,
    /// What was left out or could not be as asked, for the one who asked.
    pub notes: Vec<String>,
}

/// The widths a line may be written at, hundredths of a mm (AutoCAD's lineweights).
const WEIGHTS: [i32; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];
/// The ink every drawing is made in, and the darkest a colour may be and still be ink: written as colour 7, which a dark
/// screen draws white and a light one black.
const INK: u8 = 70;

// The handles of the file's own tables, blocks and dictionaries; what the drawing holds counts up from `FIRST`.
const H_MODEL: &str = "17";
const H_PAPER: &str = "1B";
const FIRST: u32 = 0x100;

const EOL: &str = "\r\n";

struct Out {
    s: String,
}

impl Out {
    fn pair(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = write!(self.s, "{code:>3}{EOL}{value}{EOL}");
    }
    fn point(&mut self, first: i32, p: P2, places: usize) {
        self.pair(first, num(p.0, places));
        self.pair(first + 10, num(p.1, places));
    }
}

/// `v` to `places` decimals without the zeros that end it.
fn num(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

/// What a drawing's entities are written as, collected while they are, then put between the file's tables.
struct Body<'a> {
    m: &'a Model,
    d: &'a Drawing,
    unit: Unit,
    place: Place,
    rename: &'a BTreeMap<String, String>,
    o: Out,
    next: u32,
    layers: BTreeSet<String>,
    /// Each dash pattern's linetype name, by the pattern's text.
    dashes: BTreeMap<String, (String, Vec<f64>)>,
    lo: P2,
    hi: P2,
    entities: usize,
    pictures: usize,
}

impl Body<'_> {
    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next);
        self.next += 1;
        h
    }

    /// Model point `p` px as the file has it: through the view to millimetres, to the coordinates asked for, to the unit.
    fn at(&mut self, p: (f32, f32)) -> P2 {
        let q = self.d.view.mm(p);
        let q = match self.place {
            Place::Project => q,
            Place::Survey => {
                let b = self.m.survey_point().map(|(_, b)| b.clone()).unwrap_or_default();
                P2(q.0 - b.at.0 + b.east, q.1 - b.at.1 + b.north)
            }
            Place::Shared => {
                let s = self.m.shared(q, 0.0);
                P2(s[0], s[1])
            }
        };
        let k = self.unit.mm();
        let q = P2(q.0 / k, q.1 / k);
        self.lo = P2(self.lo.0.min(q.0), self.lo.1.min(q.1));
        self.hi = P2(self.hi.0.max(q.0), self.hi.1.max(q.1));
        q
    }

    /// `pts` px as the file has them, a point to a point: those that fall on the one before are left out.
    fn ring(&mut self, pts: &[(f32, f32)], closed: bool) -> Vec<P2> {
        let places = self.unit.places();
        let same = |a: P2, b: P2| num(a.0, places) == num(b.0, places) && num(a.1, places) == num(b.1, places);
        let mut out: Vec<P2> = Vec::with_capacity(pts.len());
        for p in pts {
            let q = self.at(*p);
            if out.last().is_none_or(|l| !same(*l, q)) {
                out.push(q);
            }
        }
        if closed && out.len() > 1 && same(out[0], out[out.len() - 1]) {
            out.pop();
        }
        out
    }

    fn layer_for(&mut self, owner: Option<Id>, part: Part) -> String {
        let base = owner.and_then(|id| self.m.elements.get(id)).map(|e| layer_of(self.m, e));
        let name = match (part, base) {
            // A room's name is its identification; annotation's own lettering stays where its kind is; any other is text.
            (Part::Text, Some(b)) if b.starts_with("A-ANNO") || b == "A-AREA-IDEN" => b,
            (Part::Text, _) => "A-ANNO-TEXT".to_string(),
            (Part::Pattern | Part::LinePattern, Some(b)) => match b.strip_suffix("-OTLN") {
                Some(major) => format!("{major}-PATT"),
                None => format!("{b}-PATT"),
            },
            (Part::Pattern | Part::LinePattern, None) => "A-ANNO-PATT".to_string(),
            (_, Some(b)) => b,
            // The poché of the elements the plan cuts is filled once for them all, as one, owned by none.
            (Part::Cut, None) => "A-WALL".to_string(),
            (_, None) => "0".to_string(),
        };
        let name = self.rename.get(&name).cloned().unwrap_or(name);
        self.layers.insert(name.clone());
        name
    }

    /// The start of an entity of `kind` in `layer` in the model space: its handle, owner and layer.
    fn begin(&mut self, kind: &str, layer: &str) {
        let h = self.handle();
        self.o.pair(0, kind);
        self.o.pair(5, h);
        self.o.pair(330, H_MODEL);
        self.o.pair(100, "AcDbEntity");
        self.o.pair(8, layer);
    }

    /// The colour of an entity: ink by colour 7, any other exactly (its nearest index, for a reader that knows no
    /// true colour, and the colour itself).
    fn colour(&mut self, c: Rgb) {
        if c.0.max(c.1).max(c.2) <= INK {
            self.o.pair(62, 7);
            return;
        }
        let (mut best, mut near) = (7u8, i64::MAX);
        for i in 1..=255u8 {
            if let Some(a) = dxf::aci(i) {
                let dist = |x: u8, y: u8| (i64::from(x) - i64::from(y)).pow(2);
                let d = dist(a.0, c.0) + dist(a.1, c.1) + dist(a.2, c.2);
                if d < near {
                    (best, near) = (i, d);
                }
            }
        }
        self.o.pair(62, best);
        self.o.pair(420, (i64::from(c.0) << 16) | (i64::from(c.1) << 8) | i64::from(c.2));
    }

    fn alpha(&mut self, alpha: u8) {
        if alpha < 255 {
            self.o.pair(440, 0x0200_0000 + i64::from(alpha));
        }
    }

    /// The linetype a dash pattern (px on, off, ...) is, made when it is first wanted; none for no dashes.
    fn linetype(&mut self, dash: Option<&Vec<f32>>) -> Option<String> {
        let dash = dash?;
        let places = self.unit.places();
        let mut v: Vec<f64> = dash.iter().map(|x| f64::from(*x) / self.d.view.scale / self.unit.mm()).collect();
        if v.is_empty() || v.iter().any(|x| !x.is_finite() || *x <= 0.0) {
            return None;
        }
        if v.len() % 2 == 1 {
            let twice = v.clone();
            v.extend(twice);
        }
        let key = v.iter().map(|x| num(*x, places)).collect::<Vec<_>>().join(" ");
        let n = self.dashes.len() + 1;
        Some(self.dashes.entry(key).or_insert_with(|| (format!("BC-DASH-{n}"), v)).0.clone())
    }

    fn line(&mut self, pts: &[(f32, f32)], width: f32, color: Rgb, dash: Option<&Vec<f32>>, closed: bool, alpha: u8, i: usize) {
        let ring = self.ring(pts, closed);
        if ring.len() < 2 {
            return;
        }
        let (owner, part) = (self.d.owners.get(i).copied().flatten(), self.d.parts.get(i).copied().unwrap_or_default());
        let layer = self.layer_for(owner, part);
        let places = self.unit.places();
        let closed = closed && ring.len() > 2;
        let kind = if ring.len() == 2 { "LINE" } else { "LWPOLYLINE" };
        self.begin(kind, &layer);
        let lt = self.linetype(dash);
        if let Some(name) = &lt {
            self.o.pair(6, name);
        }
        self.colour(color);
        // A mm of paper is DENSITY px of a drawing the size of its paper; the pens' widths are in the view's own.
        let mm = f64::from(width) / (crate::prim::DENSITY * f64::from(self.d.pen_zoom()));
        if part != Part::Text && mm.is_finite() && mm >= 0.0 {
            let hundredths = mm * 100.0;
            let w =
                WEIGHTS.iter().copied().min_by(|a, b| (f64::from(*a) - hundredths).abs().total_cmp(&(f64::from(*b) - hundredths).abs()));
            if let Some(w) = w {
                self.o.pair(370, w);
            }
        }
        self.alpha(alpha);
        if ring.len() == 2 {
            self.o.pair(100, "AcDbLine");
            self.o.point(10, ring[0], places);
            self.o.pair(30, 0);
            self.o.point(11, ring[1], places);
            self.o.pair(31, 0);
        } else {
            self.o.pair(100, "AcDbPolyline");
            self.o.pair(90, ring.len());
            // 1 closed, 128 the pattern runs on through the vertices.
            self.o.pair(70, i32::from(closed) + if lt.is_some() { 128 } else { 0 });
            for p in &ring {
                self.o.point(10, *p, places);
            }
        }
        self.entities += 1;
    }

    /// One HATCH of `rings` (each a closed boundary), solid, by the odd-even rule the file's hatches fill by.
    fn hatch(&mut self, rings: &[Vec<P2>], color: Rgb, alpha: u8, i: usize) {
        let rings: Vec<&Vec<P2>> = rings.iter().filter(|r| r.len() >= 3).collect();
        if rings.is_empty() {
            return;
        }
        let (owner, part) = (self.d.owners.get(i).copied().flatten(), self.d.parts.get(i).copied().unwrap_or_default());
        let layer = self.layer_for(owner, part);
        let places = self.unit.places();
        self.begin("HATCH", &layer);
        self.colour(color);
        self.alpha(alpha);
        self.o.pair(100, "AcDbHatch");
        self.o.pair(10, 0);
        self.o.pair(20, 0);
        self.o.pair(30, 0);
        self.o.pair(210, 0);
        self.o.pair(220, 0);
        self.o.pair(230, 1);
        self.o.pair(2, "SOLID");
        self.o.pair(70, 1);
        self.o.pair(71, 0);
        self.o.pair(91, rings.len());
        for (n, r) in rings.iter().enumerate() {
            // A polyline boundary (2), the first the external one (1).
            self.o.pair(92, if n == 0 { 3 } else { 2 });
            self.o.pair(72, 0);
            self.o.pair(73, 1);
            self.o.pair(93, r.len());
            for p in r.iter() {
                self.o.point(10, *p, places);
            }
            self.o.pair(97, 0);
        }
        self.o.pair(75, 0);
        self.o.pair(76, 1);
        self.o.pair(98, 0);
        self.entities += 1;
    }

    fn fill(&mut self, rings: &[Vec<(f32, f32)>], color: Rgb, rule: Rule, alpha: u8, i: usize) {
        let signs: Vec<f64> = rings
            .iter()
            .map(|r| {
                let area: f64 = r
                    .iter()
                    .zip(r.iter().cycle().skip(1))
                    .map(|(a, b)| f64::from(a.0) * f64::from(b.1) - f64::from(b.0) * f64::from(a.1))
                    .sum();
                area
            })
            .collect();
        let alike = signs.iter().all(|a| *a >= 0.0) || signs.iter().all(|a| *a <= 0.0);
        let rings: Vec<Vec<P2>> = rings.iter().map(|r| self.ring(r, true)).collect();
        // Rings that wind alike fill as one where they overlap or meet, as separate hatches do; a ring turned against the
        // others, or an even-odd fill, is a hatch of them all, which fills by the odd-even rule.
        if rule == Rule::NonZero && alike {
            for r in &rings {
                self.hatch(std::slice::from_ref(r), color, alpha, i);
            }
        } else {
            self.hatch(&rings, color, alpha, i);
        }
    }
}

/// The layer an element is drawn on, by its category.
fn layer_of(m: &Model, e: &Element) -> String {
    let name = match e {
        Element::Wall(_) => "A-WALL",
        Element::Door(_) => "A-DOOR",
        Element::Window(_) => "A-GLAZ",
        // A curtain wall's glazing, and its mullions (the National CAD Standard's A-GLAZ-CWMG).
        Element::CurtainPanel(_) | Element::CurtainGrid(_) => "A-GLAZ",
        Element::Mullion(_) => "A-GLAZ-CWMG",
        Element::Room(_) => "A-AREA-IDEN",
        Element::RoomSeparator(_) => "A-AREA-SEPR",
        Element::Floor(_) => "A-FLOR",
        Element::Ceiling(_) => "A-CLNG",
        Element::Roof(_) => "A-ROOF",
        Element::Stair(_) => "A-FLOR-STRS",
        Element::Railing(_) => "A-FLOR-HRAL",
        Element::Ramp(_) => "A-FLOR-RAMP",
        Element::Shaft(_) => "A-FLOR-SHFT",
        Element::Column(_) => "A-COLS",
        Element::StructuralColumn(_) => "S-COLS",
        Element::Footing(_) | Element::StripFooting(_) => "S-FNDN",
        Element::Beam(_) => "S-BEAM",
        Element::Rebar(_) => "S-CONC-RBAR",
        Element::Mass(_) => "A-BLDG-OTLN",
        Element::Grid(_) => "A-GRID",
        Element::Level(_) => "A-ANNO-LEVL",
        Element::Dimension(_) | Element::Dimension3D(_) => "A-ANNO-DIMS",
        Element::Tag(_) => "A-ANNO-TAGS",
        Element::Text(_) => "A-ANNO-NOTE",
        Element::Spot(_) => "A-ANNO-SPOT",
        Element::Section(_) | Element::Elevation(_) => "A-ANNO-SYMB",
        Element::DetailLine(_) | Element::DetailGroup(_) | Element::RepeatingDetail(_) => "A-DETL",
        Element::FilledRegion(_) => "A-DETL-FILL",
        Element::Pipe(_) => "P-PIPE",
        Element::Duct(_) => "M-DUCT",
        Element::Conduit(_) => "E-CNDT",
        Element::CableTray(_) => "E-CABL-TRAY",
        Element::Wire(_) | Element::Cable(_) => "E-CABL",
        Element::DownConductor(_) | Element::GroundConductor(_) => "E-LTNG",
        Element::PropertyLine(_) | Element::Site(_) => "C-PROP",
        Element::Toposolid(_) => "C-TOPO",
        Element::SubDivision(s) => return surface_layer(&s.role, &s.type_name),
        Element::Component(c) => return component_layer(m.families.get(&c.family).map(|f| f.category)),
        Element::CadImport(_) => "X-CAD",
        Element::Image(_) => "A-IMAG",
        other => return format!("A-{}", other.category().to_ascii_uppercase()),
    };
    name.to_string()
}

/// The layer a site's surface is on: by the role the masterplan gave it, else by what it is made of.
fn surface_layer(role: &str, surface: &str) -> String {
    let (kind, use_) = role.split_once('/').unwrap_or((role, ""));
    let by_use = |u: &str| u.to_ascii_lowercase();
    let name = match kind {
        "lane" => match by_use(use_).as_str() {
            "footpath" | "promenade" | "pavement" => "L-WALK",
            "median" => "L-PLNT-LAWN",
            _ => "C-ROAD",
        },
        "car_park" => "C-PRKG-AISL",
        "paving" | "terrace" | "driveway" => "L-PAVE",
        "water" => match by_use(use_).as_str() {
            "stream" => "L-WATR-STRM",
            "pool" => "L-WATR-POOL",
            _ => "L-WATR",
        },
        "landscape" => match by_use(use_).as_str() {
            "meadow" => "L-PLNT-MEAD",
            "lawn" | "grass" => "L-PLNT-LAWN",
            _ => "L-PLNT-BEDS",
        },
        "pitch" => "L-SITE-SPRT",
        "playground" | "play_area" => "L-SITE-PLAY",
        _ => match surface.to_ascii_lowercase().as_str() {
            "asphalt" | "drive aisle" => "C-ROAD",
            "paving" | "block paving" | "gravel" | "concrete" | "terrace" | "sand" => "L-PAVE",
            "grass" => "L-PLNT-LAWN",
            "meadow" => "L-PLNT-MEAD",
            "planting" => "L-PLNT-BEDS",
            "water" => "L-WATR",
            "sports surface" => "L-SITE-SPRT",
            "play surface" => "L-SITE-PLAY",
            _ => "C-TOPO-SUBD",
        },
    };
    name.to_string()
}

/// The layer a family's components are on, by its category.
fn component_layer(category: Option<Category>) -> String {
    use Category::*;
    match category {
        Some(Furniture) => "A-FURN",
        Some(Casework) => "A-FLOR-CASE",
        Some(Plumbing) => "P-FIXT",
        Some(Drain) => "P-DRAN",
        Some(PipeFitting | PipeAccessory) => "P-PIPE-FITG",
        Some(Equipment) => "E-EQPM",
        Some(Outlet | Switch) => "E-POWR",
        Some(Lighting) => "E-LITE",
        Some(AirTerminal | Earthing | InspectionBox) => "E-LTNG",
        Some(DataOutlet | VoiceOutlet | PatchPanel | Rack) => "T-DATA",
        Some(Door) => "A-DOOR",
        Some(Window) => "A-GLAZ",
        Some(Planting) => "L-PLNT-TREE",
        Some(SiteComponent) => "L-SITE-FURN",
        _ => "A-GENM",
    }
    .to_string()
}

/// `d`, a drawing of view of `m` (the model the drawing was made from, links brought in: [`crate::link::seen`]), as a DXF file.
pub fn write(m: &Model, d: &Drawing, o: &Options) -> Written {
    let mut b = Body {
        m,
        d,
        unit: o.unit,
        place: o.place,
        rename: &o.rename,
        o: Out { s: String::new() },
        next: FIRST,
        layers: BTreeSet::new(),
        dashes: BTreeMap::new(),
        lo: P2(f64::MAX, f64::MAX),
        hi: P2(f64::MIN, f64::MIN),
        entities: 0,
        pictures: 0,
    };
    for (i, prim) in d.prims.iter().enumerate() {
        match prim {
            // What is drawn in no ink is there for the cursor (a curtain wall's extents), not for a drawing.
            Prim::Line { alpha: 0, .. } | Prim::Fill { alpha: 0, .. } => {}
            Prim::Line { pts, width, color, dash, closed, alpha } => b.line(pts, *width, *color, dash.as_ref(), *closed, *alpha, i),
            Prim::Fill { rings, color, rule, alpha } => b.fill(rings, *color, *rule, *alpha, i),
            Prim::Image { .. } => b.pictures += 1,
        }
    }
    let mut notes = d.notes.clone();
    if b.pictures > 0 {
        notes.push(format!(
            "{} picture{} in the view {} not in the DXF.",
            b.pictures,
            if b.pictures == 1 { "" } else { "s" },
            if b.pictures == 1 { "is" } else { "are" }
        ));
    }
    let (lo, hi) = if b.entities == 0 { (P2(0.0, 0.0), P2(0.0, 0.0)) } else { (b.lo, b.hi) };
    let layers: Vec<String> = b.layers.iter().cloned().collect();
    let entities = b.entities;
    let text = file(o, &mut b, lo, hi);
    Written { text, layers, entities, notes }
}

/// The whole file: its header, tables, blocks, the entities `b` collected and its objects.
fn file(o: &Options, b: &mut Body, lo: P2, hi: P2) -> String {
    let places = o.unit.places();
    let mut w = Out { s: String::new() };
    let section = |w: &mut Out, name: &str| {
        w.pair(0, "SECTION");
        w.pair(2, name);
    };
    let end = |w: &mut Out| w.pair(0, "ENDSEC");
    // The handles of the linetypes and layers follow the entities'.
    let mut next = b.next;
    let mut take = || {
        next += 1;
        format!("{:X}", next - 1)
    };
    let dashes: Vec<(String, Vec<f64>, String)> = b.dashes.values().map(|(n, v)| (n.clone(), v.clone(), take())).collect();
    let mut names: Vec<String> = b.layers.iter().cloned().collect();
    if !names.iter().any(|n| n == "0") {
        names.insert(0, "0".into());
    }
    let layers: Vec<(String, String)> = names.into_iter().map(|n| (n, take())).collect();
    let seed = next + 1;

    section(&mut w, "HEADER");
    w.pair(9, "$ACADVER");
    w.pair(1, "AC1032");
    w.pair(9, "$DWGCODEPAGE");
    w.pair(3, "ANSI_1252");
    w.pair(9, "$INSBASE");
    w.pair(10, 0);
    w.pair(20, 0);
    w.pair(30, 0);
    w.pair(9, "$EXTMIN");
    w.point(10, lo, places);
    w.pair(30, 0);
    w.pair(9, "$EXTMAX");
    w.point(10, hi, places);
    w.pair(30, 0);
    w.pair(9, "$LTSCALE");
    w.pair(40, 1);
    w.pair(9, "$CLAYER");
    w.pair(8, "0");
    w.pair(9, "$CELTYPE");
    w.pair(6, "ByLayer");
    w.pair(9, "$CECOLOR");
    w.pair(62, 256);
    w.pair(9, "$LUNITS");
    w.pair(70, 2);
    w.pair(9, "$LUPREC");
    w.pair(70, 4);
    w.pair(9, "$INSUNITS");
    w.pair(70, o.unit.code());
    w.pair(9, "$MEASUREMENT");
    w.pair(70, if matches!(o.unit, Unit::Inch | Unit::Foot) { 0 } else { 1 });
    w.pair(9, "$LWDISPLAY");
    w.pair(290, 1);
    w.pair(9, "$HANDSEED");
    w.pair(5, format!("{seed:X}"));
    end(&mut w);

    section(&mut w, "TABLES");
    // The view the file opens on: its extents.
    let (cx, cy) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let tall = ((hi.1 - lo.1).max((hi.0 - lo.0) / 1.5) * 1.1).max(1.0);
    table(&mut w, "VPORT", "8", 1, |w| {
        w.pair(0, "VPORT");
        w.pair(5, "23");
        w.pair(330, "8");
        w.pair(100, "AcDbSymbolTableRecord");
        w.pair(100, "AcDbViewportTableRecord");
        w.pair(2, "*Active");
        w.pair(70, 0);
        w.pair(10, 0);
        w.pair(20, 0);
        w.pair(11, 1);
        w.pair(21, 1);
        w.pair(12, num(cx, places));
        w.pair(22, num(cy, places));
        w.pair(13, 0);
        w.pair(23, 0);
        w.pair(14, 0.5);
        w.pair(24, 0.5);
        w.pair(15, 0.5);
        w.pair(25, 0.5);
        w.pair(16, 0);
        w.pair(26, 0);
        w.pair(36, 1);
        w.pair(17, 0);
        w.pair(27, 0);
        w.pair(37, 0);
        w.pair(40, num(tall, places));
        w.pair(41, 1.5);
        w.pair(42, 50);
        w.pair(43, 0);
        w.pair(44, 0);
        w.pair(50, 0);
        w.pair(51, 0);
        w.pair(71, 0);
        w.pair(72, 1000);
        w.pair(73, 1);
        w.pair(74, 3);
        w.pair(75, 0);
        w.pair(76, 0);
        w.pair(77, 0);
        w.pair(78, 0);
        w.pair(281, 0);
        w.pair(65, 0);
        w.pair(146, 0);
    });
    table(&mut w, "LTYPE", "2", 3 + dashes.len(), |w| {
        for (n, name) in ["ByBlock", "ByLayer", "Continuous"].iter().enumerate() {
            w.pair(0, "LTYPE");
            w.pair(5, format!("{:X}", 0x24 + n));
            w.pair(330, "2");
            w.pair(100, "AcDbSymbolTableRecord");
            w.pair(100, "AcDbLinetypeTableRecord");
            w.pair(2, name);
            w.pair(70, 0);
            w.pair(3, if *name == "Continuous" { "Solid line" } else { "" });
            w.pair(72, 65);
            w.pair(73, 0);
            w.pair(40, 0);
        }
        for (name, pattern, handle) in &dashes {
            w.pair(0, "LTYPE");
            w.pair(5, handle);
            w.pair(330, "2");
            w.pair(100, "AcDbSymbolTableRecord");
            w.pair(100, "AcDbLinetypeTableRecord");
            w.pair(2, name);
            w.pair(70, 0);
            w.pair(3, "Dashed");
            w.pair(72, 65);
            w.pair(73, pattern.len());
            w.pair(40, num(pattern.iter().sum(), places + 2));
            for (k, x) in pattern.iter().enumerate() {
                // On, then off (negative), again.
                w.pair(49, num(if k % 2 == 0 { *x } else { -*x }, places + 2));
                w.pair(74, 0);
            }
        }
    });
    table(&mut w, "LAYER", "1", layers.len(), |w| {
        for (name, handle) in &layers {
            w.pair(0, "LAYER");
            w.pair(5, handle);
            w.pair(330, "1");
            w.pair(100, "AcDbSymbolTableRecord");
            w.pair(100, "AcDbLayerTableRecord");
            w.pair(2, name);
            w.pair(70, 0);
            w.pair(62, 7);
            w.pair(6, "Continuous");
            w.pair(370, -3);
        }
    });
    table(&mut w, "STYLE", "5", 1, |w| {
        w.pair(0, "STYLE");
        w.pair(5, "29");
        w.pair(330, "5");
        w.pair(100, "AcDbSymbolTableRecord");
        w.pair(100, "AcDbTextStyleTableRecord");
        w.pair(2, "Standard");
        w.pair(70, 0);
        w.pair(40, 0);
        w.pair(41, 1);
        w.pair(50, 0);
        w.pair(71, 0);
        w.pair(42, 2.5);
        w.pair(3, "txt");
        w.pair(4, "");
    });
    table(&mut w, "VIEW", "7", 0, |_| {});
    table(&mut w, "UCS", "6", 0, |_| {});
    table(&mut w, "APPID", "3", 1, |w| {
        w.pair(0, "APPID");
        w.pair(5, "2A");
        w.pair(330, "3");
        w.pair(100, "AcDbSymbolTableRecord");
        w.pair(100, "AcDbRegAppTableRecord");
        w.pair(2, "ACAD");
        w.pair(70, 0);
    });
    table(&mut w, "BLOCK_RECORD", "9", 2, |w| {
        for (h, layout, name) in [(H_MODEL, "1A", "*Model_Space"), (H_PAPER, "1E", "*Paper_Space")] {
            w.pair(0, "BLOCK_RECORD");
            w.pair(5, h);
            w.pair(330, "9");
            w.pair(100, "AcDbSymbolTableRecord");
            w.pair(100, "AcDbBlockTableRecord");
            w.pair(2, name);
            w.pair(340, layout);
            w.pair(70, 0);
            w.pair(280, 1);
            w.pair(281, 0);
        }
    });
    end(&mut w);

    section(&mut w, "BLOCKS");
    for (record, begin, finish, name) in [(H_MODEL, "18", "19", "*Model_Space"), (H_PAPER, "1C", "1D", "*Paper_Space")] {
        w.pair(0, "BLOCK");
        w.pair(5, begin);
        w.pair(330, record);
        w.pair(100, "AcDbEntity");
        w.pair(8, "0");
        w.pair(100, "AcDbBlockBegin");
        w.pair(2, name);
        w.pair(70, 0);
        w.pair(10, 0);
        w.pair(20, 0);
        w.pair(30, 0);
        w.pair(3, name);
        w.pair(1, "");
        w.pair(0, "ENDBLK");
        w.pair(5, finish);
        w.pair(330, record);
        w.pair(100, "AcDbEntity");
        w.pair(8, "0");
        w.pair(100, "AcDbBlockEnd");
    }
    end(&mut w);

    section(&mut w, "ENTITIES");
    w.s.push_str(&b.o.s);
    end(&mut w);

    section(&mut w, "OBJECTS");
    dictionary(&mut w, "A", "0", &[("ACAD_GROUP", "C"), ("ACAD_LAYOUT", "D")]);
    dictionary(&mut w, "C", "A", &[]);
    dictionary(&mut w, "D", "A", &[("Model", "1A"), ("Layout1", "1E")]);
    for (h, name, tab, record, flags) in [("1A", "Model", 1, H_MODEL, 1024), ("1E", "Layout1", 1, H_PAPER, 0)] {
        w.pair(0, "LAYOUT");
        w.pair(5, h);
        w.pair(330, "D");
        w.pair(100, "AcDbPlotSettings");
        w.pair(1, "");
        w.pair(4, "A3");
        w.pair(6, "");
        for (code, v) in
            [(40, 7.5), (41, 20.0), (42, 7.5), (43, 20.0), (44, 420.0), (45, 297.0), (46, 0.0), (47, 0.0), (48, 0.0), (49, 0.0)]
        {
            w.pair(code, v);
        }
        for (code, v) in [(140, 0.0), (141, 0.0), (142, 1.0), (143, 1.0)] {
            w.pair(code, v);
        }
        w.pair(70, flags);
        w.pair(72, 1);
        w.pair(73, 0);
        w.pair(74, 5);
        w.pair(7, "");
        w.pair(75, 16);
        w.pair(76, 0);
        w.pair(77, 2);
        w.pair(78, 300);
        w.pair(147, 1);
        w.pair(148, 0);
        w.pair(149, 0);
        w.pair(100, "AcDbLayout");
        w.pair(1, name);
        w.pair(70, 1);
        w.pair(71, tab);
        for (code, v) in [(10, 0.0), (20, 0.0), (11, 420.0), (21, 297.0), (12, 0.0), (22, 0.0), (32, 0.0)] {
            w.pair(code, v);
        }
        for (code, v) in [(14, 1e20), (24, 1e20), (34, 1e20), (15, -1e20), (25, -1e20), (35, -1e20)] {
            w.pair(code, format!("{v:e}").replace("e20", "e+20"));
        }
        w.pair(146, 0);
        for (code, v) in [(13, 0.0), (23, 0.0), (33, 0.0), (16, 1.0), (26, 0.0), (36, 0.0), (17, 0.0), (27, 1.0), (37, 0.0)] {
            w.pair(code, v);
        }
        w.pair(76, 1);
        w.pair(330, record);
    }
    end(&mut w);
    w.pair(0, "EOF");
    w.s
}

/// A symbol table `name` of handle `handle`, `max` entries at most, its records written by `records`.
fn table(w: &mut Out, name: &str, handle: &str, max: usize, records: impl FnOnce(&mut Out)) {
    w.pair(0, "TABLE");
    w.pair(2, name);
    w.pair(5, handle);
    w.pair(330, "0");
    w.pair(100, "AcDbSymbolTable");
    w.pair(70, max);
    records(w);
    w.pair(0, "ENDTAB");
}

/// A dictionary of handle `handle`, owned by `owner`, holding `entries` (name, handle).
fn dictionary(w: &mut Out, handle: &str, owner: &str, entries: &[(&str, &str)]) {
    w.pair(0, "DICTIONARY");
    w.pair(5, handle);
    w.pair(330, owner);
    w.pair(100, "AcDbDictionary");
    w.pair(281, 1);
    for (name, h) in entries {
        w.pair(3, name);
        w.pair(350, h);
    }
}
