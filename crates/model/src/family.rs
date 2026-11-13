//! Families: BIMCraft's own parametric components (never Autodesk's `.rfa`). A family has
//! parameters, types that set its type parameters, and forms, each a profile swept across a
//! sketch plane and sized by the parameters; a component places one of its types. Formulas read
//! as Revit's do: `Width / 2`, `if(Depth > 600, 4, 2)`.

use bimcraft_i18n::{msg, ui_msg};

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::csg;
use crate::font;
use crate::geom::P2;
use crate::geom::v3::{P3, add, cross, mul};
use crate::model::{MAX_COORD, MAX_TEXT};

mod bounds;
pub mod check;
pub mod compare;
mod leaves;
mod parametric;
pub mod revit;
mod shapes;
mod validate;

pub use parametric::{
    Axis, Catalog, CatalogColumn, Dimension, DimensionKind, Editable, MAX_DIMENSION_REFERENCES, MAX_DIMENSIONS, MAX_NESTED, MAX_NESTING,
    MAX_REFERENCES, Nested, Reference, TypeCheck, ref_key,
};

/// What a family is: how IFC names its components; a door or a window is hosted in a wall, which
/// its `Width` and `Height` open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    #[default]
    Furniture,
    Casework,
    /// A sanitary fixture: a basin, a WC, a sink, a shower.
    Plumbing,
    /// A floor drain.
    Drain,
    /// An elbow, a tee: what joins pipes, a component with a connector at each end.
    PipeFitting,
    /// A valve, a cleanout.
    PipeAccessory,
    /// A distribution board.
    Equipment,
    /// A power outlet (tomada).
    Outlet,
    /// A light switch (interruptor).
    Switch,
    /// A lightning air terminal (captor).
    AirTerminal,
    /// An earthing electrode (haste de aterramento).
    Earthing,
    /// An earthing inspection box (caixa de inspeção).
    InspectionBox,
    Lighting,
    Generic,
    /// A data outlet (ponto de dados).
    DataOutlet,
    /// A voice outlet (ponto de voz).
    VoiceOutlet,
    /// A patch panel.
    PatchPanel,
    /// A rack.
    Rack,
    Door,
    Window,
}

impl Category {
    /// The categories a component may be of (a door's or a window's family is placed in a wall), in the order an import reads
    /// them.
    pub const PLACED: [Category; 17] = [
        Category::Furniture,
        Category::Plumbing,
        Category::Drain,
        Category::PipeFitting,
        Category::PipeAccessory,
        Category::Equipment,
        Category::Outlet,
        Category::Switch,
        Category::AirTerminal,
        Category::Earthing,
        Category::InspectionBox,
        Category::Lighting,
        Category::Generic,
        Category::DataOutlet,
        Category::VoiceOutlet,
        Category::PatchPanel,
        Category::Rack,
    ];

    /// How IFC writes a component of it: the product's class, its predefined type, and the object type a proxy carries where
    /// the class has no home for it (IFC4 has no lightning protection).
    pub fn ifc(self) -> (&'static str, &'static str, Option<&'static str>) {
        const OPEN: &str = ".NOTDEFINED.";
        match self {
            Category::Furniture | Category::Casework => ("IFCFURNITURE", OPEN, None),
            Category::Plumbing => ("IFCSANITARYTERMINAL", OPEN, None),
            Category::Drain => ("IFCWASTETERMINAL", ".FLOORTRAP.", None),
            Category::PipeFitting => ("IFCPIPEFITTING", OPEN, None),
            Category::PipeAccessory => ("IFCVALVE", OPEN, None),
            Category::Equipment => ("IFCELECTRICDISTRIBUTIONBOARD", ".DISTRIBUTIONBOARD.", None),
            Category::Outlet => ("IFCOUTLET", ".POWEROUTLET.", None),
            Category::Switch => ("IFCSWITCHINGDEVICE", ".TOGGLESWITCH.", None),
            // IFC4 and IFC4X3 have no class for lightning protection: a proxy of an object type a reader can filter on.
            Category::DataOutlet => ("IFCOUTLET", ".DATAOUTLET.", None),
            Category::VoiceOutlet => ("IFCOUTLET", ".TELEPHONEOUTLET.", None),
            Category::PatchPanel => ("IFCCOMMUNICATIONSAPPLIANCE", ".NETWORKAPPLIANCE.", None),
            Category::Rack => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some(msg!("Rack"))),
            Category::AirTerminal => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some("LightningTerminal")),
            Category::Earthing => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some("EarthingRod")),
            Category::InspectionBox => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some("InspectionBox")),
            Category::Lighting => ("IFCLIGHTFIXTURE", OPEN, None),
            Category::Generic | Category::Door | Category::Window => ("IFCBUILDINGELEMENTPROXY", OPEN, None),
        }
    }

    /// The kinds a family of it may say it is, each with the predefined type IFC writes a component of that kind with, the
    /// category's own where it has none. A kind says which fixture or fitting it is, by the names the sizing commands count
    /// fixtures by (`drain_size`'s and `water_size`'s `wc`, `lav`…), so a WC reaches IFC as a toilet pan, not as a sanitary
    /// terminal of no type. Where two kinds share a predefined type, reading it back gives the first.
    pub fn kinds(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Category::Plumbing => &[
                ("wc", ".TOILETPAN."),
                ("lav", ".WASHHANDBASIN."),
                ("sink", ".SINK."),
                ("laundry", ".SINK."),
                ("shower", ".SHOWER."),
                ("bath", ".BATH."),
                ("bidet", ".BIDET."),
                ("urinal", ".URINAL."),
            ],
            Category::Drain => &[("floor_drain", ".FLOORTRAP."), ("floor_waste", ".FLOORWASTE."), ("roof_drain", ".ROOFDRAIN.")],
            Category::PipeFitting => &[
                ("bend", ".BEND."),
                ("tee", ".JUNCTION."),
                ("cross", ".JUNCTION."),
                ("transition", ".TRANSITION."),
                ("coupling", ".CONNECTOR."),
            ],
            _ => &[],
        }
    }

    /// The kind of it an IFC product of predefined type `predefined` (as the file spells it, without the dots) is, if one is.
    pub fn kind_of(self, predefined: Option<&str>) -> Option<&'static str> {
        let p = predefined?;
        self.kinds().iter().find(|(_, t)| t.trim_matches('.') == p).map(|(k, _)| *k)
    }

    /// Whether a device of it draws or switches a load, so its family should hold a connector that says how much.
    pub fn electrical_device(self) -> bool {
        matches!(self, Category::Lighting | Category::Outlet | Category::Switch)
    }

    /// The classes other tools write a product of it as, besides the one BIMCraft does.
    fn classes(self) -> &'static [&'static str] {
        match self {
            Category::Furniture => &["IFCFURNISHINGELEMENT", "IFCSYSTEMFURNITUREELEMENT"],
            _ => &[],
        }
    }

    /// Every IFC class a component may be read from.
    pub fn read_classes() -> Vec<&'static str> {
        Category::PLACED.into_iter().flat_map(|c| std::iter::once(c.ifc().0).chain(c.classes().iter().copied())).collect()
    }

    /// The category an IFC product of `class` is, from its predefined type `kind` (as the file spells it, without the dots) and
    /// its object type: the one that names the object type, else the one whose predefined type it is, else the class's first.
    pub fn from_ifc(class: &str, kind: Option<&str>, object_type: Option<&str>) -> Option<Category> {
        Category::PLACED
            .into_iter()
            .filter(|c| {
                let (own, _, object) = c.ifc();
                (own == class || c.classes().contains(&class)) && (object.is_none() || object == object_type)
            })
            .fold(None, |best: Option<(Category, u8)>, c| {
                let (_, predefined, object) = c.ifc();
                let score = 2 * u8::from(object.is_some()) + u8::from(Some(predefined.trim_matches('.')) == kind);
                // Strictly better, so a tie goes to the first.
                if best.is_none_or(|(_, s)| score > s) { Some((c, score)) } else { best }
            })
            .map(|(c, _)| c)
    }

    /// Whether a family of it stands in a wall: a door's or a window's, drawn in the opening's own axes (x along
    /// the wall from the opening's middle, y across it toward the wall's left, z up from its sill).
    pub fn hosted(self) -> bool {
        matches!(self, Category::Door | Category::Window)
    }
}

/// What a hosted family reads of its wall: the thickness, mm, in formulas.
pub const WALL: &str = "Wall Thickness";
/// A wall as thick as this is assumed where a hosted family is built without one: 200 mm.
pub const DEFAULT_WALL: f64 = 200.0;
/// The parameters that open a hosted family's wall: its width and its height, and the height of its sill.
pub const WIDTH: &str = msg!("Width");
pub const HEIGHT: &str = msg!("Height");
pub const SILL: &str = ui_msg!("Sill Height");

/// What a parameter holds. `text` is a string a schedule shows and no formula reads (Revit's Text parameter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Length,
    Number,
    Angle,
    YesNo,
    Material,
    Text,
}

/// A parameter: a type's (each type its own value) or, `instance`, each component's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub instance: bool,
    /// Where no type or component sets it: a number (yes/no, true or false), a material's name, or a text's words.
    #[serde(default)]
    pub value: Value,
    /// Computed from the others, as Revit's formulas are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// The unit a `number` is shown in (`m²`, `W`, `%`): said beside it, never converted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Revit's parameter group (`Dimensions`, `Construction`, `Identity Data`): Properties and Type Properties list it under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

fn yes() -> bool {
    true
}

fn is_yes(b: &bool) -> bool {
    *b
}

/// Revit's Visibility Settings of a family's form (Family Element Visibility Settings: Plan/RCP, Front/Back, Left/Right, When
/// cut in Plan/RCP, Coarse/Medium/Fine): the detail levels a view draws it at, and whether a plan (or a ceiling plan), a view
/// along the family's y (its front or back) and a view along its x (its left or right) draw it. Each is true unless it says
/// otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Show {
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub coarse: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub medium: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub fine: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub plan: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub front: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub side: bool,
    /// Revit's Top/Bottom: a plan draws it seen from above, wholly below its cut plane.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub plan_top: bool,
    /// Revit's Plan/RCP when cut: a plan draws it where its cut plane passes through it.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub plan_cut: bool,
}

impl Default for Show {
    fn default() -> Show {
        Show { coarse: true, medium: true, fine: true, plan: true, front: true, side: true, plan_top: true, plan_cut: true }
    }
}

impl Show {
    /// Revit's default: every view, every detail level.
    pub const ALL: Show =
        Show { coarse: true, medium: true, fine: true, plan: true, front: true, side: true, plan_top: true, plan_cut: true };

    /// What a view (a plan, 3D, a section, an elevation, IFC) draws of a form whose settings are `show`: its settings, or
    /// every one, as Revit's default (a door's or a window's form that says nothing is drawn in a Coarse plan too).
    pub fn in_view(show: Option<Show>) -> Show {
        show.unwrap_or(Show::ALL)
    }

    /// What both say draws: a nested part's settings over those of a form of the family it is of.
    pub fn and(self, o: Show) -> Show {
        Show {
            coarse: self.coarse && o.coarse,
            medium: self.medium && o.medium,
            fine: self.fine && o.fine,
            plan: self.plan && o.plan,
            front: self.front && o.front,
            side: self.side && o.side,
            plan_top: self.plan_top && o.plan_top,
            plan_cut: self.plan_cut && o.plan_cut,
        }
    }

    /// Whether a view at detail level `d` draws it.
    pub fn at(&self, d: crate::graphics::Detail) -> bool {
        use crate::graphics::Detail;
        match d {
            Detail::Coarse => self.coarse,
            Detail::Medium => self.medium,
            Detail::Fine => self.fine,
        }
    }
}

/// The detail levels a symbol is drawn at (Revit's Visibility Settings of a symbolic line), each true unless it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Levels {
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub coarse: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub medium: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub fine: bool,
}

impl Default for Levels {
    fn default() -> Levels {
        Levels { coarse: true, medium: true, fine: true }
    }
}

impl Levels {
    /// What a symbol that says nothing is drawn at: every detail level but Coarse.
    pub const UNSAID: Levels = Levels { coarse: false, medium: true, fine: true };

    /// What a symbol whose settings are `levels` is drawn at: its settings, or what a symbol that says nothing is drawn at.
    pub fn of(levels: Option<Levels>) -> Levels {
        levels.unwrap_or(Levels::UNSAID)
    }

    /// Whether a view at detail level `d` draws it.
    pub fn at(&self, d: crate::graphics::Detail) -> bool {
        Show { coarse: self.coarse, medium: self.medium, fine: self.fine, ..Show::default() }.at(d)
    }
}

/// The plane a symbol's points are in: `plan`, (x, y) seen from above; `front`, (x, z) seen from the family's front, looking
/// along +y (Revit's Front elevation of a family; its Back mirrors it); `side`, (y, z) seen from +x looking along −x (Revit's
/// Right; Left mirrors it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolView {
    #[default]
    Plan,
    Front,
    Side,
}

impl SymbolView {
    fn is_plan(&self) -> bool {
        *self == SymbolView::Plan
    }

    /// The direction a view looks along, in the family's axes (up for a plan, along y for a front, along x for a side).
    fn normal(self) -> P3 {
        let mut n = [0.0; 3];
        n[self.axis()] = 1.0;
        n
    }

    /// Which of the family's axes (x 0, y 1, z 2) a view looks along.
    fn axis(self) -> usize {
        match self {
            SymbolView::Plan => 2,
            SymbolView::Front => 1,
            SymbolView::Side => 0,
        }
    }

    /// A point of this view's drawing, in the family's axes: a plan's (x, y) at the floor, a front's (x, z) and a side's (y, z).
    fn lift(self, q: P2) -> P3 {
        match self {
            SymbolView::Plan => [q.0, q.1, 0.0],
            SymbolView::Front => [q.0, 0.0, q.1],
            SymbolView::Side => [0.0, q.0, q.1],
        }
    }

    /// A point of the family's axes as this view draws it: (x, y), (x, z) or (y, z).
    fn flat(self, p: P3) -> P2 {
        match self {
            SymbolView::Plan => P2(p[0], p[1]),
            SymbolView::Front => P2(p[0], p[2]),
            SymbolView::Side => P2(p[1], p[2]),
        }
    }
}

/// Where a family came from, which the engine carries and shows and never acts on: the firm library's entry and its version
/// (set by whoever hands the definition over to load), the application and its own category name (`Revit 2025`, `Doors`),
/// and the file (`Single-Flush.rfa`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// What a family is hosted by, as Revit's families are: a level (the default), a wall (a door, a window), a floor, a ceiling, a
/// roof, a face, or nothing at all. A door's and a window's stand in a wall whatever it says; the others place on a level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Host {
    #[default]
    Level,
    Wall,
    Floor,
    Ceiling,
    Roof,
    Face,
    #[serde(rename = "none")]
    Unhosted,
}

impl Host {
    fn is_level(&self) -> bool {
        *self == Host::Level
    }
}

/// A subcategory of a family's category, as Revit's (`Frame/Mullion`, `Panel`, `Plan Swing`, `Elevation Swing`): its line
/// colour and its material where it sets them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subcategory {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

/// A door's or a window's opening in its wall where it isn't `Width` by `Height` (a frame outside the leaf): numbers or
/// formulas over the family's parameters, mm. Rectangular, from the sill up, centred on the opening.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cut {
    pub width: Expr,
    pub height: Expr,
}

/// Whether a family has Revit's flip controls: `hand` (left and right) and `facing` (in and out). Without one, the canvas
/// shows no arrow for it; the `flip` command flips it all the same, as Revit's API does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flips {
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub hand: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub facing: bool,
}

impl Default for Flips {
    fn default() -> Flips {
        Flips { hand: true, facing: true }
    }
}

impl Flips {
    fn is_default(&self) -> bool {
        *self == Flips::default()
    }
}

/// A number, or a formula over the family's parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Expr {
    Number(f64),
    Formula(String),
}

impl Default for Expr {
    fn default() -> Expr {
        Expr::Number(0.0)
    }
}

/// Where a form's profile is drawn: across the family's x and y, swept up; or upright, across x
/// and z, swept along y; or across y and z, swept along x.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    #[default]
    Xy,
    Xz,
    Yz,
}

/// A round profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Circle {
    pub centre: [Expr; 2],
    pub radius: Expr,
}

/// A round line: its middle, its radius, and the angles it runs between, degrees counter-clockwise from +x. Where it has
/// `start` and `end` (points in the family's plane, as Revit's arc is its centre and two ends, constrained), its angles
/// follow them as they move: it runs from the angle of `start` about its middle to the angle of `end`, the way round
/// that `from` and `to` say (counter-clockwise when `to` is not below `from`, else clockwise), a whole turn when the two
/// ends are one point (or on one line from its middle); its radius is `start`'s distance from its middle, not `radius`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub centre: [Expr; 2],
    pub radius: Expr,
    #[serde(default)]
    pub from: Expr,
    #[serde(default)]
    pub to: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<[Expr; 2]>,
}

/// An arc worked out: its middle, its radius, and the angles it runs between, degrees, counter-clockwise from +x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sweep {
    pub centre: P2,
    pub radius: f64,
    pub from: f64,
    pub to: f64,
}

/// How near two ends are one point, mm: an arc between them is a whole turn.
const ONE_POINT: f64 = 1e-6;

impl Curve {
    /// Its every number or formula.
    pub fn exprs(&self) -> impl Iterator<Item = &Expr> {
        self.centre.iter().chain([&self.radius, &self.from, &self.to]).chain(self.start.iter().flatten()).chain(self.end.iter().flatten())
    }

    /// Whether it follows its ends: it has both.
    pub fn follows_ends(&self) -> bool {
        self.start.is_some() && self.end.is_some()
    }

    /// Its middle, its radius and its angles, worked out by `values`. Where it follows its ends, its angles are those of its
    /// ends about its middle; the way round is that of the `from` and `to` it was given.
    fn sweep<'a>(&'a self, values: &BTreeMap<String, Val>, read: &mut HashMap<&'a str, Result<Node, String>>) -> Result<Sweep, String> {
        let mut n = |e: &'a Expr| number(e, values, read);
        let (centre, radius, from, to) = (P2(n(&self.centre[0])?, n(&self.centre[1])?), n(&self.radius)?, n(&self.from)?, n(&self.to)?);
        let (Some(start), Some(end)) = (&self.start, &self.end) else { return Ok(Sweep { centre, radius, from, to }) };
        let (start, end) = (P2(n(&start[0])?, n(&start[1])?), P2(n(&end[0])?, n(&end[1])?));
        let angle = |p: P2| (p.1 - centre.1).atan2(p.0 - centre.0).to_degrees();
        let (a0, a1) = (angle(start), angle(end));
        let ccw = to >= from;
        let mut turn = if start.dist(end) < ONE_POINT {
            360.0
        } else if ccw {
            (a1 - a0).rem_euclid(360.0)
        } else {
            (a0 - a1).rem_euclid(360.0)
        };
        // Ends at one angle but at different distances from the middle (nothing to turn through) are the whole way round.
        if turn < 1e-9 && start.dist(end) >= ONE_POINT {
            turn = 360.0;
        }
        // Its ends set its radius: where nothing binds the radius to a parameter, the ends' distance from the middle is it.
        Ok(Sweep { centre, radius: centre.dist(start), from: a0, to: if ccw { a0 + turn } else { a0 - turn } })
    }

    /// Its two ends, worked out by `values`: at its angles, `radius` from its middle; none where it doesn't work out.
    pub(crate) fn ends(&self, values: &BTreeMap<String, Val>) -> Option<(P2, P2)> {
        let s = self.sweep(values, &mut HashMap::new()).ok()?;
        let at = |deg: f64| {
            let (sin, cos) = deg.to_radians().sin_cos();
            P2(s.centre.0 + s.radius * cos, s.centre.1 + s.radius * sin)
        };
        Some((at(s.from), at(s.to)))
    }
}

/// Which side of its point a label's lettering starts, is centred or ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    fn is_left(&self) -> bool {
        *self == Align::Left
    }
}

/// A label a symbol letters: capitals `height` mm tall, `at` the middle of the line of text vertically, where `{Parameter Name}` in `value` reads
/// that parameter of the family, started, centred or ended (`align`) at `at`; it is drawn upright and unmirrored whatever the component does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub at: [Expr; 2],
    pub height: Expr,
    pub value: String,
    #[serde(default, skip_serializing_if = "Align::is_left")]
    pub align: Align,
}

/// A mark a family's plan draws in its own axes, sized by its parameters: the polyline of a `line` or an `arc`, a solid
/// disc, or a label: its anchor, which is placed with the component, and its lettering, which never is (it is drawn upright,
/// by [`lettering`], wherever it ends up).
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Line(Vec<P2>),
    Dot { centre: P2, radius: f64 },
    Label { at: P2, height: f64, text: String, align: Align },
}

impl Mark {
    /// The same mark with every point taken through `f`.
    pub fn map(&self, f: impl Fn(P2) -> P2) -> Mark {
        match self {
            Mark::Line(pts) => Mark::Line(pts.iter().map(|p| f(*p)).collect()),
            Mark::Dot { centre, radius } => Mark::Dot { centre: f(*centre), radius: *radius },
            Mark::Label { at, height, text, align } => Mark::Label { at: f(*at), height: *height, text: text.clone(), align: *align },
        }
    }

    /// The same mark drawn `k` times as large about the origin: its points and a dot's radius, not a label's lettering.
    pub fn scaled(&self, k: f64) -> Mark {
        match self {
            Mark::Dot { radius, .. } => match self.map(|p| P2(p.0 * k, p.1 * k)) {
                Mark::Dot { centre, .. } => Mark::Dot { centre, radius: radius * k },
                other => other,
            },
            _ => self.map(|p| P2(p.0 * k, p.1 * k)),
        }
    }
}

/// A mark a family's plan draws in its own axes, and no view shows otherwise (a door's swing, a socket's circle): a `line`
/// through points, an `arc`, a `text` label or a `dot`, a solid disc; `visible` (a yes/no parameter, or a formula) whether it's
/// there.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Symbol {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line: Vec<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Label>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dot: Option<Circle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    /// The plane its points are in, and so the views that draw it: plans (the default), or sections and elevations along
    /// the family's y (`front`) or x (`side`).
    #[serde(default, skip_serializing_if = "SymbolView::is_plan")]
    pub view: SymbolView,
    /// The detail levels it is drawn at; none, every level but Coarse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<Levels>,
    /// The types it is drawn in; none, every type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    /// One of its family's `subcategories`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,
    /// Revit's own result for its types, kept as the check and the fallback: drawn only in a type that draws Revit's
    /// ([`Family::draws_snapshot`]), and a parametric one only in a type that doesn't.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snapshot: bool,
}

impl Symbol {
    /// Whether a view at detail level `d` draws it: at the levels its `show` says, else at every level but Coarse.
    pub fn drawn_at(&self, d: crate::graphics::Detail) -> bool {
        Levels::of(self.show).at(d)
    }
}

/// What a connector joins: `power` carries a load, `switch` only switches one, `pipe` joins a pipe of its `domain`, `data`
/// a data cable and `earthing` an earthing conductor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorKind {
    #[default]
    Power,
    Switch,
    Pipe,
    Data,
    Earthing,
}

impl ConnectorKind {
    /// Whether it is an electrical device's, which has a `system` and draws (or switches) a load.
    pub fn electrical(self) -> bool {
        matches!(self, ConnectorKind::Power | ConnectorKind::Switch)
    }
}

/// What a pipe connector carries: cold or hot water, waste (sewage), vent, storm water or gas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    Cold,
    Hot,
    Waste,
    Vent,
    Storm,
    Gas,
}

/// Which way what it carries goes through it: into the family (`in`, as water into a basin), out of it (`out`, as waste
/// from a WC) or either way (`both`, the default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    In,
    Out,
    #[default]
    Both,
}

impl Flow {
    fn is_both(&self) -> bool {
        *self == Flow::Both
    }
}

/// The largest connector size, mm.
pub const MAX_CONNECTOR_SIZE: f64 = 2000.0;

/// How a connector's load is shared among its poles: the same on each (`power-balanced`), or each its own
/// (`power-unbalanced`); a `switch` has none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorSystem {
    #[default]
    PowerBalanced,
    PowerUnbalanced,
    Switch,
}

/// A connector's load in volt-amperes: one number (shared evenly among the poles), or, for an unbalanced connector,
/// one for each pole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Load {
    One(Expr),
    PerPhase(Vec<Expr>),
}

impl Default for Load {
    fn default() -> Load {
        Load::One(Expr::default())
    }
}

