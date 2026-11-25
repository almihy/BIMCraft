//! Curtain walls, as Revit's: a wall whose type is a curtain type ([`CurtainType`] in its [`WallType`]). It has no layers
//! and no solid of its own: a grid of lines along it and up it, a panel in each cell and mullions on the lines' segments.
//!
//! - **The grid** is the type's (its Vertical Grid and Horizontal Grid: a [`GridRule`] each), placed by the wall's own
//!   Number, Justification and Offset ([`WallGrid`]), and the lines drawn by hand: [`GridLine`] elements the wall hosts.
//!   [`grid`] works it out; nothing of the type's lines is stored.
//! - **Panels and mullions are elements** the wall hosts ([`Panel`], [`Mullion`]), each holding where it is in the wall
//!   (mm along its location line from its start, and mm over its base), so what draws, measures or exports one reads it
//!   and its wall's line and nothing else. The wall lays them out again after a command that changed it, its type, a type
//!   its type names or one of its lines ([`keep`], which the engine runs and [`apply`] makes so).
//! - **Identity.** A line has a key: a border (`u0`, `u1`, `v0`, `v1`), the type's (`ut3`: its fourth from the
//!   justification's origin) or a hand-made line's GUID (`g…`). A panel's key is its lower left lines', a mullion's its line's
//!   and that of the line its segment starts at. An element's GUID is made of its wall's and its key ([`child_guid`]), the
//!   same in every copy of the project: two copies laying one wall out make the same elements, never two of each. One
//!   whose key is still there keeps its id, its parameters and, freed, its type.
//! - **Pinned is the type's**, as Revit's: what the wall placed counts as pinned ([`placed`]); Unpin frees it (`free`),
//!   and it then keeps the type a person gave it. A mullion deleted is remembered on the wall (`no_mullion`), a segment
//!   removed to join two panels too (`merged`).
//! - **A door** in a curtain wall is Revit's panel swapped for a door: a door (`Element::Door`) the wall hosts whose
//!   `panel` is the key of the cell it fills. It is a person's element (its own GUID, a door in schedules and IFC); the
//!   layout places no panel in its cell and writes where the cell is on it ([`door_fits`]), so it is as large as its cell
//!   and goes where its cell goes. A door whose cell is gone goes to the cell that holds the place it was; with no such
//!   cell, or in a wall that is no curtain wall any more, it is deleted ([`Update::doors`]: by the engine after the
//!   command, as any door is deleted).

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_i18n::ui_msg;
use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::model::{Changed, Element, Guid, Id, Model, TOL, Wall, WallType};
use crate::walls::Axis;

/// The most lines a grid has one way: past it a spacing is too small for its wall, and the rest are left out.
pub const MAX_LINES: usize = 500;
/// Two lines nearer than this are one, mm.
pub const NEAR: f64 = 1.0;

/// How a curtain type spaces its grid lines one way: Revit's Layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    #[default]
    None,
    FixedDistance,
    FixedNumber,
    MaximumSpacing,
    MinimumSpacing,
}

impl Layout {
    pub const ALL: [Layout; 5] = [Layout::None, Layout::FixedDistance, Layout::FixedNumber, Layout::MaximumSpacing, Layout::MinimumSpacing];

    /// Its name in commands and files.
    pub fn name(self) -> &'static str {
        match self {
            Layout::None => "none",
            Layout::FixedDistance => "fixed-distance",
            Layout::FixedNumber => "fixed-number",
            Layout::MaximumSpacing => "maximum-spacing",
            Layout::MinimumSpacing => "minimum-spacing",
        }
    }

    /// As Type Properties names it, Revit's words.
    pub fn label(self) -> &'static str {
        match self {
            Layout::None => ui_msg!("None"),
            Layout::FixedDistance => ui_msg!("Fixed Distance"),
            Layout::FixedNumber => ui_msg!("Fixed Number"),
            Layout::MaximumSpacing => ui_msg!("Maximum Spacing"),
            Layout::MinimumSpacing => ui_msg!("Minimum Spacing"),
        }
    }

    pub fn from_name(name: &str) -> Option<Layout> {
        let n = name.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        Layout::ALL.into_iter().find(|l| l.name() == n)
    }

    fn is_none(&self) -> bool {
        *self == Layout::None
    }
}

/// A curtain type's grid one way: its Layout, its Spacing (mm; a Fixed Number has none) and Adjust for Mullion Size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridRule {
    #[serde(default, skip_serializing_if = "Layout::is_none")]
    pub layout: Layout,
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub spacing: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adjust_for_mullion: bool,
}

impl GridRule {
    fn is_default(&self) -> bool {
        *self == GridRule::default()
    }
}

/// The mullion types a curtain type puts on its lines one way: on those between its borders (Interior Type), on its
/// first border (Border 1 Type: a vertical grid's at the wall's start, a horizontal one's at its base) and on its last
/// (Border 2 Type). Each one of the project's mullion types by name, or empty: none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mullions {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub interior: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub border1: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub border2: String,
}

impl Mullions {
    fn is_empty(&self) -> bool {
        self.interior.is_empty() && self.border1.is_empty() && self.border2.is_empty()
    }

    /// The three names, with what each is called in a message.
    pub fn named(&self) -> [(&'static str, &str); 3] {
        [("interior", &self.interior), ("border 1", &self.border1), ("border 2", &self.border2)]
    }

    pub(crate) fn names_mut(&mut self) -> [&mut String; 3] {
        [&mut self.interior, &mut self.border1, &mut self.border2]
    }
}

/// Which mullions run through where they cross: Revit's Join Condition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JoinCondition {
    #[default]
    NotDefined,
    VerticalGridContinuous,
    HorizontalGridContinuous,
    BorderAndVerticalGridContinuous,
    BorderAndHorizontalGridContinuous,
}

impl JoinCondition {
    pub const ALL: [JoinCondition; 5] = [
        JoinCondition::NotDefined,
        JoinCondition::VerticalGridContinuous,
        JoinCondition::HorizontalGridContinuous,
        JoinCondition::BorderAndVerticalGridContinuous,
        JoinCondition::BorderAndHorizontalGridContinuous,
    ];

    pub fn name(self) -> &'static str {
        match self {
            JoinCondition::NotDefined => "not-defined",
            JoinCondition::VerticalGridContinuous => "vertical-grid-continuous",
            JoinCondition::HorizontalGridContinuous => "horizontal-grid-continuous",
            JoinCondition::BorderAndVerticalGridContinuous => "border-and-vertical-grid-continuous",
            JoinCondition::BorderAndHorizontalGridContinuous => "border-and-horizontal-grid-continuous",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            JoinCondition::NotDefined => ui_msg!("Not Defined"),
            JoinCondition::VerticalGridContinuous => ui_msg!("Vertical Grid Continuous"),
            JoinCondition::HorizontalGridContinuous => ui_msg!("Horizontal Grid Continuous"),
            JoinCondition::BorderAndVerticalGridContinuous => ui_msg!("Border and Vertical Grid Continuous"),
            JoinCondition::BorderAndHorizontalGridContinuous => ui_msg!("Border and Horizontal Grid Continuous"),
        }
    }

    pub fn from_name(name: &str) -> Option<JoinCondition> {
        let n = name.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        JoinCondition::ALL.into_iter().find(|l| l.name() == n)
    }

    fn is_default(&self) -> bool {
        *self == JoinCondition::NotDefined
    }

    /// Whether a vertical mullion runs through a horizontal one where they cross (else the horizontal one does). A
    /// border's runs through what ends on it either way, and with a Border and … condition through the other border too:
    /// a vertical border through a horizontal one with Border and Vertical Grid Continuous, and the other way round.
    fn vertical_through(self, vertical_border: bool, horizontal_border: bool) -> bool {
        match (vertical_border, horizontal_border) {
            (true, false) => true,
            (false, true) => false,
            _ => matches!(
                self,
                JoinCondition::NotDefined | JoinCondition::VerticalGridContinuous | JoinCondition::BorderAndVerticalGridContinuous
            ),
        }
    }
}

/// What makes a wall type a curtain type, as Revit's Type Properties of a curtain wall: the panel its cells are filled
/// with (one of the project's curtain panel types; empty: the project's first glazed one), its Join Condition, its grid
/// and its mullions each way. The vertical grid's lines stand along the wall; the horizontal grid's lie up it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurtainType {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub panel: String,
    #[serde(default, skip_serializing_if = "JoinCondition::is_default")]
    pub join_condition: JoinCondition,
    #[serde(default, skip_serializing_if = "GridRule::is_default")]
    pub vertical: GridRule,
    #[serde(default, skip_serializing_if = "GridRule::is_default")]
    pub horizontal: GridRule,
    #[serde(default, skip_serializing_if = "Mullions::is_empty")]
    pub vertical_mullions: Mullions,
    #[serde(default, skip_serializing_if = "Mullions::is_empty")]
    pub horizontal_mullions: Mullions,
    /// Revit's Automatically Embed: a wall of the type drawn inside a basic wall, along it, cuts its own opening in that
    /// wall and stands in it ([`embeds`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub embed: bool,
}

