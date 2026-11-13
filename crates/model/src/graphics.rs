//! Visibility and graphics: what a view hides and how it draws the rest, as Revit's
//! Visibility/Graphics does. A view (a level's plan, a section, an elevation or a 3D view) holds
//! [`Graphics`]: overrides by category, by element, and by the project's view filters, each hiding
//! what it matches or drawing it in its own colours, weights and dashes; its lines and fills in
//! projection (what lies below the cut plane, beyond a section's line) apart from those the view
//! cuts. A [`ViewTemplate`] holds the same settings under a name, for views to follow. [`Look`] is
//! what a view's graphics come to for each element: the drawing code asks it what to leave out and
//! how to draw the rest, and it dresses the finished drawing.

use bimcraft_i18n::{msg, ui_msg};

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::language::Language;
use crate::model::{Element, Guid, Id, Key, MAX_TEXT, Model, ViewRange};
use crate::pen::{MAX_PEN, Pen};
use crate::prim::{Part, Prim, Rgb};
use crate::render::Drawing;
use crate::schedule;

/// The categories a plan, a section or a 3D view draws, and so may hide and override: those of the
/// model, then those of the annotation. A family component is of its family's category (see [`COMPONENTS`]); `component`, which
/// names them all, is no row of its own (see [`COMPONENT`]).
pub const MODEL: [&str; 49] = [
    "wall",
    "floor",
    "ceiling",
    "shaft",
    "mass",
    "door",
    "window",
    "room",
    "roomseparator",
    "column",
    "structuralcolumn",
    "footing",
    "beam",
    "stripfooting",
    "pipe",
    "duct",
    "conduit",
    "cabletray",
    "wire",
    "downconductor",
    "groundconductor",
    "cable",
    "rebar",
    "furniture",
    "casework",
    "plumbing",
    "drain",
    "pipefitting",
    "pipeaccessory",
    "equipment",
    "outlet",
    "switch",
    "airterminal",
    "earthing",
    "inspectionbox",
    "lighting",
    "generic",
    "dataoutlet",
    "voiceoutlet",
    "patchpanel",
    "rack",
    "stair",
    "ramp",
    "railing",
    "roof",
    // The site and masterplan (masterplan.rs): Revit's Toposolid, Pads, Property Lines, and the site's own.
    "toposolid",
    "buildingpad",
    "propertyline",
    "site",
];
/// What every family component is, named as a whole: an override, a view filter, a view template or a schedule that names
/// `component` means every one of [`COMPONENTS`], as it did when they were one row. It is no row of Visibility/Graphics or
/// Object Styles any more, but a file or a command may still say it; a category's own override is laid over it.
pub const COMPONENT: &str = "component";
/// The categories of family components, the names a family's category goes by in a file: one row each in Visibility/Graphics
/// and Object Styles, in [`MODEL`]. A door's or a window's family is placed in a wall as a door or a window, so it has none.
pub const COMPONENTS: [&str; 18] = [
    "furniture",
    "casework",
    "plumbing",
    "drain",
    "pipefitting",
    "pipeaccessory",
    "equipment",
    "outlet",
    "switch",
    "airterminal",
    "earthing",
    "inspectionbox",
    "lighting",
    "generic",
    "dataoutlet",
    "voiceoutlet",
    "patchpanel",
    "rack",
];
pub const ANNOTATION: [&str; 10] = ["dimension", "tag", "text", "line", "region", "spot", "grid", "level", "section", "elevation"];
/// The point clouds, which views hide and override as Revit's Visibility/Graphics does on a tab of their own: not a model
/// category, so Object Styles has no row for them.
pub const SCANS: [&str; 1] = ["pointcloud"];
/// The imports, which views hide and override on a tab of their own too, as Revit's Imported Categories: `cadimport`, the
/// box over its table (Show imported categories in this view), which hides every CAD drawing and every PDF page's lines in the
/// view; and `familyimport`, its Imports in Families row, what families bring in from CAD files (none of the project's yet).
pub const IMPORTED: [&str; 2] = ["cadimport", "familyimport"];

/// A subcategory: a part of a category a view hides, shows and overrides apart from the rest, as Revit's (Rooms >
/// Reference). It is held in a view's graphics by its `key`, as a category is; `off` ones are hidden unless the view shows
/// them. A category hidden hides its subcategories. Some are a kind of element of their own (`roomseparator`), others a
/// part of their category's elements that the drawing code asks after (`roomreference`, the cross at a room's point).
#[derive(Clone, Copy, Debug)]
pub struct Sub {
    pub key: &'static str,
    pub parent: &'static str,
    /// Its name, in English, as Visibility/Graphics lists it under its category.
    pub name: &'static str,
    pub off: bool,
}

/// Every subcategory, under its category. A new one is a row here, and its drawing code asks [`Graphics::hides`] (or the
/// view's [`Look`], for a kind of element).
pub const SUBCATEGORIES: &[Sub] = &[
    Sub { key: "roomreference", parent: "room", name: ui_msg!("Reference"), off: true },
    Sub { key: "roomseparator", parent: "room", name: ui_msg!("Room Separation Lines"), off: false },
];

/// The subcategory `key` is, if it is one.
pub fn sub(key: &str) -> Option<&'static Sub> {
    SUBCATEGORIES.iter().find(|s| s.key == key)
}

/// The subcategories of category `parent`, in order.
pub fn subcategories(parent: &str) -> impl Iterator<Item = &'static Sub> + '_ {
    SUBCATEGORIES.iter().filter(move |s| s.parent == parent)
}

/// The categories a 3D view hides unless it shows them: rooms, which a 3D view draws, once shown, as see-through solids in
/// the selection's colour (and a selected one whatever it says). Every new 3D view starts with them hidden.
pub const OFF_IN_3D: [&str; 1] = ["room"];

/// Whether `key` is hidden unless a view shows it.
pub fn off_unless_shown(key: &str) -> bool {
    sub(key).is_some_and(|s| s.off)
}

/// The category of family components a family of `category` has, as a row of [`MODEL`]; none for a door's or a window's, which
/// are placed in walls as doors and windows. Every other category is named here, so a new one is a compile error until it is.
pub fn component_category(category: crate::family::Category) -> Option<&'static str> {
    use crate::family::Category as C;
    Some(match category {
        C::Furniture => "furniture",
        C::Casework => "casework",
        C::Plumbing => "plumbing",
        C::Drain => "drain",
        C::PipeFitting => "pipefitting",
        C::PipeAccessory => "pipeaccessory",
        C::Equipment => "equipment",
        C::Outlet => "outlet",
        C::Switch => "switch",
        C::AirTerminal => "airterminal",
        C::Earthing => "earthing",
        C::InspectionBox => "inspectionbox",
        C::Lighting => "lighting",
        C::Generic => "generic",
        C::DataOutlet => "dataoutlet",
        C::VoiceOutlet => "voiceoutlet",
        C::PatchPanel => "patchpanel",
        C::Rack => "rack",
        C::Door | C::Window => return None,
    })
}

