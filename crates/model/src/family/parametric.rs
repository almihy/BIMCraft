//! What keeps a family from Revit parametric in BIMCraft: its references (Revit's reference planes), the dimensions between
//! them that place them per type (labelled with a parameter, locked, or EQ), its nested parts (another family of the project
//! placed in it, its parameters from the parent's), its type catalog, and the check of its parametric result against Revit's
//! own per type (`check`, `editable`). [`Family::solve`] places the references, one axis at a time, as Revit's regeneration
//! does: a plane that defines the origin stays at it, a labelled dimension keeps its parameter's value, a locked one its own,
//! an EQ one spaces its chain equally, and a plane nothing places stays where it is.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Expr, Family, MAX_TEXT, Show, Val, nameable, number, one_line};

/// References one family may have, dimensions, nested parts, and how deep nested parts may nest.
pub const MAX_REFERENCES: usize = 256;
pub const MAX_DIMENSIONS: usize = 512;
/// The most references all of a family's dimensions measure between together.
pub const MAX_DIMENSION_REFERENCES: usize = 4 * MAX_REFERENCES;
pub const MAX_NESTED: usize = 256;
pub const MAX_NESTING: usize = 4;
/// Rows and columns a type catalog may hold.
pub const MAX_CATALOG: usize = 1000;
/// How far apart what a dimension says and where its references end up may be, mm, before it is over-constrained.
const SOLVED: f64 = 0.01;

/// The family axis a reference plane is square to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    #[default]
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }
}

/// A reference plane of the family (Revit's `Left`, `Right`, `Front`, `Exterior`…), square to `axis` at `at` along it, in the
/// family's axes: where it stands when nothing else places it. Formulas read where it ends up for a type as `ref(Name)`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    pub name: String,
    #[serde(default)]
    pub axis: Axis,
    #[serde(default)]
    pub at: Expr,
    /// Revit's Is Reference: `strong`, `weak`, `not`, `left`, `center_left_right`…: kept and shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_reference: Option<String>,
    /// Whether it defines the family's origin along its axis: it stays at it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub origin: bool,
}

/// What a dimension measures: Revit's linear ones place references; the rest are kept and shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DimensionKind {
    #[default]
    Linear,
    Angular,
    Radial,
    Diameter,
}

impl DimensionKind {
    fn is_linear(&self) -> bool {
        *self == DimensionKind::Linear
    }
}

/// A dimension between references, as Revit's family dimensions: `label`led with a parameter it keeps the references that far
/// apart, `locked` it keeps its `value`, `eq` it spaces its chain of references equally; otherwise it only says.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    #[serde(default, skip_serializing_if = "DimensionKind::is_linear")]
    pub kind: DimensionKind,
    /// The references it measures between, by name, in order.
    pub refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub eq: bool,
    /// What it measured, mm (degrees for an angular one): a locked one keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Which way along its axis the second reference is from the first: +1 or -1 (none, +1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sense: Option<Vec<i8>>,
}

fn is_zero(e: &Expr) -> bool {
    matches!(e, Expr::Number(x) if *x == 0.0)
}

/// Another family of the project placed in this one, as Revit's nested families: its type, where it stands (`at`, turned
/// `rotation` degrees about z and `mirrored` across its own x), its parameters from the parent's (numbers or formulas over the
/// parent's parameters), and `visible` whether it's there.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nested {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    pub family: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub at: [Expr; 3],
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: Expr,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Expr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    /// The types of the parent it is in; none, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<Show>,
    /// The type of it each of this family's types chooses, where they choose others than `type`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub type_in: BTreeMap<String, String>,
    /// Its own x, y and z axes in this family's (a face-based part on a wall's face), in place of `rotation` and `mirrored`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axes: Option<[[f64; 3]; 3]>,
    /// How much it is scaled about its insertion point (a number or a formula over the parent's parameters; none, as it is):
    /// Revit's planting symbol is its base family drawn at the parent's `Height` over the base's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Expr>,
}

impl Nested {
    /// Whether it is in type `type_name`: one it names, or any when it names none.
    pub fn of_type(&self, type_name: &str) -> bool {
        self.types.is_empty() || self.types.iter().any(|t| t == type_name)
    }