/// A curtain panel type, Revit's System Panel: `thickness` mm of `material` (one of the library's, or none), its middle
/// `offset` mm to the exterior side of its wall's location line. An `empty` one is Revit's Empty System Panel: a cell
/// left open, which has no solid.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelType {
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub empty: bool,
    #[serde(default, skip_serializing_if = "crate::type_data::TypeIdentity::is_empty")]
    pub identity: crate::type_data::TypeIdentity,
}

/// A mullion type, Revit's Rectangular Mullion or (`round`) Circular Mullion. A rectangular one is `thickness` mm deep
/// across its wall and `side1` + `side2` mm wide along it: `side1` to one side of its grid line (toward the wall's start,
/// or its base), `side2` to the other. A circular one is `thickness` mm across (twice its Radius). Its middle is `offset`
/// mm to the exterior side of the wall's location line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MullionType {
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub side1: f64,
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub side2: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub round: bool,
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(default, skip_serializing_if = "crate::type_data::TypeIdentity::is_empty")]
    pub identity: crate::type_data::TypeIdentity,
}

impl MullionType {
    /// How far it reaches to each side of its grid line, mm: a circular one its radius both ways.
    pub fn sides(&self) -> (f64, f64) {
        if self.round { (self.thickness / 2.0, self.thickness / 2.0) } else { (self.side1, self.side2) }
    }

    /// How wide it is along its wall, mm.
    pub fn width(&self) -> f64 {
        let (a, b) = self.sides();
        a + b
    }
}

/// Where a grid starts counting from: Revit's Justification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Justification {
    #[default]
    Beginning,
    #[serde(alias = "centre")]
    Center,
    End,
}

impl Justification {
    pub const ALL: [Justification; 3] = [Justification::Beginning, Justification::Center, Justification::End];

    pub fn name(self) -> &'static str {
        match self {
            Justification::Beginning => "beginning",
            Justification::Center => "center",
            Justification::End => "end",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Justification::Beginning => ui_msg!("Beginning"),
            Justification::Center => ui_msg!("Center"),
            Justification::End => ui_msg!("End"),
        }
    }

    pub fn from_name(name: &str) -> Option<Justification> {
        let n = name.trim().to_ascii_lowercase().replace("centre", "center");
        Justification::ALL.into_iter().find(|l| l.name() == n)
    }

    fn is_default(&self) -> bool {
        *self == Justification::Beginning
    }
}

/// The lines a Fixed Number grid has unless its wall says: Revit's own.
pub const NUMBER: u32 = 4;
/// The most lines a wall's Number asks for, Revit's own limit.
pub const MAX_NUMBER: u32 = 200;

fn number() -> u32 {
    NUMBER
}

fn is_number(n: &u32) -> bool {
    *n == NUMBER
}

/// A curtain wall's own settings of its grid one way, Revit's instance properties: the Number of lines a Fixed Number
/// layout has, the Justification its lines are counted from, and the Offset of the first from there, mm.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridSet {
    #[serde(default = "number", skip_serializing_if = "is_number")]
    pub number: u32,
    #[serde(default, skip_serializing_if = "Justification::is_default")]
    pub justification: Justification,
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub offset: f64,
}

impl Default for GridSet {
    fn default() -> GridSet {
        GridSet { number: NUMBER, justification: Justification::Beginning, offset: 0.0 }
    }
}

impl GridSet {
    fn is_default(&self) -> bool {
        *self == GridSet::default()
    }
}

/// What a wall keeps of its curtain grid: its own settings each way, the segments of grid lines removed so the panels
/// either side are one (`merged`, each a segment's key), and the mullions deleted from it (`no_mullion`, each a
/// mullion's key). A basic wall's is empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallGrid {
    #[serde(default, skip_serializing_if = "GridSet::is_default")]
    pub vertical: GridSet,
    #[serde(default, skip_serializing_if = "GridSet::is_default")]
    pub horizontal: GridSet,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub merged: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub no_mullion: BTreeSet<String>,
}

impl WallGrid {
    pub fn is_default(&self) -> bool {
        *self == WallGrid::default()
    }
}

/// A curtain grid line drawn by hand on curtain wall `host`, Revit's Curtain Grid: `vertical`, it stands `at` mm along
/// the wall's location line from its start; else it lies `at` mm over the wall's base.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridLine {
    pub host: Guid,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical: bool,
    pub at: f64,
}

/// A curtain panel of curtain wall `host`: of one of the project's curtain panel types, filling the cell named `key`,
/// from `span[0]` to `span[1]` mm along the wall's location line and from `span[2]` to `span[3]` mm over its base. The
/// wall places it and lays it out again; `free` (unpinned), it keeps its type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    #[serde(rename = "type")]
    pub type_name: String,
    pub host: Guid,
    pub key: String,
    pub span: [f64; 4],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub free: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A mullion of curtain wall `host`: of one of the project's mullion types, on the segment named `key` of a grid line.
/// `vertical`, it stands `at` mm along the wall from its start, from `from` to `to` mm over the wall's base; else it lies
/// `at` mm over the base, from `from` to `to` mm along the wall. `trim` is how far each end stops short of its segment's
/// (negative: runs past it), where it meets a mullion that runs through or runs through one itself. `flip` swaps its two
/// sides (a border's, so its Side 1 is always toward the wall's middle). The wall places it; `free` (unpinned, or put on
/// by hand), it keeps its type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mullion {
    #[serde(rename = "type")]
    pub type_name: String,
    pub host: Guid,
    pub key: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical: bool,
    pub at: f64,
    pub from: f64,
    pub to: f64,
    #[serde(default, skip_serializing_if = "crate::model::is_zero2")]
    pub trim: [f64; 2],
    /// On a border of the grid (1: the wall's start or base; 2: its end or top), it lies wholly inside the wall, its outer
    /// side on the border, as Revit's; between borders (0) it is about its grid line.
    #[serde(default, skip_serializing_if = "is_no_border")]
    pub border: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub free: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

fn is_no_border(b: &u8) -> bool {
    *b == 0
}

/// How far a mullion of type `t` on border `border` reaches to the lower and to the upper side of its grid line, mm (the
/// lower: toward the wall's start, or its base): about the line between borders, wholly inside the wall on one.
pub fn reach(t: &MullionType, border: u8) -> (f64, f64) {
    let (a, b) = t.sides();
    match border {
        1 => (0.0, a + b),
        2 => (a + b, 0.0),
        _ => (a, b),
    }
}

impl Mullion {
    /// Its length, mm: its segment's, less what its ends stop short by.
    pub fn length(&self) -> f64 {
        (self.to - self.from - self.trim[0] - self.trim[1]).max(0.0)
    }

    /// Where it starts and ends along its line, trimmed.
    pub fn ends(&self) -> (f64, f64) {
        (self.from + self.trim[0], self.to - self.trim[1])
    }
}

impl Panel {
    /// Its width along the wall and its height, mm.
    pub fn size(&self) -> (f64, f64) {
        (self.span[1] - self.span[0], self.span[3] - self.span[2])
    }
}

// ====================================================================== the grid
/// A grid line of a curtain wall: its key, where it is (mm along the wall for a vertical one, mm over the wall's base for a
/// horizontal one), the hand-made line it is (its element's GUID), and which border (0: none; 1: the first; 2: the last).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub key: String,
    pub at: f64,
    pub manual: Option<Guid>,
    pub border: u8,
}

/// A curtain wall's grid: how long the wall is along its location line and how tall over its base, its vertical lines
/// from its start to its end (`u`) and its horizontal ones from its base to its top (`v`), the borders first and last.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub length: f64,
    pub height: f64,
    pub u: Vec<Line>,
    pub v: Vec<Line>,
}

/// The wall type of wall `w` when it is a curtain type.
pub fn curtain_of<'a>(m: &'a Model, w: &Wall) -> Option<&'a CurtainType> {
    m.types.wall.get(&w.type_name).and_then(|t| t.curtain.as_ref())
}

/// Whether wall `w` is a curtain wall.
pub fn is_curtain(m: &Model, w: &Wall) -> bool {
    curtain_of(m, w).is_some()
}

/// Where a rule's own lines lie over `extent` mm: each with the number it is counted by from the justification's origin.
/// `widths`: how wide the mullions on the first border, between the borders and on the last border are (0 where the type puts
/// none), which a rule that ticks Adjust for Mullion Size divides by, so the panels show the same width of glass: the
/// borders' mullions lie wholly inside the wall and each line between has half its mullion on either side.
fn rule_lines(rule: &GridRule, set: &GridSet, extent: f64, widths: [f64; 3]) -> Vec<(i64, f64)> {
    let even = |panels: usize| -> Vec<(i64, f64)> {
        let n = panels.min(MAX_LINES + 1);
        let [b1, w, b2] = widths;
        let clear = (extent - b1 - b2 - w * (n as f64 - 1.0)) / n as f64;
        if rule.adjust_for_mullion && clear > NEAR {
            return (1..n).map(|i| (i as i64, b1 + clear * i as f64 + w * (i as f64 - 0.5))).collect();
        }
        (1..n).map(|i| (i as i64, extent * i as f64 / n as f64)).collect()
    };
    let spacing = rule.spacing;
    match rule.layout {
        Layout::None => Vec::new(),
        Layout::FixedNumber => even(set.number as usize + 1),
        _ if !(spacing.is_finite() && spacing > NEAR) => Vec::new(),
        Layout::MaximumSpacing => even(((extent / spacing) - 1e-9).ceil().max(1.0) as usize),
        Layout::MinimumSpacing => even(((extent / spacing) + 1e-9).floor().max(1.0) as usize),
        Layout::FixedDistance => {
            // From the justification's origin, every `spacing` both ways: a line on the origin itself when the offset moves
            // it into the wall (the Beginning's and the End's own origin is a border), and the Center's at the wall's middle.
            let (origin, way) = match set.justification {
                Justification::Beginning => (set.offset, 1.0),
                Justification::Center => (extent / 2.0 + set.offset, 1.0),
                Justification::End => (extent - set.offset, -1.0),
            };
            if !origin.is_finite() {
                return Vec::new();
            }
            // The first line that may lie in the wall, counted along the way the lines run from the origin.
            let from = if way > 0.0 { origin } else { extent - origin };
            let first = ((0.0 - from) / spacing).floor() as i64 - 1;
            let mut out = Vec::new();
            let mut k = first.max(-(MAX_LINES as i64) * 4);
            while out.len() < MAX_LINES {
                let at = origin + way * spacing * k as f64;
                let along = if way > 0.0 { at } else { extent - at };
                if along >= extent - NEAR {
                    break;
                }
                if along > NEAR {
                    out.push((k, at));
                }
                k += 1;
                if k > first + (MAX_LINES as i64) * 8 {
                    break;
                }
            }
            out
        }
    }
}