/// The category views and Object Styles know element `e` by: its own, but for a component, which is of its family's category
/// (`component` when its family is gone, or is a door's or a window's).
pub fn category_of(m: &Model, e: &Element) -> &'static str {
    match e {
        Element::Component(c) => m.families.get(&c.family).and_then(|f| component_category(f.category)).unwrap_or(COMPONENT),
        // A 3D view's dimensions are Dimensions to its Visibility/Graphics.
        Element::Dimension3D(_) => "dimension",
        other => other.category(),
    }
}

/// Whether `category` is one of the model's: a row of [`MODEL`], or `component`, which names the family categories.
pub fn is_model(category: &str) -> bool {
    MODEL.contains(&category) || category == COMPONENT
}

/// Whether views draw elements of `category`, or hide and show it (a subcategory).
pub fn drawn(category: &str) -> bool {
    is_model(category)
        || ANNOTATION.contains(&category)
        || SCANS.contains(&category)
        || IMPORTED.contains(&category)
        || sub(category).is_some()
}

/// How much of a mass a 3D view sees through, percent, unless its graphics say otherwise.
pub const MASS_TRANSPARENCY: u8 = 60;

/// The most overrides a view holds (by category, and by element), the filters it applies, and the view
/// filters and view templates a project has.
pub const MAX_ENTRIES: usize = 100_000;
pub const MAX_APPLIED: usize = 32;
pub const MAX_FILTERS: usize = 64;
pub const MAX_TEMPLATES: usize = 64;
/// The most edges of Linework a view holds: each is split out of its element's lines when the view is drawn.
pub const MAX_LINEWORK: usize = 2000;

/// How much a view draws of what it cuts. Coarse: the cut in one poché, an opening only its gap and jambs (a door of the
/// project's own type its leaf and swing too, as Revit's door families show theirs at every level). Medium: a
/// compound element's layers, each a fill with a line between, and an opening's leaf, swing and glass. Fine: the layers'
/// patterns too, and a floor's surface pattern.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    Coarse,
    Medium,
    #[default]
    Fine,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Coarse, Detail::Medium, Detail::Fine];

    /// What it is called, in English.
    pub fn name(self) -> &'static str {
        match self {
            Detail::Coarse => ui_msg!("Coarse"),
            Detail::Medium => ui_msg!("Medium"),
            Detail::Fine => ui_msg!("Fine"),
        }
    }

    /// A level of detail by its English name, in any case.
    pub fn parse(s: &str) -> Option<Detail> {
        Detail::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// How a 3D view draws its surfaces. Shaded: lit, in their materials' colours and textures. Hidden line: paper-white, with
/// their edges, what a face hides staying hidden. Wireframe: edges only, every one, seen through the faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualStyle {
    #[default]
    Shaded,
    HiddenLine,
    Wireframe,
}

impl VisualStyle {
    pub const ALL: [VisualStyle; 3] = [VisualStyle::Shaded, VisualStyle::HiddenLine, VisualStyle::Wireframe];

    /// What it is called, in English.
    pub fn name(self) -> &'static str {
        match self {
            VisualStyle::Shaded => ui_msg!("Shaded"),
            VisualStyle::HiddenLine => ui_msg!("Hidden Line"),
            VisualStyle::Wireframe => ui_msg!("Wireframe"),
        }
    }

    /// A style by its name, in any case, its words apart by a space, an underscore or nothing: `Hidden Line`, `hidden_line`.
    pub fn parse(s: &str) -> Option<VisualStyle> {
        let plain = |s: &str| s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
        VisualStyle::ALL.into_iter().find(|v| plain(v.name()) == plain(s))
    }
}

/// The scales, 1 : n, a view's annotation is sized for and a viewport shows it at.
pub const SCALES: std::ops::RangeInclusive<f64> = 1.0..=100_000.0;

/// How a line is dashed: not at all, or in dashes or dots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dash {
    Solid,
    Dashed,
    Dotted,
}

impl Dash {
    /// The on and off lengths a line is drawn with, px.
    pub(crate) fn pattern(self) -> Option<Vec<f32>> {
        match self {
            Dash::Solid => None,
            Dash::Dashed => Some(vec![10.0, 5.0]),
            Dash::Dotted => Some(vec![2.0, 4.0]),
        }
    }
}

/// A colour as JSON gives it: `[r, g, b]`, each 0 to 255, or `"#rrggbb"`.
mod rgb {
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    pub fn serialize<S: Serializer>(c: &Option<[u8; 3]>, s: S) -> Result<S::Ok, S::Error> {
        match c {
            Some(c) => s.collect_seq(c),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 3]>, D::Error> {
        let v = Value::deserialize(d)?;
        if v.is_null() {
            return Ok(None);
        }
        crate::model::colour(&v)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("{v} isn't a colour: [r, g, b], each 0 to 255, or \"#rrggbb\"")))
    }
}

/// One overridden pattern layer. No pattern explicitly hides the layer; no layer override inherits it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PatternFill {
    pub pattern: Option<String>,
    pub color: [u8; 3],
}

/// How lines and fills draw: line settings, the solid fill colour, and independent pattern layers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Style {
    #[serde(with = "rgb", skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<Pen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<Dash>,
    #[serde(with = "rgb", skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<PatternFill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<PatternFill>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }

    /// This style's settings, and `base`'s where it has none.
    fn over(&self, base: &Style) -> Style {
        Style {
            color: self.color.or(base.color),
            weight: self.weight.or(base.weight),
            dash: self.dash.or(base.dash),
            fill: self.fill.or(base.fill),
            foreground: self.foreground.clone().or_else(|| base.foreground.clone()),
            background: self.background.clone().or_else(|| base.background.clone()),
        }
    }

    fn check(&self, who: &str) -> Result<(), String> {
        if let Some(w) = self.weight
            && !w.is_valid()
        {
            return Err(format!("{who}: a line weight is a pen, 0 (a hairline) to {MAX_PEN}, not {}", w.0));
        }
        Ok(())
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(x: &u8) -> bool {
    *x == 0
}

fn is_fine(d: &Detail) -> bool {
    *d == Detail::Fine
}

fn is_shaded(v: &VisualStyle) -> bool {
    *v == VisualStyle::Shaded
}

/// How a view draws something: hidden, or its lines and fills as they are in projection and
/// where the view cuts them, lightened (`halftone`, halfway to the paper), and in 3D seen through
/// (`transparency`, percent).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Override {
    #[serde(skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(skip_serializing_if = "Style::is_empty")]
    pub projection: Style,
    #[serde(skip_serializing_if = "Style::is_empty")]
    pub cut: Style,
    #[serde(skip_serializing_if = "is_false")]
    pub halftone: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub transparency: u8,
    /// Shown, a subcategory hidden unless a view shows it ([`Sub::off`]).
    #[serde(skip_serializing_if = "is_false")]
    pub shown: bool,
}