/// Where a family joins a system, as Revit's connector does: what an electrical device draws, what a switch controls, where
/// a pipe or a cable meets it. `voltage`, `load_va` and `power_factor` are numbers or formulas over the family's parameters,
/// so a type or a component can set them, and so are its place (`at`, in the family's axes: x along it, y to its left, z up
/// from its base), the way it faces (`direction`, out of the family: the way a pipe leaves it) and its `size`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connector {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    /// What it's called in its family (`Waste`, `Cold water`), for people; none is fine.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: ConnectorKind,
    /// How a `power` connector shares its load, or `switch`; the other kinds have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<ConnectorSystem>,
    /// What a `pipe` connector carries; none on a pipe fitting or accessory, which takes its pipe's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<Domain>,
    #[serde(default, skip_serializing_if = "Flow::is_both")]
    pub flow: Flow,
    /// Where it is, mm, in the family's axes. A `pipe`, `data` or `earthing` connector has one; an electrical device's may.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[Expr; 3]>,
    /// The way it faces, out of the family, in its axes: any length but none. A `pipe` connector has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<[Expr; 3]>,
    /// Its size, mm: a pipe connector's nominal diameter, which it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Expr>,
    #[serde(default = "one_pole")]
    pub poles: u8,
    /// Volts.
    #[serde(default)]
    pub voltage: Expr,
    #[serde(default)]
    pub load_va: Load,
    #[serde(default = "unity")]
    pub power_factor: Expr,
    /// What the device is for: `lighting`, `receptacle`…, as a schedule would group it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub classification: String,
}

fn one_pole() -> u8 {
    1
}

fn unity() -> Expr {
    Expr::Number(1.0)
}

/// A component's connector, worked out, with its place and the way it faces in the model (none for one without a place).
pub type PlacedConnector = (ConnectorValues, Option<[f64; 3]>, Option<[f64; 3]>);

/// A connector, worked out for one type and one component's values.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectorValues {
    pub id: crate::ids::Id,
    pub guid: crate::ids::Guid,
    pub name: String,
    pub kind: ConnectorKind,
    pub system: Option<ConnectorSystem>,
    pub domain: Option<Domain>,
    pub flow: Flow,
    /// In the family's axes, mm.
    pub at: Option<[f64; 3]>,
    /// A unit vector, in the family's axes.
    pub direction: Option<[f64; 3]>,
    /// mm.
    pub size: Option<f64>,
    pub poles: u8,
    pub voltage: f64,
    /// The whole load, VA: the phases' sum.
    pub load_va: f64,
    /// Each phase's load, VA; none for a switch.
    pub load_va_by_phase: Vec<f64>,
    pub power_factor: f64,
    pub classification: String,
}

/// A solid of a family, in its own axes (x along the component's angle, y to its left, z up from
/// its base): a profile drawn in `plane` and swept across it from `from` to `to`; or, with an
/// `axis`, turned about it from `from` to `to` degrees; or triangles, as a file brings them. A
/// material's name, or a material parameter's, says what it's made of; `visible` (a yes/no
/// parameter, or a formula) whether it's there. A `void` is no solid: it cuts the solid forms listed
/// before it that it passes through, and is never seen itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    #[serde(default)]
    pub plane: Plane,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circle: Option<Circle>,
    #[serde(default)]
    pub from: Expr,
    #[serde(default)]
    pub to: Expr,
    /// A revolve: the line in the sketch plane its profile turns about, right-handed from the axis'
    /// first point toward its second; its `from` and `to` are angles, a full turn at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<[[Expr; 2]; 2]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub void: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    /// Shared, so an undo step's copy of the model doesn't copy it.
    #[serde(default, skip_serializing_if = "no_mesh")]
    pub mesh: Arc<Vec<[P3; 3]>>,
    /// Revit's Visibility Settings: the detail levels and the views it is drawn in; none (it says nothing), every one, as
    /// Revit's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<Show>,
    /// The types it is built in; none, every type. A family exported from Revit carries each type's own geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    /// A `mesh`'s outline seen from above, in the family's axes, as whoever made it traced it, in place of the hull round it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<[f64; 2]>,
    /// One of its family's `subcategories`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,
    /// Rings inside its `profile` its sweep leaves open (Revit's inner loops of an extrusion's sketch).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<[Expr; 2]>>,
    /// A blend: its profile at `to`, its `profile` being the one at `from`; the sides run between corners paired as Revit
    /// pairs them (one for one where both have as many, else along their lengths from the corners nearest each other).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top: Vec<[Expr; 2]>,
    /// A sweep: the line its profile follows, points in the family's axes (an arc as short chords), and its profile there,
    /// `section`, points in the family's axes at the path's first point; a swept blend's profile at the path's last point is
    /// `section_end`, as many corners as `section`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<[Expr; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub section: Vec<[Expr; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub section_end: Vec<[Expr; 3]>,
    /// Loops inside a sweep's profile (a tube's inside), at the path's first point, and a swept blend's at its last: swept
    /// with it as holes. A true arc of hole `k` is a `section` (or `section_end`) arc of `hole` `k + 1`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub section_holes: Vec<Vec<[Expr; 3]>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub section_end_holes: Vec<Vec<[Expr; 3]>>,
    /// A revolve's sketch plane, where it is along the axis it is square to (none, through the family's origin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Expr>,
    /// Edges of its rings that are true arcs (Revit's arcs in its sketches): drawn as a circle is, kept as arcs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arcs: Vec<Bend>,
    /// Revit's own result for its types, kept as the check and the fallback ([`Family::draws_snapshot`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snapshot: bool,
    /// The id of the Revit definition form it was made from, which every form it was split into carries: what a void's `cuts`
    /// name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// A void's, where it came from Revit: the `source` ids of the forms Revit says it cuts. Kept as Revit said it and used for
    /// nothing: Revit's lists leave out forms its void plainly cuts (and say none for one that does), and name solids made after
    /// the void, which it doesn't cut. A void cuts every solid listed before it that it reaches, whatever this says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuts: Option<Vec<String>>,
}

/// Which ring of a form an arc is an edge of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ring {
    #[default]
    Profile,
    Top,
    Hole,
    Path,
    Section,
    SectionEnd,
}

impl Ring {
    fn is_profile(&self) -> bool {
        *self == Ring::Profile
    }
}

/// An edge of a form's ring that is a true arc: the edge from corner `edge` to the next (the last to the first in a closed
/// ring), about `centre` (two numbers in a plane's ring, three in a sweep's), counter-clockwise unless `clockwise`, about
/// `normal` in 3D; its ends are the corners, so it follows them as they flex, and its radius goes from one's to the other's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bend {
    #[serde(default, skip_serializing_if = "Ring::is_profile")]
    pub ring: Ring,
    /// Which hole, for a `hole`'s.
    #[serde(default, skip_serializing_if = "is_naught")]
    pub hole: usize,
    pub edge: usize,
    pub centre: Vec<Expr>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clockwise: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f64; 3]>,
    /// A point the arc passes through between its corners (two numbers in a plane's ring): where it is given, the arc is the
    /// one through its two corners and it, its `centre` worked out from them (an arched window's head, whose centre and
    /// radius follow its width as no sum of parameters does).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<Expr>,
}

fn is_naught(n: &usize) -> bool {
    *n == 0
}

impl Form {
    /// Every number or formula it is sized by.
    pub fn exprs(&self) -> Vec<&Expr> {
        let circle = self.circle.iter().flat_map(|c| c.centre.iter().chain([&c.radius]));
        self.profile
            .iter()
            .flatten()
            .chain([&self.from, &self.to])
            .chain(&self.visible)
            .chain(circle)
            .chain(self.axis.iter().flatten().flatten())
            .chain(self.holes.iter().flatten().flatten())
            .chain(self.top.iter().flatten())
            .chain(self.path.iter().flatten())
            .chain(self.section.iter().flatten())
            .chain(self.section_end.iter().flatten())
            .chain(self.section_holes.iter().flatten().flatten())
            .chain(self.section_end_holes.iter().flatten().flatten())
            .chain(self.arcs.iter().flat_map(|b| b.centre.iter().chain(&b.through)))
            .chain(&self.at)
            .collect()
    }

    /// Whether it is built in type `type_name`: one it names, or any when it names none.
    pub fn of_type(&self, type_name: &str) -> bool {
        self.types.is_empty() || self.types.iter().any(|t| t == type_name)
    }
}

fn no_mesh(mesh: &Arc<Vec<[P3; 3]>>) -> bool {
    mesh.is_empty()
}

/// A family: its parameters, its types (each its type parameters' values where they aren't the
/// parameters' own) and its forms.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    #[serde(default)]
    pub category: Category,
    /// Which fixture or fitting of its category it is (`wc`, `bend`…), one of [`Category::kinds`]: IFC writes its
    /// components' predefined type from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The IFC class its components came in from a file as, which a generic family's go back out as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifc: Option<IfcClass>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub types: BTreeMap<String, BTreeMap<String, Value>>,
    /// Each type's identity data (Type Mark, Manufacturer…), by the type's name: only those that hold any.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identity: BTreeMap<String, crate::type_data::TypeIdentity>,
    #[serde(default)]
    pub forms: Vec<Form>,
    /// What its plan draws besides its forms: lines, arcs, labels and dots of its own, in any category.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Symbol>,
    /// How a door's or a window's leaves open, which IFC and schedules say; none, a door swings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<crate::model::Operation>,
    /// Where it joins a system: an electrical device's load, a switch. At most one is `power`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<Connector>,
    /// Where it came from: a firm library's entry, an application, a file. Carried and shown, never acted on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// What it is hosted by, as Revit says it: a level unless it says.
    #[serde(default, skip_serializing_if = "Host::is_level")]
    pub host: Host,
    /// Its category's subcategories its forms and symbols are drawn in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subcategories: Vec<Subcategory>,
    /// A door's or a window's opening in its wall, where it isn't `Width` by `Height`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<Cut>,
    /// Whether it has Revit's flip controls.
    #[serde(default, skip_serializing_if = "Flips::is_default")]
    pub flips: Flips,
    /// Its reference planes, which formulas read as `ref(Name)`, and the dimensions between them that place them per type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<Dimension>,
    /// Other families placed in it, as Revit's nested families: its own `embedded` ones first, else the project's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested: Vec<Nested>,
    /// The families its nested parts are of that it holds itself, by name, as Revit's family holds a nested family that isn't
    /// shared: never one of the project's, so the project's list shows only what is placed in it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub embedded: BTreeMap<String, Family>,
    /// Its type catalog, as Revit's `.txt` beside a family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<Catalog>,
    /// Per type, whether its parametric result matched Revit's own when it came in ([`Family::run_check`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub check: BTreeMap<String, TypeCheck>,
    /// Whether it can be edited as in Revit's Family Editor, and why not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editable: Option<Editable>,
    /// Whether it is a Revit RPC planting (a tree, a person): the 3D Revit draws of it is a render placeholder, so its check
    /// compares only its plan and its 3D is always Revit's own result ([`Family::draws_snapshot_forms`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rpc: bool,
}

impl Symbol {
    /// Whether it is drawn in type `type_name`: one it names, or any when it names none.
    pub fn of_type(&self, type_name: &str) -> bool {
        self.types.is_empty() || self.types.iter().any(|t| t == type_name)
    }
}

/// The IFC class a product came in as, which a generic family remembers (BIMCraft has no element for it): the class as STEP
/// writes it (`IFCPLATE`), its predefined type (`CURTAIN_PANEL`), and the object type that says what it is where its predefined
/// type is `USERDEFINED` or its type object is named otherwise.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IfcClass {
    pub class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predefined: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
}

/// The longest IFC class or predefined type a family remembers: IFC's longest are half of it.
pub const MAX_IFC_NAME: usize = 64;

impl IfcClass {
    /// As IFC writes it: the class and the predefined type in capitals, whatever case they were typed in.
    pub fn written(mut self) -> IfcClass {
        self.class = self.class.trim().to_ascii_uppercase();
        self.predefined = self.predefined.map(|p| p.trim().trim_matches('.').to_ascii_uppercase());
        self
    }

    /// Why it isn't one IFC can write, if it isn't: a class `IFC` and capitals or digits, a predefined type of capitals, digits
    /// and `_`, an object type of text.
    pub fn check(&self) -> Result<(), String> {
        let name = |what: &str, v: &str, ok: &dyn Fn(char) -> bool| {
            let shown: String = v.chars().take(40).collect();
            if v.is_empty() || v.len() > MAX_IFC_NAME || !v.chars().all(ok) {
                Err(format!("`ifc` `{what}` `{shown}`: IFC's names are capitals, digits and `_`, up to {MAX_IFC_NAME}"))
            } else {
                Ok(())
            }
        };
        name("class", &self.class, &|c| c.is_ascii_uppercase() || c.is_ascii_digit())?;
        if !self.class.starts_with("IFC") {
            return Err(format!("`ifc` `class` `{}`: an IFC class starts with `IFC` (`IFCPLATE`)", self.class));
        }
        if let Some(p) = &self.predefined {
            name("predefined", p, &|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')?;
        }
        if let Some(o) = &self.object_type
            && (o.trim().is_empty() || o.chars().count() > MAX_TEXT || o.chars().any(char::is_control))
        {
            return Err(format!("`ifc` `object_type`: text of 1 to {MAX_TEXT} characters on one line"));
        }
        Ok(())
    }
}

/// A parameter's value, worked out: a number (yes/no, 1 or 0), or a material's name.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Number(f64),
    Text(String),
}

/// A form's solid in the family's own axes: its triangles, its caps' first; its outline seen from
/// above, from its bottom to its top, what it's made of; a round one's sides read as one surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub tris: Vec<[P3; 3]>,
    pub caps: usize,
    pub outline: Vec<P2>,
    pub bottom: f64,
    pub top: f64,
    pub material: String,
    pub round: bool,
    /// Its form's Visibility Settings ([`Form::show`]); none, every view at every level.
    pub show: Option<Show>,
}

impl Piece {
    /// Whether a plan at detail level `d` draws it: `plan` and that level, as its form says.
    pub fn in_plan(&self, d: crate::graphics::Detail) -> bool {
        let s = Show::in_view(self.show);
        s.plan && s.at(d)
    }

    /// Whether a plan at detail level `d` cutting at height `cut` (in its own axes) draws it, as Revit's floor plan does: one
    /// the plane cuts where its Plan/RCP (and Plan/RCP when cut) say, one wholly below it (or standing on it) where its
    /// Top/Bottom says, and for a family other than a door's or a window's (`hosted`) its Plan/RCP too; one above it only
    /// for such a family, where its Plan/RCP says.
    pub fn in_plan_cut(&self, d: crate::graphics::Detail, cut: f64, hosted: bool) -> bool {
        let eps = check::CUT_EPS;
        let s = Show::in_view(self.show);
        if !s.at(d) {
            return false;
        }
        if self.bottom < cut - eps && cut < self.top {
            s.plan && s.plan_cut
        } else if self.bottom <= cut + eps {
            s.plan_top && (hosted || s.plan)
        } else {
            // Above it: a door's or a window's isn't seen; another family's plan draws it where its Plan/RCP says, as
            // Revit's plan of it (a ceiling light's, looking up) does.
            !hosted && s.plan
        }
    }

    /// Whether a view at detail level `d` draws it, whichever way it looks (a 3D view's, IFC's at Fine).
    pub fn at(&self, d: crate::graphics::Detail) -> bool {
        Show::in_view(self.show).at(d)
    }
}

/// Where one form lies before any void cuts it: its box in the family's own axes (x along a
/// component's angle, y to its left, z up from its base), which is all a warning about where a form
/// stands needs. [`Family::shapes`] works them out, one per form that builds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Its place in [`Family::forms`], counting from zero.
    pub index: usize,
    pub void: bool,
    pub min: P3,
    pub max: P3,
}

impl Shape {
    /// Whether it and `other` overlap where all three of them have substance: strictly, a face
    /// touching another's is no overlap, so a void grazing a form cuts nothing.
    pub fn over(&self, other: &Shape) -> bool {
        (0..3).all(|k| self.min[k] < other.max[k] && other.min[k] < self.max[k])
    }

    /// Whether it lies wholly outside `x` wide each way and `z` tall from `low` up: the box a door's
    /// or a window's opening fills.
    pub fn outside(&self, half_width: f64, low: f64, high: f64) -> bool {
        self.max[0] < -half_width || self.min[0] > half_width || self.max[2] < low || self.min[2] > high
    }
}

/// How deep a formula may nest.
const MAX_DEPTH: usize = 64;
/// Sides a circle's profile is drawn with.
const SEGMENTS: usize = 32;
/// Degrees a symbol's arc turns between the corners it is drawn with.
const ARC_STEP: f64 = 3.75;
/// Corners one profile may have.
pub const MAX_PROFILE: usize = 64;
/// Formulas one may take from in a row: `A` from `B` from `C`…
pub const MAX_CHAIN: usize = 16;
/// Triangles a family's types may build together.
pub const MAX_BUILT: usize = 400_000;
/// Triangles one form, as a file brings it, may have.
pub const MAX_TRIANGLES: usize = 100_000;

/// Parameters, forms, symbols and types one family may have: as many as a family exported from Revit, which carries each
/// type's own forms, may bring.
pub const MAX_PARAMETERS: usize = 256;
pub const MAX_FORMS: usize = 8192;
pub const MAX_SYMBOLS: usize = 8192;
/// Corners one ring of a family's form may have (a profile, a hole, a blend's top, a sweep's section), and points its path.
pub const MAX_RING: usize = 1024;
/// Points one symbol's `line` may have.
pub const MAX_SYMBOL_POINTS: usize = 1024;
/// Characters a text parameter's value, a subcategory's name or a source's field may hold.
pub const MAX_WORDS: usize = 1024;
pub const MAX_CONNECTORS: usize = 8;
/// The highest voltage and load a connector may hold: far past any real device, short of overflow.
pub const MAX_VOLTAGE: f64 = 1.0e5;
pub const MAX_LOAD_VA: f64 = 1.0e9;
pub const MAX_TYPES: usize = 1000;
/// The longest formula, in characters.
pub const MAX_FORMULA: usize = 1024;

/// A formula, read.
enum Node {
    Number(f64),
    Name(String),
    Minus(Box<Node>),
    /// A sum's or a product's first term, then each operator and term after it: flat, so a long one
    /// doesn't nest.
    Chain(Box<Node>, Vec<(char, Node)>),
    Op(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

/// Reads a formula: comparisons over sums over products over powers, which are right-associative.
struct Reader {
    chars: Vec<char>,
    at: usize,
    depth: usize,
}

impl Reader {
    fn peek(&mut self) -> Option<char> {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        self.at += usize::from(found);
        found
    }

    /// One level deeper; refused past `MAX_DEPTH`, so a hostile formula can't exhaust the stack.
    fn deeper(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(format!("it's nested too deep (more than {MAX_DEPTH} levels)")) } else { Ok(()) }
    }

    fn compare(&mut self) -> Result<Node, String> {
        self.deeper()?;
        let a = self.sum()?;
        let op = match self.peek() {
            Some('<') => {
                self.at += 1;
                if self.eat('=') { 'l' } else { '<' }
            }
            Some('>') => {
                self.at += 1;
                if self.eat('=') { 'g' } else { '>' }
            }
            Some('=') => {
                self.at += 1;
                '='
            }
            _ => {
                self.depth -= 1;
                return Ok(a);
            }
        };
        let b = self.sum()?;
        self.depth -= 1;
        Ok(Node::Op(op, Box::new(a), Box::new(b)))
    }

    fn sum(&mut self) -> Result<Node, String> {
        let first = self.product()?;
        let mut rest = Vec::new();
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.at += 1;
            rest.push((op, self.product()?));
        }
        Ok(if rest.is_empty() { first } else { Node::Chain(Box::new(first), rest) })
    }

    fn product(&mut self) -> Result<Node, String> {
        let first = self.power()?;
        let mut rest = Vec::new();
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.at += 1;
            rest.push((op, self.power()?));
        }
        Ok(if rest.is_empty() { first } else { Node::Chain(Box::new(first), rest) })
    }

    fn power(&mut self) -> Result<Node, String> {
        let a = self.unary()?;
        if !self.eat('^') {
            return Ok(a);
        }
        self.deeper()?;
        let b = self.power()?;
        self.depth -= 1;
        Ok(Node::Op('^', Box::new(a), Box::new(b)))
    }

    fn unary(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some(sign @ ('-' | '+')) => {
                self.at += 1;
                self.deeper()?;
                let a = self.unary()?;
                self.depth -= 1;
                Ok(if sign == '-' { Node::Minus(Box::new(a)) } else { a })
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some('(') => {
                self.at += 1;
                let a = self.compare()?;
                if self.eat(')') { Ok(a) } else { Err("a `(` isn't closed".into()) }
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                    self.at += 1;
                }
                // An exponent: 1e6, 2.5E-3.
                if self.chars.get(self.at).is_some_and(|c| matches!(c, 'e' | 'E')) {
                    let sign = usize::from(self.chars.get(self.at + 1).is_some_and(|c| matches!(c, '+' | '-')));
                    if self.chars.get(self.at + 1 + sign).is_some_and(char::is_ascii_digit) {
                        self.at += 1 + sign;
                        while self.chars.get(self.at).is_some_and(char::is_ascii_digit) {
                            self.at += 1;
                        }
                    }
                }
                let text: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect();
                text.parse().map(Node::Number).map_err(|_| format!("`{text}` isn't a number"))
            }
            // A name may hold spaces, as Revit's do: `Leg Height`.
            Some(c) if c.is_alphabetic() || c == '_' => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == ' ') {
                    self.at += 1;
                }
                let name: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect::<String>().trim_end().to_string();
                if !self.eat('(') {
                    return Ok(Node::Name(name));
                }
                let mut args = Vec::new();
                if !self.eat(')') {
                    loop {
                        args.push(self.compare()?);
                        if self.eat(')') {
                            break;
                        }
                        if !self.eat(',') {
                            return Err(format!("`{name}(` needs a `,` or a `)`"));
                        }
                    }
                }
                Ok(Node::Call(name, args))
            }
            Some(c) => Err(format!("`{c}` can't start a value there")),
            None => Err("something's missing at its end".into()),
        }
    }
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Node `n`'s value: its names' from `get`; an `if` works out only the branch it takes.
fn value(n: &Node, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    Ok(match n {
        Node::Number(x) => *x,
        Node::Name(name) => get(name)?,
        Node::Minus(a) => -value(a, get)?,
        Node::Chain(first, rest) => {
            let mut x = value(first, get)?;
            for (op, n) in rest {
                let y = value(n, get)?;
                x = match op {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    _ => x / y,
                };
            }
            x
        }
        Node::Op(op, a, b) => {
            let (x, y) = (value(a, get)?, value(b, get)?);
            match op {
                '+' => x + y,
                '-' => x - y,
                '*' => x * y,
                '/' => x / y,
                '^' => x.powf(y),
                '<' => truth(x < y),
                '>' => truth(x > y),
                'l' => truth(x <= y),
                'g' => truth(x >= y),
                // Equal to within float noise, as Revit compares.
                _ => truth((x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)),
            }
        }
        Node::Call(f, args) => {
            let one = |get: &mut dyn FnMut(&str) -> Result<f64, String>| match args.as_slice() {
                [a] => value(a, get),
                _ => Err(format!("`{f}` takes one value, not {}", args.len())),
            };
            let deg = |x: f64| x.to_radians();
            match f.to_lowercase().as_str() {
                // Where a reference plane is for the type: `ref(Left)`.
                "ref" => match args.as_slice() {
                    [Node::Name(name)] => get(&ref_key(name))?,
                    _ => return Err("`ref` takes the name of one of the family's references: `ref(Left)`".into()),
                },
                "if" => match args.as_slice() {
                    [c, a, b] => value(if value(c, get)? != 0.0 { a } else { b }, get)?,
                    _ => return Err(format!("`{f}` takes a condition and two values, not {} values", args.len())),
                },
                name @ ("and" | "or" | "min" | "max") => {
                    if args.is_empty() {
                        return Err(format!("`{f}` takes one value or more"));
                    }
                    let xs = args.iter().map(|a| value(a, get)).collect::<Result<Vec<f64>, String>>()?;
                    match name {
                        "and" => truth(xs.iter().all(|x| *x != 0.0)),
                        "or" => truth(xs.iter().any(|x| *x != 0.0)),
                        "min" => xs.iter().copied().fold(f64::INFINITY, f64::min),
                        _ => xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    }
                }
                "not" => truth(one(get)? == 0.0),
                "abs" => one(get)?.abs(),
                "sqrt" => one(get)?.sqrt(),
                "round" => one(get)?.round(),
                "roundup" => one(get)?.ceil(),
                "rounddown" => one(get)?.floor(),
                "sin" => deg(one(get)?).sin(),
                "cos" => deg(one(get)?).cos(),
                "tan" => deg(one(get)?).tan(),
                "asin" => one(get)?.asin().to_degrees(),
                "acos" => one(get)?.acos().to_degrees(),
                "atan" => one(get)?.atan().to_degrees(),
                "exp" => one(get)?.exp(),
                "ln" => one(get)?.ln(),
                "log" => one(get)?.log10(),
                "pi" if args.is_empty() => std::f64::consts::PI,
                _ => return Err(format!("`{f}` isn't a function: there are {FUNCTIONS}")),
            }
        }
    })
}