/// The lines one way over `extent` mm: the borders, the rule's and the hand-made ones (`manual`: each line's GUID and where
/// it is), in order; a line within a millimetre of one before it is left out (a hand-made one before the rule's).
fn lines_of(prefix: char, rule: &GridRule, set: &GridSet, extent: f64, widths: [f64; 3], manual: &[(Guid, f64)]) -> Vec<Line> {
    let mut inner: Vec<Line> = manual
        .iter()
        .filter(|(_, at)| at.is_finite() && *at > NEAR && *at < extent - NEAR)
        .map(|(g, at)| Line { key: format!("g{g}"), at: *at, manual: Some(*g), border: 0 })
        .collect();
    inner.extend(rule_lines(rule, set, extent, widths).into_iter().map(|(k, at)| Line {
        key: format!("{prefix}t{k}"),
        at,
        manual: None,
        border: 0,
    }));
    // Hand-made lines first among those at one place, so the rule's is the one left out.
    inner.sort_by(|a, b| a.at.total_cmp(&b.at).then(b.manual.is_some().cmp(&a.manual.is_some())).then(a.key.cmp(&b.key)));
    let mut out = vec![Line { key: format!("{prefix}0"), at: 0.0, manual: None, border: 1 }];
    for l in inner {
        if out.last().is_some_and(|p| l.at - p.at > NEAR) && out.len() <= MAX_LINES {
            out.push(l);
        }
    }
    out.push(Line { key: format!("{prefix}1"), at: extent, manual: None, border: 2 });
    out
}

/// The hand-made grid lines of the wall whose GUID is `host`: each one's GUID and where it is, vertical ones and then
/// horizontal ones.
fn manual_lines(m: &Model, host: Guid) -> (Vec<(Guid, f64)>, Vec<(Guid, f64)>) {
    let (mut u, mut v) = (Vec::new(), Vec::new());
    for id in m.hosted_by(host) {
        if let (Some(Element::CurtainGrid(l)), Some(g)) = (m.elements.get(id), m.elements.guid(id)) {
            if l.vertical { u.push((g, l.at)) } else { v.push((g, l.at)) }
        }
    }
    (u, v)
}

/// Curtain wall `id`'s grid; none for a wall that is not one.
pub fn grid(m: &Model, id: Id) -> Option<Grid> {
    let w = m.wall(id)?;
    let ct = curtain_of(m, w)?;
    let host = m.elements.guid(id)?;
    let (length, height) = (Axis::of(w).length(), m.own_top(w) - m.own_base(w));
    if !(length.is_finite() && height.is_finite() && length > NEAR && height > NEAR) {
        return None;
    }
    let (mu, mv) = manual_lines(m, host);
    // How wide each way's mullions are: the first border's, those between, the last border's.
    let widths = |ms: &Mullions| {
        let of = |n: &str| m.types.mullion.get(n).map_or(0.0, MullionType::width);
        [of(&ms.border1), of(&ms.interior), of(&ms.border2)]
    };
    Some(Grid {
        length,
        height,
        u: lines_of('u', &ct.vertical, &w.curtain.vertical, length, widths(&ct.vertical_mullions), &mu),
        v: lines_of('v', &ct.horizontal, &w.curtain.horizontal, height, widths(&ct.horizontal_mullions), &mv),
    })
}

/// The key of the segment of line `line` that starts where line `from` crosses it: a mullion's, and a removed segment's.
pub fn segment_key(line: &str, from: &str) -> String {
    format!("{line}|{from}")
}

/// The GUID of the panel or mullion named `key` (a mullion's with `m:` before it) of the wall whose GUID is `host`, in a
/// project of `seed` (its `uid`): the same in every copy of the project.
pub fn child_guid(seed: u64, host: Guid, key: &str) -> Guid {
    let n = format!("{host}/{key}").bytes().fold(0xCBF2_9CE4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3));
    Guid::seeded(seed ^ 0x6375_7274_6169_6E00, n)
}

/// A cell of the grid: columns `i` (between vertical lines `i` and `i + 1`) and rows `j`.
type Cell = (usize, usize);

impl Grid {
    /// Whether the segment of vertical line `i` beside row `j` (or, `vertical` false, of horizontal line `i` under column
    /// `j`) is there: a border's always is, another's unless `merged` names it.
    fn has_segment(&self, merged: &BTreeSet<String>, vertical: bool, i: usize, j: usize) -> bool {
        let (lines, across) = if vertical { (&self.u, &self.v) } else { (&self.v, &self.u) };
        match (lines.get(i), across.get(j)) {
            (Some(l), Some(f)) => l.border != 0 || !merged.contains(&segment_key(&l.key, &f.key)),
            _ => true,
        }
    }

    /// The panels: each the cells it covers, from its lower left one to its upper right one. Cells either side of a
    /// removed segment are one panel where together they make a rectangle; where they would not, each stays its own.
    pub fn panels(&self, merged: &BTreeSet<String>) -> Vec<(Cell, Cell)> {
        let (nu, nv) = (self.u.len().saturating_sub(1), self.v.len().saturating_sub(1));
        if nu == 0 || nv == 0 {
            return Vec::new();
        }
        let at = |i: usize, j: usize| j * nu + i;
        let mut group: Vec<usize> = (0..nu * nv).collect();
        fn find(g: &mut [usize], mut x: usize) -> usize {
            while let Some(&p) = g.get(x) {
                if p == x {
                    break;
                }
                x = p;
            }
            x
        }
        if !merged.is_empty() {
            for j in 0..nv {
                for i in 0..nu {
                    if i + 1 < nu && !self.has_segment(merged, true, i + 1, j) {
                        let (a, b) = (find(&mut group, at(i, j)), find(&mut group, at(i + 1, j)));
                        if let Some(slot) = group.get_mut(a.max(b)) {
                            *slot = a.min(b);
                        }
                    }
                    if j + 1 < nv && !self.has_segment(merged, false, j + 1, i) {
                        let (a, b) = (find(&mut group, at(i, j)), find(&mut group, at(i, j + 1)));
                        if let Some(slot) = group.get_mut(a.max(b)) {
                            *slot = a.min(b);
                        }
                    }
                }
            }
        }
        // Each group's box and how many cells it has: a rectangle fills its box.
        let mut boxes: BTreeMap<usize, (Cell, Cell, usize)> = BTreeMap::new();
        for j in 0..nv {
            for i in 0..nu {
                let root = find(&mut group, at(i, j));
                let b = boxes.entry(root).or_insert(((i, j), (i, j), 0));
                b.0 = (b.0.0.min(i), b.0.1.min(j));
                b.1 = (b.1.0.max(i), b.1.1.max(j));
                b.2 += 1;
            }
        }
        let mut out = Vec::new();
        for (root, (lo, hi, count)) in boxes {
            if (hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1) == count {
                out.push((lo, hi));
            } else {
                // Not a rectangle: its cells one by one.
                for j in 0..nv {
                    for i in 0..nu {
                        if find(&mut group, at(i, j)) == root {
                            out.push(((i, j), (i, j)));
                        }
                    }
                }
            }
        }
        out.sort_by_key(|(lo, _)| (lo.1, lo.0));
        out
    }

    /// The vertical line nearest `s` mm along the wall and the horizontal one nearest `z` mm over its base, as indices.
    pub fn nearest(&self, s: f64, z: f64) -> (usize, usize) {
        let near = |lines: &[Line], x: f64| {
            lines.iter().enumerate().min_by(|a, b| (a.1.at - x).abs().total_cmp(&(b.1.at - x).abs())).map_or(0, |(i, _)| i)
        };
        (near(&self.u, s), near(&self.v, z))
    }

    /// The column `s` mm along the wall is in and the row `z` mm over its base is in.
    pub fn cell(&self, s: f64, z: f64) -> Cell {
        let slot = |lines: &[Line], x: f64| lines.iter().rposition(|l| l.at <= x + 1e-6).unwrap_or(0).min(lines.len().saturating_sub(2));
        (slot(&self.u, s), slot(&self.v, z))
    }
}

