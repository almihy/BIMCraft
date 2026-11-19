//! Curtain walls in the window (the model's are `bimcraft_core::curtain`), as Revit shows them: the family a type is of
//! ("Basic Wall", "Curtain Wall", "System Panel", "Rectangular Mullion": [`family`], which the status bar, the Type Selector,
//! Type Properties and the Project Browser all name a type by), the types the Type Selector lists under each family
//! ([`listed`]), and what Type Properties shows of a curtain wall type, a curtain panel type and a mullion type ([`Form`]).
//! A curtain wall's own rows in the Properties palette are `properties::rows("curtainwall")`.

use bimcraft_core::curtain::{EMPTY, GLAZED, JoinCondition, Layout, SOLID, STARTER_MULLIONS, STARTER_WALLS};
use bimcraft_core::model::{Model, WallFunction};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{TextEdit, WidgetInfo, WidgetType};
use serde_json::{Map, Value, json};

use crate::lang::{t, tf};
use crate::properties::combo;
use crate::typeprops::Table;

pub(crate) mod tools;

pub(crate) const BASIC_WALL: &str = msg!("Basic Wall");
pub(crate) const CURTAIN_WALL: &str = msg!("Curtain Wall");
pub(crate) const SYSTEM_PANEL: &str = msg!("System Panel");
pub(crate) const EMPTY_PANEL: &str = msg!("Empty System Panel");
pub(crate) const RECTANGULAR: &str = msg!("Rectangular Mullion");
pub(crate) const CIRCULAR: &str = msg!("Circular Mullion");

/// The family type `name` of `category` is of, as Revit names the families of its system types, in English (`lang::t`
/// translates it); none for a category whose types have no family. A starter type the project hasn't made yet (Storefront
/// before the first wall of it, 25mm Radius before a type names it) is of the family it will be of.
pub(crate) fn family(m: &Model, category: &str, name: &str) -> Option<&'static str> {
    Some(match category {
        "wall" => match m.types.wall.get(name) {
            Some(t) if t.is_curtain() => CURTAIN_WALL,
            None if STARTER_WALLS.contains(&name) => CURTAIN_WALL,
            _ => BASIC_WALL,
        },
        "curtainpanel" => match m.types.curtain_panel.get(name) {
            Some(t) if t.empty => EMPTY_PANEL,
            None if name == EMPTY => EMPTY_PANEL,
            _ => SYSTEM_PANEL,
        },
        "mullion" => {
            let starter = || STARTER_MULLIONS.iter().any(|(n, round, ..)| *n == name && *round);
            if m.types.mullion.get(name).map_or_else(starter, |t| t.round) { CIRCULAR } else { RECTANGULAR }
        }
        "floor" => msg!("Floor"),
        "roof" => msg!("Basic Roof"),
        "ceiling" => msg!("Compound Ceiling"),
        _ => return None,
    })
}

/// A type as Revit names it in a list of several families' types: "Rectangular Mullion : 50 x 150mm".
pub(crate) fn named(m: &Model, category: &str, name: &str) -> String {
    match family(m, category, name) {
        Some(f) => format!("{} : {name}", t(f)),
        None => name.to_string(),
    }
}

/// The families of `category`'s types, in the order the Type Selector lists them; none for a category of one family.
fn families(category: &str) -> &'static [&'static str] {
    match category {
        "wall" => &[BASIC_WALL, CURTAIN_WALL],
        "curtainpanel" => &[SYSTEM_PANEL, EMPTY_PANEL],
        "mullion" => &[RECTANGULAR, CIRCULAR],
        _ => &[],
    }
}

/// The starter types of `category` a project has from the first use of one (`curtain::ensure_starter` and its like).
fn starters(category: &str) -> Vec<&'static str> {
    match category {
        "wall" => STARTER_WALLS.to_vec(),
        "curtainpanel" => vec![GLAZED, SOLID, EMPTY],
        "mullion" => STARTER_MULLIONS.iter().map(|(n, ..)| *n).collect(),
        _ => Vec::new(),
    }
}

/// The types of `category` to pick from: the project's, then, with `unmade`, the starter types it hasn't made yet (what
/// names one makes it: the `wall` command a starter curtain wall type, the `type` command a starter panel or mullion type).
pub(crate) fn names(m: &Model, category: &str, unmade: bool) -> Vec<String> {
    let mut names: Vec<String> = m.type_names(category).into_iter().map(String::from).collect();
    if unmade {
        for s in starters(category) {
            if !names.iter().any(|n| n == s) {
                names.push(s.to_string());
            }
        }
    }
    names
}