    /// The type of it the parent's type `parent` places.
    pub fn type_for(&self, parent: &str) -> &str {
        self.type_in.get(parent).map_or(self.type_name.as_str(), String::as_str)
    }
}

/// A column of a type catalog: its name, its kind (`length`, `number`, `text`…) and its unit, as Revit's catalog header says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogColumn {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// A type catalog (Revit's `.txt` beside a family): its columns and a row of values for each type it offers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    #[serde(default)]
    pub columns: Vec<CatalogColumn>,
    #[serde(default)]
    pub rows: BTreeMap<String, BTreeMap<String, Value>>,
}

/// Whether a type's parametric result matched Revit's own when the family came in, and why not: the check runs once, at
/// import, for each type as Revit exported it ([`Family::run_check`]). A matched type draws its parametric forms; one that
/// didn't, Revit's (its `snapshot` forms and symbols), until someone changes it. `edited`: changed since it came in (its values,
/// the family's definition, or switched to BIMCraft's rebuild): BIMCraft's rebuild is what it is from then on, drawn whatever
/// the check said, and never compared with Revit's result again. `wall`: a door's or a window's, the thickness of the wall
/// Revit's result was made in, which it was checked in; `opening`: the opening Revit cut in it. A type with no check (one made since, not like another) was never
/// checked.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeCheck {
    pub matched: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall: Option<f64>,
    /// A door's or a window's opening in the wall as Revit made it, mm: its width, its height and where it starts over the
    /// sill (negative: below it). The check compared it with the one BIMCraft cuts; a type drawn as Revit made it cuts it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening: Option<[f64; 3]>,
}

impl TypeCheck {
    /// Whether it draws Revit's result: it didn't match, and nobody has changed it since.
    pub fn as_revit(&self) -> bool {
        !self.matched && !self.edited
    }
}

/// Whether the family can be edited in BIMCraft as in Revit's Family Editor: every type matched and nothing left
/// untranslated; else why not, in plain words, and what the translation couldn't carry over.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Editable {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untranslated: Vec<String>,
}

/// The name a reference's position goes by among a type's values: what `ref(Name)` reads.
pub fn ref_key(name: &str) -> String {
    format!("ref({name})")
}

impl Family {
    /// Whether it carries Revit's own result for some types (`snapshot` forms or symbols): the fallback.
    pub fn has_snapshot(&self) -> bool {
        self.forms.iter().any(|f| f.snapshot) || self.symbols.iter().any(|y| y.snapshot)
    }

    /// Whether type `type_name` draws Revit's own result (its `snapshot` forms and symbols) rather than its parametric ones:
    /// one whose check failed and that nobody has changed since ([`TypeCheck::as_revit`]), or any type of a family that came
    /// with Revit's result and no definition. A type never checked (made since) draws its definition.
    pub fn draws_snapshot(&self, type_name: &str) -> bool {
        if !self.has_snapshot() {
            return false;
        }
        match self.check.get(type_name) {
            Some(c) => c.as_revit(),
            None => {
                !self.has_parametric()
                    && (self.forms.iter().any(|f| f.snapshot && f.of_type(type_name))
                        || self.symbols.iter().any(|y| y.snapshot && y.of_type(type_name)))
            }
        }
    }

    /// Whether a nested part of this family, type `type_name` sized by `values` (what its parent hands it, worked out in a wall
    /// `wall` thick), is drawn as Revit made it ([`Family::draws_snapshot`]) rather than from its definition. Revit always
    /// regenerates a nested family with the values its parent hands it, so its result for the type is right only at the
    /// type's own values: where the parent hands others, the definition draws it (a family with no definition keeps Revit's).
    pub(crate) fn draws_revit_result(&self, type_name: &str, values: &BTreeMap<String, Val>, wall: f64) -> bool {
        if !self.draws_snapshot(type_name) {
            return false;
        }
        if !self.has_parametric() {
            return true;
        }
        let Ok(own) = self.values_in(type_name, &BTreeMap::new(), wall) else { return true };
        values.iter().all(|(k, v)| match (v, own.get(k)) {
            (Val::Number(a), Some(Val::Number(b))) => (a - b).abs() <= 0.01,
            (Val::Number(_), _) => false,
            _ => true,
        })
    }