// ====================================================================== what a wall has
/// The panels and mullions curtain wall `id` has now, as its grid, its type and what a person changed give them: each
/// with its GUID. None for a wall that is not a curtain wall (it has none).
pub fn laid(m: &Model, id: Id) -> Vec<(Guid, Element)> {
    let Some(g) = grid(m, id) else { return Vec::new() };
    let (Some(w), Some(host)) = (m.wall(id), m.elements.guid(id)) else { return Vec::new() };
    let Some(ct) = curtain_of(m, w) else { return Vec::new() };
    let mut out = Vec::new();
    let default_panel = default_panel(m, ct);
    let existing = |guid: Guid| m.elements.id(guid).and_then(|id| m.elements.get(id));
    // A cell a door fills has no panel.
    let doors: BTreeSet<String> = door_fits(m, host, Some(&g), &w.curtain.merged).0.into_iter().map(|d| d.key).collect();

    for (lo, hi) in g.panels(&w.curtain.merged) {
        let (Some(u0), Some(u1), Some(v0), Some(v1)) = (g.u.get(lo.0), g.u.get(hi.0 + 1), g.v.get(lo.1), g.v.get(hi.1 + 1)) else {
            continue;
        };
        let key = segment_key(&u0.key, &v0.key);
        if doors.contains(&key) {
            continue;
        }
        let guid = child_guid(m.uid, host, &key);
        let (type_name, free, params) = match existing(guid) {
            Some(Element::CurtainPanel(p)) if p.free => (p.type_name.clone(), true, p.params.clone()),
            Some(Element::CurtainPanel(p)) => (default_panel.clone(), false, p.params.clone()),
            _ => (default_panel.clone(), false, BTreeMap::new()),
        };
        if type_name.is_empty() {
            continue;
        }
        out.push((guid, Element::CurtainPanel(Panel { type_name, host, key, span: [u0.at, u1.at, v0.at, v1.at], free, params })));
    }

    // Mullions: on each line's segments between the lines that cross it. A segment's mullion is the type's for that line
    // (its interior, or a border's), unless it was deleted; a freed one, or one put on by hand where the type has none, is
    // kept as it is.
    let side = |ms: &Mullions, border: u8| match border {
        1 => ms.border1.clone(),
        2 => ms.border2.clone(),
        _ => ms.interior.clone(),
    };
    let mut found: Vec<Mullion> = Vec::new();
    for vertical in [true, false] {
        let (lines, across, ms) = if vertical { (&g.u, &g.v, &ct.vertical_mullions) } else { (&g.v, &g.u, &ct.horizontal_mullions) };
        for (i, line) in lines.iter().enumerate() {
            for (j, pair) in across.windows(2).enumerate() {
                let [from, to] = pair else { continue };
                if !g.has_segment(&w.curtain.merged, vertical, i, j) {
                    continue;
                }
                let key = format!("m:{}", segment_key(&line.key, &from.key));
                let guid = child_guid(m.uid, host, &key);
                let was = match existing(guid) {
                    Some(Element::Mullion(x)) => Some(x),
                    _ => None,
                };
                let (type_name, free) = match was {
                    Some(x) if x.free => (x.type_name.clone(), true),
                    _ if w.curtain.no_mullion.contains(&key) => continue,
                    _ => (side(ms, line.border), false),
                };
                if type_name.is_empty() || !m.types.mullion.contains_key(&type_name) {
                    continue;
                }
                found.push(Mullion {
                    type_name,
                    host,
                    key,
                    vertical,
                    at: line.at,
                    from: from.at,
                    to: to.at,
                    trim: [0.0, 0.0],
                    border: line.border,
                    free,
                    params: was.map(|x| x.params.clone()).unwrap_or_default(),
                });
            }
        }
    }
    trim(m, ct.join_condition, &g, &mut found);
    out.extend(found.into_iter().map(|x| (child_guid(m.uid, host, &x.key), Element::Mullion(x))));
    out
}

/// The panel type a curtain type fills its cells with: the one it names, or the project's first that is not empty.
fn default_panel(m: &Model, ct: &CurtainType) -> String {
    if m.types.curtain_panel.contains_key(&ct.panel) {
        return ct.panel.clone();
    }
    m.types.curtain_panel.iter().find(|(_, t)| !t.empty).map(|(n, _)| n.clone()).unwrap_or_default()
}

/// Each mullion's ends cut where it meets one that runs through, and run on to the far side of one it runs through itself,
/// by the type's Join Condition.
fn trim(m: &Model, join: JoinCondition, g: &Grid, all: &mut [Mullion]) {
    // What stands on each crossing: (vertical line's place, horizontal line's place) -> the widest mullion each way there.
    let key = |s: f64, z: f64| ((s * 16.0).round() as i64, (z * 16.0).round() as i64);
    let mut at_u: BTreeMap<(i64, i64), (f64, f64)> = BTreeMap::new();
    let mut at_v: BTreeMap<(i64, i64), (f64, f64)> = BTreeMap::new();
    for x in all.iter() {
        let Some(t) = m.types.mullion.get(&x.type_name) else { continue };
        let sides = reach(t, x.border);
        for end in [x.from, x.to] {
            let (k, map) = if x.vertical { (key(x.at, end), &mut at_u) } else { (key(end, x.at), &mut at_v) };
            let e = map.entry(k).or_insert((0.0, 0.0));
            *e = (e.0.max(sides.0), e.1.max(sides.1));
        }
    }
    let border = |lines: &[Line], at: f64| lines.iter().any(|l| l.border != 0 && (l.at - at).abs() <= NEAR);
    for x in all.iter_mut() {
        for (n, end) in [x.from, x.to].into_iter().enumerate() {
            let (k, other) = if x.vertical { (key(x.at, end), &at_v) } else { (key(end, x.at), &at_u) };
            let Some(&(s1, s2)) = other.get(&k) else { continue };
            let (vb, hb) = if x.vertical { (border(&g.u, x.at), border(&g.v, end)) } else { (border(&g.u, end), border(&g.v, x.at)) };
            let through = join.vertical_through(vb, hb) == x.vertical;
            // The crossing mullion reaches `s1` toward the lower side of its line and `s2` toward the upper: this end
            // stops at the side it comes from, or runs through to the far one.
            let (near, far) = if n == 0 { (s2, s1) } else { (s1, s2) };
            if let Some(slot) = x.trim.get_mut(n) {
                *slot = if through { -far } else { near };
            }
        }
    }
}

// ====================================================================== doors
/// A door that fills a cell of its curtain wall: the door, the cell's key and where the cell is (as a panel's span).
#[derive(Clone, Debug, PartialEq)]
pub struct DoorFit {
    pub id: Id,
    pub key: String,
    pub cell: [f64; 4],
}

/// The wall a curtain wall's door is in: its host's GUID, for a door that fills a cell (`panel` set).
pub fn door_host(e: &Element) -> Option<Guid> {
    match e {
        Element::Door(o) if o.panel.is_some() => Some(o.host),
        _ => None,
    }
}