/// `names` of `category` as the Type Selector lists them, as Revit's: under the family each is of, a heading for each
/// family that has any ("Basic Wall", then "Curtain Wall"). One list with no heading while they are all of one family.
pub(crate) fn listed(m: &Model, category: &str, names: &[String]) -> Vec<(Option<&'static str>, Vec<String>)> {
    // By name within a family: the starter types the project hasn't made yet among its own.
    let of = |f: &str| {
        let mut of: Vec<String> = names.iter().filter(|n| family(m, category, n) == Some(f)).cloned().collect();
        of.sort();
        of
    };
    let groups: Vec<(Option<&'static str>, Vec<String>)> =
        families(category).iter().map(|f| (Some(*f), of(f))).filter(|(_, names)| !names.is_empty()).collect();
    if groups.len() < 2 { vec![(None, names.to_vec())] } else { groups }
}

/// A curtain type's grid one way, as typed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GridForm {
    layout: Layout,
    spacing: String,
    adjust: bool,
}

/// What Type Properties shows of a type that is a curtain wall's, as typed: Revit's Curtain Wall, System Panel (and Empty
/// System Panel) and Rectangular (and Circular) Mullion.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Form {
    Wall {
        function: WallFunction,
        /// The curtain panel type its cells are filled with, "" for none named.
        panel: String,
        join: JoinCondition,
        /// The vertical grid, then the horizontal.
        grids: [GridForm; 2],
        /// The vertical mullions' Interior, Border 1 and Border 2 types, then the horizontal ones'; "" for none.
        mullions: [[String; 3]; 2],
    },
    /// `empty`, it is Revit's Empty System Panel, which has no parameters.
    Panel { empty: bool, offset: String, thickness: String, material: String },
    /// `round`, a Circular Mullion: `thickness` is then its Radius, and it has no widths.
    Mullion { round: bool, offset: String, thickness: String, side1: String, side2: String, material: String },
}

/// The two ways of a grid: the key of each in the `type` command's `curtain`, with its group's and its mullions' names.
const WAYS: [(&str, &str, &str, &str); 2] = [
    ("vertical", msg!("Vertical Grid"), "vertical_mullions", msg!("Vertical Mullions")),
    ("horizontal", msg!("Horizontal Grid"), "horizontal_mullions", msg!("Horizontal Mullions")),
];

/// A curtain type's mullions one way: the key of each in the `type` command, with its label.
const BORDERS: [(&str, &str); 3] =
    [("interior", msg!("Interior Type")), ("border1", msg!("Border 1 Type")), ("border2", msg!("Border 2 Type"))];

/// No type picked, as Revit's lists say it.
const NONE: &str = msg!("None");
/// No material picked: the category's, as Revit's material fields say it.
const BY_CATEGORY: &str = msg!("<By Category>");

impl Form {
    /// The form of type `name` of `category`, where it is one of a curtain wall's: a curtain wall type, a curtain panel
    /// type, a mullion type. None for any other type, a basic wall's too.
    pub(crate) fn of(m: &Model, category: &str, name: &str) -> Option<Form> {
        let length = |x: f64| m.units.length(x);
        match category {
            "wall" => {
                let t = m.types.wall.get(name)?;
                let c = t.curtain.as_ref()?;
                let grid = |g: &bimcraft_core::curtain::GridRule| GridForm {
                    layout: g.layout,
                    spacing: length(g.spacing),
                    adjust: g.adjust_for_mullion,
                };
                let set = |ms: &bimcraft_core::curtain::Mullions| [ms.interior.clone(), ms.border1.clone(), ms.border2.clone()];
                Some(Form::Wall {
                    function: t.function,
                    panel: c.panel.clone(),
                    join: c.join_condition,
                    grids: [grid(&c.vertical), grid(&c.horizontal)],
                    mullions: [set(&c.vertical_mullions), set(&c.horizontal_mullions)],
                })
            }
            "curtainpanel" => {
                let t = m.types.curtain_panel.get(name)?;
                Some(Form::Panel { empty: t.empty, offset: length(t.offset), thickness: length(t.thickness), material: t.material.clone() })
            }
            "mullion" => {
                let t = m.types.mullion.get(name)?;
                Some(Form::Mullion {
                    round: t.round,
                    offset: length(t.offset),
                    thickness: length(if t.round { t.thickness / 2.0 } else { t.thickness }),
                    side1: length(t.side1),
                    side2: length(t.side2),
                    material: t.material.clone(),
                })
            }
            _ => None,
        }
    }