/// The functions a formula calls.
const FUNCTIONS: &str =
    "ref, if, and, or, not, min, max, abs, sqrt, round, roundup, rounddown, sin, cos, tan, asin, acos, atan, exp, ln, log and pi()";

/// The names `formula` reads, in every branch of its `if`s; or why it can't be worked out: it
/// doesn't read, or calls a function there isn't or with the wrong number of values.
pub fn names(formula: &str) -> Result<Vec<String>, String> {
    fn walk(n: &Node, out: &mut Vec<String>) -> Result<(), String> {
        match n {
            Node::Number(_) => {}
            Node::Name(name) if !out.contains(name) => out.push(name.clone()),
            Node::Name(_) => {}
            Node::Minus(a) => walk(a, out)?,
            Node::Chain(first, rest) => {
                walk(first, out)?;
                for (_, n) in rest {
                    walk(n, out)?;
                }
            }
            Node::Op(_, a, b) => {
                walk(a, out)?;
                walk(b, out)?;
            }
            Node::Call(f, args) if f.eq_ignore_ascii_case("ref") => match args.as_slice() {
                [Node::Name(name)] => {
                    let key = ref_key(name);
                    if !out.contains(&key) {
                        out.push(key);
                    }
                }
                _ => return Err("`ref` takes the name of one of the family's references: `ref(Left)`".into()),
            },
            Node::Call(f, args) => {
                let fits = match f.to_lowercase().as_str() {
                    "if" => args.len() == 3,
                    "and" | "or" | "min" | "max" => !args.is_empty(),
                    "pi" => args.is_empty(),
                    "not" | "abs" | "sqrt" | "round" | "roundup" | "rounddown" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
                    | "exp" | "ln" | "log" => args.len() == 1,
                    _ => return Err(format!("`{f}` isn't a function: there are {FUNCTIONS}")),
                };
                if !fits {
                    return Err(format!("`{f}` doesn't take {} values", args.len()));
                }
                for a in args {
                    walk(a, out)?;
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&parse(formula)?, &mut out).map_err(|e| format!("formula `{formula}`: {e}"))?;
    Ok(out)
}

/// `formula` with each read of the name `from` read as `to`: a name as `Reader` reads one (letters, digits, `_` and spaces from a
/// letter, but not one a `(` follows, which is a function), so `Area` is not `Area Total` or `area_2`, and a number's exponent
/// (`1e6`) is no name. The rest of it as it was.
pub fn rename_in_formula(formula: &str, from: &str, to: &str) -> String {
    let chars: Vec<char> = formula.chars().collect();
    let (mut out, mut at) = (String::new(), 0);
    while let Some(&c) = chars.get(at) {
        if c.is_ascii_digit() || c == '.' {
            // A number, its exponent too.
            let start = at;
            while chars.get(at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                at += 1;
            }
            if chars.get(at).is_some_and(|c| matches!(c, 'e' | 'E')) {
                let sign = usize::from(chars.get(at + 1).is_some_and(|c| matches!(c, '+' | '-')));
                if chars.get(at + 1 + sign).is_some_and(char::is_ascii_digit) {
                    at += 1 + sign;
                    while chars.get(at).is_some_and(char::is_ascii_digit) {
                        at += 1;
                    }
                }
            }
            out.extend(chars.get(start..at).unwrap_or_default());
        } else if c.is_alphabetic() || c == '_' {
            let start = at;
            while chars.get(at).is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == ' ') {
                at += 1;
            }
            let run: String = chars.get(start..at).unwrap_or_default().iter().collect();
            let (name, spaces) = (run.trim_end(), &run[run.trim_end().len()..]);
            let call = chars.get(at) == Some(&'(');
            out.push_str(if name == from && !call { to } else { name });
            out.push_str(spaces);
        } else {
            out.push(c);
            at += 1;
        }
    }
    out
}

/// `formula` worked out, its parameters' values from `get`: numbers, lengths in mm and angles in
/// degrees; a comparison is 1 when it holds, else 0.
pub fn eval(formula: &str, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    worked(formula, &parse(formula)?, get)
}

/// `formula`, read.
fn parse(formula: &str) -> Result<Node, String> {
    if formula.trim().is_empty() {
        return Err("a formula is empty".into());
    }
    let mut r = Reader { chars: formula.chars().collect(), at: 0, depth: 0 };
    // Where it stopped, a character of the formula counted from 1: what can't be read is worth
    // pointing at, not only naming.
    let node = match r.compare() {
        Ok(node) => node,
        Err(e) => return Err(format!("formula `{formula}`: {e} (character {})", r.at + 1)),
    };
    match r.peek() {
        Some(c) => Err(format!("formula `{formula}`: `{c}` follows where it should end (character {})", r.at + 1)),
        None => Ok(node),
    }
}

/// `formula`, read as `node`, worked out: a number, or why it isn't one.
fn worked(formula: &str, node: &Node, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    let said = |e: String| format!("formula `{formula}`: {e}");
    let x = value(node, get).map_err(said)?;
    if x.is_finite() { Ok(x) } else { Err(said(format!("it comes to {x}, which isn't a number"))) }
}

/// Whether `text` is of 1 to `most` characters on one line.
fn one_line(text: &str, most: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= most && !text.chars().any(char::is_control)
}

/// Whether `name` is one a formula can say: a letter or `_` first, then letters, digits, `_` and
/// spaces, none at its end; up to `MAX_TEXT` long.
pub fn nameable(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ' ')
        && !name.ends_with(' ')
        && name.chars().count() <= MAX_TEXT
}

/// A stored value read as parameter `p` holds it.
pub(crate) fn read(p: &Parameter, v: &Value) -> Result<Val, String> {
    let bad = |what: &str| format!("parameter `{}` holds {what}, not {v}", p.name);
    match p.kind {
        Kind::Material => match v {
            Value::String(s) => Ok(Val::Text(s.clone())),
            Value::Null => Ok(Val::Text(String::new())),
            _ => Err(bad("a material's name")),
        },
        Kind::YesNo => match v {
            Value::Bool(b) => Ok(Val::Number(truth(*b))),
            Value::Null => Ok(Val::Number(0.0)),
            v => v.as_f64().filter(|x| x.is_finite()).map(|x| Val::Number(truth(x != 0.0))).ok_or_else(|| bad("yes or no")),
        },
        Kind::Text => match v {
            Value::String(s) => Ok(Val::Text(s.clone())),
            Value::Null => Ok(Val::Text(String::new())),
            _ => Err(bad("text")),
        },
        Kind::Length | Kind::Number | Kind::Angle => match v {
            Value::Null => Ok(Val::Number(0.0)),
            v => v.as_f64().filter(|x| x.is_finite()).map(Val::Number).ok_or_else(|| bad("a number")),
        },
    }
}

impl Family {
    /// Each material it names, given to `f`, which may put another in its place: its material parameters' values, its
    /// types' values of them, and its forms' materials (a form made of a material parameter names that, which is no
    /// material's name).
    pub fn material_refs_mut(&mut self, f: &mut dyn FnMut(&mut String)) {
        let params: BTreeSet<String> = self.parameters.iter().filter(|p| p.kind == Kind::Material).map(|p| p.name.clone()).collect();
        for p in self.parameters.iter_mut().filter(|p| p.kind == Kind::Material) {
            if let Value::String(m) = &mut p.value {
                f(m);
            }
        }
        for values in self.types.values_mut() {
            for (k, v) in values.iter_mut() {
                if let (true, Value::String(m)) = (params.contains(k), v) {
                    f(m);
                }
            }
        }
        for form in self.forms.iter_mut().filter(|x| !x.material.is_empty() && !params.contains(&x.material)) {
            f(&mut form.material);
        }
        for m in self.subcategories.iter_mut().filter_map(|s| s.material.as_mut()) {
            f(m);
        }
        for inner in self.embedded.values_mut() {
            inner.material_refs_mut(f);
        }
    }

    /// Whether `name` is one of its material parameters.
    pub fn material_parameter(&self, name: &str) -> bool {
        self.parameters.iter().any(|p| p.name == name && p.kind == Kind::Material)
    }

    /// The materials it names: its material parameters' values, its types', and its forms' own.
    pub fn materials(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for p in self.parameters.iter().filter(|p| p.kind == Kind::Material) {
            out.extend(p.value.as_str());
            out.extend(self.types.values().filter_map(|t| t.get(&p.name)).filter_map(Value::as_str));
        }
        out.extend(self.forms.iter().filter(|f| !self.material_parameter(&f.material)).map(|f| f.material.as_str()));
        out.extend(self.subcategories.iter().filter_map(|s| s.material.as_deref()));
        for inner in self.embedded.values() {
            out.extend(inner.materials());
        }
        out.retain(|m| !m.is_empty());
        out
    }

    /// Material `from`, renamed `to` wherever it names it.
    pub fn rename_material(&mut self, from: &str, to: &str) {
        let params: Vec<String> = self.parameters.iter().filter(|p| p.kind == Kind::Material).map(|p| p.name.clone()).collect();
        let swap = |v: &mut Value| {
            if v.as_str() == Some(from) {
                *v = Value::String(to.into());
            }
        };
        self.parameters.iter_mut().filter(|p| p.kind == Kind::Material).for_each(|p| swap(&mut p.value));
        self.types.values_mut().flat_map(|t| t.iter_mut()).filter(|(k, _)| params.contains(k)).for_each(|(_, v)| swap(v));
        self.forms.iter_mut().filter(|f| f.material == from && !params.contains(&f.material)).for_each(|f| f.material = to.into());
        self.subcategories.iter_mut().filter(|s| s.material.as_deref() == Some(from)).for_each(|s| s.material = Some(to.into()));
        self.embedded.values_mut().for_each(|inner| inner.rename_material(from, to));
    }

    /// A mesh form's traced `outline` that crosses itself taken away, so its hull stands for it, as a draw would have it: tested
    /// once, as the family is loaded, not at every draw of every type (a ring's pairs of edges are many); in the families it
    /// holds itself too.
    pub fn clear_crossing_outlines(&mut self) {
        for x in &mut self.forms {
            if x.outline.len() > 2 && crosses_itself(&x.outline.iter().map(|[a, b]| P2(*a, *b)).collect::<Vec<_>>()) {
                x.outline.clear();
            }
        }
        self.embedded.values_mut().for_each(Family::clear_crossing_outlines);
    }

    /// Type `from` renamed `to` in the lists of types its forms and symbols are of.
    pub fn rename_type(&mut self, from: &str, to: &str) {
        let lists = self
            .forms
            .iter_mut()
            .map(|f| &mut f.types)
            .chain(self.symbols.iter_mut().map(|y| &mut y.types))
            .chain(self.nested.iter_mut().map(|n| &mut n.types));
        lists.flat_map(|l| l.iter_mut()).filter(|t| *t == from).for_each(|t| *t = to.to_string());
        if let Some(c) = self.check.remove(from) {
            self.check.insert(to.to_string(), c);
        }
        // The type each nested part's parent type chose goes with the type's name.
        for n in &mut self.nested {
            if let Some(v) = n.type_in.remove(from) {
                n.type_in.insert(to.to_string(), v);
            }
        }
    }

    /// Type `name` gone: taken out of the lists of types its forms and symbols are of, and a form or a symbol that was of it
    /// alone gone with it (left of no type, it would be of every one).
    pub fn forget_type(&mut self, name: &str) {
        let gone = |types: &Vec<String>| !types.is_empty() && types.iter().all(|t| t == name);
        self.forms.retain(|f| !gone(&f.types));
        self.symbols.retain(|y| !gone(&y.types));
        self.nested.retain(|n| !gone(&n.types));
        let lists = self
            .forms
            .iter_mut()
            .map(|f| &mut f.types)
            .chain(self.symbols.iter_mut().map(|y| &mut y.types))
            .chain(self.nested.iter_mut().map(|n| &mut n.types));
        lists.for_each(|l| l.retain(|t| t != name));
        self.check.remove(name);
        self.nested.iter_mut().for_each(|n| {
            n.type_in.remove(name);
        });
    }

    /// Type `new`, made like `old`, is of each form and symbol `old` is of: a duplicated type keeps its forms.
    pub fn type_like(&mut self, new: &str, old: &str) {
        let lists = self
            .forms
            .iter_mut()
            .map(|f| &mut f.types)
            .chain(self.symbols.iter_mut().map(|y| &mut y.types))
            .chain(self.nested.iter_mut().map(|n| &mut n.types));
        for l in lists.filter(|l| l.iter().any(|t| t == old) && !l.iter().any(|t| t == new)) {
            l.push(new.to_string());
        }
        self.check_like(new, old);
        // The copy chooses the nested types its original did, as Revit keeps them.
        for n in &mut self.nested {
            if let Some(v) = n.type_in.get(old).cloned() {
                n.type_in.entry(new.to_string()).or_insert(v);
            }
        }
    }

    /// Its parameters' values for type `type_name`, a component's `instance` values over them, and
    /// its formulas worked out; a hosted family's in a wall of the usual thickness.
    pub fn values(&self, type_name: &str, instance: &BTreeMap<String, Value>) -> Result<BTreeMap<String, Val>, String> {
        self.values_in(type_name, instance, DEFAULT_WALL)
    }

    /// [`Family::values`], with a hosted family's wall `wall` mm thick: what its formulas read as `Wall Thickness`.
    pub fn values_in(&self, type_name: &str, instance: &BTreeMap<String, Value>, wall: f64) -> Result<BTreeMap<String, Val>, String> {
        let set = self.types.get(type_name).ok_or_else(|| {
            let names: Vec<&str> = self.types.keys().map(String::as_str).collect();
            format!("type `{type_name}` doesn't exist (there are: {})", names.join(", "))
        })?;
        let mut done = BTreeMap::new();
        if self.category.hosted() {
            done.insert(WALL.to_string(), Val::Number(wall));
        }
        for p in &self.parameters {
            self.resolve(&p.name, set, instance, &mut done, &mut Vec::new())?;
        }
        // Where its references are for the type: solved, else where each stands (the check says why it isn't solved).
        let _ = self.place_references(&mut done);
        Ok(done)
    }

    /// [`Family::values_in`] with `given` (a parent family's numbers for a nested part's parameters) over its types' and its
    /// instance values: what a nested part of it is sized by.
    pub fn values_given(&self, type_name: &str, given: &BTreeMap<String, f64>, wall: f64) -> Result<BTreeMap<String, Val>, String> {
        let mut types = self.types.clone();
        let set = types.get_mut(type_name).ok_or_else(|| format!("type `{type_name}` doesn't exist"))?;
        let mut instance = BTreeMap::new();
        for (k, x) in given {
            let Some(p) = self.parameters.iter().find(|p| p.name == *k && p.formula.is_none()) else { continue };
            let v = match p.kind {
                Kind::YesNo => Value::Bool(*x != 0.0),
                Kind::Length | Kind::Number | Kind::Angle => serde_json::json!(x),
                Kind::Material | Kind::Text => continue,
            };
            if p.instance {
                instance.insert(k.clone(), v);
            } else {
                set.insert(k.clone(), v);
            }
        }
        let set = set.clone();
        let mut done = BTreeMap::new();
        if self.category.hosted() {
            done.insert(WALL.to_string(), Val::Number(wall));
        }
        for p in &self.parameters {
            self.resolve(&p.name, &set, &instance, &mut done, &mut Vec::new())?;
        }
        let _ = self.place_references(&mut done);
        Ok(done)
    }

    /// Parameter `name`'s value, and those it takes from, into `done`; `path`, the formulas being
    /// worked out, catches one that comes back to itself.
    fn resolve(
        &self,
        name: &str,
        set: &BTreeMap<String, Value>,
        instance: &BTreeMap<String, Value>,
        done: &mut BTreeMap<String, Val>,
        path: &mut Vec<String>,
    ) -> Result<Val, String> {
        if let Some(v) = done.get(name) {
            return Ok(v.clone());
        }
        let p = self.parameters.iter().find(|p| p.name == name).ok_or_else(|| format!("`{name}` isn't one of its parameters"))?;
        if path.iter().any(|n| n == name) {
            return Err(format!("`{name}`'s formula comes back to itself"));
        }
        let v = match &p.formula {
            Some(_) if path.len() >= MAX_CHAIN => {
                return Err(format!("`{name}` takes from formulas taking from more than {MAX_CHAIN} others in a row: nested too deep"));
            }
            Some(f) => {
                path.push(name.to_string());
                let x = eval(f, &mut |n: &str| match self.resolve(n, set, instance, done, path)? {
                    Val::Number(x) => Ok(x),
                    Val::Text(_) => Err(format!("`{n}` is a material or text, not a number")),
                });
                path.pop();
                Val::Number(x.map_err(|e| format!("parameter `{name}`: {e}"))?)
            }
            None => read(p, if p.instance { instance.get(name) } else { set.get(name) }.unwrap_or(&p.value))?,
        };
        done.insert(name.to_string(), v.clone());
        Ok(v)
    }

    /// How IFC writes a component of it: [`Category::ifc`], with its kind's predefined type where it says one. A generic family
    /// that remembers the class it came in as writes that class, and its predefined type where IFC4 has it for the class (else
    /// none defined; nothing for a class with none); a proxy that names it, for a class IFC4 doesn't have.
    pub fn ifc(&self) -> (Cow<'static, str>, Cow<'static, str>, Option<Cow<'static, str>>) {
        if let Some(m) = self.remembered() {
            let object = m.object_type.clone().map(Cow::Owned);
            return match crate::ifc_classes::class(&m.class) {
                Some(c) if c.3.is_empty() => (c.0.into(), "".into(), object),
                Some(c) => {
                    let p = m.predefined.as_deref().filter(|p| crate::ifc_classes::takes(c, p)).unwrap_or("NOTDEFINED");
                    (c.0.into(), format!(".{p}.").into(), object)
                }
                None => ("IFCBUILDINGELEMENTPROXY".into(), ".USERDEFINED.".into(), Some(object.unwrap_or_else(|| m.class.clone().into()))),
            };
        }
        let (class, predefined, object) = self.category.ifc();
        let kind = self.kind.as_deref().and_then(|k| self.category.kinds().iter().find(|(n, _)| *n == k)).map(|(_, t)| *t);
        (class.into(), kind.unwrap_or(predefined).into(), object.map(Cow::Borrowed))
    }

    /// The IFC class it remembers, where it is a generic family: what its components go out as.
    pub fn remembered(&self) -> Option<&IfcClass> {
        self.ifc.as_ref().filter(|_| self.category == Category::Generic)
    }

    /// Whether it's sound: its parameters named once each, so formulas can say them, and holding
    /// what their kinds hold; its types setting only the type parameters no formula works out; its
    /// materials in the library (`has_material`); and every type building.
    pub fn check(&self, has_material: &dyn Fn(&str) -> bool) -> Result<(), String> {
        self.check_at(has_material, 0)
    }

    /// [`Family::check`] of a family held `depth` deep in another ([`Family::embedded`]).
    pub(crate) fn check_at(&self, has_material: &dyn Fn(&str) -> bool, depth: usize) -> Result<(), String> {
        for (what, n, most) in [
            ("parameters", self.parameters.len(), MAX_PARAMETERS),
            ("forms", self.forms.len(), MAX_FORMS),
            ("symbols", self.symbols.len(), MAX_SYMBOLS),
            ("connectors", self.connectors.len(), MAX_CONNECTORS),
            ("types", self.types.len(), MAX_TYPES),
        ] {
            if n > most {
                return Err(format!("it has more than {most} {what}"));
            }
        }
        if self.types.is_empty() {
            return Err("it needs a type: add one to `types`".into());
        }
        self.check_described()?;
        self.check_parametric()?;
        self.check_embedded(has_material, depth)?;
        if let Some(kind) = &self.kind {
            let kinds = self.category.kinds();
            if !kinds.iter().any(|(k, _)| k == kind) {
                let shown: String = kind.chars().take(40).collect();
                let names: Vec<&str> = kinds.iter().map(|(k, _)| *k).collect();
                return Err(if names.is_empty() {
                    format!("`kind` `{shown}`: a family of this category has no kinds; leave `kind` out")
                } else {
                    format!("`kind` is one of {}, not `{shown}`", names.join(", "))
                });
            }
        }
        if let Some(m) = &self.ifc {
            m.check()?;
        }
        if self.category.hosted() {
            for name in [WIDTH, HEIGHT] {
                if !self.parameters.iter().any(|p| p.name == name && p.kind == Kind::Length) {
                    return Err(format!("a door or window family needs a length parameter `{name}`: the size of the opening it makes"));
                }
            }
            if self.parameters.iter().any(|p| p.name == WALL) {
                return Err(format!("`{WALL}` is the wall's, which a door or a window reads; name the parameter otherwise"));
            }
        }
        let material_ok = |who: &str, m: &str| {
            if m.is_empty() || has_material(m) {
                Ok(())
            } else {
                Err(format!("{who}: material `{m}` isn't in the library; add it with `material`"))
            }
        };
        for sub in &self.subcategories {
            material_ok(&format!("subcategory `{}`", sub.name), sub.material.as_deref().unwrap_or_default())?;
        }
        let long = |f: &str| f.chars().count() > MAX_FORMULA;
        let mut names = BTreeSet::new();
        for p in &self.parameters {
            let who = format!("parameter `{}`", p.name);
            if !nameable(&p.name) {
                return Err(format!("{who}: a name starts with a letter and holds letters, digits, `_` and spaces, up to {MAX_TEXT}"));
            }
            if !names.insert(p.name.as_str()) {
                return Err(format!("{who} is named twice"));
            }
            for (what, v) in [("unit", &p.unit), ("group", &p.group)] {
                if let Some(v) = v
                    && !one_line(v, MAX_TEXT)
                {
                    return Err(format!("{who}: its `{what}` is text of 1 to {MAX_TEXT} characters on one line"));
                }
            }
            match (&p.formula, p.kind) {
                (Some(_), Kind::Material) => return Err(format!("{who}: a material takes no formula")),
                (Some(_), Kind::Text) => return Err(format!("{who}: a text takes no formula")),
                (Some(f), _) if long(f) => return Err(format!("{who}: its formula is longer than {MAX_FORMULA} characters")),
                _ => {}
            }
            match read(p, &p.value)? {
                Val::Text(m) if p.kind == Kind::Material => material_ok(&who, &m)?,
                Val::Text(t) if t.chars().count() > MAX_WORDS => {
                    return Err(format!("{who}: its text is longer than {MAX_WORDS} characters"));
                }
                _ => {}
            }
        }
        for (t, set) in &self.types {
            let who = format!("type `{t}`");
            if t.trim().is_empty() || t.chars().count() > MAX_TEXT {
                return Err(format!("{who}: a type needs a name, up to {MAX_TEXT} characters"));
            }
            for (k, v) in set {
                let p = self.parameters.iter().find(|p| p.name == *k).ok_or_else(|| format!("{who}: `{k}` isn't one of its parameters"))?;
                if p.instance {
                    return Err(format!("{who}: `{k}` is an instance parameter; each component sets its own"));
                }
                if p.formula.is_some() {
                    return Err(format!("{who}: `{k}` is worked out by its formula"));
                }
                match read(p, v)? {
                    Val::Text(m) if p.kind == Kind::Material => material_ok(&who, &m)?,
                    Val::Text(t) if t.chars().count() > MAX_WORDS => {
                        return Err(format!("{who}: `{k}`'s text is longer than {MAX_WORDS} characters"));
                    }
                    _ => {}
                }
            }
        }
        for (i, form) in self.forms.iter().enumerate() {
            let who = format!("form {}", i + 1);
            let ways = usize::from(!form.profile.is_empty())
                + usize::from(form.circle.is_some())
                + usize::from(!form.mesh.is_empty())
                + usize::from(!form.path.is_empty());
            if ways != 1 {
                return Err(format!("{who}: it takes a `profile`, a `circle`, a `mesh` or a `path`, one of them"));
            }
            if form.path.is_empty() && (!form.section.is_empty() || !form.section_end.is_empty()) {
                return Err(format!("{who}: a `section` is a sweep's profile, along its `path`"));
            }
            if (!form.top.is_empty() || !form.holes.is_empty()) && form.profile.is_empty() {
                return Err(format!("{who}: a `top` or `holes` go with a `profile`"));
            }
            if form.axis.is_some() && !form.mesh.is_empty() {
                return Err(format!("{who}: a `mesh` is as it is; an `axis` turns a `profile` or a `circle`"));
            }
            if !self.parameters.iter().any(|p| p.name == form.material && p.kind == Kind::Material) {
                material_ok(&who, &form.material)?;
            }
            self.check_part(&who, &form.types, form.subcategory.as_deref())?;
            if let Some(cuts) = &form.cuts
                && (!form.void || cuts.len() > MAX_FORMS || cuts.iter().any(|c| c.chars().count() > MAX_TEXT))
            {
                return Err(format!("{who}: `cuts` is a void's, up to {MAX_FORMS} names of up to {MAX_TEXT} characters"));
            }
            if form.source.as_ref().is_some_and(|s| s.chars().count() > MAX_TEXT) {
                return Err(format!("{who}: its `source` is longer than {MAX_TEXT} characters"));
            }
            if form.exprs().into_iter().any(|e| matches!(e, Expr::Formula(f) if long(f))) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
        }
        for (i, y) in self.symbols.iter().enumerate() {
            self.check_part(&format!("symbol {}", i + 1), &y.types, y.subcategory.as_deref())?;
            if y.view != SymbolView::Plan && (y.text.is_some() || y.dot.is_some()) {
                return Err(format!("symbol {}: a `front` or `side` symbol is a `line` or an `arc`, as Revit's symbolic lines are", i + 1));
            }
            if let Some(c) = &y.arc
                && c.start.is_some() != c.end.is_some()
            {
                return Err(format!("symbol {}: an arc takes both `start` and `end`, or neither", i + 1));
            }
            let Some(label) = &y.text else { continue };
            for name in fields(&label.value).map_err(|e| format!("symbol {}: {e}", i + 1))? {
                if !self.parameters.iter().any(|p| p.name == name) {
                    return Err(format!("symbol {}: its text reads `{{{name}}}`, which isn't one of the family's parameters", i + 1));
                }
            }
        }
        let symbol_exprs = self.symbols.iter().flat_map(|y| {
            let label = y.text.iter().flat_map(|l| l.at.iter().chain([&l.height]));
            let dot = y.dot.iter().flat_map(|c| c.centre.iter().chain([&c.radius]));
            y.line.iter().flatten().chain(&y.visible).chain(y.arc.iter().flat_map(Curve::exprs)).chain(label).chain(dot)
        });
        if symbol_exprs.into_iter().any(|e| matches!(e, Expr::Formula(f) if long(f))) {
            return Err(format!("a formula of a symbol is longer than {MAX_FORMULA} characters"));
        }
        self.check_connectors()?;
        if let Some(cut) = &self.cut {
            if !self.category.hosted() {
                return Err("a `cut` is a door's or a window's: the opening it makes in its wall".into());
            }
            if [&cut.width, &cut.height].into_iter().any(|e| matches!(e, Expr::Formula(f) if long(f))) {
                return Err(format!("its `cut`: a formula of it is longer than {MAX_FORMULA} characters"));
            }
        }
        // Each formula read once across the types; past what a family may build, refused.
        let (mut read, mut built, mut cuts) = (HashMap::new(), 0, csg::Meter::default());
        for t in self.types.keys() {
            let pieces = self
                .values(t, &BTreeMap::new())
                .and_then(|v| {
                    self.all_marks(t, &v)?;
                    self.cut_size(&v)?;
                    self.build(
                        t,
                        &v,
                        &mut read,
                        &mut cuts,
                        Build { snapshot: self.draws_snapshot_forms(t), others: &|_| None, depth: 0, spent: &std::cell::Cell::new(0) },
                    )
                })
                .map_err(|e| format!("type `{t}`: {e}"))?;
            built += pieces.iter().map(|p| p.tris.len()).sum::<usize>();
            if built > MAX_BUILT {
                return Err(format!("its types build more than {MAX_BUILT} triangles together"));
            }
        }
        Ok(())
    }

    /// Whether its connectors are sound: at most one `power`; each kind with its system, its poles and its numbers
    /// in range, for every type.
    fn check_connectors(&self) -> Result<(), String> {
        if self.connectors.iter().filter(|c| c.kind == ConnectorKind::Power).count() > 1 {
            return Err("it has two power connectors: keep one, a device draws its load through one".into());
        }
        let long = |e: &Expr| matches!(e, Expr::Formula(f) if f.chars().count() > MAX_FORMULA);
        for (i, c) in self.connectors.iter().enumerate() {
            let who = format!("connector {}", i + 1);
            if c.classification.chars().count() > MAX_TEXT || c.name.chars().count() > MAX_TEXT {
                return Err(format!("{who}: its name or classification is longer than {MAX_TEXT} characters"));
            }
            let placed = c.at.iter().flatten().chain(c.direction.iter().flatten()).chain(c.size.iter());
            if placed.into_iter().any(long) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
            if c.kind != ConnectorKind::Pipe && c.domain.is_some() {
                return Err(format!("{who}: a `domain` is a pipe connector's: cold, hot, waste, vent, storm or gas"));
            }
            if c.kind.electrical() {
                if c.flow != Flow::Both {
                    return Err(format!("{who}: a `flow` is a pipe's, data's or earthing's connector's, not an electrical device's"));
                }
                if c.size.is_some() {
                    return Err(format!("{who}: a `size` is a pipe's, data's or earthing's connector's, not an electrical device's"));
                }
            } else {
                if c.system.is_some() {
                    return Err(format!("{who}: a `system` is a power or a switch connector's, not a {:?} one's", c.kind).to_lowercase());
                }
                if c.at.is_none() {
                    return Err(format!("{who}: a pipe, data or earthing connector needs `at`, where it is in the family's axes"));
                }
                if !matches!(&c.load_va, Load::One(Expr::Number(x)) if *x == 0.0) {
                    return Err(format!("{who}: only a power connector carries a load; leave `load_va` out"));
                }
            }
            if c.kind == ConnectorKind::Pipe {
                // A fitting's or a valve's takes the domain of the pipe it joins, as its system is the pipe's.
                if c.domain.is_none() && !matches!(self.category, Category::PipeFitting | Category::PipeAccessory) {
                    return Err(format!(
                        "{who}: a pipe connector needs its `domain`: cold, hot, waste, vent, storm or gas (only a pipe fitting's or accessory's takes its pipe's)"
                    ));
                }
                if c.direction.is_none() || c.size.is_none() {
                    return Err(format!(
                        "{who}: a pipe connector needs its `direction` (the way the pipe leaves it) and its `size` (DN, mm)"
                    ));
                }
            }
            let loads: &[Expr] = match &c.load_va {
                Load::One(e) => std::slice::from_ref(e),
                Load::PerPhase(v) => v,
            };
            if [&c.voltage, &c.power_factor].into_iter().chain(loads).any(long) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
            match (c.kind, c.system) {
                (ConnectorKind::Power, Some(ConnectorSystem::PowerBalanced | ConnectorSystem::PowerUnbalanced))
                | (ConnectorKind::Switch, Some(ConnectorSystem::Switch)) => {}
                (ConnectorKind::Power, _) => {
                    return Err(format!("{who}: a power connector's system is `power-balanced` or `power-unbalanced`"));
                }
                (ConnectorKind::Switch, _) => {
                    return Err(format!("{who}: a switch connector's system is `switch`"));
                }
                _ => {}
            }
            if !(1..=3).contains(&c.poles) {
                return Err(format!("{who}: poles are 1, 2 or 3, not {}", c.poles));
            }
            if let Load::PerPhase(v) = &c.load_va
                && (c.system != Some(ConnectorSystem::PowerUnbalanced) || v.len() != usize::from(c.poles))
            {
                return Err(format!(
                    "{who}: a load per phase is a list of one for each pole ({}) on a `power-unbalanced` connector; a balanced one takes a single load",
                    c.poles
                ));
            }
        }
        for t in self.types.keys() {
            self.connector_values(t, &BTreeMap::new()).map_err(|e| format!("type `{t}`: {e}"))?;
        }
        Ok(())
    }

    /// Whether what it says of itself is sound: its `source`'s fields text on one line; its subcategories named once each,
    /// their materials in the library is [`Family::check`]'s to say.
    fn check_described(&self) -> Result<(), String> {
        if let Some(src) = &self.source {
            for (what, v) in [("library", &src.library), ("app", &src.app), ("category", &src.category), ("file", &src.file)] {
                if let Some(v) = v
                    && !one_line(v, MAX_WORDS)
                {
                    return Err(format!("`source` `{what}`: text of 1 to {MAX_WORDS} characters on one line"));
                }
            }
        }
        let mut names = BTreeSet::new();
        for (i, sub) in self.subcategories.iter().enumerate() {
            if !one_line(&sub.name, MAX_TEXT) {
                return Err(format!("subcategory {}: a name of 1 to {MAX_TEXT} characters on one line", i + 1));
            }
            if !names.insert(sub.name.as_str()) {
                return Err(format!("subcategory `{}` is named twice", sub.name));
            }
            if let Some(m) = &sub.material
                && !one_line(m, MAX_TEXT)
            {
                return Err(format!("subcategory `{}`: its material is a name of 1 to {MAX_TEXT} characters", sub.name));
            }
        }
        Ok(())
    }

    /// Whether a form's or a symbol's (`who`) types are the family's, and its subcategory one of its subcategories.
    fn check_part(&self, who: &str, types: &[String], subcategory: Option<&str>) -> Result<(), String> {
        if let Some(t) = types.iter().find(|t| !self.types.contains_key(*t)) {
            let shown: String = t.chars().take(40).collect();
            return Err(format!("{who}: its `types` name `{shown}`, which isn't one of the family's types"));
        }
        if let Some(sub) = subcategory
            && !self.subcategories.iter().any(|s| s.name == sub)
        {
            let shown: String = sub.chars().take(40).collect();
            return Err(format!("{who}: subcategory `{shown}` isn't one of the family's `subcategories`"));
        }
        Ok(())
    }

    /// The width and height of the opening a door or a window of these `values` cuts in its wall: its `cut`'s, else its
    /// `Width` and `Height`; none where it has neither.
    pub fn cut_size(&self, values: &BTreeMap<String, Val>) -> Result<Option<(f64, f64)>, String> {
        let Some(cut) = &self.cut else { return Ok(None) };
        let mut read = HashMap::new();
        let said = |e: String| format!("its `cut`: {e}");
        let w = number(&cut.width, values, &mut read).map_err(said)?;
        let h = number(&cut.height, values, &mut read).map_err(said)?;
        if w <= 0.0 || h <= 0.0 {
            return Err(format!("its `cut` is {w} by {h} mm; an opening needs a width and a height above zero"));
        }
        Ok(Some((w, h)))
    }

    /// Its connectors, worked out for type `type_name` and a component's `instance` values.
    pub fn connector_values(&self, type_name: &str, instance: &BTreeMap<String, Value>) -> Result<Vec<ConnectorValues>, String> {
        if self.connectors.is_empty() {
            return Ok(Vec::new());
        }
        let values = self.values(type_name, instance)?;
        let mut read = HashMap::new();
        let mut out = Vec::new();
        for (i, c) in self.connectors.iter().enumerate() {
            let who = format!("connector {}", i + 1);
            let said = |what: &str, e: String| format!("{who}: its {what}: {e}");
            let voltage = number(&c.voltage, &values, &mut read).map_err(|e| said("voltage", e))?;
            let power_factor = number(&c.power_factor, &values, &mut read).map_err(|e| said("power factor", e))?;
            let by_phase = match &c.load_va {
                Load::One(e) => {
                    let all = number(e, &values, &mut read).map_err(|e| said("load", e))?;
                    if c.kind == ConnectorKind::Switch {
                        if all != 0.0 {
                            return Err(format!("{who}: a switch carries no load; leave `load_va` out"));
                        }
                        Vec::new()
                    } else {
                        vec![all / f64::from(c.poles.max(1)); usize::from(c.poles)]
                    }
                }
                Load::PerPhase(v) => {
                    v.iter().map(|e| number(e, &values, &mut read).map_err(|e| said("load", e))).collect::<Result<_, _>>()?
                }
            };
            if c.kind == ConnectorKind::Power {
                if voltage <= 0.0 || voltage > MAX_VOLTAGE {
                    return Err(format!("{who}: its voltage is {voltage} V; it takes more than 0 and up to {MAX_VOLTAGE}"));
                }
                if power_factor <= 0.0 || power_factor > 1.0 {
                    return Err(format!("{who}: its power factor is {power_factor}; it takes more than 0 and up to 1"));
                }
                if let Some(x) = by_phase.iter().find(|x| !(0.0..=MAX_LOAD_VA).contains(*x)) {
                    return Err(format!("{who}: its load is {x} VA; it takes 0 up to {MAX_LOAD_VA}"));
                }
            }
            let at = match &c.at {
                None => None,
                Some(v) => Some(point(v, &values, &mut read).map_err(|e| said("place", e))?),
            };
            let direction = match &c.direction {
                None => None,
                Some(v) => {
                    let d = point(v, &values, &mut read).map_err(|e| said("direction", e))?;
                    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if len < 1e-9 {
                        return Err(format!("{who}: its direction has no length: give the way it faces"));
                    }
                    Some(d.map(|x| x / len))
                }
            };
            let size = match &c.size {
                None => None,
                Some(e) => {
                    let x = number(e, &values, &mut read).map_err(|e| said("size", e))?;
                    if x <= 0.0 || x > MAX_CONNECTOR_SIZE {
                        return Err(format!("{who}: its size is {x} mm; it takes more than 0 and up to {MAX_CONNECTOR_SIZE}"));
                    }
                    Some(x)
                }
            };
            out.push(ConnectorValues {
                id: c.id,
                guid: c.guid,
                name: c.name.clone(),
                kind: c.kind,
                system: c.system,
                domain: c.domain,
                flow: c.flow,
                at,
                direction,
                size,
                poles: c.poles,
                voltage,
                load_va: by_phase.iter().sum(),
                load_va_by_phase: by_phase,
                power_factor,
                classification: c.classification.clone(),
            });
        }
        Ok(out)
    }

    /// Its visible forms' solids in type `type_name` (those of no type, and those of it), sized by `values`, less what its
    /// voids cut.
    pub fn pieces(&self, type_name: &str, values: &BTreeMap<String, Val>) -> Result<Vec<Piece>, String> {
        self.pieces_with(type_name, values, &|_| None)
    }

    /// [`Family::pieces`] with its nested parts, the families they are of found by `others` (the project's).
    pub fn pieces_with(&self, type_name: &str, values: &BTreeMap<String, Val>, others: Others) -> Result<Vec<Piece>, String> {
        let snapshot = self.draws_snapshot_forms(type_name);
        self.build(
            type_name,
            values,
            &mut HashMap::new(),
            &mut csg::Meter::default(),
            Build { snapshot, others, depth: 0, spent: &std::cell::Cell::new(0) },
        )
    }

    /// The marks of its visible symbols in `view` and type `type_name`, sized by `values`, in its own axes, drawn at detail
    /// level `detail` ([`Symbol::drawn_at`]; none, at any): an arc as a polyline of short chords, a label as its anchor
    /// (lettered by [`lettering`] where it is placed), a dot as it is. A `plan` mark's points are (x, y), a `front` one's
    /// (x, z) and a `side` one's (y, z).
    pub fn marks(
        &self,
        type_name: &str,
        values: &BTreeMap<String, Val>,
        view: SymbolView,
        detail: Option<crate::graphics::Detail>,
    ) -> Result<Vec<Mark>, String> {
        self.marks_as(type_name, values, view, detail, self.draws_snapshot(type_name))
    }

    /// [`Family::marks`] of its `snapshot` symbols (Revit's result) or of its others (its definition's), as `snapshot` says.
    fn marks_as(
        &self,
        type_name: &str,
        values: &BTreeMap<String, Val>,
        view: SymbolView,
        detail: Option<crate::graphics::Detail>,
        snapshot: bool,
    ) -> Result<Vec<Mark>, String> {
        self.marks_where(values, &|y: &Symbol| {
            y.view == view && y.of_type(type_name) && y.snapshot == snapshot && detail.is_none_or(|d| y.drawn_at(d))
        })
    }

    /// The marks of all its symbols type `type_name` draws, whatever view and detail level draws them: what checks them.
    pub fn all_marks(&self, type_name: &str, values: &BTreeMap<String, Val>) -> Result<Vec<Mark>, String> {
        let which = self.draws_snapshot(type_name);
        self.marks_where(values, &|y: &Symbol| y.of_type(type_name) && y.snapshot == which)
    }

    /// [`Family::marks`] with its nested parts' own symbols that lie in `view` placed where they stand (in a type drawn from
    /// its definition: Revit's result has them already), the families they are of its own ([`Family::embedded`]) or `others`.
    pub fn marks_with(
        &self,
        type_name: &str,
        values: &BTreeMap<String, Val>,
        view: SymbolView,
        detail: Option<crate::graphics::Detail>,
        others: Others,
    ) -> Result<Vec<Mark>, String> {
        let mut out = self.marks(type_name, values, view, detail)?;
        if !self.draws_snapshot(type_name) {
            self.nested_marks(type_name, values, (view, detail), others, (0, &std::cell::Cell::new(0)), &mut out)?;
        }
        Ok(out)
    }

    /// Its nested parts' marks in `view` in type `type_name`, in its axes, onto `out`: each of the part's own symbols whose
    /// view faces the way this one does (a part's plan symbols in a parent's plan where it stands upright, and in the
    /// parent's front where it is turned onto a face), there, and drawn at `detail`.
    pub(crate) fn nested_marks(
        &self,
        type_name: &str,
        values: &BTreeMap<String, Val>,
        (view, detail): (SymbolView, Option<crate::graphics::Detail>),
        others: Others,
        (depth, spent): (usize, &std::cell::Cell<usize>),
        out: &mut Vec<Mark>,
    ) -> Result<(), String> {
        if self.nested.is_empty() {
            return Ok(());
        }
        if depth >= MAX_NESTING {
            return Err(format!("nested more than {MAX_NESTING} deep"));
        }
        let mut read = HashMap::new();
        for (i, n) in self.nested.iter().enumerate().filter(|(_, n)| n.of_type(type_name)) {
            let said = |e: String| format!("nested part {} ({}): {e}", i + 1, n.family);
            let seen = |s: Show| match view {
                SymbolView::Plan => s.plan,
                SymbolView::Front => s.front,
                SymbolView::Side => s.side,
            };
            if n.show.is_some_and(|s| !seen(s) || detail.is_some_and(|d| !s.at(d))) {
                continue;
            }
            if let Some(v) = &n.visible
                && number(v, values, &mut read).map_err(said)? == 0.0
            {
                continue;
            }
            let Some(inner) = self.nested_family(&n.family, others) else { continue };
            spent.set(spent.get() + 1);
            if spent.get() > MAX_NESTED {
                return Err(format!("it draws more than {MAX_NESTED} nested parts in all"));
            }
            let (at, axes) = nested_frame(n, values, &mut read).map_err(said)?;
            let mut given = BTreeMap::new();
            for (k, e) in &n.params {
                given.insert(k.clone(), number(e, values, &mut read).map_err(said)?);
            }
            let wall = match values.get(WALL) {
                Some(Val::Number(x)) => *x,
                _ => DEFAULT_WALL,
            };
            let inner_type = n.type_for(type_name);
            let inner_values = inner.values_given(inner_type, &given, wall).map_err(said)?;
            let revit = inner.draws_revit_result(inner_type, &inner_values, wall);
            let scale = nested_scale(n, values, &mut read).map_err(said)?;
            // The way each of its views faces in this family's axes, and which of this one's: a view of the part is drawn
            // where it is square to the view being drawn, however it is turned.
            let faces = view.normal();
            for inner_view in [SymbolView::Plan, SymbolView::Front, SymbolView::Side] {
                if crate::geom::v3::dot(axes[inner_view.axis()], faces).abs() < 1.0 - 1e-6 {
                    continue;
                }
                let mut marks = inner.marks_as(inner_type, &inner_values, inner_view, detail, revit).map_err(said)?;
                if !revit {
                    inner
                        .nested_marks(inner_type, &inner_values, (inner_view, detail), others, (depth + 1, spent), &mut marks)
                        .map_err(said)?;
                }
                let place = |q: P2| {
                    let p = inner_view.lift(q);
                    let p = add(at, add(add(mul(axes[0], p[0]), mul(axes[1], p[1])), mul(axes[2], p[2])));
                    view.flat(p)
                };
                out.extend(marks.iter().map(|m| m.scaled(scale).map(place)));
            }
            if out.len() > MAX_SYMBOLS * 4 {
                return Err(said(format!("its nested parts draw more than {} marks", MAX_SYMBOLS * 4)));
            }
        }
        Ok(())
    }

    /// The marks of its visible symbols `pick` keeps, sized by `values`, each worked out and checked.
    fn marks_where<'a>(&'a self, values: &BTreeMap<String, Val>, pick: &dyn Fn(&Symbol) -> bool) -> Result<Vec<Mark>, String> {
        let mut read = HashMap::new();
        let mut out = Vec::new();
        for (i, sym) in self.symbols.iter().enumerate() {
            if !pick(sym) {
                continue;
            }
            let said = |e: String| format!("symbol {}: {e}", i + 1);
            let ways = usize::from(!sym.line.is_empty())
                + usize::from(sym.arc.is_some())
                + usize::from(sym.text.is_some())
                + usize::from(sym.dot.is_some());
            if ways != 1 || sym.line.len() > MAX_SYMBOL_POINTS {
                return Err(said(format!(
                    "it takes a `line` of up to {MAX_SYMBOL_POINTS} points, an `arc`, a `text` or a `dot`, one of them"
                )));
            }
            if let Some(v) = &sym.visible
                && number(v, values, &mut read).map_err(said)? == 0.0
            {
                continue;
            }
            let mut n = |e: &'a Expr| number(e, values, &mut read).map_err(said);
            if let Some(c) = &sym.dot {
                let (centre, r) = (P2(n(&c.centre[0])?, n(&c.centre[1])?), n(&c.radius)?);
                if r <= 0.0 {
                    return Err(said(format!("its dot has radius {r}; it needs a radius above zero")));
                }
                out.push(Mark::Dot { centre, radius: r });
                continue;
            }
            if let Some(l) = &sym.text {
                let (at, height) = (P2(n(&l.at[0])?, n(&l.at[1])?), n(&l.height)?);
                if height <= 0.0 {
                    return Err(said(format!("its text has height {height}; it needs a height above zero")));
                }
                let text = said_text(&l.value, values).map_err(said)?;
                out.push(Mark::Label { at, height, text, align: l.align });
                continue;
            }
            let mut pts = Vec::new();
            for [a, b] in &sym.line {
                pts.push(P2(n(a)?, n(b)?));
            }
            if let Some(c) = &sym.arc {
                let Sweep { centre, radius: r, from, to } = c.sweep(values, &mut read).map_err(said)?;
                if r <= 0.0 || (to - from).abs() > 360.0 + 1e-9 || to == from {
                    return Err(said(format!(
                        "its arc has radius {r} and runs from {from} to {to} degrees; it needs a radius above zero and up to a full turn"
                    )));
                }
                let steps = ((to - from).abs() / ARC_STEP).ceil().max(1.0) as usize;
                pts.extend((0..=steps).map(|k| {
                    let (s, c) = (from + (to - from) * k as f64 / steps as f64).to_radians().sin_cos();
                    P2(centre.0 + r * c, centre.1 + r * s)
                }));
            }
            out.push(Mark::Line(pts));
        }
        Ok(out)
    }

    /// Its visible forms' solids, sized by `values`, less what its voids cut; `read`, its formulas as
    /// read already, and `cuts`, what the cutting may still cost.
    fn build<'a>(
        &'a self,
        type_name: &str,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
        cuts: &mut csg::Meter,
        how: Build,
    ) -> Result<Vec<Piece>, String> {
        let (mut out, mut voids) = (Vec::new(), Vec::new());
        for i in 0..self.forms.len() {
            if !self.forms[i].of_type(type_name) || self.forms[i].snapshot != how.snapshot {
                continue;
            }
            let what = format!("form {}", i + 1);
            let Some(piece) = self.solid(i, values, read).map_err(|e| format!("{what}: {e}"))? else {
                continue;
            };
            if self.forms[i].void {
                voids.push((i, out.len(), piece.tris));
            } else {
                out.push(piece);
            }
        }
        // Each void cut from every solid listed before it that it reaches (Revit's `cuts` lists don't limit it: they leave out forms
        // its void cuts and name some it doesn't). A cut solid is no longer one round surface,
        // and its outline seen from above is as it was. ponytail: a notch or a hole doesn't show in plan; trace it if it must.
        for (i, before, void) in &voids {
            for piece in out.iter_mut().take(*before) {
                let tris = csg::subtract(&piece.tris, void, cuts).map_err(|e| format!("form {}: {e}", i + 1))?;
                if tris != piece.tris {
                    let (bottom, top) = heights(&tris);
                    *piece = Piece {
                        caps: tris.len(),
                        tris,
                        bottom,
                        top,
                        round: false,
                        outline: std::mem::take(&mut piece.outline),
                        material: std::mem::take(&mut piece.material),
                        show: piece.show,
                    };
                }
            }
        }
        // Its nested parts: Revit's result for a type has them in its own forms already.
        if !how.snapshot {
            for (i, n) in self.nested.iter().enumerate().filter(|(_, n)| n.of_type(type_name)) {
                let said = |e: String| format!("nested part {} ({}): {e}", i + 1, n.family);
                out.extend(self.nested_pieces(n, n.type_for(type_name), values, read, cuts, how).map_err(said)?);
            }
        }
        out.retain(|p| !p.tris.is_empty());
        Ok(out)
    }

    /// Nested part `n`'s solids in this family's axes, sized by this family's `values`: none where it isn't there or the
    /// project has no family of its name.
    fn nested_pieces<'a>(
        &'a self,
        n: &'a Nested,
        inner_type: &str,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
        cuts: &mut csg::Meter,
        how: Build,
    ) -> Result<Vec<Piece>, String> {
        if let Some(v) = &n.visible
            && number(v, values, read)? == 0.0
        {
            return Ok(Vec::new());
        }
        let Some(inner) = self.nested_family(&n.family, how.others) else { return Ok(Vec::new()) };
        if how.depth >= MAX_NESTING {
            return Err(format!("nested more than {MAX_NESTING} deep"));
        }
        let mut given = BTreeMap::new();
        for (k, e) in &n.params {
            given.insert(k.clone(), number(e, values, read)?);
        }
        let wall = match values.get(WALL) {
            Some(Val::Number(x)) => *x,
            _ => DEFAULT_WALL,
        };
        let inner_values = inner.values_given(inner_type, &given, wall)?;
        how.spent.set(how.spent.get() + 1);
        if how.spent.get() > MAX_NESTED {
            return Err(format!("it builds more than {MAX_NESTED} nested parts in all"));
        }
        let deeper = Build {
            snapshot: inner.draws_revit_forms(inner_type, &inner_values, wall),
            others: how.others,
            depth: how.depth + 1,
            spent: how.spent,
        };
        let pieces = inner.build(inner_type, &inner_values, &mut HashMap::new(), cuts, deeper)?;
        let (at, axes) = nested_frame(n, values, read)?;
        let det = crate::geom::v3::dot(axes[0], cross(axes[1], axes[2]));
        let mirrored = det < 0.0;
        let scale = nested_scale(n, values, read)?;
        let place = |p: P3| add(at, add(add(mul(axes[0], p[0] * scale), mul(axes[1], p[1] * scale)), mul(axes[2], p[2] * scale)));
        let plan = |q: &P2| {
            let p = place([q.0, q.1, 0.0]);
            P2(p[0], p[1])
        };
        Ok(pieces
            .into_iter()
            .map(|pc| {
                let tris = pc
                    .tris
                    .iter()
                    .map(|[a, b, c]| if mirrored { [place(*a), place(*c), place(*b)] } else { [place(*a), place(*b), place(*c)] })
                    .collect::<Vec<_>>();
                // Upright, its outline from above is its own placed; turned onto a face, the hull of what it comes to.
                let upright = axes[2] == [0.0, 0.0, 1.0];
                let mut outline: Vec<P2> = if upright {
                    pc.outline.iter().map(plan).collect()
                } else {
                    crate::geom::hull(tris.iter().flatten().map(|p: &P3| P2(p[0], p[1])).collect())
                };
                if mirrored && upright {
                    outline.reverse();
                }
                let (bottom, top) = heights(&tris);
                // Drawn where both its nested part's settings and its own form's say.
                let show = match (n.show, pc.show) {
                    (Some(a), Some(b)) => Some(a.and(b)),
                    (a, b) => a.or(b),
                };
                Piece { tris, outline, bottom, top, show, ..pc }
            })
            .collect())
    }

    /// Form `i`'s own solid, sized by `values`, before any void cuts it: `None` where its `visible`
    /// says it isn't there. `read`, its formulas as read already.
    fn solid<'a>(
        &'a self,
        i: usize,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<Option<Piece>, String> {
        let form = &self.forms[i];
        let mut num = |e: &'a Expr| number(e, values, read);
        if let Some(v) = &form.visible
            && num(v)? == 0.0
        {
            return Ok(None);
        }
        let material = match values.get(&form.material) {
            Some(Val::Text(m)) => m.clone(),
            _ => form.material.clone(),
        };
        let piece = if !form.mesh.is_empty() {
            mesh(&form.mesh, &form.outline, material)?
        } else if !form.path.is_empty() {
            let holes = form.section_holes.iter().chain(&form.section_end_holes);
            if form.path.len() > MAX_RING
                || form.section.len() > MAX_RING
                || form.section_end.len() > MAX_RING
                || form.section_holes.len() + form.section_end_holes.len() > MAX_RING
                || holes.clone().any(|h| h.len() > MAX_RING)
            {
                return Err(format!("its path or its profile has more than {MAX_RING} points"));
            }
            if !form.section_end_holes.is_empty() && form.section_end_holes.len() != form.section_holes.len() {
                return Err("a swept blend has as many loops inside its profile at its end as at its start".into());
            }
            let mut pts =
                |v: &'a [[Expr; 3]]| -> Result<Vec<P3>, String> { v.iter().map(|[a, b, c]| Ok([num(a)?, num(b)?, num(c)?])).collect() };
            let (path, section, end) = (pts(&form.path)?, pts(&form.section)?, pts(&form.section_end)?);
            let mut inner = Vec::new();
            for (k, h) in form.section_holes.iter().enumerate() {
                inner.push((k, pts(h)?, form.section_end_holes.get(k).map(|e| pts(e)).transpose()?.unwrap_or_default()));
            }
            let mut bends = |ring: Ring, hole: usize| -> Result<Vec<(usize, P3, P3, bool)>, String> {
                let mut out = Vec::new();
                for b in form.arcs.iter().filter(|b| b.ring == ring && b.hole == hole) {
                    let [x, y, z] = b.centre.as_slice() else { return Err("an arc of a sweep has a centre of three numbers".into()) };
                    out.push((b.edge, [num(x)?, num(y)?, num(z)?], b.normal.unwrap_or([0.0, 0.0, 1.0]), b.clockwise));
                }
                Ok(out)
            };
            let closed = path.len() > 2 && path.first() == path.last();
            let path = shapes::bent3(path, &bends(Ring::Path, 0)?, closed);
            let section = shapes::bent3(section, &bends(Ring::Section, 0)?, true);
            let end = shapes::bent3(end, &bends(Ring::SectionEnd, 0)?, true);
            let mut holes = Vec::new();
            for (k, s, e) in inner {
                let s = shapes::bent3(s, &bends(Ring::Section, k + 1)?, true);
                let e = shapes::bent3(e, &bends(Ring::SectionEnd, k + 1)?, true);
                if s.len() > MAX_RING * 4 || e.len() > MAX_RING * 4 {
                    return Err(format!("its arcs come to more than {} points", MAX_RING * 4));
                }
                holes.push((s, e));
            }
            if section.len() > MAX_RING * 4 || path.len() > MAX_RING * 4 {
                return Err(format!("its arcs come to more than {} points", MAX_RING * 4));
            }
            // Counted before it is built: a path and a profile each of thousands of points would make tens of millions.
            let around = section.len().max(end.len()) + holes.iter().map(|(s, e)| s.len().max(e.len())).sum::<usize>();
            too_fine(path.len().saturating_mul(around).saturating_mul(2).saturating_add(4 * section.len()), "sweep")?;
            hulled(shapes::along_holed(&path, &section, &end, &holes)?, material)
        } else {
            if !form.outline.is_empty() {
                return Err("an `outline` is a `mesh`'s: a `profile` or a `circle` has its own".into());
            }
            let ring: Vec<P2> = match &form.circle {
                Some(c) => {
                    let (x, y, r) = (num(&c.centre[0])?, num(&c.centre[1])?, num(&c.radius)?);
                    if r <= 0.0 {
                        return Err(format!("its circle's radius is {r}; it needs one above zero"));
                    }
                    (0..SEGMENTS)
                        .map(|k| (k as f64 * std::f64::consts::TAU / SEGMENTS as f64).sin_cos())
                        .map(|(s, c)| P2(x + r * c, y + r * s))
                        .collect()
                }
                None if form.profile.len() > MAX_RING => return Err(format!("its profile has more than {MAX_RING} corners")),
                None => form.profile.iter().map(|[a, b]| Ok(P2(num(a)?, num(b)?))).collect::<Result<_, String>>()?,
            };
            // Its true arcs, drawn as a circle is.
            let mut bends = |ring: Ring, hole: usize| -> Result<Vec<(usize, P2, bool)>, String> {
                let corners: &[[Expr; 2]] = match ring {
                    Ring::Top => &form.top,
                    Ring::Hole => form.holes.get(hole).map_or(&[], Vec::as_slice),
                    _ => &form.profile,
                };
                let mut out = Vec::new();
                for b in form.arcs.iter().filter(|b| b.ring == ring && b.hole == hole) {
                    let [x, y] = b.centre.as_slice() else { return Err("an arc of a profile has a centre of two numbers".into()) };
                    let mut centre = P2(num(x)?, num(y)?);
                    // Through a point between its corners: the circle through the three.
                    if let ([tx, ty], Some(a), Some(c)) =
                        (b.through.as_slice(), corners.get(b.edge), corners.get((b.edge + 1) % corners.len().max(1)))
                    {
                        let (a, m, c) = (P2(num(&a[0])?, num(&a[1])?), P2(num(tx)?, num(ty)?), P2(num(&c[0])?, num(&c[1])?));
                        if let Some(o) = circumcentre(a, m, c) {
                            centre = o;
                        }
                    }
                    out.push((b.edge, centre, b.clockwise));
                }
                Ok(out)
            };
            let (profile_bends, top_bends) = (bends(Ring::Profile, 0)?, bends(Ring::Top, 0)?);
            let hole_bends = (0..form.holes.len()).map(|k| bends(Ring::Hole, k)).collect::<Result<Vec<_>, _>>()?;
            let ring = if form.circle.is_none() { shapes::bent2(ring, &profile_bends) } else { ring };
            let (from, to) = (num(&form.from)?, num(&form.to)?);
            if ring.len() < 3 || crate::geom::area(&ring).abs() < 1.0 {
                return Err("its profile has no area: it needs three corners or more, or a circle".into());
            }
            // A blend may run down: its profile at `from` above its top at `to`.
            let down = !form.top.is_empty() && to < from;
            if to <= from && !down {
                let unit = if form.axis.is_some() { " degrees" } else { "" };
                return Err(format!("sweeps nothing: from {from} to {to}{unit}"));
            }
            let ring = if crate::geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring };
            // A revolve's sketch plane where it is along the axis it is square to: its triangles moved there.
            let lift = match &form.at {
                Some(e) => embed(form.plane, P2(0.0, 0.0), num(e)?),
                None => [0.0; 3],
            };
            let lifted = |tris: Vec<[P3; 3]>| -> Vec<[P3; 3]> { tris.into_iter().map(|t| t.map(|q| add(q, lift))).collect() };
            // A revolve of a profile with holes (a ring's section): the profile turned, less each hole turned.
            if let (Some([p, q]), false, true) = (&form.axis, form.holes.is_empty(), form.top.is_empty()) {
                if to - from > 360.0 || form.holes.len() > MAX_RING || form.holes.iter().any(|h| h.len() > MAX_RING) {
                    return Err(format!("it turns more than all the way round, or its holes have more than {MAX_RING} corners"));
                }
                let mut corner = |c: &'a [Expr; 2]| -> Result<P2, String> { Ok(P2(num(&c[0])?, num(&c[1])?)) };
                let axis = [corner(p)?, corner(q)?];
                let (tris, _) = revolve(&ring, axis, (from, to), form.plane)?;
                let mut tris = lifted(tris);
                for (h, b) in form.holes.iter().zip(&hole_bends) {
                    let hole: Vec<P2> = h.iter().map(|[a, c]| Ok(P2(num(a)?, num(c)?))).collect::<Result<_, String>>()?;
                    let hole = shapes::bent2(hole, b);
                    let hole = if crate::geom::area(&hole) < 0.0 { hole.into_iter().rev().collect() } else { hole };
                    let (inner, _) = revolve(&hole, axis, (from, to), form.plane)?;
                    let inner = lifted(inner);
                    tris.extend(inner.into_iter().map(|[a, b, c]| [a, c, b]));
                }
                return Ok(Some(Piece { show: form.show, ..hulled(tris, material) }));
            }
            if !form.top.is_empty() || !form.holes.is_empty() {
                if form.top.len() > MAX_RING || form.holes.len() > MAX_RING || form.holes.iter().any(|h| h.len() > MAX_RING) {
                    return Err(format!("its top or its holes have more than {MAX_RING} corners"));
                }
                if form.axis.is_some() || form.circle.is_some() {
                    return Err("a `top` or `holes` go with a `profile` swept across its plane, not turned or round".into());
                }
                let mut ring_of =
                    |v: &'a [[Expr; 2]]| -> Result<Vec<P2>, String> { v.iter().map(|[a, b]| Ok(P2(num(a)?, num(b)?))).collect() };
                let tris = if form.top.is_empty() {
                    let holes = form.holes.iter().map(|h| ring_of(h)).collect::<Result<Vec<_>, _>>()?;
                    let holes: Vec<Vec<P2>> = holes.into_iter().zip(&hole_bends).map(|(h, b)| shapes::bent2(h, b)).collect();
                    too_fine((ring.len() + holes.iter().map(Vec::len).sum::<usize>()).saturating_mul(4), "profile")?;
                    shapes::holed(&ring, &holes, from, to, form.plane)?
                } else {
                    let top = shapes::bent2(ring_of(&form.top)?, &top_bends);
                    if top.len() < 3 || crate::geom::area(&top).abs() < 1.0 {
                        return Err("its top has no area: it needs three corners or more".into());
                    }
                    too_fine((ring.len() + top.len()).saturating_mul(4), "blend")?;
                    shapes::blend(&ring, &top, from, to, form.plane)?
                };
                return Ok(Some(Piece { show: form.show, ..hulled(tris, material) }));
            }
            match &form.axis {
                Some([p, q]) => {
                    if to - from > 360.0 {
                        return Err(format!("turns more than all the way round: from {from} to {to} degrees, 360 at most"));
                    }
                    let mut corner = |c: &'a [Expr; 2]| -> Result<P2, String> { Ok(P2(num(&c[0])?, num(&c[1])?)) };
                    let axis = [corner(p)?, corner(q)?];
                    let (tris, outline) = revolve(&ring, axis, (from, to), form.plane)?;
                    let tris = lifted(tris);
                    let outline = outline.into_iter().map(|q| P2(q.0 + lift[0], q.1 + lift[1])).collect();
                    let (bottom, top) = heights(&tris);
                    Piece { caps: tris.len(), tris, outline, bottom, top, material, round: false, show: None }
                }
                None => {
                    let (tris, caps) = sweep(&ring, from, to, form.plane)?;
                    let span = |k: usize| {
                        ring.iter()
                            .map(|p| if k == 0 { p.0 } else { p.1 })
                            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
                    };
                    let ((a0, a1), (b0, b1)) = (span(0), span(1));
                    let (outline, bottom, top) = match form.plane {
                        Plane::Xy => (ring.clone(), from, to),
                        Plane::Xz => (vec![P2(a0, from), P2(a1, from), P2(a1, to), P2(a0, to)], b0, b1),
                        Plane::Yz => (vec![P2(from, a0), P2(to, a0), P2(to, a1), P2(from, a1)], b0, b1),
                    };
                    Piece { tris, caps, outline, bottom, top, material, round: form.circle.is_some(), show: None }
                }
            }
        };
        Ok(Some(Piece { show: form.show, ..piece }))
    }

    /// Where each of its forms lies, sized by `values`, before any void cuts it: its place in
    /// `forms`, whether it is a void, and the box its solid fills in the family's own axes. A form
    /// that builds to nothing on its own — a formula it can't read, a profile with no area — is left
    /// out: what a warning tells is best worked out around it, not stopped by it.
    pub fn shapes(&self, type_name: &str, values: &BTreeMap<String, Val>) -> Vec<Shape> {
        let mut read = HashMap::new();
        let mut out = Vec::new();
        let which = self.draws_snapshot_forms(type_name);
        for i in (0..self.forms.len()).filter(|i| self.forms[*i].of_type(type_name) && self.forms[*i].snapshot == which) {
            let Some(piece) = self.solid(i, values, &mut read).ok().flatten() else {
                continue;
            };
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for p in piece.tris.iter().flatten() {
                for k in 0..3 {
                    min[k] = min[k].min(p[k]);
                    max[k] = max[k].max(p[k]);
                }
            }
            if min[0].is_finite() && max[0].is_finite() {
                out.push(Shape { index: i, void: self.forms[i].void, min, max });
            }
        }
        out
    }

    /// What's wrong with it, which a change may not go ahead with, and what's worth knowing, which
    /// doesn't stop it: every finding, not only the first, so they're fixed in one go.
    pub fn problems(&self, has_material: &dyn Fn(&str) -> bool) -> (Vec<String>, Vec<String>) {
        validate::problems(self, has_material)
    }
}