/// The doors that fill cells of the wall whose GUID is `host`, by id.
pub fn doors_of(m: &Model, host: Guid) -> Vec<Id> {
    let mut ids: Vec<Id> = m.lookup().referrers(&host).filter(|id| m.elements.get(*id).and_then(door_host) == Some(host)).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The cells of grid `g` as its panels fill them (joined cells are one): each one's key and span.
pub fn cells(g: &Grid, merged: &BTreeSet<String>) -> Vec<(String, [f64; 4])> {
    g.panels(merged)
        .into_iter()
        .filter_map(|(lo, hi)| {
            let (u0, u1, v0, v1) = (g.u.get(lo.0)?, g.u.get(hi.0 + 1)?, g.v.get(lo.1)?, g.v.get(hi.1 + 1)?);
            Some((segment_key(&u0.key, &v0.key), [u0.at, u1.at, v0.at, v1.at]))
        })
        .collect()
}

/// The cell among `cells` that holds the place `s` mm along the wall and `z` mm over its base.
pub fn cell_at(cells: &[(String, [f64; 4])], s: f64, z: f64) -> Option<&(String, [f64; 4])> {
    if !(s.is_finite() && z.is_finite()) {
        return None;
    }
    cells.iter().find(|(_, c)| s >= c[0] - 1e-6 && s < c[1] - 1e-6 && z >= c[2] - 1e-6 && z < c[3] - 1e-6)
}

/// The middle of the cell a door fills, as it stands: mm along its wall and mm over the wall's base.
fn door_middle(o: &crate::model::Opening) -> (f64, f64) {
    let height = o.cell_size().map_or(0.0, |(_, h)| h);
    (o.offset, o.sill + (height / 2.0).max(1.0))
}

/// The doors of the wall whose GUID is `host`, each in the cell it fills now, and those that have none (to delete). A
/// door stays in the cell its key names; one whose cell is gone goes to the cell that holds the place its middle was. Of
/// two doors in one cell the one of the lower GUID stays (the same in every copy of the project), and a door that kept its
/// cell before one that came to it. With no grid (`g` none: the wall is no curtain wall, or too small for one), none has a
/// cell.
pub fn door_fits(m: &Model, host: Guid, g: Option<&Grid>, merged: &BTreeSet<String>) -> (Vec<DoorFit>, Vec<Id>) {
    let mut doors: Vec<(Guid, Id, &crate::model::Opening)> = doors_of(m, host)
        .into_iter()
        .filter_map(|id| match (m.elements.guid(id), m.elements.get(id)) {
            (Some(guid), Some(Element::Door(o))) => Some((guid, id, o)),
            _ => None,
        })
        .collect();
    if doors.is_empty() {
        return (Vec::new(), Vec::new());
    }
    doors.sort_by_key(|(guid, ..)| *guid);
    let Some(g) = g else { return (Vec::new(), doors.into_iter().map(|(_, id, _)| id).collect()) };
    let cells = cells(g, merged);
    let (mut fits, mut gone): (Vec<DoorFit>, Vec<Id>) = (Vec::new(), Vec::new());
    let mut moved = Vec::new();
    for (_, id, o) in doors {
        match cells.iter().find(|(key, _)| Some(key) == o.panel.as_ref()) {
            Some((key, _)) if fits.iter().any(|f| f.key == *key) => gone.push(id),
            Some((key, cell)) => fits.push(DoorFit { id, key: key.clone(), cell: *cell }),
            None => moved.push((id, o)),
        }
    }
    for (id, o) in moved {
        let (s, z) = door_middle(o);
        match cell_at(&cells, s, z) {
            Some((key, cell)) if !fits.iter().any(|f| f.key == *key) => fits.push(DoorFit { id, key: key.clone(), cell: *cell }),
            _ => gone.push(id),
        }
    }
    (fits, gone)
}

/// Each door of curtain wall `id` named by the cell that holds the place it stands at now, whatever cell its key names:
/// after the wall's lines were named anew (the wall flipped, mirrored, split, or a copy), when a key that is still a
/// cell's may be another cell's.
pub fn rehome_doors(m: &mut Model, id: Id) {
    let (Some(g), Some(host)) = (grid(m, id), m.elements.guid(id)) else { return };
    let Some(merged) = m.wall(id).map(|w| w.curtain.merged.clone()) else { return };
    let cells = cells(&g, &merged);
    for door in doors_of(m, host) {
        let Some(Element::Door(o)) = m.elements.get(door) else { continue };
        let (s, z) = door_middle(o);
        let Some((key, _)) = cell_at(&cells, s, z) else { continue };
        if o.panel.as_ref() != Some(key)
            && let Some(Element::Door(o)) = m.elements.get_mut(door)
        {
            o.panel = Some(key.clone());
        }
    }
}

/// The door that fills the cell named `key` of the wall whose GUID is `host`.
pub fn door_in(m: &Model, host: Guid, key: &str) -> Option<Id> {
    doors_of(m, host).into_iter().find(|id| matches!(m.elements.get(*id), Some(Element::Door(o)) if o.panel.as_deref() == Some(key)))
}

// ====================================================================== kept after each command
/// What [`keep`] found to do: elements to take out, and elements to put (each at the id it had, or new, with its GUID).
#[derive(Default)]
pub struct Update {
    remove: Vec<Id>,
    put: Vec<(Option<Id>, Guid, Element)>,
    /// The doors that have no cell any more: [`apply`] leaves them for whoever runs the layout to delete as a door is
    /// deleted, with all that hangs on it (the engine, after a command; [`drop_doors`], with no engine).
    doors: Vec<Id>,
}

impl Update {
    pub fn is_empty(&self) -> bool {
        self.remove.is_empty() && self.put.is_empty() && self.doors.is_empty()
    }

    /// The doors to delete: those whose wall is no curtain wall any more, or whose cell went with no other at its place.
    pub fn doors(&self) -> Vec<Id> {
        let mut ids = self.doors.clone();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// `doors` taken out with their tags and pins, where no engine deletes them (a file being read).
pub fn drop_doors(m: &mut Model, doors: &[Id]) {
    let mut with: Vec<Id> = Vec::new();
    for id in doors {
        if let (Some(Element::Door(_)), Some(guid)) = (m.elements.get(*id), m.elements.guid(*id)) {
            with.extend(m.lookup().referrers(&guid).filter(|t| matches!(m.elements.get(*t), Some(Element::Tag(t)) if t.host == guid)));
            m.pinned.remove(&guid);
        }
    }
    for id in doors.iter().copied().chain(with) {
        m.elements.remove(id);
    }
}

/// Whether the element is a curtain wall's own: a panel, a mullion or a hand-made grid line. Its host's GUID.
pub fn host_of(e: &Element) -> Option<Guid> {
    match e {
        Element::CurtainPanel(p) => Some(p.host),
        Element::Mullion(x) => Some(x.host),
        Element::CurtainGrid(l) => Some(l.host),
        _ => None,
    }
}

/// Whether the wall placed the panel or mullion and still holds it, as Revit's pin on what a curtain type placed.
pub fn placed(e: &Element) -> bool {
    matches!(e, Element::CurtainPanel(Panel { free: false, .. }) | Element::Mullion(Mullion { free: false, .. }))
}

/// After a command: the curtain walls it touched laid out again. A wall is touched when it changed, when one of its panels,
/// mullions or grid lines did, when a level did (its height may have), or when the types did (every curtain wall, then).
/// What [`apply`] then changes; empty when nothing needs it.
pub fn keep(m: &Model, before: &Model) -> Update {
    let all = || m.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect::<BTreeSet<Id>>();
    let mut orphans: Vec<Id> = Vec::new();
    let walls: BTreeSet<Id> = match m.elements.changed_since(&before.elements) {
        _ if m.types != before.types || m.uid != before.uid => all(),
        Changed::All => all(),
        Changed::Ids(ids) => {
            let mut walls = BTreeSet::new();
            let mut level = false;
            for id in ids {
                for x in [m, before] {
                    match x.elements.get(id) {
                        Some(Element::Wall(_)) => {
                            walls.insert(id);
                            // A wall that is gone takes what it hosted with it.
                            if !m.elements.contains_key(id)
                                && let Some(g) = before.elements.guid(id)
                            {
                                orphans.extend(before.hosted_by(g).into_iter().filter(|c| m.elements.contains_key(*c)));
                            }
                        }
                        Some(Element::Level(_) | Element::Roof(_) | Element::Floor(_)) => level = true,
                        Some(e) => {
                            // (A door that fills a cell too: its cell's panel is back when it goes.)
                            if let Some(h) = host_of(e).or_else(|| door_host(e)) {
                                match m.elements.id(h) {
                                    Some(w) => {
                                        walls.insert(w);
                                    }
                                    None if m.elements.contains_key(id) => orphans.push(id),
                                    None => {}
                                }
                            }
                        }
                        None => {}
                    }
                }
            }
            if level { all() } else { walls }
        }
    };
    let mut up = Update { remove: orphans, put: Vec::new(), doors: Vec::new() };
    up.remove.sort_unstable();
    up.remove.dedup();
    for id in walls {
        relay(m, id, &mut up);
    }
    up
}

/// Every curtain wall laid out again: a file read from elsewhere, or from before.
pub fn keep_all(m: &mut Model) {
    let mut up = Update::default();
    let walls: Vec<Id> = m.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect();
    for id in walls {
        relay(m, id, &mut up);
    }
    let doors = up.doors();
    apply(m, up);
    drop_doors(m, &doors);
}

/// Wall `id`'s panels and mullions as they are against as they should be: what differs, into `up`.
fn relay(m: &Model, id: Id, up: &mut Update) {
    let Some(host) = m.elements.guid(id) else { return };
    let Some(w) = m.wall(id) else { return };
    let mine: Vec<Id> = m.hosted_by(host);
    if !is_curtain(m, w) {
        // A basic wall has none of these: a wall whose type was changed loses them, and the doors that filled its cells.
        up.remove.extend(mine.into_iter().filter(|c| m.elements.get(*c).is_some_and(|e| host_of(e).is_some())));
        up.doors.extend(doors_of(m, host));
        return;
    }
    // Its doors: each in its cell, as large as it; one with no cell goes.
    let g = if doors_of(m, host).is_empty() { None } else { grid(m, id) };
    let (fits, gone) = door_fits(m, host, g.as_ref(), &w.curtain.merged);
    up.doors.extend(gone);
    for fit in fits {
        let Some(Element::Door(o)) = m.elements.get(fit.id) else { continue };
        let [s0, s1, z0, _] = fit.cell;
        let (offset, sill) = ((s0 + s1) / 2.0, z0);
        if o.panel.as_ref() != Some(&fit.key) || o.cell != Some(fit.cell) || o.offset != offset || o.sill != sill {
            let door = crate::model::Opening { panel: Some(fit.key), cell: Some(fit.cell), offset, sill, ..o.clone() };
            if let Some(guid) = m.elements.guid(fit.id) {
                up.put.push((Some(fit.id), guid, Element::Door(door)));
            }
        }
    }
    let want = laid(m, id);
    let wanted: BTreeSet<Guid> = want.iter().map(|(g, _)| *g).collect();
    for c in mine {
        let Some(e) = m.elements.get(c) else { continue };
        let ours = matches!(e, Element::CurtainPanel(_) | Element::Mullion(_));
        if ours && !m.elements.guid(c).is_some_and(|g| wanted.contains(&g)) {
            up.remove.push(c);
        }
    }
    for (guid, e) in want {
        match m.elements.id(guid) {
            Some(c) if m.elements.get(c) == Some(&e) => {}
            // The GUID is another kind of element's: leave both be (a file from elsewhere).
            Some(c) if !m.elements.get(c).is_some_and(|x| host_of(x).is_some()) => {}
            c => up.put.push((c, guid, e)),
        }
    }
}

/// What [`keep`] worked out, made so.
pub fn apply(m: &mut Model, up: Update) {
    for id in up.remove {
        m.elements.remove(id);
    }
    for (id, guid, e) in up.put {
        match id {
            Some(id) => {
                m.elements.insert(id, e);
            }
            None => {
                let id = m.fresh_id();
                // A GUID taken meanwhile leaves the element out: the next layout puts it.
                let _ = m.elements.insert_as(id, guid, e);
            }
        }
    }
}

// ====================================================================== where they are
/// A panel's or a mullion's wall: its id, the wall and its location line.
pub fn host<'a>(m: &'a Model, host: Guid) -> Option<(Id, &'a Wall, Axis)> {
    let id = m.elements.id(host)?;
    let w = m.wall(id)?;
    Some((id, w, Axis::of(w)))
}

/// A panel in the model: its four corners in plan and height (lower start, lower end, upper end, upper start) on the
/// plane through its middle, the way its outside faces, and its thickness. Flat from one side to the other, as Revit's
/// on a curved wall. None when its wall or its type is gone, or its type is empty.
pub fn panel_face(m: &Model, p: &Panel) -> Option<([[f64; 3]; 4], P2, f64)> {
    let t = m.types.curtain_panel.get(&p.type_name).filter(|t| !t.empty)?;
    let (_, w, axis) = host(m, p.host)?;
    let [s0, s1, z0, z1] = p.span;
    let (a, b) = (axis.at(s0, 0.0), axis.at(s1, 0.0));
    let along = (b - a).unit();
    if !(along.0.is_finite() && along.1.is_finite()) {
        return None;
    }
    // The exterior is to the wall's left.
    let out = along.perp();
    let (a, b) = (a + out * t.offset, b + out * t.offset);
    let base = m.own_base(w);
    Some(([[a.0, a.1, base + z0], [b.0, b.1, base + z0], [b.0, b.1, base + z1], [a.0, a.1, base + z1]], out, t.thickness))
}

/// A mullion in the model: the two ends of its middle line (x, y, z), the way its width runs (a unit vector in plan for a
/// vertical one; straight up, `None`, for a horizontal one), the way its depth runs (out of the wall, in plan), and its
/// type. A horizontal one on a curved wall is straight from end to end. None when its wall or type is gone.
pub fn mullion_line<'a>(m: &'a Model, x: &Mullion) -> Option<([f64; 3], [f64; 3], Option<P2>, P2, &'a MullionType)> {
    let t = m.types.mullion.get(&x.type_name)?;
    let (_, w, axis) = host(m, x.host)?;
    let base = m.own_base(w);
    let (from, to) = x.ends();
    if to - from <= TOL {
        return None;
    }
    // Its middle line: on its grid line, or on a border half its width inside it.
    let (lower, upper) = reach(t, x.border);
    let shift = (upper - lower) / 2.0 - (t.sides().1 - t.sides().0) / 2.0;
    if x.vertical {
        let along = axis.tangent(x.at);
        let out = along.perp();
        let p = axis.at(x.at, 0.0) + along * shift + out * t.offset;
        Some(([p.0, p.1, base + from], [p.0, p.1, base + to], Some(along), out, t))
    } else {
        let (a, b) = (axis.at(x.from, 0.0), axis.at(x.to, 0.0));
        let along = (b - a).unit();
        if !(along.0.is_finite() && along.1.is_finite()) {
            return None;
        }
        let out = along.perp();
        // Trimmed along the straight line between its segment's ends.
        let (a, b) = (a + along * x.trim[0] + out * t.offset, b - along * x.trim[1] + out * t.offset);
        Some(([a.0, a.1, base + x.at + shift], [b.0, b.1, base + x.at + shift], None, out, t))
    }
}

/// The types a wall type's curtain names: its panel and its six mullions, each with what it is called.
pub fn names(t: &WallType) -> Vec<(&'static str, &'static str, &str)> {
    let Some(c) = &t.curtain else { return Vec::new() };
    let mut out = vec![("curtain_panel", "curtain panel", c.panel.as_str())];
    for (which, ms) in [("vertical", &c.vertical_mullions), ("horizontal", &c.horizontal_mullions)] {
        for (what, n) in ms.named() {
            let _ = (which, what);
            out.push(("mullion", "mullion", n));
        }
    }
    out
}

// ====================================================================== what a project starts with
/// The curtain wall types every project can draw without making one, as Revit's template has them: the plain Curtain
/// Wall (one panel, no grid), Exterior Glazing (a grid, no mullions) and Storefront (a grid with mullions). A project gets
/// one, with the panel and mullion types it names, when it is first asked for ([`ensure_starter`]): a project that draws
/// no curtain wall holds nothing of them, and an older build still opens it.
pub const STARTER_WALLS: [&str; 3] = ["Curtain Wall", "Exterior Glazing", "Storefront"];
/// The starter curtain panel types: Revit's System Panel Glazed and Solid, and its Empty System Panel.
pub const GLAZED: &str = "Glazed";
pub const SOLID: &str = "Solid";
pub const EMPTY: &str = "Empty";
/// The starter mullion types: Revit's Rectangular Mullion 50 x 150mm and 30mm Square, and Circular Mullion 25mm Radius
/// and 50mm Radius.
pub const MULLION: &str = "50 x 150mm";
pub const STARTER_MULLIONS: [(&str, bool, f64, f64); 4] = [
    (MULLION, false, 150.0, 25.0),
    ("30mm Square", false, 30.0, 15.0),
    ("25mm Radius", true, 50.0, 0.0),
    ("50mm Radius", true, 100.0, 0.0),
];

/// How deep a curtain type's walls reach across their line, mm: its deepest panel or mullion, which its `thickness` holds.
pub fn depth(m: &Model, c: &CurtainType) -> f64 {
    let panel = m.types.curtain_panel.get(&default_panel(m, c)).map_or(0.0, |t| t.thickness + 2.0 * t.offset.abs());
    let mullions = [&c.vertical_mullions, &c.horizontal_mullions]
        .into_iter()
        .flat_map(|ms| ms.named())
        .filter_map(|(_, n)| m.types.mullion.get(n))
        .map(|t| t.thickness + 2.0 * t.offset.abs());
    mullions.fold(panel, f64::max).max(25.0)
}

/// The starter curtain panel types the project lacks, made: Glazed (glass), Solid and Empty.
pub fn ensure_panels(m: &mut Model) {
    let material = |m: &Model, name: &str| {
        let word = m.language.word(name).to_string();
        if m.materials.contains_key(&word) { word } else { String::new() }
    };
    let (glass, metal) = (material(m, "Glass"), material(m, "Metal"));
    let types = &mut m.types.curtain_panel;
    for (name, material, empty) in [(GLAZED, glass, false), (SOLID, metal, false), (EMPTY, String::new(), true)] {
        if !types.contains_key(name) {
            types.insert(name.to_string(), PanelType { thickness: 25.0, material, empty, ..PanelType::default() });
        }
    }
}

/// The starter mullion types the project lacks, made.
pub fn ensure_mullions(m: &mut Model) {
    let word = m.language.word("Metal").to_string();
    let material = if m.materials.contains_key(&word) { word } else { String::new() };
    for (name, round, thickness, side) in STARTER_MULLIONS {
        if !m.types.mullion.contains_key(name) {
            let t = MullionType { thickness, side1: side, side2: side, round, material: material.clone(), ..MullionType::default() };
            m.types.mullion.insert(name.to_string(), t);
        }
    }
}

/// Starter curtain wall type `name` made, with what it names, when the project has no wall type of that name: whether it
/// is one of [`STARTER_WALLS`] (there now).
pub fn ensure_starter(m: &mut Model, name: &str) -> bool {
    if !STARTER_WALLS.contains(&name) {
        return false;
    }
    if m.types.wall.contains_key(name) {
        return true;
    }
    ensure_panels(m);
    let rule = |layout, spacing| GridRule { layout, spacing, adjust_for_mullion: false };
    let mut c = CurtainType { panel: GLAZED.into(), ..CurtainType::default() };
    match name {
        "Exterior Glazing" => {
            c.vertical = rule(Layout::FixedDistance, 1500.0);
            c.horizontal = rule(Layout::FixedDistance, 4000.0);
        }
        "Storefront" => {
            ensure_mullions(m);
            // Revit's Storefront embeds itself in the basic wall it is drawn in.
            c.embed = true;
            c.vertical = rule(Layout::MaximumSpacing, 1500.0);
            c.horizontal = rule(Layout::FixedDistance, 2400.0);
            let all = || Mullions { interior: MULLION.into(), border1: MULLION.into(), border2: MULLION.into() };
            (c.vertical_mullions, c.horizontal_mullions) = (all(), all());
        }
        _ => {}
    }
    let thickness = depth(m, &c);
    m.types.wall.insert(name.to_string(), WallType { thickness, curtain: Some(c), ..WallType::default() });
    true
}

/// Every curtain wall type's `thickness` brought to how deep it reaches now ([`depth`]): after a panel or a mullion type
/// it names changed.
pub fn refresh_depths(m: &mut Model) {
    let now: Vec<(String, f64)> = m
        .types
        .wall
        .iter()
        .filter_map(|(n, t)| t.curtain.as_ref().map(|c| (n.clone(), depth(m, c))))
        .filter(|(n, d)| m.types.wall.get(n).is_some_and(|t| (t.thickness - d).abs() > 1e-9))
        .collect();
    for (n, d) in now {
        if let Some(t) = m.types.wall.get_mut(&n) {
            t.thickness = d;
        }
    }
}

// ====================================================================== worked on by hand
/// A segment of a grid line, between two lines that cross it: which way its line runs (`vertical`: it stands), its line
/// among its way's lines and the crossing line it starts at (indices), its key ([`segment_key`]), where its line is and
/// where it runs from and to (mm along the wall or over its base), and which border its line is (0: none).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub vertical: bool,
    pub line: usize,
    pub from: usize,
    pub key: String,
    pub at: f64,
    pub span: [f64; 2],
    pub border: u8,
}