    /// Its rows in Type Properties' table, group by group as Revit's dialog has them; the Identity Data group comes after.
    pub(crate) fn rows(&mut self, tb: &mut Table, m: &Model) {
        let unit = m.units.name();
        let materials: Vec<String> = m.materials.keys().cloned().collect();
        match self {
            Form::Wall { function, panel, join, grids, mullions } => {
                tb.group("Construction");
                let functions: Vec<String> = WallFunction::ALL.iter().map(|f| t(f.label()).to_string()).collect();
                let mut picked = t(function.label()).to_string();
                tb.pick(t("Function"), &mut picked, &functions);
                if let Some(f) = WallFunction::ALL.into_iter().find(|f| t(f.label()) == picked) {
                    *function = f;
                }
                type_row(tb, "Construction", msg!("Curtain Panel"), m, "curtainpanel", panel);
                let joins: Vec<String> = JoinCondition::ALL.iter().map(|j| t(j.label()).to_string()).collect();
                let mut picked = t(join.label()).to_string();
                tb.pick(t(msg!("Join Condition")), &mut picked, &joins);
                if let Some(j) = JoinCondition::ALL.into_iter().find(|j| t(j.label()) == picked) {
                    *join = j;
                }
                for ((_, group, ..), grid) in WAYS.iter().zip(grids.iter_mut()) {
                    tb.group(group);
                    grid_rows(tb, group, grid, unit);
                }
                for ((.., group), set) in WAYS.iter().zip(mullions.iter_mut()) {
                    tb.group(group);
                    for ((_, label), name) in BORDERS.iter().zip(set.iter_mut()) {
                        type_row(tb, group, label, m, "mullion", name);
                    }
                }
            }
            // Revit's Empty System Panel leaves its cell open: nothing to set but what it is called.
            Form::Panel { empty: true, .. } => {}
            Form::Panel { offset, thickness, material, .. } => {
                tb.group("Constraints");
                tb.text(t("Offset"), offset, unit);
                tb.group("Materials and Finishes");
                material_row(tb, material, &materials);
                tb.group("Dimensions");
                tb.text(t("Thickness"), thickness, unit);
            }
            Form::Mullion { round, offset, thickness, side1, side2, material } => {
                tb.group("Constraints");
                tb.text(t("Offset"), offset, unit);
                if !*round {
                    tb.group("Construction");
                    tb.text(t("Thickness"), thickness, unit);
                }
                tb.group("Materials and Finishes");
                material_row(tb, material, &materials);
                tb.group("Dimensions");
                if *round {
                    tb.text(t(msg!("Radius")), thickness, unit);
                } else {
                    tb.text(t(msg!("Width on side 2")), side2, unit);
                    tb.text(t(msg!("Width on side 1")), side1, unit);
                }
            }
        }
    }

    /// What the `type` command takes for what changed since `was` (with `all`, all of it, as a copy has it), or why
    /// something typed is no length.
    pub(crate) fn params(&self, was: &Form, units: Units, all: bool) -> Result<Map<String, Value>, String> {
        let length = |label: &str, typed: &str| {
            units.parse(typed).map(|x| json!(x)).ok_or_else(|| tf("{}: `{}` is not a length", &[t(label), typed]))
        };
        let mut p = Map::new();
        match (self, was) {
            (
                Form::Wall { function, panel, join, grids, mullions },
                Form::Wall { function: function_was, panel: panel_was, join: join_was, grids: grids_was, mullions: mullions_was },
            ) => {
                if all || function != function_was {
                    p.insert("function".into(), json!(function.name()));
                }
                let mut c = Map::new();
                if all || panel != panel_was {
                    c.insert("panel".into(), json!(panel));
                }
                if all || join != join_was {
                    c.insert("join_condition".into(), json!(join.name()));
                }
                for (i, (key, group, set_key, _)) in WAYS.iter().enumerate() {
                    let (g, g_was) = (&grids[i], &grids_was[i]);
                    let mut rule = Map::new();
                    if all || g.layout != g_was.layout {
                        rule.insert("layout".into(), json!(g.layout.name()));
                    }
                    if all || g.spacing != g_was.spacing {
                        let label = format!("{}, {}", t(group), t("Spacing"));
                        rule.insert("spacing".into(), length(&label, &g.spacing)?);
                    }
                    if all || g.adjust != g_was.adjust {
                        rule.insert("adjust_for_mullion".into(), json!(g.adjust));
                    }
                    if !rule.is_empty() {
                        c.insert((*key).into(), Value::Object(rule));
                    }
                    let mut set = Map::new();
                    for (j, (border, _)) in BORDERS.iter().enumerate() {
                        if all || mullions[i][j] != mullions_was[i][j] {
                            set.insert((*border).into(), json!(mullions[i][j]));
                        }
                    }
                    if !set.is_empty() {
                        c.insert((*set_key).into(), Value::Object(set));
                    }
                }
                if !c.is_empty() {
                    p.insert("curtain".into(), Value::Object(c));
                }
            }
            (Form::Panel { empty, offset, thickness, material }, Form::Panel { offset: o, thickness: th, material: mat, .. }) => {
                if all {
                    p.insert("empty".into(), json!(empty));
                }
                if !*empty {
                    if all || offset != o {
                        p.insert("offset".into(), length("Offset", offset)?);
                    }
                    if all || thickness != th {
                        p.insert("thickness".into(), length("Thickness", thickness)?);
                    }
                }
                if all || material != mat {
                    p.insert("material".into(), json!(material));
                }
            }
            (
                Form::Mullion { round, offset, thickness, side1, side2, material },
                Form::Mullion { offset: o, thickness: th, side1: s1, side2: s2, material: mat, .. },
            ) => {
                if all {
                    p.insert("round".into(), json!(round));
                }
                if all || offset != o {
                    p.insert("offset".into(), length("Offset", offset)?);
                }
                if all || thickness != th {
                    let (key, label) = if *round { ("radius", "Radius") } else { ("thickness", "Thickness") };
                    p.insert(key.into(), length(label, thickness)?);
                }
                if !*round {
                    if all || side1 != s1 {
                        p.insert("side1".into(), length("Width on side 1", side1)?);
                    }
                    if all || side2 != s2 {
                        p.insert("side2".into(), length("Width on side 2", side2)?);
                    }
                }
                if all || material != mat {
                    p.insert("material".into(), json!(material));
                }
            }
            _ => {}
        }
        Ok(p)
    }
}