impl Override {
    pub(crate) fn check_patterns(&self, m: &Model) -> Result<(), String> {
        for s in [&self.projection, &self.cut] {
            for name in [&s.foreground, &s.background].into_iter().flatten().filter_map(|p| p.pattern.as_ref()) {
                if !m.fill_patterns.contains_key(name) {
                    return Err(format!("there's no fill pattern `{name}`"));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn patterns_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [&mut self.projection, &mut self.cut]
            .into_iter()
            .flat_map(|s| [&mut s.foreground, &mut s.background])
            .flatten()
            .filter_map(|p| p.pattern.as_mut())
    }

    pub fn is_empty(&self) -> bool {
        *self == Override::default()
    }

    /// This override's settings, and `base`'s where it has none.
    pub(crate) fn over(&self, base: &Override) -> Override {
        Override {
            hidden: self.hidden || base.hidden,
            projection: self.projection.over(&base.projection),
            cut: self.cut.over(&base.cut),
            halftone: self.halftone || base.halftone,
            transparency: if self.transparency > 0 { self.transparency } else { base.transparency },
            shown: self.shown || base.shown,
        }
    }

    pub(crate) fn check(&self, who: &str) -> Result<(), String> {
        self.projection.check(&format!("{who} projection"))?;
        self.cut.check(&format!("{who} cut"))?;
        if self.transparency > 100 {
            return Err(format!("{who}: transparency is a percent, 0 to 100, not {}", self.transparency));
        }
        Ok(())
    }
}

/// A view filter a view applies: its override for what the filter picks out (hidden, or drawn
/// another way). Filters apply in order, the first prevailing where they disagree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "AppliedJson", into = "AppliedJson")]
pub struct Applied {
    pub filter: String,
    pub over: Override,
}

/// An applied filter as JSON has it: the override's settings beside the filter's name.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppliedJson {
    filter: String,
    #[serde(default, skip_serializing_if = "is_false")]
    hidden: bool,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    projection: Style,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    cut: Style,
    #[serde(default, skip_serializing_if = "is_false")]
    halftone: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    transparency: u8,
}

impl From<AppliedJson> for Applied {
    fn from(a: AppliedJson) -> Applied {
        let over = Override {
            hidden: a.hidden,
            projection: a.projection,
            cut: a.cut,
            halftone: a.halftone,
            transparency: a.transparency,
            shown: false,
        };
        Applied { filter: a.filter, over }
    }
}

impl From<Applied> for AppliedJson {
    fn from(a: Applied) -> AppliedJson {
        let Override { hidden, projection, cut, halftone, transparency, .. } = a.over;
        AppliedJson { filter: a.filter, hidden, projection, cut, halftone, transparency }
    }
}

/// A view's visibility and graphics: overrides by category (`categories`, the category's name),
/// by the project's view filters, and by element (`elements`, its GUID); the template it follows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Graphics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub categories: BTreeMap<String, Override>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Applied>,
    /// By element, its GUID.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub elements: BTreeMap<Guid, Override>,
    /// The phase it shows (the project's last unless it says), and the phase filter that says how
    /// each status of what's in it shows (every status as its category does, unless it says).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_filter: Option<String>,
    /// The design option it shows of each set, by set (a set's primary, unless it says).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// The scale, 1 : `scale`, a plan's or a section's annotation is sized for; none, annotation keeps its size on screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// How much it draws of what it cuts.
    #[serde(skip_serializing_if = "is_fine")]
    pub detail: Detail,
    /// How a 3D view draws its surfaces.
    #[serde(skip_serializing_if = "is_shaded")]
    pub style: VisualStyle,
    /// Canvas grid owned by this view, or supplied by its view template.
    #[serde(skip_serializing_if = "crate::background_grid::BackgroundGrid::is_default")]
    pub background_grid: crate::background_grid::BackgroundGrid,
    /// Revit's Linework: an edge of an element drawn in a line style of the view's own, or not at all.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub linework: Vec<Linework>,
    /// The layers of each CAD drawing it hides, by the drawing's name: Revit's Imported Categories, layer by layer.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub layers: BTreeMap<String, BTreeSet<String>>,
    /// The layers of each PDF page's lines it hides, by the image's name: listed with the CAD drawings' in Imported
    /// Categories, and shown and hidden as theirs are.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pdf_layers: BTreeMap<String, BTreeSet<String>>,
    /// How it draws each CAD drawing, by the drawing's name: Imported Categories' row of the import and of each of its
    /// layers.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub imports: BTreeMap<String, ImportLook>,
    /// How it draws each PDF page's lines, by the image's name, as `imports` does a drawing's.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pdf_imports: BTreeMap<String, ImportLook>,
}

/// How a view draws an imported drawing (a DWG's, or a PDF page's lines), as Revit's Visibility/Graphics' Imported Categories
/// has it: the import's own row (`all`: hidden, its projection lines' colour, weight and pattern, halftone) and each layer's
/// (`layers`: its lines and halftone, by the layer's name). A layer's own settings win over the import's; which layers it
/// hides is `layers` and `pdf_layers` of its graphics.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImportLook {
    #[serde(skip_serializing_if = "Override::is_empty")]
    pub all: Override,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub layers: BTreeMap<String, Override>,
}

impl ImportLook {
    pub fn is_empty(&self) -> bool {
        *self == ImportLook::default()
    }

    /// Its overrides with their patterns taken off, which an import has none of, as in Revit: a file that holds them (written
    /// while BIMCraft let an import's Patterns be overridden) reads with them left out.
    fn drop_patterns(&mut self) {
        for o in std::iter::once(&mut self.all).chain(self.layers.values_mut()) {
            o.projection = lines_of(&o.projection);
        }
    }

    /// How it draws layer `layer`'s lines: the layer's settings, and the import's where it has none.
    pub fn of_layer(&self, layer: &str) -> Override {
        match self.layers.get(layer) {
            Some(o) => o.over(&self.all),
            None => self.all.clone(),
        }
    }

    /// Whether it is sound: an import's row sets its visibility, its projection lines and its halftone; a layer's its lines
    /// and its halftone (whether it shows is the view's `layers`). Its Patterns can't be overridden, as in Revit.
    fn check(&self, who: &str) -> Result<(), String> {
        let only_lines = |o: &Override, what: &str| -> Result<(), String> {
            if o.projection != lines_of(&o.projection) || !o.cut.is_empty() || o.transparency != 0 || o.shown {
                return Err(format!(
                    "{what}: an import sets its projection lines (colour, weight, pattern) and halftone only; its Patterns can't be overridden, as in Revit"
                ));
            }
            o.check(what)
        };
        only_lines(&self.all, &format!("{who}'s row"))?;
        if self.layers.len() > crate::model::MAX_HIDDEN {
            return Err(format!("{who}: it sets more than {} layers", crate::model::MAX_HIDDEN));
        }
        for (name, o) in &self.layers {
            if name.chars().count() > crate::model::MAX_TEXT {
                return Err(format!("{who}: a layer's name is longer than {} characters", crate::model::MAX_TEXT));
            }
            if o.hidden {
                return Err(format!("{who}: layer `{name}` is hidden with the view's `layers`, not an override"));
            }
            only_lines(o, &format!("{who}'s layer `{name}`"))?;
        }
        Ok(())
    }
}

/// A style's lines alone (its colour, weight and dash), its patterns taken off: what an import's row and layers set.
fn lines_of(s: &Style) -> Style {
    Style { color: s.color, weight: s.weight, dash: s.dash, ..Style::default() }
}

/// What a view and a template hold of an import's Patterns, left out ([`ImportLook::drop_patterns`]): its imports' and PDF
/// pages' rows and layers, and Imports in Families'.
pub(crate) fn drop_import_patterns(g: &mut Graphics) {
    for look in g.imports.values_mut().chain(g.pdf_imports.values_mut()) {
        look.drop_patterns();
    }
    for c in IMPORTED {
        if let Some(o) = g.categories.get_mut(c) {
            o.projection = lines_of(&o.projection);
        }
    }
}