/// `e` as a number: itself, or its formula worked out over `values`; `read`, the formulas as read already.
/// The names between braces in a label's `value`: `{Circuit}` reads the parameter `Circuit`.
fn fields(value: &str) -> Result<Vec<&str>, String> {
    let (mut out, mut rest) = (Vec::new(), value);
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            return Err("its text has a `{` with no `}` to close it".into());
        };
        out.push(rest[open + 1..open + close].trim());
        rest = &rest[open + close + 1..];
    }
    Ok(out)
}

/// A label's `value` with each `{Parameter}` replaced by that parameter's value: a number as plainly as it reads (3, 2.5), a
/// material by its name.
fn said_text(value: &str, values: &BTreeMap<String, Val>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    for name in fields(value)? {
        let Some(open) = rest.find('{') else { break };
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').unwrap_or(0) + open;
        rest = &rest[close + 1..];
        match values.get(name) {
            Some(Val::Number(x)) => {
                let plain = format!("{x:.3}");
                out.push_str(plain.trim_end_matches('0').trim_end_matches('.'));
            }
            Some(Val::Text(t)) => out.push_str(t),
            None => return Err(format!("its text reads `{{{name}}}`, which isn't one of the family's parameters")),
        }
        if out.chars().count() > MAX_TEXT {
            break;
        }
    }
    out.push_str(rest);
    if out.trim().is_empty() {
        return Err("its text is empty; give it a `value` with something to letter".into());
    }
    if out.chars().count() > MAX_TEXT {
        return Err(format!("its text is longer than {MAX_TEXT} characters"));
    }
    Ok(out)
}