impl Segment {
    /// The key of the mullion on it.
    pub fn mullion_key(&self) -> String {
        format!("m:{}", self.key)
    }

    /// Its middle: mm along the wall from its start, and mm over the wall's base.
    pub fn middle(&self) -> [f64; 2] {
        let mid = (self.span[0] + self.span[1]) / 2.0;
        if self.vertical { [self.at, mid] } else { [mid, self.at] }
    }
}

impl Grid {
    /// The segment of line `line` (a standing one when `vertical`) that starts at crossing line `from`.
    pub fn segment(&self, vertical: bool, line: usize, from: usize) -> Option<Segment> {
        let (lines, across) = if vertical { (&self.u, &self.v) } else { (&self.v, &self.u) };
        let (l, a, b) = (lines.get(line)?, across.get(from)?, across.get(from + 1)?);
        Some(Segment { vertical, line, from, key: segment_key(&l.key, &a.key), at: l.at, span: [a.at, b.at], border: l.border })
    }

    /// Every segment of every line, borders too: the standing lines', then the lying ones'.
    pub fn segments(&self) -> Vec<Segment> {
        let mut out = Vec::new();
        for vertical in [true, false] {
            let (lines, across) = if vertical { (&self.u, &self.v) } else { (&self.v, &self.u) };
            for i in 0..lines.len() {
                for j in 0..across.len().saturating_sub(1) {
                    out.extend(self.segment(vertical, i, j));
                }
            }
        }
        out
    }