/// An edge of `element` as the view draws it, from `from` to `to` (mm in the view's terms), in line style `style` instead
/// of its own; none, it isn't drawn (Revit's `<Invisible lines>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Linework {
    pub element: Guid,
    pub from: crate::geom::P2,
    pub to: crate::geom::P2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

impl Graphics {
    pub fn is_default(&self) -> bool {
        *self == Graphics::default()
    }

    /// Whether it holds what [`crate::model::LAYERS_FORMAT`] 21 brought: a PDF page's layers hidden, or how an import or a PDF page
    /// draws; a build that reads up to 20 would refuse it as a field it doesn't know.
    pub fn holds_imports(&self) -> bool {
        !self.pdf_layers.is_empty() || !self.imports.is_empty() || !self.pdf_imports.is_empty()
    }

    /// The scale, 1 : n, these hold a view at in a project in `units`: their own, else the units' default (1 : 100, or
    /// 1/8" = 1'-0"). Every plan, section and elevation has a scale, so what it draws is paper at that scale: its
    /// lettering, marks and pens. Read it of [`Graphics::resolved`] for the one in force, as [`scale_of`] does.
    pub fn scale_n(&self, units: crate::units::Units) -> f64 {
        self.scale.unwrap_or_else(|| units.default_scale())
    }