/// `text` in our lettering, capitals `height` tall, `at` the middle of its line vertically and, as `align` says, its start,
/// middle or end (y up), as polylines.
pub fn lettering(text: &str, at: P2, height: f64, align: Align) -> Vec<Vec<P2>> {
    let h = height as f32;
    let width = f64::from(font::width(text, h));
    let left = at.0
        - match align {
            Align::Left => 0.0,
            Align::Center => width / 2.0,
            Align::Right => width,
        };
    font::strokes(text, 0.0, 0.0, h)
        .into_iter()
        .map(|line| line.into_iter().map(|(x, y)| P2(left + f64::from(x), at.1 - height / 2.0 - f64::from(y))).collect())
        .collect()
}

fn number<'a>(e: &'a Expr, values: &BTreeMap<String, Val>, read: &mut HashMap<&'a str, Result<Node, String>>) -> Result<f64, String> {
    let x = match e {
        Expr::Number(x) => *x,
        Expr::Formula(f) => match read.entry(f.as_str()).or_insert_with(|| parse(f)) {
            Ok(node) => worked(f, node, &mut |n: &str| match values.get(n) {
                Some(Val::Number(x)) => Ok(*x),
                Some(Val::Text(_)) => Err(format!("`{n}` is a material or text, not a number")),
                None => Err(format!("`{n}` isn't one of its parameters")),
            })?,
            Err(e) => return Err(e.clone()),
        },
    };
    if x.is_finite() && x.abs() <= MAX_COORD { Ok(x) } else { Err(format!("{x} is not a usable number")) }
}

/// Three numbers or formulas, worked out.
fn point<'a>(
    v: &'a [Expr; 3],
    values: &BTreeMap<String, Val>,
    read: &mut HashMap<&'a str, Result<Node, String>>,
) -> Result<[f64; 3], String> {
    Ok([number(&v[0], values, read)?, number(&v[1], values, read)?, number(&v[2], values, read)?])
}

/// Where nested part `n` stands in its parent sized by `values`, and its own x, y and z axes in the parent's: given, or turned
/// about z and mirrored across its x.
fn nested_frame<'a>(
    n: &'a Nested,
    values: &BTreeMap<String, Val>,
    read: &mut HashMap<&'a str, Result<Node, String>>,
) -> Result<(P3, [P3; 3]), String> {
    let at = point(&n.at, values, read)?;
    let (sin, cos) = number(&n.rotation, values, read)?.to_radians().sin_cos();
    let flip = if n.mirrored { -1.0 } else { 1.0 };
    Ok((at, n.axes.unwrap_or([[cos, sin, 0.0], [-flip * sin, flip * cos, 0.0], [0.0, 0.0, 1.0]])))
}

/// How many times as large nested part `n` is drawn, sized by `values`: one where it says nothing; else a number above zero.
fn nested_scale<'a>(
    n: &'a Nested,
    values: &BTreeMap<String, Val>,
    read: &mut HashMap<&'a str, Result<Node, String>>,
) -> Result<f64, String> {
    let Some(e) = &n.scale else { return Ok(1.0) };
    let k = number(e, values, read)?;
    if !(k.is_finite() && k > 0.0 && k <= MAX_SCALE) {
        return Err(format!("its scale is {k}; it needs a number above zero and up to {MAX_SCALE}"));
    }
    Ok(k)
}

/// The centre of the circle through `a`, `m` and `c`; none where they lie on a line.
fn circumcentre(a: P2, m: P2, c: P2) -> Option<P2> {
    let d = 2.0 * (a.0 * (m.1 - c.1) + m.0 * (c.1 - a.1) + c.0 * (a.1 - m.1));
    if d.abs() < 1e-9 {
        return None;
    }
    let (sa, sm, sc) = (a.0 * a.0 + a.1 * a.1, m.0 * m.0 + m.1 * m.1, c.0 * c.0 + c.1 * c.1);
    let o = P2((sa * (m.1 - c.1) + sm * (c.1 - a.1) + sc * (a.1 - m.1)) / d, (sa * (c.0 - m.0) + sm * (a.0 - c.0) + sc * (m.0 - a.0)) / d);
    (o.0.is_finite() && o.1.is_finite()).then_some(o)
}

/// A piece of `tris`, its outline the hull round them from above.
fn hulled(tris: Vec<[P3; 3]>, material: String) -> Piece {
    let (bottom, top) = heights(&tris);
    let outline = crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect());
    Piece { caps: tris.len(), tris, outline, bottom, top, material, round: false, show: None }
}

/// The most a nested part may be scaled by.
const MAX_SCALE: f64 = 1000.0;

/// The families a nested part may be of, by name: the project's.
pub type Others<'o> = &'o dyn Fn(&str) -> Option<&'o Family>;

/// How [`Family::build`] builds: Revit's own forms or the parametric ones, the families nested parts are of, how deep.
#[derive(Clone, Copy)]
struct Build<'o, 's> {
    snapshot: bool,
    others: Others<'o>,
    depth: usize,
    /// Nested parts built so far, of every depth: at most [`MAX_NESTED`].
    spent: &'s std::cell::Cell<usize>,
}

/// How low and how high `tris` reach.
fn heights(tris: &[[P3; 3]]) -> (f64, f64) {
    tris.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])))
}

/// The point of a plane's sketch at `p`, `t` along the way it's swept; with `t` zero, the direction
/// `p` is in the family's axes.
fn embed(plane: Plane, p: P2, t: f64) -> P3 {
    match plane {
        Plane::Xy => [p.0, p.1, t],
        Plane::Xz => [p.0, t, p.1],
        Plane::Yz => [t, p.0, p.1],
    }
}