    /// The segment nearest the place `s` mm along the wall and `z` mm over its base: of the line nearest it (a standing
    /// one or a lying one, whichever is nearer; `way` says which when given), the segment the place is beside.
    pub fn segment_near(&self, s: f64, z: f64, way: Option<bool>) -> Option<Segment> {
        if !(s.is_finite() && z.is_finite()) {
            return None;
        }
        let (i, j) = self.nearest(s, z);
        let (du, dv) = ((self.u.get(i)?.at - s).abs(), (self.v.get(j)?.at - z).abs());
        let (ci, cj) = self.cell(s, z);
        if way.unwrap_or(du <= dv) { self.segment(true, i, cj) } else { self.segment(false, j, ci) }
    }

    /// The segment at the place: as [`Grid::segment_near`], when its line is within a millimetre of it.
    pub fn segment_at(&self, vertical: bool, s: f64, z: f64) -> Option<Segment> {
        self.segment_near(s, z, Some(vertical)).filter(|g| (g.at - if vertical { s } else { z }).abs() <= NEAR)
    }

    /// With the segments `merged` names removed, the joined cells that are no panel: together not a rectangle, or with a
    /// segment still standing inside them. Each as the box round it, from its lower left cell to its upper right one
    /// (column, row). Empty when every panel is sound.
    pub fn trouble(&self, merged: &BTreeSet<String>) -> Vec<((usize, usize), (usize, usize))> {
        let (nu, nv) = (self.u.len().saturating_sub(1), self.v.len().saturating_sub(1));
        if nu == 0 || nv == 0 || merged.is_empty() {
            return Vec::new();
        }
        let at = |i: usize, j: usize| j * nu + i;
        let mut group: Vec<usize> = (0..nu * nv).collect();
        fn find(g: &[usize], mut x: usize) -> usize {
            while let Some(&p) = g.get(x) {
                if p == x {
                    break;
                }
                x = p;
            }
            x
        }
        let join = |group: &mut Vec<usize>, a: usize, b: usize| {
            let (a, b) = (find(group, a), find(group, b));
            if let Some(slot) = group.get_mut(a.max(b)) {
                *slot = a.min(b);
            }
        };
        for j in 0..nv {
            for i in 0..nu {
                if i + 1 < nu && !self.has_segment(merged, true, i + 1, j) {
                    join(&mut group, at(i, j), at(i + 1, j));
                }
                if j + 1 < nv && !self.has_segment(merged, false, j + 1, i) {
                    join(&mut group, at(i, j), at(i, j + 1));
                }
            }
        }
        // Each group's box, how many cells it has, and whether a segment stands inside it.
        type Joined = (Cell, Cell, usize, bool);
        let mut boxes: BTreeMap<usize, Joined> = BTreeMap::new();
        for j in 0..nv {
            for i in 0..nu {
                let root = find(&group, at(i, j));
                // A segment still there between two cells of one panel stands inside it.
                let inside = (i + 1 < nu && self.has_segment(merged, true, i + 1, j) && find(&group, at(i + 1, j)) == root)
                    || (j + 1 < nv && self.has_segment(merged, false, j + 1, i) && find(&group, at(i, j + 1)) == root);
                let b = boxes.entry(root).or_insert(((i, j), (i, j), 0, false));
                b.0 = (b.0.0.min(i), b.0.1.min(j));
                b.1 = (b.1.0.max(i), b.1.1.max(j));
                b.2 += 1;
                b.3 |= inside;
            }
        }
        boxes
            .into_values()
            .filter(|(lo, hi, count, inside)| *inside || (hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1) != *count)
            .map(|(lo, hi, ..)| (lo, hi))
            .collect()
    }

    /// The segments between the cells of the box from cell `lo` to cell `hi` (column, row): none of a border.
    pub fn within(&self, lo: (usize, usize), hi: (usize, usize)) -> Vec<Segment> {
        let mut out = Vec::new();
        for i in lo.0 + 1..=hi.0 {
            for j in lo.1..=hi.1 {
                out.extend(self.segment(true, i, j));
            }
        }
        for j in lo.1 + 1..=hi.1 {
            for i in lo.0..=hi.0 {
                out.extend(self.segment(false, j, i));
            }
        }
        out.retain(|g| g.border == 0);
        out
    }
}

/// Curtain wall `id` laid out now: its panels and mullions as its grid, its type and what a person changed give them. The
/// engine does it after every command ([`keep`]); a command that answers with them, or changed what they are named by,
/// does it itself.
pub fn lay(m: &mut Model, id: Id) {
    let mut up = Update::default();
    relay(m, id, &mut up);
    apply(m, up);
}

/// What a person did to a curtain wall's grid, held by where it is and not by its lines' names: the segments removed,
/// the mullions deleted, and the panels and mullions freed or put on by hand. A wall copied, mirrored, flipped or split
/// names its lines anew (a copy's hand-made lines are other elements, a wall turned round counts from its other end), so
/// what was done is taken off the wall as it was ([`hand`]) and put on the wall as it is ([`hand_to`]).
#[derive(Clone, Debug, Default)]
pub struct Hand {
    /// The wall it was taken off: put back on that wall, a freed panel or mullion keeps its id.
    wall: Option<Guid>,
    merged: Vec<(bool, [f64; 2])>,
    no_mullion: Vec<(bool, [f64; 2])>,
    panels: Vec<([f64; 2], Id, Panel)>,
    mullions: Vec<([f64; 2], Id, Mullion)>,
}

impl Hand {
    pub fn is_empty(&self) -> bool {
        self.merged.is_empty() && self.no_mullion.is_empty() && self.panels.is_empty() && self.mullions.is_empty()
    }
}

/// What a person did to curtain wall `id`'s grid, by place. Empty for a wall that is not a curtain wall.
pub fn hand(m: &Model, id: Id) -> Hand {
    let mut out = Hand::default();
    let (Some(g), Some(w), Some(host)) = (grid(m, id), m.wall(id), m.elements.guid(id)) else { return out };
    out.wall = Some(host);
    if !(w.curtain.merged.is_empty() && w.curtain.no_mullion.is_empty()) {
        for seg in g.segments() {
            if seg.border == 0 && w.curtain.merged.contains(&seg.key) {
                out.merged.push((seg.vertical, seg.middle()));
            }
            if w.curtain.no_mullion.contains(&seg.mullion_key()) {
                out.no_mullion.push((seg.vertical, seg.middle()));
            }
        }
    }
    for c in m.hosted_by(host) {
        match m.elements.get(c) {
            Some(Element::CurtainPanel(p)) if p.free => {
                out.panels.push(([(p.span[0] + p.span[1]) / 2.0, (p.span[2] + p.span[3]) / 2.0], c, p.clone()));
            }
            Some(Element::Mullion(x)) if x.free => {
                let mid = (x.from + x.to) / 2.0;
                out.mullions.push((if x.vertical { [x.at, mid] } else { [mid, x.at] }, c, x.clone()));
            }
            _ => {}
        }
    }
    out
}