    /// The settings in force: these, but for the groups the template followed controls, which are its.
    pub fn resolved(&self, m: &Model) -> std::borrow::Cow<'_, Graphics> {
        let Some(t) = self.template.as_ref().and_then(|n| m.view_templates.get(n)) else { return std::borrow::Cow::Borrowed(self) };
        let mut g = self.clone();
        if t.controls.contains(&Group::Categories) {
            g.categories = t.graphics.categories.clone();
        }
        if t.controls.contains(&Group::Filters) {
            g.filters = t.graphics.filters.clone();
        }
        if t.controls.contains(&Group::Phase) {
            (g.phase, g.phase_filter) = (t.graphics.phase.clone(), t.graphics.phase_filter.clone());
        }
        if t.controls.contains(&Group::Options) {
            g.options = t.graphics.options.clone();
        }
        if t.controls.contains(&Group::Scale) {
            g.scale = t.graphics.scale;
        }
        if t.controls.contains(&Group::Detail) {
            g.detail = t.graphics.detail;
        }
        if t.controls.contains(&Group::Style) {
            g.style = t.graphics.style;
        }
        if t.controls.contains(&Group::BackgroundGrid) {
            g.background_grid = t.graphics.background_grid;
        }
        if t.controls.contains(&Group::Imports) {
            g.layers = t.graphics.layers.clone();
            g.pdf_layers = t.graphics.pdf_layers.clone();
            g.imports = t.graphics.imports.clone();
            g.pdf_imports = t.graphics.pdf_imports.clone();
        }
        std::borrow::Cow::Owned(g)
    }

    /// The view template it follows, when that controls `group`: the group's settings aren't the view's own to change.
    pub fn controlled_by(&self, m: &Model, group: Group) -> Option<&str> {
        let name = self.template.as_deref()?;
        m.view_templates.get(name)?.controls.contains(&group).then_some(name)
    }

    /// Whether it hides and overrides nothing: its scale and detail level change what the drawing code draws, not what a
    /// view's [`Look`] says to leave out.
    fn draws_as_is(&self) -> bool {
        (&self.template, &self.phase, &self.phase_filter) == (&None, &None, &None)
            && self.categories.is_empty()
            && self.filters.is_empty()
            && self.elements.is_empty()
            && self.options.is_empty()
    }

    /// Whether a 3D view with these graphics draws category `c`: as [`Graphics::hides`] says, and one of [`OFF_IN_3D`] only
    /// where the view shows it.
    pub fn shows_in_3d(&self, c: &str) -> bool {
        !self.hides(c) && (!OFF_IN_3D.contains(&c) || self.categories.get(c).is_some_and(|o| o.shown))
    }

    /// Whether category or subcategory `c` is hidden: hidden itself, off and not shown, or its category hidden.
    pub fn hides(&self, c: &str) -> bool {
        let own = match self.categories.get(c) {
            Some(o) => o.hidden || (off_unless_shown(c) && !o.shown),
            None => off_unless_shown(c),
        };
        own || sub(c).is_some_and(|s| self.hides(s.parent)) || (COMPONENTS.contains(&c) && self.hides(COMPONENT))
    }

    /// These with `component` split into the family categories it names: each takes its own override laid over it, and
    /// `component` is gone. What a view draws is the same, and every category has the row an editor shows it by.
    pub fn split_component(&mut self) {
        let Some(all) = self.categories.remove(COMPONENT) else { return };
        for key in COMPONENTS {
            let merged = self.categories.get(key).map_or_else(|| all.clone(), |own| own.over(&all));
            self.categories.insert(key.to_string(), merged);
        }
    }

    /// The category that hides `c` from this view, the one to show to bring it back: `c` itself, else what it is under
    /// (`component`, for a category of a family component). None when it isn't hidden.
    pub fn hidden_by(&self, c: &'static str) -> Option<&'static str> {
        let own = match self.categories.get(c) {
            Some(o) => o.hidden || (off_unless_shown(c) && !o.shown),
            None => off_unless_shown(c),
        };
        if own {
            return Some(c);
        }
        let parent = sub(c).map(|s| s.parent).or_else(|| COMPONENTS.contains(&c).then_some(COMPONENT))?;
        self.hidden_by(parent)
    }

    /// Whether it is sound in `m`: known categories, overrides in range, filters and the template
    /// there are.
    pub fn check(&self, m: &Model) -> Result<(), String> {
        self.background_grid.check()?;
        if self.categories.len() > MAX_ENTRIES || self.elements.len() > MAX_ENTRIES {
            return Err(format!("it has more than {MAX_ENTRIES} overrides"));
        }
        if self.filters.len() > MAX_APPLIED {
            return Err(format!("it applies {} filters; a view applies up to {MAX_APPLIED}", self.filters.len()));
        }
        for (c, o) in &self.categories {
            if !drawn(c) {
                return Err(format!(
                    "`{c}` isn't a category views draw; there are: {}",
                    MODEL
                        .iter()
                        .chain(&ANNOTATION)
                        .chain(&SCANS)
                        .chain(&IMPORTED)
                        .chain(&[COMPONENT])
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            o.check(&format!("category `{c}`"))?;
            if IMPORTED.contains(&c.as_str()) && o.projection != lines_of(&o.projection) {
                return Err(format!("category `{c}`: an import's Patterns can't be overridden, as in Revit; it takes its lines only"));
            }
        }
        for (id, o) in &self.elements {
            o.check(&format!("element {id}"))?;
        }
        if self.linework.len() > MAX_LINEWORK {
            return Err(format!(
                "it has {} edges of Linework; a view holds up to {MAX_LINEWORK}, so take some off (linework with remove)",
                self.linework.len()
            ));
        }
        for (i, l) in self.linework.iter().enumerate() {
            let ends = [l.from, l.to];
            if !ends.iter().all(|p| p.is_finite() && p.0.abs() <= crate::model::MAX_COORD && p.1.abs() <= crate::model::MAX_COORD) {
                return Err(format!("Linework edge {i}: its ends are numbers within {} mm", crate::model::MAX_COORD));
            }
            if let Some(st) = &l.style
                && !m.line_styles.contains_key(st)
            {
                return Err(format!("Linework edge {i}: there's no line style `{st}`{}", known(m.line_styles.keys())));
            }
        }
        for o in self.categories.values().chain(self.elements.values()).chain(self.filters.iter().map(|a| &a.over)) {
            o.check_patterns(m)?;
        }
        if self.layers.len() > crate::cad::MAX_CADS || self.layers.values().any(|l| l.len() > crate::model::MAX_HIDDEN) {
            return Err(format!(
                "it hides layers of more than {} CAD drawings, or more than {} of one",
                crate::cad::MAX_CADS,
                crate::model::MAX_HIDDEN
            ));
        }
        if self.pdf_layers.len() > crate::raster::MAX_TYPES || self.pdf_layers.values().any(|l| l.len() > crate::model::MAX_HIDDEN) {
            return Err(format!(
                "it hides layers of more than {} PDF pages, or more than {} of one",
                crate::raster::MAX_TYPES,
                crate::model::MAX_HIDDEN
            ));
        }
        for (cad, layers) in self.layers.iter().chain(&self.pdf_layers) {
            if let Some(n) = std::iter::once(cad).chain(layers).find(|n| n.chars().count() > crate::model::MAX_TEXT) {
                return Err(format!("the layer or drawing `{n}` is longer than {} characters", crate::model::MAX_TEXT));
            }
        }
        if self.imports.len() > crate::cad::MAX_CADS || self.pdf_imports.len() > crate::raster::MAX_TYPES {
            return Err(format!(
                "it sets how more than {} CAD drawings or {} PDF pages draw",
                crate::cad::MAX_CADS,
                crate::raster::MAX_TYPES
            ));
        }
        for (name, look) in &self.imports {
            look.check(&format!("CAD drawing `{name}`"))?;
        }
        for (name, look) in &self.pdf_imports {
            look.check(&format!("PDF page `{name}`"))?;
        }
        let mut seen = BTreeSet::new();
        for a in &self.filters {
            if !m.view_filters.contains_key(&a.filter) {
                return Err(format!("there's no view filter `{}`{}", a.filter, known(m.view_filters.keys())));
            }
            if !seen.insert(a.filter.as_str()) {
                return Err(format!("filter `{}` is applied twice", a.filter));
            }
            a.over.check(&format!("filter `{}`", a.filter))?;
        }
        if let Some(t) = &self.template
            && !m.view_templates.contains_key(t)
        {
            return Err(format!("there's no view template `{t}`{}", known(m.view_templates.keys())));
        }
        if let Some(p) = &self.phase
            && !m.phases.contains(p)
        {
            return Err(format!("there's no phase `{p}` (there are: {})", m.phases.join(", ")));
        }
        if let Some(f) = &self.phase_filter
            && !m.phase_filters.contains_key(f)
        {
            return Err(format!("there's no phase filter `{f}`{}", known(m.phase_filters.keys())));
        }
        if let Some(n) = self.scale
            && !SCALES.contains(&n)
        {
            return Err(format!("its scale 1 : {n} isn't between 1 : {} and 1 : {}", SCALES.start(), SCALES.end()));
        }
        if self.options.len() > crate::phasing::MAX_OPTION_SETS {
            return Err(format!(
                "it chooses options of {} sets; a project has up to {}",
                self.options.len(),
                crate::phasing::MAX_OPTION_SETS
            ));
        }
        for (set, option) in &self.options {
            match m.option_sets.get(set) {
                None => return Err(format!("there's no design option set `{set}`{}", known(m.option_sets.keys()))),
                Some(s) if !s.options.contains(option) => {
                    return Err(format!("design option set `{set}` has no option `{option}` (there are: {})", s.options.join(", ")));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

/// ` (there are: a, b)`, or nothing when there are none.
fn known<'a>(names: impl Iterator<Item = &'a String>) -> String {
    let names: Vec<&str> = names.map(String::as_str).collect();
    if names.is_empty() { String::new() } else { format!(" (there are: {})", names.join(", ")) }
}

/// A view filter: the elements of its categories that hold every one of its rules (a schedule
/// filter's: a field, how to compare it, a value), for views to hide or override.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewFilter {
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<schedule::Filter>,
}

impl ViewFilter {
    /// The elements it picks out.
    pub fn matching(&self, m: &Model) -> BTreeSet<Id> {
        schedule::picks(m, &self.categories, &self.rules)
    }
}

impl ViewFilter {
    /// Whether it is sound: categories there are schedules of, and rules every one of them has.
    pub fn check(&self, m: &Model) -> Result<(), String> {
        if self.categories.is_empty() {
            return Err(format!("it needs categories, one or more of: {}", schedule::categories().join(", ")));
        }
        for c in &self.categories {
            if !schedule::categories().contains(&c.as_str()) {
                return Err(format!("`{c}` isn't a category filters pick from; there are: {}", schedule::categories().join(", ")));
            }
        }
        schedule::check_rules(m, &self.categories, &self.rules)
    }
}

/// A group of a view's settings a template can control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    /// The categories' overrides and which are hidden.
    Categories,
    /// The filters applied.
    Filters,
    /// A plan's view range.
    Range,
    /// The phase it shows and its phase filter.
    Phase,
    /// The design options it shows.
    Options,
    /// A plan's, section's or elevation's scale.
    Scale,
    /// A plan's, section's or elevation's detail level.
    Detail,
    /// A 3D view's visual style.
    Style,
    /// The canvas background grid display settings.
    #[serde(rename = "background_grid")]
    BackgroundGrid,
    /// Imported Categories: which layers of each CAD drawing and PDF page it hides, and how each draws (Revit's V/G Overrides
    /// Import).
    Imports,
}

impl Group {
    pub const ALL: [Group; 10] = [
        Group::Categories,
        Group::Filters,
        Group::Range,
        Group::Phase,
        Group::Options,
        Group::Scale,
        Group::Detail,
        Group::Style,
        Group::BackgroundGrid,
        Group::Imports,
    ];
}

/// The kind of view a template is for, as Revit's are: plans (floor and ceiling), elevations and sections, or 3D views.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ViewKind {
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "section")]
    Section,
    #[serde(rename = "3d")]
    ThreeD,
}

impl ViewKind {
    pub const ALL: [ViewKind; 3] = [ViewKind::Plan, ViewKind::Section, ViewKind::ThreeD];

    /// The kind view element `e` is, if it is a view a template can be for.
    pub fn of(e: &Element) -> Option<ViewKind> {
        match e {
            Element::Level(_) | Element::PlanView(_) => Some(ViewKind::Plan),
            Element::Section(_) | Element::Elevation(_) => Some(ViewKind::Section),
            Element::View3D(_) => Some(ViewKind::ThreeD),
            _ => None,
        }
    }

    /// The groups a template for this kind of view holds: a plan's view range, a 3D view's visual style, the scale, detail
    /// level and background grids of the others.
    pub fn groups(self) -> Vec<Group> {
        Group::ALL
            .into_iter()
            .filter(|g| {
                !matches!(
                    (self, g),
                    (ViewKind::Plan, Group::Style)
                        | (ViewKind::Section, Group::Range | Group::Style)
                        | (ViewKind::ThreeD, Group::Range | Group::Scale | Group::Detail | Group::BackgroundGrid)
                )
            })
            .collect()
    }
}

/// A view template: settings under a name for views to follow, the groups in `controls`, for views of `kind` (any, when it
/// says none).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewTemplate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ViewKind>,
    pub controls: BTreeSet<Group>,
    #[serde(default, skip_serializing_if = "Graphics::is_default")]
    pub graphics: Graphics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_range: Option<ViewRange>,
}

impl ViewTemplate {
    /// Whether it is sound: its graphics are a view's, without the parts that are a view's own.
    pub fn check(&self, m: &Model) -> Result<(), String> {
        if self.graphics.template.is_some() || !self.graphics.elements.is_empty() {
            return Err("a template holds categories and filters; the elements and the template a view follows are its own".into());
        }
        self.graphics.check(m)?;
        if let Some(r) = &self.view_range {
            crate::model::range_ok("its", r)?;
        }
        Ok(())
    }
}

/// The view templates a new project starts with, named in `language`: three plan templates for the electrical disciplines,
/// each controlling the view's categories alone (its scale, detail and range stay the view's own). Power draws luminaires in
/// halftone behind the outlets and equipment; Lighting hides the outlets; Ceiling keeps luminaires and cable trays and hides
/// what stands on the floor or the wall. A project from before has none, unless it made them.
pub fn starter_templates(language: Language) -> BTreeMap<String, ViewTemplate> {
    let template = |hidden: &[&str], halftone: &[&str]| {
        let mut categories = BTreeMap::new();
        for c in hidden {
            categories.insert((*c).to_string(), Override { hidden: true, ..Override::default() });
        }
        for c in halftone {
            categories.insert((*c).to_string(), Override { halftone: true, ..Override::default() });
        }
        ViewTemplate {
            kind: Some(ViewKind::Plan),
            controls: BTreeSet::from([Group::Categories]),
            graphics: Graphics { categories, ..Graphics::default() },
            view_range: None,
        }
    };
    [
        (msg!("Electrical Power"), template(&[], &["lighting"])),
        (msg!("Electrical Lighting"), template(&["outlet"], &[])),
        (msg!("Electrical Ceiling"), template(&["outlet", "switch", "equipment", "dataoutlet", "voiceoutlet", "patchpanel", "rack"], &[])),
    ]
    .into_iter()
    .map(|(name, t)| (language.word(name).to_string(), t))
    .collect()
}

/// Whether the project's view filters and view templates are sound.
pub(crate) fn check_project(m: &Model) -> Result<(), String> {
    if m.view_filters.len() > MAX_FILTERS {
        return Err(format!("the project has {} view filters; it holds up to {MAX_FILTERS}", m.view_filters.len()));
    }
    if m.view_templates.len() > MAX_TEMPLATES {
        return Err(format!("the project has {} view templates; it holds up to {MAX_TEMPLATES}", m.view_templates.len()));
    }
    let named = |who: &str, n: &str| {
        if n.trim().is_empty() || n.chars().count() > MAX_NAME {
            Err(format!("{who} `{n}`: a name is 1 to {MAX_NAME} characters"))
        } else {
            Ok(())
        }
    };
    for (n, f) in &m.view_filters {
        named(ui_msg!("view filter"), n)?;
        f.check(m).map_err(|e| format!("view filter `{n}`: {e}"))?;
    }
    for (n, t) in &m.view_templates {
        named(ui_msg!("view template"), n)?;
        t.check(m).map_err(|e| format!("view template `{n}`: {e}"))?;
    }
    Ok(())
}

/// Whether changing view `old` into `new` leaves be what the view template `new` follows controls:
/// its categories, its filters, phase, design options, scale, detail level, visual style, a plan's view range.
/// (Unfollowing and changing in one go is fine.)
pub fn check_followed(m: &Model, old: &Element, new: &Element) -> Result<(), String> {
    let (Some(a), Some(b)) = (graphics_of_element(old), graphics_of_element(new)) else { return Ok(()) };
    let Some((name, t)) = b.template.as_ref().and_then(|n| Some((n, m.view_templates.get(n)?))) else { return Ok(()) };
    let follows = |what: Group, same: bool, label: &str| {
        if t.controls.contains(&what) && !same {
            return Err(format!("its {label} follow view template `{name}`; change the template, or unfollow it (`template: null`)"));
        }
        Ok(())
    };
    follows(Group::Categories, a.categories == b.categories, "categories")?;
    follows(Group::Filters, a.filters == b.filters, "filters")?;
    follows(Group::Phase, (&a.phase, &a.phase_filter) == (&b.phase, &b.phase_filter), "phase and phase filter")?;
    follows(Group::Options, a.options == b.options, "design options")?;
    follows(Group::Scale, a.scale == b.scale, "scale")?;
    follows(Group::Detail, a.detail == b.detail, "detail level")?;
    follows(Group::Style, a.style == b.style, "visual style")?;
    follows(Group::BackgroundGrid, a.background_grid == b.background_grid, "background grids")?;
    let imports = |g: &Graphics| (g.layers.clone(), g.pdf_layers.clone(), g.imports.clone(), g.pdf_imports.clone());
    follows(Group::Imports, imports(a) == imports(b), "imported categories")?;
    if let (Element::Level(x), Element::Level(y)) = (old, new) {
        follows(Group::Range, x.view_range == y.view_range, "view range")?;
    }
    if let (Element::PlanView(x), Element::PlanView(y)) = (old, new) {
        follows(Group::Range, x.view_range == y.view_range, "view range")?;
    }
    Ok(())
}

/// The graphics element `e` holds, if it is a view.
pub fn graphics_of_element(e: &Element) -> Option<&Graphics> {
    match e {
        Element::Level(l) => Some(&l.graphics),
        Element::PlanView(v) => Some(&v.graphics),
        Element::Section(v) | Element::Elevation(v) => Some(&v.graphics),
        Element::View3D(v) => Some(&v.graphics),
        _ => None,
    }
}

/// The scale, 1 : n, view `id` is held at: its own, or its template's where that controls it, else its project units'
/// default.
pub fn scale_of(m: &Model, id: impl Key) -> f64 {
    graphics_of(m, id).map_or_else(|| m.units.default_scale(), |g| g.resolved(m).scale_n(m.units))
}

/// The graphics view `view` holds, to change.
pub fn graphics_mut(e: &mut Element) -> Option<&mut Graphics> {
    match e {
        Element::Level(l) => Some(&mut l.graphics),
        Element::PlanView(v) => Some(&mut v.graphics),
        Element::Section(v) | Element::Elevation(v) => Some(&mut v.graphics),
        Element::View3D(v) => Some(&mut v.graphics),
        _ => None,
    }
}

/// The graphics view `view` holds: a level's, a plan view's, a section's, an elevation's or a 3D view's. A dependent view has
/// its parent's.
pub fn graphics_of(m: &Model, view: impl Key) -> Option<&Graphics> {
    graphics_of_element(m.elements.get(m.plan_source(view))?)
}

/// What a view's graphics come to for each element of the model.
#[derive(Default, PartialEq)]
pub struct Look {
    hidden: BTreeSet<Id>,
    /// The layers of each CAD drawing it hides, by the drawing's name.
    layers: BTreeMap<String, BTreeSet<String>>,
    /// The layers of each PDF page's lines it hides, by the image's name.
    pdf_layers: BTreeMap<String, BTreeSet<String>>,
    /// How it draws each CAD drawing, and each PDF page's lines.
    imports: BTreeMap<String, ImportLook>,
    pdf_imports: BTreeMap<String, ImportLook>,
    /// Imported Categories' Show imported categories in this view cleared: no import is drawn, nor a PDF page's lines.
    imports_hidden: bool,
    paint: BTreeMap<Id, Override>,
    style: VisualStyle,
    /// The section box a 3D view is cut to, its min and max corners, while it's on.
    section: Option<([f64; 3], [f64; 3])>,
    /// A 3D view's Graphic Display Options: its edges, silhouettes and ambient shadows.
    display: crate::camera::DisplayOptions,
    /// Its detail level, which says which of a family's forms it draws.
    detail: Detail,
    /// The way a section or an elevation looks, in plan: which of a family's forms it draws, by their front and side.
    looking: Option<crate::geom::P2>,
}

/// `c` halfway to the paper.
fn halftone(c: Rgb) -> Rgb {
    let half = |v: u8| v + (255 - v) / 2;
    Rgb(half(c.0), half(c.1), half(c.2))
}

impl Look {
    /// The resolved fill layers for the renderer, after category, filter and element precedence.
    pub(crate) fn pattern_style(&self, id: Id, cut: bool) -> Option<&Style> {
        let o = self.paint.get(&id)?;
        let s = if cut { &o.cut } else { &o.projection };
        (s.foreground.is_some() || s.background.is_some()).then_some(s)
    }

    pub(crate) fn pattern_styles(&self) -> BTreeMap<Id, [Style; 2]> {
        self.paint
            .iter()
            .filter(|(id, _)| self.pattern_style(**id, false).is_some() || self.pattern_style(**id, true).is_some())
            .map(|(id, o)| (*id, [o.projection.clone(), o.cut.clone()]))
            .collect()
    }

    /// What view `view` looks like: nothing hidden or painted when it isn't a view, or has no graphics.
    pub fn of(m: &Model, view: impl Key) -> Look {
        let look = graphics_of(m, view).map_or_else(Look::default, |own| Look::with(m, own));
        let display = match m.elements.get(view) {
            Some(Element::View3D(v)) => v.display,
            _ => Default::default(),
        };
        let detail = graphics_of(m, view).map_or_else(Detail::default, |g| g.resolved(m).detail);
        let looking = m.section(view).map(|v| v.looking());
        Look { section: m.section_box(view), display, detail, looking, ..look }
    }

    /// Its detail level.
    pub fn detail(&self) -> Detail {
        self.detail
    }

    /// Whether it draws triangle `t`: its element shown, and its form drawn at the view's detail level and, in a section or
    /// an elevation, the way it looks ([`crate::view3d::Vis`]).
    pub fn draws(&self, t: &crate::view3d::Tri) -> bool {
        self.shows(t.owner) && t.vis.drawn(self.detail, self.looking)
    }

    /// Whether a triangle of `vis` is drawn at its detail level and the way it looks, whatever element it is of.
    pub fn draws_vis(&self, vis: crate::view3d::Vis) -> bool {
        vis.drawn(self.detail, self.looking)
    }

    /// How a 3D view draws its edges and shadows: its Graphic Display Options.
    pub fn display(&self) -> crate::camera::DisplayOptions {
        self.display
    }

    /// The section box the view is cut to, its min and max corners: none unless it's a 3D view with one turned on.
    pub fn section_box(&self) -> Option<([f64; 3], [f64; 3])> {
        self.section
    }

    /// This look cut to section box `section` instead: a box being dragged, before its command runs.
    pub fn with_section_box(self, section: Option<([f64; 3], [f64; 3])>) -> Look {
        Look { section, ..self }
    }

    /// What the default 3D view looks like: as its own view's graphics say once something has changed
    /// them, else as those of a view with none (its last phase, and its sets' primary options).
    pub fn of_3d(m: &Model) -> Look {
        m.default_3d().map_or_else(|| Look::with(m, &Graphics::default()), |id| Look::of(m, id))
    }

    /// What a view with graphics `own` looks like.
    /// ponytail: worked out on every draw; cache it per model and view if a filter on a slow field
    /// (a room's area) makes redraws slow.
    pub fn with(m: &Model, own: &Graphics) -> Look {
        let g = own.resolved(m);
        let mut look = Look {
            style: g.style,
            detail: g.detail,
            layers: g.layers.clone(),
            pdf_layers: g.pdf_layers.clone(),
            imports: g.imports.clone(),
            pdf_imports: g.pdf_imports.clone(),
            imports_hidden: g.hides("cadimport"),
            ..Look::default()
        };
        // Nothing to look for: no graphics of its own, and nothing in the project phased or in an option.
        let linked = m.elements.keys().next_back().is_some_and(|id| crate::link::is_linked(*id));
        if own.draws_as_is() && !linked && !crate::phasing::any(m) && m.rare("mass").is_empty() {
            return look;
        }
        // The phase it shows, as where it stands in the project's order, and how its phase filter shows each status.
        let at = g.phase.as_deref().and_then(|n| crate::phasing::index(m, n)).unwrap_or(m.phases.len().saturating_sub(1));
        let filter = g.phase_filter.as_deref().and_then(|n| m.phase_filters.get(n));
        // Each applied filter, and the elements it picks out.
        let applied: Vec<(&Applied, BTreeSet<Id>)> =
            g.filters.iter().filter_map(|a| Some((a, m.view_filters.get(&a.filter)?.matching(m)))).collect();
        for (&id, e) in &m.elements {
            let cat = category_of(m, e);
            if !drawn(cat) {
                continue;
            }
            // What isn't in its phase isn't there; what its phase filter doesn't display is hidden; what it
            // overrides is drawn as the project's override says, under whatever else the view says.
            let mut base = Override::default();
            if let Some(status) = crate::phasing::status(m, e, at) {
                match filter.map_or(crate::phasing::Presentation::ByCategory, |f| f.of(status)) {
                    crate::phasing::Presentation::NotDisplayed => {
                        look.hidden.insert(id);
                        continue;
                    }
                    crate::phasing::Presentation::Overridden => base = m.phase_overrides.get(&status).cloned().unwrap_or_default(),
                    crate::phasing::Presentation::ByCategory => {}
                }
            } else if e.params().is_some() {
                look.hidden.insert(id);
                continue;
            }
            // What a link builds is a reference: halftone, under whatever the view says.
            if crate::link::is_linked(id) {
                base.halftone = true;
            }
            // A mass is seen through in 3D unless the view says otherwise.
            if matches!(e, Element::Mass(_)) && base.transparency == 0 {
                base.transparency = MASS_TRANSPARENCY;
            }
            // In a design option the view doesn't show, it isn't shown either.
            if !crate::phasing::option_shown(m, &g, e) {
                look.hidden.insert(id);
                continue;
            }
            // The category's, then each filter's from the last to the first, then the element's own.
            // A component takes what `component` says of all of them, under what its own category does.
            let all = if COMPONENTS.contains(&cat) {
                g.categories.get(COMPONENT).map_or_else(|| base.clone(), |c| c.over(&base))
            } else {
                base.clone()
            };
            let mut over = g.categories.get(cat).map_or(all.clone(), |c| c.over(&all));
            for (a, picked) in applied.iter().rev().filter(|(_, picked)| picked.contains(&id)) {
                let _ = picked;
                over = a.over.over(&over);
            }
            if let Some(own) = m.elements.guid(id).and_then(|k| g.elements.get(&k)) {
                over = own.over(&over);
            }
            // A subcategory's elements go with their category, hidden.
            if over.hidden || sub(cat).is_some_and(|s| g.hides(s.parent)) {
                look.hidden.insert(id);
            } else if !over.is_empty() {
                look.paint.insert(id, over);
            }
        }
        look
    }

    /// How the view draws surfaces in 3D.
    pub fn style(&self) -> VisualStyle {
        self.style
    }

    /// The layers of the CAD drawing `cad` the view hides.
    pub fn hidden_layers(&self, cad: &str) -> Option<&BTreeSet<String>> {
        self.layers.get(cad)
    }

    /// The layers of the lines of the PDF page `image` the view hides.
    pub fn hidden_pdf_layers(&self, image: &str) -> Option<&BTreeSet<String>> {
        self.pdf_layers.get(image)
    }

    /// How the view draws the CAD drawing `cad`, when it says.
    pub fn import_look(&self, cad: &str) -> Option<&ImportLook> {
        self.imports.get(cad)
    }

    /// How the view draws the lines of the PDF page `image`, when it says.
    pub fn pdf_import_look(&self, image: &str) -> Option<&ImportLook> {
        self.pdf_imports.get(image)
    }

    /// Whether the view draws the lines of the PDF page `image`: neither when its own row is cleared, nor when the view shows
    /// no imports.
    pub fn draws_pdf_lines(&self, image: &str) -> bool {
        !self.imports_hidden && !self.pdf_imports.get(image).is_some_and(|l| l.all.hidden)
    }

    /// Whether the view draws element `id`.
    pub fn shows(&self, id: Id) -> bool {
        !self.hidden.contains(&id)
    }

    /// The colour of element `id`'s fill where the view cuts it, which is `default` unless it says
    /// otherwise: what the drawing code fills the union of cut walls with.
    pub fn cut_fill(&self, id: Id, default: Rgb) -> Rgb {
        let Some(o) = self.paint.get(&id) else { return default };
        let base = if self.pattern_style(id, true).is_some() { crate::prim::PAPER } else { default };
        let c = o.cut.fill.map_or(base, |[r, g, b]| Rgb(r, g, b));
        if o.halftone { halftone(c) } else { c }
    }

    /// Whether element `id` is drawn halftone.
    pub fn half(&self, id: Id) -> bool {
        self.paint.get(&id).is_some_and(|o| o.halftone)
    }

    /// The colour a 3D view shades element `id` in, `base` unless it says otherwise.
    pub fn surface(&self, id: Id, base: Rgb) -> Rgb {
        let Some(o) = self.paint.get(&id) else { return base };
        let c = o.projection.fill.map_or(base, |[r, g, b]| Rgb(r, g, b));
        if o.halftone { halftone(c) } else { c }
    }

    /// Whether the view fills element `id`'s surfaces with a colour of its own (its projection's fill), whatever theirs are.
    pub fn filled(&self, id: Id) -> bool {
        self.paint.get(&id).is_some_and(|o| o.projection.fill.is_some())
    }

    /// The colour of the edges of element `id`'s surfaces in a 3D view, and of what a section sees
    /// beyond its line: `base` unless a projection's line colour says otherwise, lighter in halftone.
    pub fn edge(&self, id: Id, base: Rgb) -> Rgb {
        let Some(o) = self.paint.get(&id) else { return base };
        let c = o.projection.color.map_or(base, |[r, g, b]| Rgb(r, g, b));
        if o.halftone { halftone(c) } else { c }
    }

    /// How much of element `id`'s surface a 3D view sees through, percent.
    pub fn transparency(&self, id: Id) -> u8 {
        self.paint.get(&id).map_or(0, |o| o.transparency)
    }

    /// Whether it changes anything.
    pub fn is_plain(&self) -> bool {
        self.hidden.is_empty() && self.paint.is_empty()
    }

    /// `d` with its primitives drawn as the view says: the colours, weights and dashes of each
    /// element's lines in projection and cut, its fills, lighter in halftone.
    pub fn dress(&self, d: &mut Drawing) {
        if self.paint.is_empty() {
            return;
        }
        let mut halves = Vec::new();
        for (i, (((prim, owner), part), pen)) in d.prims.iter_mut().zip(&d.owners).zip(&d.parts).zip(d.pens.iter_mut()).enumerate() {
            let Some(over) = owner.and_then(|id| self.paint.get(&id)) else { continue };
            if over.halftone && !matches!(prim, Prim::Image { .. } if *part != Part::Cells) {
                halves.push(i);
            }
            let style = if *part == Part::Cut { &over.cut } else { &over.projection };
            match prim {
                Prim::Line { color, dash, .. } => {
                    if *part != Part::Pattern
                        && let Some([r, g, b]) = style.color
                    {
                        *color = Rgb(r, g, b);
                    }
                    if !matches!(part, Part::Text | Part::Pattern) {
                        if let Some(p) = style.weight {
                            *pen = Some(p);
                        }
                        if let Some(k) = style.dash {
                            *dash = k.pattern();
                        }
                    }
                    if over.halftone {
                        *color = halftone(*color);
                    }
                }
                Prim::Fill { color, .. } => {
                    if *part == Part::LinePattern {
                        if let Some([r, g, b]) = style.color {
                            *color = Rgb(r, g, b);
                        }
                        if pen.is_some()
                            && let Some(p) = style.weight
                        {
                            *pen = Some(p);
                        }
                    } else if *part != Part::Pattern
                        && let Some([r, g, b]) = style.fill
                    {
                        *color = Rgb(r, g, b);
                    }
                    if over.halftone {
                        *color = halftone(*color);
                    }
                }
                // A dense drawing's cells, as its lines would be.
                Prim::Image { rgba, .. } if *part == Part::Cells => {
                    for px in rgba.as_chunks_mut::<4>().0 {
                        if let Some([r, g, b]) = style.color {
                            px[..3].copy_from_slice(&[r, g, b]);
                        }
                        if over.halftone {
                            let Rgb(r, g, b) = halftone(Rgb(px[0], px[1], px[2]));
                            px[..3].copy_from_slice(&[r, g, b]);
                        }
                    }
                }
                Prim::Image { .. } => {}
            }
        }
        for i in halves {
            d.set_half(i);
        }
    }
}

/// Longest name of a filter or a template.
pub const MAX_NAME: usize = MAX_TEXT;

#[cfg(test)]
#[path = "graphics_category_tests.rs"]
mod category_tests;