/// The triangles that cap a profile `ring`, counter-clockwise; or why it can't be capped: it crosses itself.
fn cap(ring: &[P2]) -> Result<Vec<[usize; 3]>, String> {
    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
    let caps = crate::geom::triangulate(ring);
    let covered: f64 = caps.iter().map(|[a, b, c]| (pt(*b) - pt(*a)).cross(pt(*c) - pt(*a)) / 2.0).sum();
    let area = crate::geom::area(ring);
    if (covered - area).abs() > 1e-6 * area + 1e-3 {
        return Err("its profile crosses itself".into());
    }
    Ok(caps)
}

/// `ring`, counter-clockwise in `plane`, swept across it from `from` to `to`: its caps, then its
/// sides, every triangle turned out; and how many the caps take.
fn sweep(ring: &[P2], from: f64, to: f64, plane: Plane) -> Result<(Vec<[P3; 3]>, usize), String> {
    let at = |p: P2, t: f64| embed(plane, p, t);
    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
    let caps = cap(ring)?;
    let mut tris = Vec::with_capacity(caps.len() * 2 + ring.len() * 2);
    for &[a, b, c] in &caps {
        tris.push([at(pt(a), to), at(pt(b), to), at(pt(c), to)]);
        tris.push([at(pt(a), from), at(pt(c), from), at(pt(b), from)]);
    }
    for (a, b) in crate::geom::ring_edges(ring) {
        let (a0, b0, a1, b1) = (at(a, from), at(b, from), at(a, to), at(b, to));
        tris.push([a0, b0, b1]);
        tris.push([a0, b1, a1]);
    }
    // Across x and z the sweep runs along y: a mirror image of the others' axes, so its triangles
    // turn the other way to face out.
    if plane == Plane::Xz {
        tris.iter_mut().for_each(|t| t.swap(1, 2));
    }
    Ok((tris, caps.len() * 2))
}

/// `ring`, counter-clockwise in `plane`, turned about the line through `axis` (in `plane`) from `from` to
/// `to` degrees, right-handed from the axis' first point toward its second: its triangles, every one turned
/// out, a full turn's sides only and a part-turn's with its two ends capped; and its outline seen from above.
fn revolve(ring: &[P2], [p0, p1]: [P2; 2], (from, to): (f64, f64), plane: Plane) -> Result<(Vec<[P3; 3]>, Vec<P2>), String> {
    if p0.dist(p1) < 1e-6 {
        return Err("its axis starts and ends at one point: it needs a direction".into());
    }
    let caps = cap(ring)?;
    // Each corner's distance along the axis, and out from it to its left (none, on it).
    let (along, left) = ((p1 - p0).unit(), (p1 - p0).unit().perp());
    let reach: Vec<(f64, f64)> =
        ring.iter().map(|c| ((*c - p0).dot(along), (*c - p0).dot(left))).map(|(t, r)| (t, if r.abs() < 1e-6 { 0.0 } else { r })).collect();
    let (least, most) = reach.iter().fold((0.0_f64, 0.0_f64), |(lo, hi), (_, r)| (lo.min(*r), hi.max(*r)));
    if least < 0.0 && most > 0.0 {
        return Err("its profile crosses its axis: draw it to one side of it".into());
    }
    if least == 0.0 && most == 0.0 {
        return Err("its profile lies along its axis: it needs some width off it".into());
    }
    // In 3D: along the axis (`u`), out from it (`n`), and the way a corner out to its left turns (`w`). One out to
    // the right turns the other way, which turns its triangles inside out.
    let (o, u, n) = (embed(plane, p0, 0.0), embed(plane, along, 0.0), embed(plane, left, 0.0));
    let w = cross(u, n);
    let turn = to - from;
    let full = turn > 360.0 - 1e-9;
    let steps = (turn / 360.0 * SEGMENTS as f64 - 1e-9).ceil().max(1.0) as usize;
    let angle = |k: usize| (from + turn * (if full { k % steps } else { k }) as f64 / steps as f64).to_radians().sin_cos();
    let at = |i: usize, k: usize| -> P3 {
        let (t, r) = reach.get(i).copied().unwrap_or_default();
        let (s, c) = angle(k);
        add(add(o, mul(u, t)), mul(add(mul(n, c), mul(w, s)), r)).map(|x| (x * 1e9).round() / 1e9 + 0.0)
    };
    let mut tris = Vec::new();
    let mut push = |t: [P3; 3]| {
        // A corner on the axis stays put as it turns, which leaves a side with no area.
        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
            tris.push(if least < 0.0 { [t[0], t[2], t[1]] } else { t });
        }
    };
    if !full {
        for &[a, b, c] in &caps {
            push([at(a, steps), at(b, steps), at(c, steps)]);
            push([at(a, 0), at(c, 0), at(b, 0)]);
        }
    }
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        for k in 0..steps {
            let (a0, b0, a1, b1) = (at(i, k), at(j, k), at(i, k + 1), at(j, k + 1));
            push([a0, b0, b1]);
            push([a0, b1, a1]);
        }
    }
    // Seen from above, a part-turn about an upright axis is a slice of a ring from the profile's widest to its
    // narrowest; any other, the hull of what it comes to. ponytail: the hull stands for the outline of the rest.
    let outline = if u[0].abs() < 1e-9 && u[1].abs() < 1e-9 && !full {
        let inner = reach.iter().map(|(_, r)| r.abs()).fold(f64::INFINITY, f64::min);
        // A profile to the axis' right is out along minus `n`.
        let side = if least < 0.0 { -1.0 } else { 1.0 };
        let arc = |r: f64, k: usize| {
            let (s, c) = angle(k);
            P2(o[0] + side * r * (n[0] * c + w[0] * s), o[1] + side * r * (n[1] * c + w[1] * s))
        };
        let mut pts: Vec<P2> = (0..=steps).map(|k| arc(most.max(-least), k)).collect();
        if inner > 0.0 {
            pts.extend((0..=steps).rev().map(|k| arc(inner, k)));
        } else {
            pts.push(P2(o[0], o[1]));
        }
        if crate::geom::area(&pts) < 0.0 {
            pts.reverse();
        }
        pts
    } else {
        crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect())
    };
    Ok((tris, outline))
}

/// Refuses a form whose triangles, counted before they are made, come to more than a piece may hold.
fn too_fine(triangles: usize, what: &str) -> Result<(), String> {
    if triangles > MAX_TRIANGLES {
        return Err(format!("its {what} would make more than {MAX_TRIANGLES} triangles"));
    }
    Ok(())
}

/// How near a point is to a line, or a segment's end to another's, for them to meet, mm (an area, for the turn of three points).
const MEET: f64 = 1e-9;

/// The most points of a traced outline a draw tests for crossing itself; a longer one was tested as its family was loaded.
const TESTED_AT_DRAW: usize = 1024;

/// Whether the closed `ring` crosses or touches itself: two of its edges that share no corner meet.
fn crosses_itself(ring: &[P2]) -> bool {
    let n = ring.len();
    let edge = |i: usize| (ring[i], ring[(i + 1) % n]);
    (0..n).any(|i| {
        let (a, b) = edge(i);
        // Edge `i + 1` and, round the ring's end, edge `n - 1` for the first, share a corner with it.
        (i + 2..n).filter(|j| !(i == 0 && *j == n - 1)).any(|j| {
            let (c, d) = edge(j);
            segments_meet(a, b, c, d)
        })
    })
}

/// Whether the segments `a`-`b` and `c`-`d` meet: cross, touch or overlap.
fn segments_meet(a: P2, b: P2, c: P2, d: P2) -> bool {
    let turn = |p: P2, q: P2, r: P2| (q - p).cross(r - p);
    let within = |p: P2, q: P2, r: P2| {
        r.0 >= p.0.min(q.0) - MEET && r.0 <= p.0.max(q.0) + MEET && r.1 >= p.1.min(q.1) - MEET && r.1 <= p.1.max(q.1) + MEET
    };
    let (d1, d2, d3, d4) = (turn(c, d, a), turn(c, d, b), turn(a, b, c), turn(a, b, d));
    if ((d1 > MEET && d2 < -MEET) || (d1 < -MEET && d2 > MEET)) && ((d3 > MEET && d4 < -MEET) || (d3 < -MEET && d4 > MEET)) {
        return true;
    }
    (d1.abs() <= MEET && within(c, d, a))
        || (d2.abs() <= MEET && within(c, d, b))
        || (d3.abs() <= MEET && within(a, b, c))
        || (d4.abs() <= MEET && within(a, b, d))
}

/// A form a file brought as triangles: its outline seen from above, the one it `traced` (counter-clockwise), else the hull
/// round them.
fn mesh(tris: &[[P3; 3]], traced: &[[f64; 2]], material: String) -> Result<Piece, String> {
    if tris.len() > MAX_TRIANGLES {
        return Err(format!("it has more than {MAX_TRIANGLES} triangles"));
    }
    if tris.iter().flatten().flatten().any(|x| !x.is_finite() || x.abs() > MAX_COORD) {
        return Err("a corner of it isn't a usable number".into());
    }
    let (bottom, top) = tris.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
    let outline = if traced.is_empty() {
        // ponytail: the convex hull stands for its outline where none was traced.
        crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect())
    } else {
        if traced.len() < 3 || traced.len() > crate::model::MAX_POINTS {
            return Err(format!("its `outline` has {} points: it takes 3 to {}", traced.len(), crate::model::MAX_POINTS));
        }
        if traced.iter().flatten().any(|x| !x.is_finite() || x.abs() > MAX_COORD) {
            return Err("a point of its `outline` isn't a usable number".into());
        }
        let ring: Vec<P2> = traced.iter().map(|[x, y]| P2(*x, *y)).collect();
        // A long one was tested when it was loaded ([`Family::clear_crossing_outlines`]); here, every draw of every type would
        // test its pairs of edges again.
        if traced.len() <= TESTED_AT_DRAW && crosses_itself(&ring) {
            // A traced outline that zig-zags or crosses itself fills nothing: the hull stands for it, as if none was traced.
            crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect())
        } else if crate::geom::area(&ring) < 0.0 {
            ring.into_iter().rev().collect()
        } else {
            ring
        }
    };
    Ok(Piece { tris: tris.to_vec(), caps: tris.len(), outline, bottom, top, material, round: false, show: None })
}

/// The families a new project starts with, BIMCraft's own: tables, chairs, beds, sofas, desks,
/// wardrobes, base cabinets, WCs, washbasins and bathtubs, named in `language`, their formulas in
/// its words. A table or a chair stands on its centre, its back (a chair's) to its +y; the rest
/// stand with their backs on their x axis, their fronts toward -y, so they go against a wall.
pub fn library(language: crate::language::Language) -> BTreeMap<String, Family> {
    use serde_json::json;
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, height, length) = (w("Width"), w(msg!("Depth")), w("Height"), w(msg!("Length")));
    let (thick, leg, seat, mat) = (w(msg!("Thickness")), w(msg!("Leg Width")), w(msg!("Seat Height")), w("Material"));
    let (timber, fabric, porcelain, concrete) = (w(msg!("Timber")), w(msg!("Fabric")), w(msg!("Porcelain")), w(msg!("Concrete")));
    // A box: x from `x0` to `x1`, y from `y0` to `y1`, up from `z0` to `z1`, of `m`.
    let block = |[x0, y0, x1, y1]: [Value; 4], z0: Value, z1: Value, m: &str| json!({"profile": [[x0, y0], [x1.clone(), y0], [x1, y1.clone()], [x0, y1]], "from": z0, "to": z1, "material": m});
    let f = |e: String| Value::String(e);
    let n = |x: f64| json!(x);
    let size = |name: &str, value: f64| json!({"name": name, "value": value});
    let material = |name: &str, value: &str| json!({"name": name, "kind": "material", "value": value});
    let (half_w, half_l) = (format!("{width} / 2"), format!("{length} / 2"));
    let neg = |e: &str| f(format!("-({e})"));
    let mut families = Vec::new();
    // Four legs at the corners of a top `w` by `d`, 50 in from its edges, up to `z`.
    let legs = |w: &str, d: &str, z: &str| -> Vec<Value> {
        [(true, true), (true, false), (false, true), (false, false)]
            .into_iter()
            .map(|(east, north)| {
                let x = if east {
                    [f(format!("({w}) / 2 - 50 - {leg}")), f(format!("({w}) / 2 - 50"))]
                } else {
                    [f(format!("-({w}) / 2 + 50")), f(format!("-({w}) / 2 + 50 + {leg}"))]
                };
                let y = if north {
                    [f(format!("({d}) / 2 - 50 - {leg}")), f(format!("({d}) / 2 - 50"))]
                } else {
                    [f(format!("-({d}) / 2 + 50")), f(format!("-({d}) / 2 + 50 + {leg}"))]
                };
                let [x0, x1] = x;
                let [y0, y1] = y;
                block([x0, y0, x1, y1], n(0.0), f(z.to_string()), &mat)
            })
            .collect()
    };
    let top = format!("{height} - {thick}");
    let mut forms = vec![block(
        [neg(&half_w), f(format!("-{depth} / 2")), f(half_w.clone()), f(format!("{depth} / 2"))],
        f(top.clone()),
        f(height.clone()),
        &mat,
    )];
    forms.extend(legs(&width, &depth, &top));
    families.push((
        w(msg!("Table")),
        json!({
            "parameters": [size(&width, 1600.0), size(&depth, 900.0), size(&height, 750.0), size(&thick, 40.0), size(&leg, 60.0), material(&mat, &timber)],
            "types": {"1600 x 900": {}, "1200 x 800": {&width: 1200, &depth: 800}, "2000 x 1000": {&width: 2000, &depth: 1000}},
            "forms": forms
        }),
    ));
    // A chair: its seat, its back over the seat's rear edge, and its legs.
    let under = format!("{seat} - {thick}");
    let mut forms = vec![
        block(
            [neg(&half_w), f(format!("-{depth} / 2")), f(half_w.clone()), f(format!("{depth} / 2"))],
            f(under.clone()),
            f(seat.clone()),
            &mat,
        ),
        json!({"plane": "xz", "profile": [[neg(&half_w), seat], [half_w, seat], [half_w, height], [neg(&half_w), height]],
               "from": format!("{depth} / 2 - {thick}"), "to": format!("{depth} / 2"), "material": mat}),
    ];
    forms.extend(legs(&format!("{width} + 100"), &format!("{depth} + 100"), &under));
    families.push((
        w(msg!("Chair")),
        json!({
            "parameters": [size(&width, 450.0), size(&depth, 450.0), size(&seat, 450.0), size(&height, 850.0), size(&thick, 40.0), size(&leg, 40.0), material(&mat, &timber)],
            "types": {"450 x 450": {}},
            "forms": forms
        }),
    ));
    // A bed against a wall: its base, its mattress, its headboard, and a pillow, two on a double.
    let head = w(msg!("Headboard Height"));
    let pillow = |x0: String, x1: String, shown: &str| {
        let mut b = block([f(x0), n(-500.0), f(x1), n(-100.0)], f(height.clone()), f(format!("{height} + 120")), &fabric);
        b["visible"] = json!(shown);
        b
    };
    families.push((
        w(msg!("Bed")),
        json!({
            "parameters": [size(&width, 1600.0), size(&length, 2000.0), size(&height, 450.0), size(&head, 900.0), material(&mat, &timber)],
            "types": {"1600 x 2000": {}, "900 x 2000": {&width: 900}, "1800 x 2000": {&width: 1800}},
            "forms": [
                block([neg(&half_w), neg(&length), f(half_w.clone()), n(0.0)], n(0.0), n(300.0), &mat),
                block([f(format!("-({width}) / 2 + 20")), f(format!("-({length}) + 20")), f(format!("{width} / 2 - 20")), n(-60.0)], n(300.0), f(height.clone()), &fabric),
                {"plane": "xz", "profile": [[neg(&half_w), 0], [half_w, 0], [half_w, head], [neg(&half_w), head]], "from": -60, "to": 0, "material": mat},
                pillow(format!("-({width}) / 2 + 80"), "-40".into(), &format!("{width} > 1200")),
                pillow("40".into(), format!("{width} / 2 - 80"), &format!("{width} > 1200")),
                pillow(format!("-({width}) / 2 + 80"), format!("{width} / 2 - 80"), &format!("{width} <= 1200"))
            ]
        }),
    ));
    // A sofa: its seat between its arms, its back.
    let arm = w(msg!("Arm Width"));
    families.push((
        w(msg!("Sofa")),
        json!({
            "parameters": [size(&width, 2000.0), size(&depth, 900.0), size(&seat, 420.0), size(&height, 800.0), size(&arm, 180.0), material(&mat, &fabric)],
            "types": {"2000 x 900": {}, "1600 x 900": {&width: 1600}},
            "forms": [
                block([f(format!("-({width}) / 2 + {arm}")), neg(&depth), f(format!("{width} / 2 - {arm}")), n(-200.0)], n(0.0), f(seat.clone()), &mat),
                block([neg(&half_w), n(-200.0), f(half_w.clone()), n(0.0)], n(0.0), f(height.clone()), &mat),
                block([neg(&half_w), neg(&depth), f(format!("-({width}) / 2 + {arm}")), n(-200.0)], n(0.0), n(600.0), &mat),
                block([f(format!("{width} / 2 - {arm}")), neg(&depth), f(half_w.clone()), n(-200.0)], n(0.0), n(600.0), &mat)
            ]
        }),
    ));
    // A desk: its top on two side panels, a modesty panel between them.
    families.push((
        w(msg!("Desk")),
        json!({
            "parameters": [size(&width, 1400.0), size(&depth, 700.0), size(&height, 740.0), size(&thick, 30.0), material(&mat, &timber)],
            "types": {"1400 x 700": {}, "1200 x 600": {&width: 1200, &depth: 600}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(top.clone()), f(height.clone()), &mat),
                block([neg(&half_w), f(format!("-({depth}) + 20")), f(format!("-({width}) / 2 + 25")), n(-20.0)], n(0.0), f(top.clone()), &mat),
                block([f(format!("{width} / 2 - 25")), f(format!("-({depth}) + 20")), f(half_w.clone()), n(-20.0)], n(0.0), f(top.clone()), &mat),
                block([f(format!("-({width}) / 2 + 25")), n(-60.0), f(format!("{width} / 2 - 25")), n(-40.0)], n(300.0), f(top.clone()), &mat)
            ]
        }),
    ));
    // A wardrobe on its plinth.
    families.push((
        w(msg!("Wardrobe")),
        json!({
            "category": "casework",
            "parameters": [size(&width, 1200.0), size(&depth, 600.0), size(&height, 2100.0), material(&mat, &timber)],
            "types": {"1200 x 600": {}, "900 x 600": {&width: 900}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], n(80.0), f(height.clone()), &mat),
                block([f(format!("-({width}) / 2 + 50")), f(format!("-({depth}) + 50")), f(format!("{width} / 2 - 50")), n(0.0)], n(0.0), n(80.0), &mat)
            ]
        }),
    ));
    // A base cabinet: its plinth, set back for toes; its carcass; its worktop over the front.
    let worktop = w(msg!("Worktop"));
    families.push((
        w(msg!("Base Cabinet")),
        json!({
            "category": "casework",
            "parameters": [size(&width, 1800.0), size(&depth, 600.0), size(&height, 900.0), material(&mat, &timber), material(&worktop, &concrete)],
            "types": {"1800 x 600": {}, "1200 x 600": {&width: 1200}, "600 x 600": {&width: 600}},
            "forms": [
                block([neg(&half_w), f(format!("-({depth}) + 60")), f(half_w.clone()), n(0.0)], n(0.0), n(100.0), &mat),
                block([neg(&half_w), f(format!("-({depth}) + 20")), f(half_w.clone()), n(0.0)], n(100.0), f(format!("{height} - 40")), &mat),
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(format!("{height} - 40")), f(height.clone()), &worktop)
            ]
        }),
    ));
    // A WC: its cistern against the wall, its bowl.
    families.push((
        w(msg!("WC")),
        json!({
            "category": "plumbing",
            "kind": "wc",
            "types": {"400 x 650": {}},
            "forms": [
                block([n(-200.0), n(-180.0), n(200.0), n(0.0)], n(380.0), n(800.0), &porcelain),
                block([n(-160.0), n(-380.0), n(160.0), n(-150.0)], n(0.0), n(400.0), &porcelain),
                {"circle": {"centre": [0, -470], "radius": 180}, "from": 0, "to": 400, "material": porcelain}
            ],
            // Its waste through the floor under the bowl, and the cistern's cold water in the wall.
            "connectors": [
                crate::fixtures::pipe("waste", "out", [n(0.0), n(-300.0), n(0.0)], [0.0, 0.0, -1.0], 100.0),
                crate::fixtures::pipe("cold", "in", [n(150.0), n(0.0), n(200.0)], [0.0, 1.0, 0.0], 20.0)
            ]
        }),
    ));
    // A washbasin on its pedestal.
    families.push((
        w(msg!("Washbasin")),
        json!({
            "category": "plumbing",
            "kind": "lav",
            "parameters": [size(&width, 600.0), size(&depth, 450.0), size(&height, 850.0)],
            "types": {"600 x 450": {}, "500 x 400": {&width: 500, &depth: 400}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(format!("{height} - 160")), f(height.clone()), &porcelain),
                {"circle": {"centre": [0, -150], "radius": 90}, "from": 0, "to": format!("{height} - 160"), "material": porcelain}
            ],
            // Its waste and its cold water in the wall behind the pedestal.
            "connectors": [
                crate::fixtures::pipe("waste", "out", [n(0.0), n(0.0), f(format!("{height} - 300"))], [0.0, 1.0, 0.0], 40.0),
                crate::fixtures::pipe("cold", "in", [n(-100.0), n(0.0), f(format!("{height} - 200"))], [0.0, 1.0, 0.0], 20.0)
            ]
        }),
    ));
    // A bathtub along a wall: its bottom, and its four sides round the tub.
    let rim = height.clone();
    let inner = format!("-({width}) + 60");
    families.push((
        w(msg!("Bathtub")),
        json!({
            "category": "plumbing",
            "kind": "bath",
            "parameters": [size(&length, 1700.0), size(&width, 750.0), size(&height, 560.0)],
            "types": {"1700 x 750": {}, "1500 x 700": {&length: 1500, &width: 700}},
            "forms": [
                block([neg(&half_l), neg(&width), f(half_l.clone()), n(0.0)], n(0.0), n(120.0), &porcelain),
                block([neg(&half_l), n(-60.0), f(half_l.clone()), n(0.0)], n(120.0), f(rim.clone()), &porcelain),
                block([neg(&half_l), neg(&width), f(half_l.clone()), f(inner.clone())], n(120.0), f(rim.clone()), &porcelain),
                block([neg(&half_l), f(inner.clone()), f(format!("-({length}) / 2 + 60")), n(-60.0)], n(120.0), f(rim.clone()), &porcelain),
                block([f(format!("{length} / 2 - 60")), f(inner.clone()), f(half_l), n(-60.0)], n(120.0), f(rim), &porcelain)
            ],
            // Its waste through the floor at one end, and the mixer's cold and hot water in the wall above that end.
            "connectors": [
                crate::fixtures::pipe("waste", "out", [f(format!("-({length}) / 2 + 150")), f(format!("-({width}) / 2")), n(0.0)], [0.0, 0.0, -1.0], 40.0),
                crate::fixtures::pipe("cold", "in", [f(format!("-({length}) / 2 + 100")), n(0.0), f(format!("{height} + 200"))], [0.0, 1.0, 0.0], 20.0),
                crate::fixtures::pipe("hot", "in", [f(format!("-({length}) / 2 + 250")), n(0.0), f(format!("{height} + 200"))], [0.0, 1.0, 0.0], 20.0)
            ]
        }),
    ));
    families.extend(hosted_library(language));
    families.extend(crate::fixtures::library(language));
    families.into_iter().filter_map(|(name, v)| serde_json::from_value(v).ok().map(|f| (name, f))).collect()
}