/// `hand` put on curtain wall `id` as it is now, each place `s` mm along the wall it was taken from now `along(s)` mm
/// along this one: its removed segments, deleted mullions and freed panels and mullions named by this wall's lines, and
/// the wall laid out. What has no place on this wall (a segment whose line is not here) is left out. Whatever the wall
/// remembered and held freed before is replaced.
pub fn hand_to(m: &mut Model, id: Id, hand: &Hand, along: &dyn Fn(f64) -> f64) {
    let Some(host) = m.elements.guid(id) else { return };
    let freed: Vec<Id> = m
        .hosted_by(host)
        .into_iter()
        .filter(|c| m.elements.get(*c).is_some_and(|e| host_of(e).is_some() && !placed(e) && !matches!(e, Element::CurtainGrid(_))))
        .collect();
    for c in freed {
        m.elements.remove(c);
    }
    let Some(g) = grid(m, id) else { return };
    // A place off this wall (the other half of a wall split) names nothing here.
    let on = |s: f64, z: f64| s >= -NEAR && s <= g.length + NEAR && z >= -NEAR && z <= g.height + NEAR;
    let seg = |vertical: bool, at: &[f64; 2]| g.segment_at(vertical, along(at[0]), at[1]).filter(|_| on(along(at[0]), at[1]));
    let merged: BTreeSet<String> = hand.merged.iter().filter_map(|(v, at)| seg(*v, at)).filter(|x| x.border == 0).map(|x| x.key).collect();
    // Only what leaves every panel sound: a segment whose neighbours are not here would leave half a join.
    let merged = if g.trouble(&merged).is_empty() { merged } else { BTreeSet::new() };
    let no_mullion: BTreeSet<String> = hand.no_mullion.iter().filter_map(|(v, at)| seg(*v, at)).map(|x| x.mullion_key()).collect();
    let mut put: Vec<(String, Id, Element)> = Vec::new();
    let cells = g.panels(&merged);
    for (at, was, p) in &hand.panels {
        // (A type this project lacks, of a wall from another file: the wall's own type's instead.)
        if !on(along(at[0]), at[1]) || !m.types.curtain_panel.contains_key(&p.type_name) {
            continue;
        }
        let (i, j) = g.cell(along(at[0]), at[1]);
        let Some((lo, _)) = cells.iter().find(|(lo, hi)| (lo.0..=hi.0).contains(&i) && (lo.1..=hi.1).contains(&j)) else { continue };
        let (Some(u), Some(v)) = (g.u.get(lo.0), g.v.get(lo.1)) else { continue };
        let key = segment_key(&u.key, &v.key);
        put.push((key.clone(), *was, Element::CurtainPanel(Panel { host, key, free: true, ..p.clone() })));
    }
    for (at, was, x) in &hand.mullions {
        if !m.types.mullion.contains_key(&x.type_name) {
            continue;
        }
        let Some(s) = seg(x.vertical, at) else { continue };
        let key = s.mullion_key();
        put.push((key.clone(), *was, Element::Mullion(Mullion { host, key, free: true, ..x.clone() })));
    }
    if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
        (w.curtain.merged, w.curtain.no_mullion) = (merged, no_mullion);
    }
    for (key, was, e) in put {
        let guid = child_guid(m.uid, host, &key);
        // On the wall it was taken off, it is the element it was, at its id. The name may be another's now (a pinned
        // panel's, on a wall turned round): this one takes its place, and the layout makes the other again.
        let its_own = hand.wall == Some(host) && !m.elements.contains_key(was);
        match (m.elements.id(guid), its_own) {
            (Some(c), false) => {
                m.elements.insert(c, e);
            }
            (there, _) => {
                if let Some(c) = there {
                    m.elements.remove(c);
                }
                let c = if its_own { was } else { m.fresh_id() };
                let _ = m.elements.insert_as(c, guid, e);
            }
        }
    }
    // The wall's lines are named anew: its doors are in the cells that hold them now.
    rehome_doors(m, id);
    lay(m, id);
}

/// A mullion of type `type_name` put by hand on the segment whose mullion key is `key` of the wall whose GUID is `host`,
/// free of the wall's type: where it is is the layout's to fill in ([`lay`]). Its id, or none when one is there already.
pub fn put_mullion(m: &mut Model, host: Guid, seg: &Segment, type_name: &str) -> Option<Id> {
    let key = seg.mullion_key();
    let guid = child_guid(m.uid, host, &key);
    if m.elements.id(guid).is_some() {
        return None;
    }
    let x = Mullion {
        type_name: type_name.to_string(),
        host,
        key,
        vertical: seg.vertical,
        at: seg.at,
        from: seg.span[0],
        to: seg.span[1],
        trim: [0.0, 0.0],
        border: seg.border,
        free: true,
        params: BTreeMap::new(),
    };
    let id = m.fresh_id();
    m.elements.insert_as(id, guid, Element::Mullion(x)).ok().map(|()| id)
}

/// Whether the wall's type gives the panel or mullion something to be: a panel always (the type's panel), a mullion when
/// the type names a mullion type for the line it is on. One the type gives nothing is only ever there by hand.
pub fn typed(m: &Model, e: &Element) -> bool {
    match e {
        Element::CurtainPanel(_) => true,
        Element::Mullion(x) => host(m, x.host).and_then(|(_, w, _)| curtain_of(m, w)).is_some_and(|ct| {
            let ms = if x.vertical { &ct.vertical_mullions } else { &ct.horizontal_mullions };
            let name = match x.border {
                1 => &ms.border1,
                2 => &ms.border2,
                _ => &ms.interior,
            };
            m.types.mullion.contains_key(name)
        }),
        _ => false,
    }
}

/// A wall's own grid settings as they are once the wall runs the other way (flipped, or a mirror image), so its type's
/// lines stay where they are in the model: counted from the Beginning they are counted from the End, and an Offset from
/// the Center is to the other side.
pub fn reverse(grid: &mut WallGrid) {
    let set = &mut grid.vertical;
    match set.justification {
        Justification::Beginning => set.justification = Justification::End,
        Justification::End => set.justification = Justification::Beginning,
        Justification::Center => set.offset = -set.offset + 0.0,
    }
}


/// Where the curtain wall `cw` cuts the basic wall `host`, as Revit's Automatically Embed: its type embeds, it stands at
/// heights the host has, and its line runs inside the host's body along the host's line (a straight one along a straight
/// wall, a curved one round the same centre). The stretch along the host's location line it covers (mm from the host's
/// start, held to the host's ends) and the heights between its base and its top; none when it is not embedded there.
pub fn embeds(m: &Model, cw: &Wall, host: &Wall) -> Option<((f64, f64), (f64, f64))> {
    if !curtain_of(m, cw).is_some_and(|c| c.embed) || is_curtain(m, host) {
        return None;
    }
    let (z0, z1) = (m.base_z(cw), m.top_z(cw));
    let (h0, h1) = (m.base_z(host), m.top_z(host));
    if z1 <= h0 + TOL || z0 >= h1 - TOL {
        return None;
    }
    let (own, axis) = (crate::walls::Axis::of(cw), crate::walls::Axis::of(host));
    let (ext, int) = crate::walls::faces(m, host);
    let (len, length) = (own.length(), axis.length());
    if len <= TOL || (own.radius().is_some() != axis.radius().is_some()) {
        return None;
    }
    // Along the host's line: every point of the curtain wall's line inside the host's body, and on the host's line where
    // its own is (round the same centre, a curved one).
    let mut along = Vec::with_capacity(9);
    for k in 0..=8 {
        let q = own.at(len * f64::from(k) / 8.0, 0.0);
        let (s, across) = axis.project(q);
        if axis.distance(q, across) > TOL {
            // Past one of the host's ends: that part cuts nothing; anywhere else it is off the host's line.
            if s <= TOL || s >= length - TOL {
                continue;
            }
            return None;
        }
        if !(int - TOL..=ext + TOL).contains(&across) {
            return None;
        }
        along.push(s);
    }
    if along.len() < 2 {
        return None;
    }
    let (s0, s1) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), s| (a.min(*s), b.max(*s)));
    (s1 - s0 > TOL).then_some(((s0.max(0.0), s1.min(length)), (z0.max(h0), z1.min(h1))))
}

/// The cuts curtain walls embedded in basic wall `id` make in it ([`embeds`]), each as [`embeds`] gives it.
pub fn embedded_cuts(m: &Model, id: Id, _index: Option<&crate::walls::Index>) -> Vec<((f64, f64), (f64, f64))> {
    let Some(host) = m.wall(id) else { return Vec::new() };
    if is_curtain(m, host) || !m.types.wall.values().any(|t| t.curtain.as_ref().is_some_and(|c| c.embed)) {
        return Vec::new();
    }
    let Some(b) = crate::walls::reach(m, host) else { return Vec::new() };
    let mut out = Vec::new();
    for other in m.lookup().walls_near_at(&[(b, m.base_z(host), m.top_z(host))], m.thickness(host) + TOL) {
        if other == id {
            continue;
        }
        if let Some(cw) = m.wall(other)
            && let Some(cut) = embeds(m, cw, host)
        {
            out.push(cut);
        }
    }
    out
}