    /// Whether a nested part's solids are Revit's own forms ([`Family::draws_snapshot_forms`]): always for an RPC family's, and
    /// else where Revit's result is right at the values its parent hands it ([`Family::draws_revit_result`]).
    pub(crate) fn draws_revit_forms(&self, type_name: &str, values: &BTreeMap<String, Val>, wall: f64) -> bool {
        (self.draws_snapshot_forms(type_name) && !self.draws_snapshot(type_name)) || self.draws_revit_result(type_name, values, wall)
    }

    /// Whether type `type_name`'s solids are Revit's own result (its `snapshot` forms): when it draws that result
    /// ([`Family::draws_snapshot`]), and always for an RPC family's, whose 3D is a render placeholder no rules can rebuild.
    pub fn draws_snapshot_forms(&self, type_name: &str) -> bool {
        (self.rpc && self.forms.iter().any(|f| f.snapshot && f.of_type(type_name))) || self.draws_snapshot(type_name)
    }

    /// Where each reference ends up for these `values` (a type's parameters worked out): by name, along its axis, mm. Over
    /// or wrongly constrained, why.
    pub fn solve(&self, values: &BTreeMap<String, Val>) -> Result<BTreeMap<String, f64>, String> {
        let mut out = BTreeMap::new();
        if self.references.is_empty() {
            return Ok(out);
        }
        let mut read = std::collections::HashMap::new();
        let mut free = BTreeMap::new();
        for r in &self.references {
            let at = number(&r.at, values, &mut read).map_err(|e| format!("reference `{}`: {e}", r.name))?;
            free.insert(r.name.as_str(), (r.axis, at));
        }
        for axis in Axis::ALL {
            let names: Vec<&str> = self.references.iter().filter(|r| r.axis == axis).map(|r| r.name.as_str()).collect();
            if names.is_empty() {
                continue;
            }
            let col = |n: &str| names.iter().position(|m| *m == n);
            // Each equation: its coefficients by column, and what they come to.
            let mut rows: Vec<(Vec<f64>, f64, String)> = Vec::new();
            // What stays where it is: the plane at the origin, and a host's face (where the wall puts it).
            let fixed = |r: &Reference| r.origin || r.is_reference.as_deref().is_some_and(|k| k.starts_with("host_"));
            for r in self.references.iter().filter(|r| r.axis == axis && fixed(r)) {
                let mut a = vec![0.0; names.len()];
                if let Some(k) = col(&r.name) {
                    a[k] = 1.0;
                    rows.push((a, free.get(r.name.as_str()).map_or(0.0, |f| f.1), format!("reference `{}` at the origin", r.name)));
                }
            }
            for (i, d) in self.dimensions.iter().enumerate().filter(|(_, d)| d.kind == DimensionKind::Linear) {
                let who = format!("dimension {}", i + 1);
                let ks: Vec<Option<usize>> = d.refs.iter().map(|n| col(n)).collect();
                // A dimension of another axis is that axis' to solve.
                if ks.iter().all(Option::is_none) {
                    continue;
                }
                let ks: Vec<usize> = ks
                    .into_iter()
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("{who} measures between references along different axes"))?;
                if ks.len() < 2 {
                    continue;
                }
                let sense = |j: usize| if d.sense.as_ref().and_then(|s| s.get(j)).is_some_and(|s| *s < 0) { -1.0 } else { 1.0 };
                let gap = |j: usize| {
                    let mut a = vec![0.0; names.len()];
                    a[ks[j + 1]] += sense(j);
                    a[ks[j]] -= sense(j);
                    a
                };
                if d.eq {
                    for j in 0..ks.len().saturating_sub(2) {
                        let a: Vec<f64> = gap(j).iter().zip(gap(j + 1)).map(|(x, y)| x - y).collect();
                        rows.push((a, 0.0, format!("{who} (EQ)")));
                    }
                    continue;
                }
                let target = match (&d.label, d.locked) {
                    (Some(label), _) => match values.get(label) {
                        Some(Val::Number(x)) => *x,
                        _ => return Err(format!("{who}: its label `{label}` isn't a number parameter of the family")),
                    },
                    (None, true) => d.value.ok_or_else(|| format!("{who} is locked but says no value"))?,
                    (None, false) => continue,
                };
                if ks.len() != 2 {
                    return Err(format!("{who}: a labelled or locked dimension measures between two references"));
                }
                rows.push((gap(0), target, who));
            }
            let at: Vec<f64> = names.iter().map(|n| free.get(n).map_or(0.0, |f| f.1)).collect();
            let solved = solve_rows(rows, &at)?;
            for (n, x) in names.iter().zip(solved) {
                out.insert(n.to_string(), x);
            }
        }
        Ok(out)
    }

    /// `values` with each reference's place for them added as `ref(Name)`: solved, else where each stands. Why it isn't
    /// solved, if it isn't.
    pub(crate) fn place_references(&self, values: &mut BTreeMap<String, Val>) -> Result<(), String> {
        if self.references.is_empty() {
            return Ok(());
        }
        match self.solve(values) {
            Ok(placed) => {
                for (n, x) in placed {
                    values.insert(ref_key(&n), Val::Number(x));
                }
                Ok(())
            }
            Err(e) => {
                let mut read = std::collections::HashMap::new();
                for r in &self.references {
                    if let Ok(x) = number(&r.at, values, &mut read) {
                        values.insert(ref_key(&r.name), Val::Number(x));
                    }
                }
                Err(e)
            }
        }
    }

    /// Whether its references, dimensions, nested parts and catalog are sound: names a formula can say, once each; dimensions
    /// between references it has, labelled with number parameters; nested parts of families named, within the limits.
    pub(crate) fn check_parametric(&self) -> Result<(), String> {
        for (what, n, most) in [
            ("references", self.references.len(), MAX_REFERENCES),
            ("dimensions", self.dimensions.len(), MAX_DIMENSIONS),
            ("nested parts", self.nested.len(), MAX_NESTED),
        ] {
            if n > most {
                return Err(format!("it has more than {most} {what}"));
            }
        }
        // Each dimension is a row of the solver for every gap it measures: all of them together are bounded, as the solver's work
        // is, at every draw.
        if self.dimensions.iter().map(|d| d.refs.len()).sum::<usize>() > MAX_DIMENSION_REFERENCES {
            return Err(format!("its dimensions measure between more than {MAX_DIMENSION_REFERENCES} references in all"));
        }
        let mut names = std::collections::BTreeSet::new();
        for r in &self.references {
            if !nameable(&r.name) {
                return Err(format!("reference `{}`: a name starts with a letter and holds letters, digits, `_` and spaces", r.name));
            }
            if !names.insert(r.name.as_str()) {
                return Err(format!("reference `{}` is named twice", r.name));
            }
            if let Some(k) = &r.is_reference
                && !one_line(k, MAX_TEXT)
            {
                return Err(format!("reference `{}`: its `is_reference` is a word", r.name));
            }
        }
        for (i, d) in self.dimensions.iter().enumerate() {
            let who = format!("dimension {}", i + 1);
            if d.refs.len() < 2 && d.kind == DimensionKind::Linear || d.refs.len() > MAX_REFERENCES {
                return Err(format!("{who}: it measures between two references or more"));
            }
            if let Some(r) = d.refs.iter().find(|r| !names.contains(r.as_str())) {
                let shown: String = r.chars().take(40).collect();
                return Err(format!("{who}: `{shown}` isn't one of the family's references"));
            }
            if let Some(label) = &d.label
                && !self
                    .parameters
                    .iter()
                    .any(|p| p.name == *label && matches!(p.kind, super::Kind::Length | super::Kind::Number | super::Kind::Angle))
            {
                let shown: String = label.chars().take(40).collect();
                return Err(format!("{who}: its label `{shown}` isn't a length or number parameter of the family"));
            }
            if d.value.is_some_and(|x| !x.is_finite() || x.abs() > crate::model::MAX_COORD) {
                return Err(format!("{who}: its value isn't a usable number"));
            }
            if d.sense.as_ref().is_some_and(|s| s.len() > d.refs.len()) {
                return Err(format!("{who}: it has more senses than gaps"));
            }
        }
        for (i, n) in self.nested.iter().enumerate() {
            let who = format!("nested part {}", i + 1);
            if !one_line(&n.family, MAX_TEXT) || !one_line(&n.type_name, MAX_TEXT) {
                return Err(format!("{who}: it names a family and a type"));
            }
            if let Some(t) = n.types.iter().find(|t| !self.types.contains_key(*t)) {
                let shown: String = t.chars().take(40).collect();
                return Err(format!("{who}: its `types` name `{shown}`, which isn't one of the family's types"));
            }
            if let Some(a) = &n.axes {
                let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
                    + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
                if a.iter().flatten().any(|x| !x.is_finite() || x.abs() > 1e3) || det.abs() < 1e-6 {
                    return Err(format!("{who}: its `axes` are three directions that aren't flat"));
                }
            }
            if n.type_in.len() > super::MAX_TYPES || n.type_in.iter().any(|(t, v)| !self.types.contains_key(t) || !one_line(v, MAX_TEXT)) {
                return Err(format!("{who}: its `type_in` names a type of the family and the nested family's type for it"));
            }
            if n.params.len() > super::MAX_PARAMETERS {
                return Err(format!("{who}: it sets more than {} parameters", super::MAX_PARAMETERS));
            }
        }
        if let Some(c) = &self.catalog
            && (c.columns.len() > MAX_CATALOG || c.rows.len() > MAX_CATALOG || c.rows.values().any(|r| r.len() > MAX_CATALOG))
        {
            return Err(format!("its type catalog holds more than {MAX_CATALOG} rows or columns"));
        }
        // What a type's check kept of the wall and the opening Revit cut sizes the wall's cut for a type drawn as Revit made it.
        let size = |x: f64| x.is_finite() && (1.0..=crate::model::MAX_COORD).contains(&x);
        for (t, c) in &self.check {
            let shown: String = t.chars().take(40).collect();
            if c.wall.is_some_and(|w| !size(w)) {
                return Err(format!("its check of type `{shown}`: its wall isn't a usable size"));
            }
            if c.opening.is_some_and(|[w, h, b]| !size(w) || !size(h) || !b.is_finite() || b.abs() > crate::model::MAX_COORD) {
                return Err(format!("its check of type `{shown}`: its opening isn't a usable size"));
            }
        }
        Ok(())
    }

    /// Whether the families it holds itself are sound, each as a family of the project is: at most [`MAX_NESTED`] of them,
    /// held at most [`MAX_NESTING`] deep, each named.
    pub(crate) fn check_embedded(&self, has_material: &dyn Fn(&str) -> bool, depth: usize) -> Result<(), String> {
        if self.embedded.is_empty() {
            return Ok(());
        }
        if depth >= MAX_NESTING {
            return Err(format!("it holds families nested more than {MAX_NESTING} deep"));
        }
        if self.embedded.len() > MAX_NESTED {
            return Err(format!("it holds more than {MAX_NESTED} nested families"));
        }
        for (name, inner) in &self.embedded {
            let shown: String = name.chars().take(40).collect();
            if !one_line(name, MAX_TEXT) {
                return Err(format!("a family it holds is named `{shown}`: a name is one line"));
            }
            inner.check_at(has_material, depth + 1).map_err(|e| format!("the nested family `{shown}` it holds: {e}"))?;
        }
        Ok(())
    }

    /// The family a nested part named `name` is of: one it holds itself, else the project's (`others`).
    pub fn nested_family<'r, 's: 'r, 'o: 'r>(&'s self, name: &str, others: super::Others<'o>) -> Option<&'r Family> {
        self.embedded.get(name).or_else(|| others(name))
    }

    /// Type `new`, made like `old` (Duplicate), checked as `old` was: a duplicate of a type that matched, or that was changed
    /// since, is parametric; one of a type drawn as Revit made it is drawn so too, till it is changed.
    pub(crate) fn check_like(&mut self, new: &str, old: &str) {
        if let Some(c) = self.check.get(old).cloned() {
            self.check.insert(new.to_string(), c);
        }
    }
}