/// A grid's rows under its group: Layout, Spacing (typed only while the layout spaces its lines by it: Fixed Distance,
/// Maximum Spacing and Minimum Spacing; greyed for None and Fixed Number, as Revit's) and Adjust for Mullion Size.
fn grid_rows(tb: &mut Table, group: &str, grid: &mut GridForm, unit: &str) {
    let layouts: Vec<String> = Layout::ALL.iter().map(|l| t(l.label()).to_string()).collect();
    let label = t(msg!("Layout"));
    tb.row(label, |ui, l| {
        ui.push_id((group, "layout"), |ui| {
            if let Some(picked) = combo(ui, (0, group), l, t(grid.layout.label()), &layouts)
                && let Some(layout) = Layout::ALL.into_iter().find(|x| t(x.label()) == picked)
            {
                grid.layout = layout;
            }
        });
    });
    let spaced = !matches!(grid.layout, Layout::None | Layout::FixedNumber);
    tb.row(t(msg!("Spacing")), |ui, l| {
        ui.push_id((group, "spacing"), |ui| {
            let field = TextEdit::singleline(&mut grid.spacing).hint_text(unit).desired_width(ui.available_width());
            ui.add_enabled(spaced, field).labelled_by(l);
        });
    });
    let label = t(msg!("Adjust for Mullion Size"));
    tb.row(label, |ui, l| {
        ui.push_id((group, "adjust"), |ui| {
            let r = ui.checkbox(&mut grid.adjust, "");
            let now = grid.adjust;
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, now, label));
            r.labelled_by(l);
        });
    });
}

/// A row picking one of the project's types of `category` (and the starter ones it hasn't made yet), or None: each named
/// with its family, as Revit's lists are ("System Panel : Glazed"). `name` is the type's, "" for none.
fn type_row(tb: &mut Table, group: &str, label: &str, m: &Model, category: &str, name: &mut String) {
    let types = names(m, category, true);
    let options: Vec<String> = std::iter::once(t(NONE).to_string()).chain(types.iter().map(|n| named(m, category, n))).collect();
    let now = if name.is_empty() { t(NONE).to_string() } else { named(m, category, name) };
    tb.row(t(label), |ui, l| {
        ui.push_id((group, label), |ui| {
            if let Some(picked) = combo(ui, (0, group), l, &now, &options) {
                *name = types.iter().find(|n| named(m, category, n) == picked).cloned().unwrap_or_default();
            }
        });
    });
}

/// The Material row: one of the library's, or the category's (none).
fn material_row(tb: &mut Table, material: &mut String, materials: &[String]) {
    let none = t(BY_CATEGORY).to_string();
    let options: Vec<String> = std::iter::once(none.clone()).chain(materials.iter().cloned()).collect();
    let mut picked = if material.is_empty() { none.clone() } else { material.clone() };
    tb.pick(t("Material"), &mut picked, &options);
    *material = if picked == none { String::new() } else { picked };
}