/// The doors and windows a new project starts with, in `language`: a single door, a double door, a sliding door, a
/// fixed window, a casement window and a sliding window, each with several sizes and saying how it opens. Their forms
/// stand in the opening's axes (x along the wall from the middle of the opening, y across it, 0 in the middle of the
/// wall and + toward its exterior, z up from the sill), and they take the thickness of the wall they're in,
/// `Wall Thickness`, as theirs.
fn hosted_library(language: crate::language::Language) -> Vec<(String, Value)> {
    use serde_json::json;
    let w = |s: &str| language.word(s).to_string();
    let (frame, depth, leaf) = (w(msg!("Frame Width")), w(msg!("Frame Depth")), w(msg!("Leaf Thickness")));
    let (frame_mat, leaf_mat, glass, metal, timber) =
        (w(msg!("Frame Material")), w(msg!("Leaf Material")), w(msg!("Glass")), w(msg!("Metal")), w("Timber"));
    let (width, height, wall, sill) = (WIDTH, HEIGHT, WALL, SILL);
    let size = |name: &str, value: f64| json!({"name": name, "value": value});
    let material = |name: &str, value: &str| json!({"name": name, "kind": "material", "value": value});
    // A box from `x.0` to `x.1` along, `y.0` to `y.1` across and `z.0` up to `z.1`, each a formula, made of `m`.
    let block = |x: [&str; 2], y: [&str; 2], z: [&str; 2], m: &str| json!({"profile": [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]], "from": z[0], "to": z[1], "material": m});
    // What draws a box, as `block` does.
    type Block<'a> = &'a dyn Fn([&str; 2], [&str; 2], [&str; 2], &str) -> Value;
    // The frame of a door: two jambs and a head across the wall's thickness, in the opening's width and height.
    let (left, right) = (format!("-{width} / 2 + {frame}"), format!("{width} / 2 - {frame}"));
    let (half_w, minus_half_w) = (format!("{width} / 2"), format!("-{width} / 2"));
    let (half_t, minus_half_t) = (format!("{wall} / 2"), format!("-{wall} / 2"));
    let under_head = format!("{height} - {frame}");
    let door_frame = |bx: Block<'_>| {
        vec![
            bx([&minus_half_w, &left], [&minus_half_t, &half_t], ["0", &under_head], &frame_mat),
            bx([&right, &half_w], [&minus_half_t, &half_t], ["0", &under_head], &frame_mat),
            bx([&minus_half_w, &half_w], [&minus_half_t, &half_t], [&under_head, height], &frame_mat),
        ]
    };
    let leaf_y = [format!("-{leaf} / 2"), format!("{leaf} / 2")];
    let (face_in, face_out) = (format!("-{leaf} / 2 - 55"), format!("{leaf} / 2 + 55"));
    // A handle is not drawn at Coarse, nor in a plan seen from above (below its cut): a door's plan is its jambs, its leaf and
    // its swing.
    let handle = |x: [&str; 2], bx: Block<'_>| {
        let mut both =
            vec![bx(x, [&leaf_y[1], &face_out], ["1000", "1030"], &metal), bx(x, [&face_in, &leaf_y[0]], ["1000", "1030"], &metal)];
        for form in &mut both {
            form["show"] = json!({"coarse": false, "plan_top": false});
        }
        both
    };
    let sizes = |names: &[(&str, f64, f64)]| -> Value {
        names.iter().map(|(n, wd, h)| (n.to_string(), json!({width: wd, height: h}))).collect::<serde_json::Map<String, Value>>().into()
    };
    let mut out = Vec::new();

    // A door's leaf, shut in its opening: built in 3D and never drawn in a plan, which draws it open (Revit's door families'
    // panel, its Visibility Settings' Plan/RCP off).
    let leaf_form = |x: [&str; 2], y: [&str; 2]| {
        let mut v = block(x, y, ["0", &under_head], &leaf_mat);
        v["show"] = json!({"plan": false});
        v
    };
    // A leaf drawn open in plan, at every detail level as Revit's Panel and Plan Swing are: hinged at `at` on the exterior
    // face, square to the wall, `run` long and `leaf` thick toward the side `toward` (+1 along x, -1 back), and the arc its
    // free edge sweeps from where it shuts (`from` to `to` degrees).
    let every = json!({});
    let open_leaf = |at: &str, run: &str, toward: &str, from: i32, to: i32| {
        let side = format!("{at} {toward} {leaf}");
        let tip = format!("{wall} / 2 + {run}");
        vec![
            json!({"line": [[at, &half_t], [at, &tip], [&side, &tip], [&side, &half_t], [at, &half_t]], "show": every}),
            json!({"arc": {"centre": [at, &half_t], "radius": run, "from": from, "to": to}, "show": every}),
        ]
    };

    // A single door, hinged on the jamb toward the wall's start and swinging to its exterior.
    let run = format!("{width} - 2 * {frame}");
    let mut forms = door_frame(&block);
    forms.push(leaf_form([&left, &right], [&leaf_y[0], &leaf_y[1]]));
    forms.extend(handle([&format!("{width} / 2 - {frame} - 120"), &format!("{width} / 2 - {frame} - 40")], &block));
    out.push((
        w(msg!("Single Door")),
        json!({
            "category": "door",
            "operation": "swing",
            "parameters": [size(width, 900.0), size(height, 2100.0), size(&frame, 50.0), size(&leaf, 40.0), material(&frame_mat, &timber), material(&leaf_mat, &timber)],
            "types": sizes(&[("900 x 2100", 900.0, 2100.0), ("800 x 2100", 800.0, 2100.0), ("1000 x 2100", 1000.0, 2100.0)]),
            "forms": forms,
            "symbols": open_leaf(&left, &run, "+", 0, 90)
        }),
    ));

    // A double door: two leaves, each hinged on its jamb, meeting in the middle.
    let each = format!("({width} - 2 * {frame}) / 2");
    let mut forms = door_frame(&block);
    forms.push(leaf_form([&left, "-1"], [&leaf_y[0], &leaf_y[1]]));
    forms.push(leaf_form(["1", &right], [&leaf_y[0], &leaf_y[1]]));
    forms.extend(handle(["-120", "-40"], &block));
    forms.extend(handle(["40", "120"], &block));
    let both = [open_leaf(&left, &each, "+", 0, 90), open_leaf(&right, &each, "-", 90, 180)].concat();
    out.push((
        w(msg!("Double Door")),
        json!({
            "category": "door",
            "operation": "double-swing",
            "parameters": [size(width, 1500.0), size(height, 2100.0), size(&frame, 50.0), size(&leaf, 40.0), material(&frame_mat, &timber), material(&leaf_mat, &timber)],
            "types": sizes(&[("1500 x 2100", 1500.0, 2100.0), ("1200 x 2100", 1200.0, 2100.0), ("1800 x 2100", 1800.0, 2100.0)]),
            "forms": forms,
            "symbols": both
        }),
    ));

    // A sliding door (porta de correr): two leaves on two tracks in the frame, overlapping 50 in the middle; the one
    // toward the exterior slides past the other. The plan draws each leaf on its track.
    let (past_l, past_r) = ("25", "-25");
    let inner_track = [format!("-{leaf} - 5"), "-5".to_string()];
    let outer_track = ["5".to_string(), format!("{leaf} + 5")];
    let mut forms = door_frame(&block);
    forms.push(leaf_form([&left, past_l], [&inner_track[0], &inner_track[1]]));
    forms.push(leaf_form([past_r, &right], [&outer_track[0], &outer_track[1]]));
    forms.push(block(["15", "55"], [&outer_track[1], &format!("{leaf} + 30")], ["1000", "1150"], &metal));
    out.push((
        w(msg!("Sliding Door")),
        json!({
            "category": "door",
            "operation": "sliding",
            "parameters": [size(width, 2000.0), size(height, 2100.0), size(&frame, 50.0), size(&leaf, 40.0), material(&frame_mat, &metal), material(&leaf_mat, &glass)],
            "types": sizes(&[("2000 x 2100", 2000.0, 2100.0), ("1500 x 2100", 1500.0, 2100.0), ("2400 x 2100", 2400.0, 2100.0)]),
            "forms": forms,
            // Each leaf a panel on its track, at every detail level, as Revit's sliding doors draw theirs.
            "symbols": [
                {"line": [[&left, &inner_track[0]], [past_l, &inner_track[0]], [past_l, &inner_track[1]], [&left, &inner_track[1]], [&left, &inner_track[0]]], "show": every},
                {"line": [[past_r, &outer_track[0]], [&right, &outer_track[0]], [&right, &outer_track[1]], [past_r, &outer_track[1]], [past_r, &outer_track[0]]], "show": every}
            ]
        }),
    ));

    // The frame of a window: its jambs, its head and its sill, as deep as it says or as the wall is, if that's less.
    let deep = format!("min({depth}, {wall})");
    let (half_d, minus_half_d) = (format!("{deep} / 2"), format!("-{deep} / 2"));
    let (inner_l, inner_r, inner_b, inner_t) = (left.clone(), right.clone(), frame.clone(), under_head.clone());
    let window_frame = |bx: Block<'_>| {
        vec![
            bx([&minus_half_w, &inner_l], [&minus_half_d, &half_d], ["0", height], &frame_mat),
            bx([&inner_r, &half_w], [&minus_half_d, &half_d], ["0", height], &frame_mat),
            bx([&inner_l, &inner_r], [&minus_half_d, &half_d], [&inner_t, height], &frame_mat),
            bx([&inner_l, &inner_r], [&minus_half_d, &half_d], ["0", &inner_b], &frame_mat),
        ]
    };
    let window_sizes = sizes(&[("1200 x 1500", 1200.0, 1500.0), ("600 x 900", 600.0, 900.0), ("1800 x 1200", 1800.0, 1200.0)]);
    let mut types = window_sizes.clone();
    if let Some(t) = types.as_object_mut() {
        for (name, at) in [("600 x 900", 1100.0), ("1800 x 1200", 1000.0)] {
            if let Some(Value::Object(set)) = t.get_mut(name) {
                set.insert(sill.into(), json!(at));
            }
        }
    }
    let pane = |x: [&str; 2], z: [&str; 2]| block(x, ["-3", "3"], z, &glass);

    // A fixed window: its frame round one pane.
    let mut forms = window_frame(&block);
    forms.push(pane([&inner_l, &inner_r], [&inner_b, &inner_t]));
    out.push((
        w(msg!("Fixed Window")),
        json!({
            "category": "window",
            "operation": "fixed",
            "parameters": [size(width, 1200.0), size(height, 1500.0), size(sill, 900.0), size(&frame, 50.0), size(&depth, 90.0), material(&frame_mat, &timber)],
            "types": types.clone(),
            "forms": forms
        }),
    ));

    // A casement window: two sashes meeting in the middle, each a box with a void through it, its bars 40 wide
    // (20 where the sashes meet), and its pane after the void, so the void leaves it whole.
    let mut forms = window_frame(&block);
    let sash = |outer: [&str; 2], bars: [i32; 2]| -> Vec<Value> {
        let inside = [format!("({}) + {}", outer[0], bars[0]), format!("({}) - {}", outer[1], bars[1])];
        let (low, high) = (format!("{inner_b} + 40"), format!("{inner_t} - 40"));
        let mut hole = block([&inside[0], &inside[1]], ["-30", "30"], [&low, &high], "");
        hole["void"] = json!(true);
        vec![block(outer, ["-20", "20"], [&inner_b, &inner_t], &frame_mat), hole, pane([&inside[0], &inside[1]], [&low, &high])]
    };
    forms.extend(sash([&inner_l, "-1"], [40, 20]));
    forms.extend(sash(["1", &inner_r], [20, 40]));
    out.push((
        w(msg!("Casement Window")),
        json!({
            "category": "window",
            "operation": "double-swing",
            "parameters": [size(width, 1200.0), size(height, 1500.0), size(sill, 900.0), size(&frame, 50.0), size(&depth, 90.0), material(&frame_mat, &timber)],
            "types": types.clone(),
            "forms": forms
        }),
    ));

    // A sliding window (janela de correr): two sashes on two tracks, each a frame of bars 40 wide round its pane,
    // overlapping 40 in the middle; the one toward the exterior slides past the other.
    let mut forms = window_frame(&block);
    let (low, high) = (format!("{inner_b} + 40"), format!("{inner_t} - 40"));
    let sliding_sash = |x: [&str; 2], y: [&str; 2]| -> Vec<Value> {
        let (stile_l, stile_r) = (format!("({}) + 40", x[0]), format!("({}) - 40", x[1]));
        let middle = format!("(({}) + ({})) / 2", y[0], y[1]);
        let pane_y = [format!("{middle} - 3"), format!("{middle} + 3")];
        vec![
            block([x[0], &stile_l], y, [&inner_b, &inner_t], &frame_mat),
            block([&stile_r, x[1]], y, [&inner_b, &inner_t], &frame_mat),
            block([&stile_l, &stile_r], y, [&inner_b, &low], &frame_mat),
            block([&stile_l, &stile_r], y, [&high, &inner_t], &frame_mat),
            block([&stile_l, &stile_r], [&pane_y[0], &pane_y[1]], [&low, &high], &glass),
        ]
    };
    let (track_in, track_out) = ([format!("-{deep} / 2 + 5"), "-2".to_string()], ["2".to_string(), format!("{deep} / 2 - 5")]);
    forms.extend(sliding_sash([&inner_l, "20"], [&track_in[0], &track_in[1]]));
    forms.extend(sliding_sash(["-20", &inner_r], [&track_out[0], &track_out[1]]));
    let (in_y, out_y) = (format!("-{deep} / 4"), format!("{deep} / 4"));
    out.push((
        w(msg!("Sliding Window")),
        json!({
            "category": "window",
            "operation": "sliding",
            "parameters": [size(width, 1200.0), size(height, 1500.0), size(sill, 900.0), size(&frame, 50.0), size(&depth, 90.0), material(&frame_mat, &metal)],
            "types": types,
            "forms": forms,
            "symbols": [
                {"line": [[&inner_l, &in_y], ["20", &in_y]]},
                {"line": [["-20", &out_y], [&inner_r, &out_y]]}
            ]
        }),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with<'a>(values: &'a [(&'a str, f64)]) -> impl FnMut(&str) -> Result<f64, String> + 'a {
        move |n: &str| values.iter().find(|(k, _)| *k == n).map(|(_, v)| *v).ok_or_else(|| format!("no `{n}`"))
    }

    #[test]
    fn formulas_read_as_revits_do() {
        let values = [("Width", 1000.0), ("Leg Height", 700.0)];
        for (formula, want) in [
            ("Width / 2 + 10", 510.0),
            ("2 + 3 * 4", 14.0),
            ("(2 + 3) * 4", 20.0),
            ("-Width + 1", -999.0),
            ("2 ^ 3 ^ 2", 512.0),
            ("Leg Height - 2*3", 694.0),
            ("if(Width > 1000, 4, 2)", 2.0),
            ("if(Width >= 1000, 4, 2)", 4.0),
            ("if (Width = 1000, 1, 0)", 1.0),
            ("not(Width < 1000)", 1.0),
            ("and(1, 0) + or(1, 0)", 1.0),
            ("sqrt(16) + abs(-2) + min(3, 5) + max(3, 5)", 14.0),
            ("round(2.5) + roundup(2.1) + rounddown(2.9)", 8.0),
            ("sin(30) + cos(60)", 1.0),
            ("pi() * 0", 0.0),
            // The branch not taken isn't worked out.
            ("if(Width > 0, 1, 1 / 0)", 1.0),
        ] {
            let got = eval(formula, &mut with(&values)).unwrap();
            assert!((got - want).abs() < 1e-9, "{formula}: {got}, not {want}");
        }
        for (formula, says) in [
            ("Width +", "`Width +`"),
            ("Nope + 1", "Nope"),
            ("1 / 0", "isn't a number"),
            ("sqrt(-1)", "isn't a number"),
            ("if(1, 2)", "if"),
            ("frobnicate(1)", "frobnicate"),
            ("2 3", "`2 3`"),
            ("", "empty"),
        ] {
            let e = eval(formula, &mut with(&values)).unwrap_err();
            assert!(e.contains(says), "{formula}: {e}");
        }
        // Nested past reason: refused, not a stack overflow.
        let deep = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
        assert!(eval(&deep, &mut with(&values)).unwrap_err().contains("deep"));
    }

    fn table() -> Family {
        serde_json::from_value(json!({
            "parameters": [
                {"name": "Width", "value": 1000},
                {"name": "Depth", "value": 600},
                {"name": "Height", "instance": true, "value": 750},
                {"name": "Top", "formula": "Height - 30"},
                {"name": "Area", "kind": "number", "formula": "Width * Depth / 1e6"},
                {"name": "Legs", "kind": "yesno", "value": true},
                {"name": "Finish", "kind": "material", "value": "Timber"}
            ],
            "types": {"1000 x 600": {}, "Small": {"Width": 800}},
            "forms": [
                {"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "from": "Top", "to": "Height", "material": "Finish"},
                {"profile": [[0, 0], [50, 0], [50, 50], [0, 50]], "from": 0, "to": "Top", "visible": "Legs"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn types_and_components_set_parameters_and_formulas_follow() {
        let f = table();
        let v = f.values("Small", &BTreeMap::from([("Height".to_string(), json!(700))])).unwrap();
        let n = |k: &str| match v.get(k) {
            Some(Val::Number(x)) => *x,
            other => panic!("{k}: {other:?}"),
        };
        assert_eq!([n("Width"), n("Depth"), n("Height"), n("Top"), n("Area"), n("Legs")], [800.0, 600.0, 700.0, 670.0, 0.48, 1.0]);
        assert_eq!(v.get("Finish"), Some(&Val::Text("Timber".into())));
        let mut cyclic = f.clone();
        cyclic.parameters[0].formula = Some("Depth + Top".into());
        cyclic.parameters[1].formula = Some("Width - 1".into());
        let e = cyclic.values("Small", &BTreeMap::new()).unwrap_err();
        assert!(e.contains("Width") && e.contains("itself"), "{e}");
        assert!(f.values("Huge", &BTreeMap::new()).unwrap_err().contains("Huge"));
    }

    #[test]
    fn long_formulas_and_long_chains_of_them_never_exhaust_the_stack() {
        // A flat sum of many terms reads; parameters each taking the next, deeper than any family
        // needs, are refused, and neither runs a small stack dry.
        let worked = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let flat = format!("1{}", " + 1".repeat(600));
                let sum = eval(&flat, &mut |_| Ok(0.0));
                let terms = format!("P{{}}{}", " + 1".repeat(500));
                let parameters: Vec<Value> = (0..64)
                    .map(|i| {
                        if i == 63 {
                            json!({"name": "P63", "value": 1})
                        } else {
                            json!({"name": format!("P{i}"), "formula": terms.replace("{}", &(i + 1).to_string())})
                        }
                    })
                    .collect();
                let f: Family = serde_json::from_value(json!({"parameters": parameters, "types": {"A": {}}})).unwrap();
                (sum, f.values("A", &BTreeMap::new()))
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(worked.0, Ok(601.0));
        assert!(worked.1.unwrap_err().contains("deep"));
    }

    #[test]
    fn a_sweep_too_fine_to_build_is_refused_before_it_is_built() {
        let ring = |n: usize, r: f64| -> Vec<Value> {
            (0..n).map(|k| (k as f64 * std::f64::consts::TAU / n as f64).sin_cos()).map(|(s, c)| json!([r * c, r * s, 0.0])).collect()
        };
        let path: Vec<Value> = (0..800).map(|k| json!([0.0, 0.0, k as f64 * 5.0])).collect();
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "forms": [{"path": path, "section": ring(800, 50.0)}]})).unwrap();
        let e = f.check(&|_| true).unwrap_err();
        assert!(e.contains("would make more than"), "{e}");
        // The same sweep a sixth as fine is built.
        let path: Vec<Value> = (0..20).map(|k| json!([0.0, 0.0, k as f64 * 5.0])).collect();
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "forms": [{"path": path, "section": ring(40, 50.0)}]})).unwrap();
        assert!(f.check(&|_| true).is_ok());
    }

    #[test]
    fn a_type_renamed_deleted_or_duplicated_keeps_the_nested_types_it_chose() {
        let family = || -> Family {
            serde_json::from_value(json!({"types": {"A": {}, "B": {}},
                "nested": [{"family": "Handle", "type": "Standard", "type_in": {"B": "Other"}}]}))
            .unwrap()
        };
        // Deleted: the type's choice goes with it, so the family stays sound.
        let mut f = family();
        f.forget_type("B");
        f.types.remove("B");
        assert!(f.check_parametric().is_ok());
        assert!(f.nested[0].type_in.is_empty());
        // Renamed: the choice follows the type.
        let mut f = family();
        f.rename_type("A", "C");
        f.rename_type("B", "D");
        f.types = f
            .types
            .iter()
            .map(|(k, v)| {
                (
                    if k == "A" {
                        "C".into()
                    } else if k == "B" {
                        "D".into()
                    } else {
                        k.clone()
                    },
                    v.clone(),
                )
            })
            .collect();
        assert!(f.check_parametric().is_ok());
        assert_eq!(f.nested[0].type_in.get("D").map(String::as_str), Some("Other"));
        assert!(!f.nested[0].type_in.contains_key("B"));
        // Duplicated: the copy chooses what its original chose.
        let mut f = family();
        f.types.insert("E".into(), BTreeMap::new());
        f.type_like("E", "B");
        assert_eq!(f.nested[0].type_in.get("E").map(String::as_str), Some("Other"));
        assert!(f.check_parametric().is_ok());
    }

    /// A ring of `n` points on a circle, the two at `k` swapped where `swap`: it crosses itself there.
    fn circle_ring(n: usize, swap: Option<usize>) -> Vec<[f64; 2]> {
        let mut r: Vec<[f64; 2]> =
            (0..n).map(|k| (k as f64 * std::f64::consts::TAU / n as f64).sin_cos()).map(|(s, c)| [500.0 * c, 500.0 * s]).collect();
        if let Some(k) = swap {
            r.swap(k, k + 1);
        }
        r
    }

    #[test]
    fn an_outline_that_crosses_itself_is_cleared_once_at_load_not_tested_at_every_draw() {
        let tris = [[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 100.0]]];
        let family = |outline: Vec<[f64; 2]>| -> Family {
            serde_json::from_value(json!({"types": {"A": {}}, "forms": [{"mesh": tris, "outline": outline}]})).unwrap()
        };
        let mut f = family(circle_ring(5000, Some(2000)));
        revit::translate(&mut f, &json!({}), &|_| true);
        assert!(f.forms[0].outline.is_empty(), "a crossing outline of five thousand points was kept");
        // One that doesn't cross stays.
        let mut f = family(circle_ring(5000, None));
        revit::translate(&mut f, &json!({}), &|_| true);
        assert_eq!(f.forms[0].outline.len(), 5000);
        // A large outline is trusted at a draw (it was looked at when loaded): no test of its pairs of edges.
        let pieces = f.pieces("A", &f.values("A", &BTreeMap::new()).unwrap()).unwrap();
        assert_eq!(pieces[0].outline.len(), 5000);
    }

    #[test]
    fn a_checks_opening_and_wall_are_sizes_it_can_cut() {
        let with = |check: Value| -> Result<(), String> {
            let f: Family = serde_json::from_value(json!({"types": {"900": {}}, "check": {"900": check}})).unwrap();
            f.check_parametric()
        };
        assert!(with(json!({"matched": false, "opening": [900.0, 2100.0, 0.0], "wall": 200.0})).is_ok());
        assert!(with(json!({"matched": false, "opening": [900.0, 1200.0, -150.0]})).is_ok(), "an opening below the sill is Revit's");
        for bad in [
            json!({"matched": false, "opening": [-5.0, 2100.0, 0.0]}),
            json!({"matched": false, "opening": [900.0, 1e12, 0.0]}),
            json!({"matched": false, "opening": [900.0, 2100.0, 1e12]}),
        ] {
            let e = with(bad.clone()).unwrap_err();
            assert!(e.contains("isn't a usable size"), "{bad}: {e}");
        }
        for bad in [json!({"matched": false, "wall": 0.0}), json!({"matched": false, "wall": 1e12})] {
            assert!(with(bad.clone()).unwrap_err().contains("wall isn't a usable size"), "{bad}");
        }
        // JSON has no NaN; a number that overflows reads as infinite in the types, caught by the same range.
        let mut f: Family = serde_json::from_value(json!({"types": {"900": {}}, "check": {"900": {"matched": false}}})).unwrap();
        f.check.get_mut("900").unwrap().opening = Some([f64::NAN, 2100.0, 0.0]);
        assert!(f.check_parametric().is_err());
    }

    #[test]
    fn dimensions_measuring_between_too_many_references_in_all_are_refused_quickly() {
        let refs = |n: usize| -> Vec<Value> { (0..n).map(|k| json!({"name": format!("R{k}"), "at": k as f64})).collect() };
        let eq = |n: usize| -> Vec<Value> {
            (0..n).map(|k| json!({"refs": (0..256).map(|j| format!("R{j}")).collect::<Vec<_>>(), "eq": true, "id": k})).collect()
        };
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "references": refs(256), "dimensions": eq(512)})).unwrap();
        let started = std::time::Instant::now();
        let e = f.check(&|_| true).unwrap_err();
        assert!(e.contains("references in all"), "{e}");
        assert!(started.elapsed().as_secs() < 5);
        // Four of them are as many as are allowed.
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "references": refs(256), "dimensions": eq(4)})).unwrap();
        assert!(f.check_parametric().is_ok());
    }

    #[test]
    fn a_translation_stops_adding_dimensions_at_the_references_budget_and_says_so() {
        let planes: Vec<Value> = (0..300)
            .map(|k| json!({"id": format!("p{k}"), "name": format!("Plane {k}"), "normal": [1, 0, 0], "at": {"A": k as f64}}))
            .collect();
        let dimension = |k: usize| json!({"id": format!("d{k}"), "kind": "linear", "refs": (0..256).map(|j| format!("p{j}")).collect::<Vec<_>>(), "eq": true});
        let def = json!({"reference_planes": planes, "dimensions": (0..8).map(dimension).collect::<Vec<_>>()});
        let mut f: Family = serde_json::from_value(json!({"types": {"A": {}}})).unwrap();
        let made = revit::translate(&mut f, &def, &|_| true);
        let spent: usize = f.dimensions.iter().map(|d| d.refs.len()).sum();
        assert!(spent <= 4 * MAX_REFERENCES && !f.dimensions.is_empty(), "{spent} references in all");
        assert!(f.check_parametric().is_ok(), "the family it makes is one its own check takes: {:?}", f.check_parametric());
        let said = f.editable.as_ref().map(|e| e.untranslated.join("; ")).unwrap_or_default();
        assert!(said.contains("dimensions") && said.contains("more than"), "{said}");
        let _ = made;
    }

    #[test]
    fn an_arc_that_follows_its_ends_follows_their_distance_from_its_middle() {
        let at = |w: f64| BTreeMap::from([("Width".to_string(), Val::Number(w))]);
        let n = |x: f64| Expr::Number(x);
        let w = || Expr::Formula("Width".into());
        // A door's swing: its radius not bound to anything, its ends on the leaf's corner when open and shut.
        let swing = Curve {
            centre: [n(0.0), n(0.0)],
            radius: n(762.0),
            from: n(0.0),
            to: n(90.0),
            start: Some([w(), n(0.0)]),
            end: Some([n(0.0), w()]),
        };
        let (a, b) = swing.ends(&at(915.0)).unwrap();
        assert!(a.dist(P2(915.0, 0.0)) < 1e-6 && b.dist(P2(0.0, 915.0)) < 1e-6, "{a:?} {b:?}");
        // Ends on one line from the middle at different distances: a whole turn, not nothing.
        let odd = Curve { start: Some([n(100.0), n(0.0)]), end: Some([n(300.0), n(0.0)]), ..swing };
        let sweep = odd.sweep(&at(900.0), &mut HashMap::new()).unwrap();
        assert!((sweep.to - sweep.from - 360.0).abs() < 1e-9, "{sweep:?}");
    }

    #[test]
    fn a_family_that_nests_itself_is_refused_with_the_way_it_does() {
        let nesting = |inside: &str| -> Family {
            serde_json::from_value(json!({"types": {"A": {}}, "nested": [{"family": inside, "type": "A"}]})).unwrap()
        };
        let mut m = crate::model::Model::new("Test");
        m.families.insert("Self".into(), nesting("Self"));
        let e = {
            m.sync_library();
            m.check()
        }
        .unwrap_err();
        assert!(e.contains("nests itself") && e.contains("`Self`"), "{e}");
        // Through others, and where it is named.
        let mut m = crate::model::Model::new("Test");
        m.families.insert("A".into(), nesting("B"));
        m.families.insert("B".into(), nesting("C"));
        assert!(
            {
                m.sync_library();
                m.check()
            }
            .is_ok(),
            "a chain that ends is sound"
        );
        m.families.insert("C".into(), nesting("A"));
        let e = {
            m.sync_library();
            m.check()
        }
        .unwrap_err();
        assert!(e.contains("nests itself") && e.contains("`A`") && e.contains("`B`") && e.contains("`C`"), "{e}");
        // A family that holds the one it nests itself doesn't reach the project's.
        let mut m = crate::model::Model::new("Test");
        let mut a = nesting("A");
        a.embedded.insert("A".into(), serde_json::from_value(json!({"types": {"A": {}}})).unwrap());
        m.families.insert("A".into(), a);
        {
            m.sync_library();
            m.check()
        }
        .unwrap();
    }

    #[test]
    fn a_family_too_big_to_build_is_refused() {
        let ring = |n: usize| -> Vec<Value> {
            (0..n).map(|k| (k as f64 * std::f64::consts::TAU / n as f64).sin_cos()).map(|(s, c)| json!([100.0 * c, 100.0 * s])).collect()
        };
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "forms": [{"profile": ring(MAX_RING + 1), "to": 10}]})).unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("corners"));
        // Many types, each building a little: past what one family may build, refused.
        let types: serde_json::Map<String, Value> = (0..MAX_TYPES).map(|k| (format!("T{k}"), json!({}))).collect();
        let form = json!({"profile": ring(MAX_PROFILE), "to": 10});
        let f: Family = serde_json::from_value(json!({"types": types, "forms": [form, form]})).unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("triangles"));
    }

    /// A closed surface's volume: the triangles' signed tetrahedra from the origin, positive while
    /// they face out.
    fn volume(tris: &[[P3; 3]]) -> f64 {
        tris.iter().map(|[a, b, c]| crate::geom::v3::dot(*a, crate::geom::v3::cross(*b, *c)) / 6.0).sum::<f64>()
    }

    #[test]
    fn forms_sweep_their_profiles_across_their_planes() {
        let f = table();
        let v = f.values("1000 x 600", &BTreeMap::new()).unwrap();
        let pieces = f.pieces("", &v).unwrap();
        assert_eq!(pieces.len(), 2);
        let top = &pieces[0];
        assert_eq!((top.bottom, top.top, top.material.as_str(), top.round), (720.0, 750.0, "Timber", false));
        assert!((volume(&top.tris) - 1000.0 * 600.0 * 30.0).abs() < 1e-3, "{}", volume(&top.tris));
        assert_eq!(top.outline, [P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 600.0), P2(0.0, 600.0)]);
        // Hidden: no leg.
        let mut no_legs = v.clone();
        no_legs.insert("Legs".into(), Val::Number(0.0));
        assert_eq!(f.pieces("", &no_legs).unwrap().len(), 1);
        // Upright profiles: a back across x and z, swept along y; a rail across y and z, along x;
        // a round post, its sides one surface.
        let f: Family = serde_json::from_value(json!({
            "types": {"A": {}},
            "forms": [
                {"plane": "xz", "profile": [[0, 400], [500, 400], [500, 900], [0, 900]], "from": 0, "to": 40},
                {"plane": "yz", "profile": [[0, 0], [300, 0], [300, 100]], "from": 10, "to": 210},
                {"circle": {"centre": [100, 100], "radius": 20}, "from": 0, "to": 1000}
            ]
        }))
        .unwrap();
        let pieces = f.pieces("A", &f.values("A", &BTreeMap::new()).unwrap()).unwrap();
        let back = &pieces[0];
        assert!((volume(&back.tris) - 500.0 * 500.0 * 40.0).abs() < 1e-3);
        assert_eq!((back.bottom, back.top), (400.0, 900.0));
        assert_eq!(back.outline, [P2(0.0, 0.0), P2(500.0, 0.0), P2(500.0, 40.0), P2(0.0, 40.0)]);
        let rail = &pieces[1];
        assert!((volume(&rail.tris) - 300.0 * 100.0 / 2.0 * 200.0).abs() < 1e-3);
        assert_eq!(rail.outline, [P2(10.0, 0.0), P2(210.0, 0.0), P2(210.0, 300.0), P2(10.0, 300.0)]);
        let post = &pieces[2];
        let ring = crate::geom::area(&post.outline);
        assert!(
            post.round && (volume(&post.tris) - ring * 1000.0).abs() < 1e-3 && (ring / (std::f64::consts::PI * 400.0) - 1.0).abs() < 0.01
        );
        // Nothing to sweep, or a profile with no area: said.
        let mut flat = table();
        flat.forms[0].to = Expr::Formula("Top".into());
        assert!(flat.pieces("Small", &flat.values("Small", &BTreeMap::new()).unwrap()).unwrap_err().contains("form 1"));
    }

    /// The pieces of a family of `forms`, its one type built.
    fn built(forms: Value) -> Result<Vec<Piece>, String> {
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "parameters": [{"name": "R", "value": 300}], "forms": forms}))
            .map_err(|e| e.to_string())?;
        f.check(&|_| true)?;
        f.pieces("A", &f.values("A", &BTreeMap::new())?)
    }

    #[test]
    fn a_revolve_turns_its_profile_about_its_axis() {
        // A tube: a rectangle across x and z, 100 to `R` out from the z axis, turned all the way round it.
        let ring = json!([[100, 0], ["R", 0], ["R", 500], [100, 500]]);
        let tube = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 360}])).unwrap();
        let [p] = tube.as_slice() else { panic!("{tube:?}") };
        // Its sides are polygons of 32 sides: their area is a hair under the circle's.
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin() / std::f64::consts::PI;
        let want = std::f64::consts::PI * (300.0 * 300.0 - 100.0 * 100.0) * 500.0 * k;
        assert!((volume(&p.tris) / want - 1.0).abs() < 1e-6, "{} not {want}", volume(&p.tris));
        assert_eq!((p.bottom, p.top), (0.0, 500.0));
        assert!(
            (crate::geom::area(&p.outline) / (std::f64::consts::PI * 300.0 * 300.0 * k) - 1.0).abs() < 1e-6,
            "seen from above, its rim"
        );
        // A quarter of it, closed at both ends; its axis given the other way round, or across x and y.
        let quarter = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 1], [0, 0]], "from": 0, "to": 90}])).unwrap();
        assert!((volume(&quarter[0].tris) / (want / 4.0) - 1.0).abs() < 1e-6, "{}", volume(&quarter[0].tris));
        let half = built(json!([{"profile": [[100, 0], [300, 0], [300, 50], [100, 50]], "axis": [[0, 0], [0, 1]], "to": 180}])).unwrap();
        let ring_of = std::f64::consts::PI * (300.0 * 300.0 - 100.0 * 100.0) * 50.0 * k;
        assert!((volume(&half[0].tris) / (ring_of / 2.0) - 1.0).abs() < 1e-6, "{}", volume(&half[0].tris));
        assert!((half[0].top - half[0].bottom - 300.0).abs() < 1e-6, "turned out of its plane, 300 to one side");
        // A profile across its axis, an axis of one point, a turn of nothing or more than all the way: said.
        for (form, says) in [
            (json!({"plane": "xz", "profile": [[-100, 0], [300, 0], [300, 500]], "axis": [[0, 0], [0, 1]], "to": 360}), "crosses its axis"),
            (json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 0]], "to": 360}), "axis"),
            (json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "from": 90, "to": 90}), "nothing"),
            (json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 400}), "360"),
        ] {
            let e = built(json!([form])).unwrap_err();
            assert!(e.contains(says) && e.contains("form 1"), "{e}");
        }
    }

    #[test]
    fn a_revolve_faces_out_whatever_plane_side_and_direction_it_turns() {
        // The same tube, 100 to 300 out and 500 along: drawn across each plane, to either side of its axis, the axis given
        // either way round, from any angle: one volume, turned out.
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin();
        let want = k * (300.0 * 300.0 - 100.0 * 100.0) * 500.0;
        let across = |a0: f64, a1: f64| json!([[a0, 0], [a1, 0], [a1, 500], [a0, 500]]);
        for (plane, profile, axis) in [
            ("xz", across(100.0, 300.0), json!([[0, 0], [0, 1]])),
            ("xz", across(100.0, 300.0), json!([[0, 1], [0, 0]])),
            ("xz", across(-300.0, -100.0), json!([[0, 0], [0, 1]])),
            ("xz", across(-300.0, -100.0), json!([[0, 1], [0, 0]])),
            ("yz", across(100.0, 300.0), json!([[0, 0], [0, 1]])),
            ("yz", across(-300.0, -100.0), json!([[0, 1], [0, 0]])),
            ("xy", json!([[0, 100], [500, 100], [500, 300], [0, 300]]), json!([[0, 0], [1, 0]])),
            ("xy", json!([[0, -300], [500, -300], [500, -100], [0, -100]]), json!([[1, 0], [0, 0]])),
            ("xy", json!([[0, 100], [500, 100], [500, 300], [0, 300]]), json!([[0, 0], [1, 0]])),
        ] {
            for (from, to) in [(0, 360), (-90, 270), (30, 390)] {
                let tube = built(json!([{"plane": plane, "profile": profile, "axis": axis, "from": from, "to": to}]));
                let tube = tube.unwrap_or_else(|e| panic!("{plane} {profile} {axis}: {e}"));
                let v = volume(&tube[0].tris);
                assert!((v / want - 1.0).abs() < 1e-6, "{plane} {profile} {axis} {from} to {to}: {v} not {want}");
            }
        }
        // A part turn is closed by two caps, turned out. About an axis beside the origin, with neither cap in a plane through it
        // (a cap in one adds nothing to the volume, however it faces), the volume is the sector's only if both are.
        let sector = k / 4.0 * (300.0 * 300.0 - 100.0 * 100.0) * 500.0;
        for (from, to) in [(30, 120), (-120, -30)] {
            for axis in [json!([[1000, 0], [1000, 1]]), json!([[1000, 1], [1000, 0]])] {
                let part =
                    built(json!([{"plane": "xz", "profile": across(1100.0, 1300.0), "axis": axis, "from": from, "to": to}])).unwrap();
                let v = volume(&part[0].tris);
                assert!((v / sector - 1.0).abs() < 1e-6, "{axis} {from} to {to}: {v} not {sector}");
            }
        }
    }

    #[test]
    fn a_part_turn_about_an_upright_axis_shows_a_slice_in_plan_and_a_circle_makes_a_torus() {
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin();
        let ring = json!([[100, 0], [300, 0], [300, 500], [100, 500]]);
        let quarter = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 90}])).unwrap();
        let outline = &quarter[0].outline;
        // 4 sin δ (R² − r²) where δ is a step's angle: a quarter of the ring, as the polygon makes it, counter-clockwise, from +x.
        assert!((crate::geom::area(outline) / (k / 4.0 * (300.0 * 300.0 - 100.0 * 100.0)) - 1.0).abs() < 1e-6, "{outline:?}");
        assert!(outline.iter().all(|p| p.0 > -1e-6 && p.1 > -1e-6), "in the first quarter: {outline:?}");
        // The same turned the other way round the axis, the other way round the plan.
        let down = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 1], [0, 0]], "to": 90}])).unwrap();
        assert!(down[0].outline.iter().all(|p| p.0 > -1e-6 && p.1 < 1e-6), "{:?}", down[0].outline);
        // A profile on the axis: a slice of a disc, its centre one of its corners.
        let wedge =
            built(json!([{"plane": "xz", "profile": [[0, 0], [300, 0], [300, 500], [0, 500]], "axis": [[0, 0], [0, 1]], "to": 90}]));
        assert!((crate::geom::area(&wedge.unwrap()[0].outline) / (k / 4.0 * 300.0 * 300.0) - 1.0).abs() < 1e-6);
        // A circle turned about an axis beside it: a torus. Its volume is its circle's area times the way its centre goes.
        let donut = built(json!([{"plane": "xz", "circle": {"centre": [500, 100], "radius": 100}, "axis": [[0, 0], [0, 1]], "to": 360}]));
        let donut = donut.unwrap();
        let (v, exact) = (volume(&donut[0].tris), 2.0 * std::f64::consts::PI.powi(2) * 500.0 * 100.0 * 100.0);
        assert!((v / exact - 1.0).abs() < 0.03, "{v} not about {exact}");
        assert_eq!((donut[0].bottom, donut[0].top), (0.0, 200.0), "its circle's lowest and highest");
        // A mesh has no axis to turn about, and a form of more than one kind is still refused.
        let f: Family = serde_json::from_value(
            json!({"types": {"A": {}}, "forms": [{"mesh": [[[0, 0, 0], [1, 0, 0], [0, 1, 0]]], "axis": [[0, 0], [0, 1]]}]}),
        )
        .unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("axis"));
    }

    #[test]
    fn a_void_cuts_the_forms_it_passes_through_and_shows_nothing_itself() {
        let slab = json!({"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "from": 0, "to": 100});
        let hole = |shown: Value| json!({"profile": [[400, 400], [600, 400], [600, 600], [400, 600]], "from": -10, "to": 110, "void": true, "visible": shown});
        let cut = built(json!([slab, hole(json!(1))])).unwrap();
        let [p] = cut.as_slice() else { panic!("{cut:?}") };
        assert!((volume(&p.tris) - (1000.0 * 1000.0 - 200.0 * 200.0) * 100.0).abs() < 1e-3, "{}", volume(&p.tris));
        // Hidden, it cuts nothing; away from the slab, nothing is cut.
        assert!((volume(&built(json!([slab, hole(json!(0))])).unwrap()[0].tris) - 1e8).abs() < 1e-3);
        let away = json!({"profile": [[2000, 0], [2100, 0], [2100, 100], [2000, 100]], "to": 100, "void": true});
        assert!((volume(&built(json!([slab, away])).unwrap()[0].tris) - 1e8).abs() < 1e-3);
        // A void cuts what's listed before it: a form after it is whole, as a pane in the hole of a sash.
        let pane = json!({"profile": [[450, 450], [550, 450], [550, 550], [450, 550]], "from": 0, "to": 100});
        let both = built(json!([slab, hole(json!(1)), pane])).unwrap();
        assert_eq!(both.len(), 2);
        assert!((volume(&both[0].tris) - 9.6e7).abs() < 1e-3 && (volume(&both[1].tris) - 100.0 * 100.0 * 100.0).abs() < 1e-3);
        // A round bowl out of a block, as a basin's: what's left is the block less the bowl.
        let block = json!({"profile": [[-300, -300], [300, -300], [300, 300], [-300, 300]], "from": 0, "to": 200});
        let bowl = json!({"plane": "xz", "profile": [[0, 50], [200, 50], [200, 250], [0, 250]], "axis": [[0, 0], [0, 1]], "to": 360, "void": true});
        let basin = built(json!([block, bowl])).unwrap();
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin();
        assert!((volume(&basin[0].tris) - (600.0 * 600.0 * 200.0 - k * 200.0 * 200.0 * 150.0)).abs() < 1.0, "{}", volume(&basin[0].tris));
    }

    #[test]
    fn voids_of_many_triangles_never_exhaust_the_stack() {
        // Two revolves of 64 corners each, the one cutting the other, on a small stack: done, and quickly.
        let ring: Vec<Value> = (0..MAX_PROFILE)
            .map(|k| (k as f64 * std::f64::consts::TAU / MAX_PROFILE as f64).sin_cos())
            .map(|(s, c)| json!([400.0 + 100.0 * c, 300.0 + 100.0 * s]))
            .collect();
        let worked = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                let started = std::time::Instant::now();
                let solid = json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 360});
                let void =
                    json!({"profile": [[-1000, -1000], [1000, -1000], [1000, 0], [-1000, 0]], "from": -10, "to": 1000, "void": true});
                (built(json!([solid, void])), started.elapsed())
            })
            .unwrap()
            .join()
            .unwrap();
        let (pieces, took) = worked;
        let pieces = pieces.unwrap();
        assert!(volume(&pieces[0].tris) > 0.0 && took.as_secs_f64() < 10.0, "{took:?}: a hang, not a slow machine");
    }
    /// A one-type lighting family holding `connectors`.
    fn lamp(connectors: Value) -> Family {
        serde_json::from_value(json!({
            "category": "lighting",
            "parameters": [{"name": "Load", "kind": "number", "value": 0}],
            "types": {"9 W": {"Load": 9}, "18 W": {"Load": 18}},
            "connectors": connectors
        }))
        .unwrap()
    }

    fn power() -> Value {
        json!({"kind": "power", "system": "power-balanced", "poles": 1, "voltage": 127, "load_va": "Load", "power_factor": 0.9,
               "classification": "lighting"})
    }

    #[test]
    fn a_family_holds_connectors_and_a_connector_reads_its_load_from_the_type() {
        let f = lamp(json!([power()]));
        assert_eq!(f.check(&|_| true), Ok(()));
        for (t, load) in [("9 W", 9.0), ("18 W", 18.0)] {
            let c = f.connector_values(t, &BTreeMap::new()).unwrap();
            assert_eq!(c.len(), 1);
            assert_eq!((c[0].kind, c[0].system, c[0].poles), (ConnectorKind::Power, Some(ConnectorSystem::PowerBalanced), 1));
            assert_eq!((c[0].voltage, c[0].load_va, c[0].power_factor), (127.0, load, 0.9));
            assert_eq!((c[0].load_va_by_phase.clone(), c[0].classification.as_str()), (vec![load], "lighting"));
        }
        // Written back as it was read, so a loaded family file keeps them.
        let back: Family = serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn a_family_without_connectors_is_as_it_was() {
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}})).unwrap();
        assert!(f.connectors.is_empty());
        assert_eq!(f.check(&|_| true), Ok(()));
        assert!(serde_json::to_value(&f).unwrap().get("connectors").is_none(), "an empty list isn't written");
        assert!(f.connector_values("A", &BTreeMap::new()).unwrap().is_empty());
    }

    #[test]
    fn two_power_connectors_are_refused_and_the_message_says_so() {
        let e = lamp(json!([power(), power()])).check(&|_| true).unwrap_err();
        assert!(e.contains("two power connectors") && e.contains("keep one"), "{e}");
        // A switch beside a power connector is no second power connector.
        let sw = json!({"kind": "switch", "system": "switch"});
        assert_eq!(lamp(json!([power(), sw])).check(&|_| true), Ok(()));
    }

    #[test]
    fn a_connector_is_checked_for_what_it_says() {
        let bad = |change: Value, says: &str| {
            let mut c = power();
            c.as_object_mut().unwrap().extend(change.as_object().unwrap().clone());
            let e = lamp(json!([c])).check(&|_| true).unwrap_err();
            assert!(e.contains(says), "{change}: {e}");
        };
        bad(json!({"system": "switch"}), "system");
        bad(json!({"poles": 0}), "poles");
        bad(json!({"poles": 4}), "poles");
        bad(json!({"voltage": 0}), "voltage");
        bad(json!({"voltage": -127}), "voltage");
        bad(json!({"voltage": 1e12}), "voltage");
        bad(json!({"power_factor": 0}), "power factor");
        bad(json!({"power_factor": 1.5}), "power factor");
        bad(json!({"load_va": -1}), "load");
        bad(json!({"load_va": 1e300}), "load");
        bad(json!({"load_va": "Nope"}), "Nope");
        bad(json!({"load_va": [1, 2]}), "per phase");
        bad(json!({"classification": "x".repeat(MAX_TEXT + 1)}), "classification");
        let sw = |extra: Value| {
            let mut c = json!({"kind": "switch", "system": "switch"});
            c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            lamp(json!([c])).check(&|_| true)
        };
        assert_eq!(sw(json!({})), Ok(()));
        assert!(sw(json!({"load_va": 10})).unwrap_err().contains("switch"));
        assert!(sw(json!({"system": "power-balanced"})).unwrap_err().contains("system"));
        let many: Vec<Value> = (0..=MAX_CONNECTORS).map(|_| json!({"kind": "switch", "system": "switch"})).collect();
        assert!(lamp(json!(many)).check(&|_| true).unwrap_err().contains("connectors"));
    }

    #[test]
    fn an_unbalanced_connector_may_load_each_phase_differently() {
        let f = lamp(json!([{"kind": "power", "system": "power-unbalanced", "poles": 3, "voltage": 220,
                              "load_va": [1000, 2000, "Load"], "power_factor": 0.8}]));
        assert_eq!(f.check(&|_| true), Ok(()));
        let c = &f.connector_values("9 W", &BTreeMap::new()).unwrap()[0];
        assert_eq!(c.load_va_by_phase, vec![1000.0, 2000.0, 9.0]);
        assert_eq!(c.load_va, 3009.0);
        // A single number is shared out evenly; a balanced connector takes no list.
        let shared = lamp(json!([{"kind": "power", "system": "power-unbalanced", "poles": 3, "voltage": 220, "load_va": 3000}]));
        assert_eq!(shared.connector_values("9 W", &BTreeMap::new()).unwrap()[0].load_va_by_phase, vec![1000.0; 3]);
        let e = lamp(json!([{"kind": "power", "system": "power-balanced", "poles": 3, "voltage": 220, "load_va": [1, 2, 3]}]))
            .check(&|_| true)
            .unwrap_err();
        assert!(e.contains("balanced"), "{e}");
    }
}