/// The places of `at.len()` references meeting every equation `rows` (coefficients by column, what they come to, who says
/// it), a reference no equation places at its `at`: Gauss-Jordan elimination, the free columns given their place. Over
/// constrained (two equations that can't both hold), which.
fn solve_rows(rows: Vec<(Vec<f64>, f64, String)>, at: &[f64]) -> Result<Vec<f64>, String> {
    let n = at.len();
    let mut rows = rows;
    let mut pivots: Vec<(usize, usize)> = Vec::new();
    let mut r = 0;
    for c in 0..n {
        let Some(best) = (r..rows.len()).max_by(|a, b| rows[*a].0[c].abs().total_cmp(&rows[*b].0[c].abs())) else { break };
        if rows[best].0[c].abs() < 1e-9 {
            continue;
        }
        rows.swap(r, best);
        let lead = rows[r].0[c];
        let (a, b) = (rows[r].0.iter().map(|x| x / lead).collect::<Vec<f64>>(), rows[r].1 / lead);
        rows[r].0 = a;
        rows[r].1 = b;
        for k in 0..rows.len() {
            if k == r {
                continue;
            }
            let f = rows[k].0[c];
            if f.abs() < 1e-12 {
                continue;
            }
            let (pivot, rhs) = (rows[r].0.clone(), rows[r].1);
            for (x, p) in rows[k].0.iter_mut().zip(&pivot) {
                *x -= f * p;
            }
            rows[k].1 -= f * rhs;
        }
        pivots.push((r, c));
        r += 1;
        if r == rows.len() {
            break;
        }
    }
    // What's left over is an equation of nothing: it holds, or two dimensions say different things.
    for (a, b, who) in rows.iter().skip(r) {
        if a.iter().all(|x| x.abs() < 1e-9) && b.abs() > SOLVED {
            return Err(format!("it is over-constrained: {who} can't hold with the others (off by {:.1} mm)", b.abs()));
        }
    }
    let pivot_cols: Vec<usize> = pivots.iter().map(|(_, c)| *c).collect();
    let mut x: Vec<f64> = at.to_vec();
    for (row, c) in &pivots {
        let (a, b, _) = &rows[*row];
        let rest: f64 = (0..n).filter(|k| !pivot_cols.contains(k)).map(|k| a[k] * at[k]).sum();
        x[*c] = b - rest;
    }
    if x.iter().any(|v| !v.is_finite() || v.abs() > crate::model::MAX_COORD) {
        return Err("its references end up out of reach: a dimension's value isn't usable".into());
    }
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(a: &[f64], b: f64) -> (Vec<f64>, f64, String) {
        (a.to_vec(), b, "d".into())
    }

    #[test]
    fn rows_place_what_they_say_and_leave_the_rest_where_it_is() {
        // Centre at 0, Right - Left = 900, EQ: Centre - Left = Right - Centre; Top free.
        let rows = vec![row(&[0.0, 1.0, 0.0, 0.0], 0.0), row(&[-1.0, 0.0, 1.0, 0.0], 900.0), row(&[-1.0, 2.0, -1.0, 0.0], 0.0)];
        let x = solve_rows(rows, &[-100.0, 5.0, 100.0, 777.0]).unwrap();
        assert!((x[0] + 450.0).abs() < 1e-9 && x[1].abs() < 1e-9 && (x[2] - 450.0).abs() < 1e-9 && x[3] == 777.0, "{x:?}");
        // Two dimensions that disagree.
        let e = solve_rows(vec![row(&[-1.0, 1.0], 900.0), row(&[-1.0, 1.0], 800.0)], &[0.0, 0.0]).unwrap_err();
        assert!(e.contains("over-constrained"), "{e}");
        // Two that agree are no conflict.
        assert!(solve_rows(vec![row(&[-1.0, 1.0], 900.0), row(&[-2.0, 2.0], 1800.0)], &[0.0, 0.0]).is_ok());
    }
}
