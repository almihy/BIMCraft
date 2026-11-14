//! Object Styles, as Revit's Manage > Object Styles: the pen each model category draws its lines in, where a view cuts it
//! and where it sees it beyond the cut, in every view of the project. A view's Visibility/Graphics overrides them.
//!
//! A drawing marks the lines that are its category's with [`BY_CATEGORY`], and [`Model::object_style`] gives the pen once
//! the drawing knows who owns each line; the rest of an element's lines (a wall's layers, a door's jambs, a stair's walk
//! line) keep their own.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graphics::{COMPONENT, COMPONENTS, MODEL};
use crate::model::{Element, Model};
use crate::pen::{MAX_PEN, Pen};

/// The pen a drawing draws a line in that is its category's, cut or in projection as the line's part is: not a pen a file
/// or a command can give (past [`MAX_PEN`]), only a mark for [`Model::object_style`] to replace.
#[doc(hidden)]
pub const BY_CATEGORY: Pen = Pen(u8::MAX);

/// A category's pens: its lines in projection, and where a view cuts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectStyle {
    pub projection: Pen,
    pub cut: Pen,
}

/// What a project's categories start with, ours: what's cut, [`Pen::CUT`], and what's seen, [`Pen::PROJECTION`]; a door's
/// and a window's cut, which is their leaf and frame, [`Pen::MEDIUM`], as is a railing seen; a mass, outlined where it's cut
/// as where it's seen, [`Pen::PROJECTION`] both.
pub fn default_of(category: &str) -> ObjectStyle {
    match category {
        "mass" => ObjectStyle { projection: Pen::PROJECTION, cut: Pen::PROJECTION },
        "door" | "window" => ObjectStyle { projection: Pen::PROJECTION, cut: Pen::MEDIUM },
        "railing" => ObjectStyle { projection: Pen::MEDIUM, cut: Pen::CUT },
        _ => ObjectStyle { projection: Pen::PROJECTION, cut: Pen::CUT },
    }
}

impl ObjectStyle {
    /// An error saying what to fix, unless both are pens.
    pub fn check(&self, category: &str) -> Result<(), String> {
        for (what, p) in [("projection", self.projection), ("cut", self.cut)] {
            if !p.is_valid() {
                return Err(format!("object style `{category}` {what}: a line weight is a pen, 0 (a hairline) to {MAX_PEN}, not {}", p.0));
            }
        }
        Ok(())
    }
}

/// An error saying what to fix, unless `category` is one Object Styles has: a model category views draw, or `component`, which
/// stands for every family category that has none of its own.
pub fn known(category: &str) -> Result<(), String> {
    if MODEL.contains(&category) || category == COMPONENT {
        Ok(())
    } else {
        Err(format!(
            "`{category}` isn't a model category; Object Styles has: {} (and `{COMPONENT}`, for every family category)",
            MODEL.join(", ")
        ))
    }
}

/// What `category` draws in when `styles` hold nothing of its own: for a family category, what `component` says of them all
/// (a project from before the rows were split holds only that), else what a category starts with.
pub fn inherited(styles: &BTreeMap<String, ObjectStyle>, category: &str) -> ObjectStyle {
    match styles.get(COMPONENT) {
        Some(all) if COMPONENTS.contains(&category) => *all,
        _ => default_of(category),
    }
}

/// What a family's subcategory draws in in this project, Revit's Object Style `Category/Subcategory`: the pens its lines are seen
/// and cut in, their pattern (by name) and their colour; what it says nothing of is its category's. A family that comes into
/// the project brings its subcategories' (as it names them), each unless the project has one of that name: Revit keeps the
/// project's Object Styles when a family is loaded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubcategoryStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<Pen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<Pen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// The material its geometry is made of, by name (Revit's Object Styles Material column).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

impl SubcategoryStyle {
    /// An error saying what to fix, unless what it holds are pens and a short name.
    pub fn check(&self, key: &str) -> Result<(), String> {
        for (what, p) in [("projection", self.projection), ("cut", self.cut)] {
            if let Some(p) = p
                && !(1..=MAX_PEN).contains(&p.0)
            {
                return Err(format!("object style `{key}` {what}: a line weight is a pen, 1 to {MAX_PEN}, not {}", p.0));
            }
        }
        if self.pattern.as_ref().is_some_and(|p| p.is_empty() || p.chars().count() > crate::model::MAX_TEXT) {
            return Err(format!("object style `{key}`: its line pattern is a name of 1 to {} characters", crate::model::MAX_TEXT));
        }
        if self.material.as_ref().is_some_and(|m| m.is_empty() || m.chars().count() > crate::model::MAX_TEXT) {
            return Err(format!("object style `{key}`: its material is a name of 1 to {} characters", crate::model::MAX_TEXT));
        }
        Ok(())
    }

    /// Whether it says nothing: no pen, pattern, colour or material.
    pub fn is_empty(&self) -> bool {
        *self == SubcategoryStyle::default()
    }
}

impl Model {
    /// What subcategory `sub` of `category` draws in: the project's Object Style of `Category/Subcategory`; each of its pens it
    /// says nothing of is none (the category's, by [`BY_CATEGORY`]). None where the project has none.
    pub fn subcategory_style(&self, category: &str, sub: &str) -> Option<&SubcategoryStyle> {
        self.subcategory_styles.get(&format!("{category}/{sub}"))
    }

    /// Each subcategory `f` names added to the project's Object Styles as `Category/Subcategory`, with its line patterns added
    /// to the project's where it has none of the name; what the project has of either already stays as it is.
    pub fn take_subcategory_styles(&mut self, f: &crate::family::Family) {
        let category = Self::object_style_category_of(f);
        for s in &f.subcategories {
            let style = SubcategoryStyle {
                projection: s.projection,
                cut: s.cut,
                pattern: s.pattern.clone(),
                color: s.color,
                material: s.material.clone(),
            };
            if !style.is_empty() {
                self.subcategory_styles.entry(format!("{category}/{}", s.name)).or_insert(style);
            }
        }
        for (name, pattern) in &f.line_patterns {
            if !name.eq_ignore_ascii_case(crate::lines::SOLID) {
                self.line_patterns.entry(name.clone()).or_insert_with(|| pattern.clone());
            }
        }
    }

    /// The Object Styles category family `f`'s subcategories are under: a door's or a window's, else its family category's, else
    /// `component`.
    pub fn object_style_category_of(f: &crate::family::Family) -> &'static str {
        use crate::family::Category as C;
        match f.category {
            C::Door => "door",
            C::Window => "window",
            other => crate::graphics::component_category(other).unwrap_or(crate::graphics::COMPONENT),
        }
    }

    /// Every subcategory Object Styles lists, as (`Category/Subcategory`, category, subcategory), in that order: those of the
    /// project's families and those the project has a style of (a family since deleted).
    pub fn subcategory_rows(&self) -> Vec<(String, String, String)> {
        let mut rows: BTreeMap<String, (String, String)> = BTreeMap::new();
        for f in self.families.values() {
            let category = Self::object_style_category_of(f);
            for s in &f.subcategories {
                rows.entry(format!("{category}/{}", s.name)).or_insert_with(|| (category.to_string(), s.name.clone()));
            }
        }
        for key in self.subcategory_styles.keys() {
            if let Some((category, name)) = key.split_once('/') {
                rows.entry(key.clone()).or_insert_with(|| (category.to_string(), name.to_string()));
            }
        }
        let mut rows: Vec<(String, String, String)> = rows.into_iter().map(|(k, (c, n))| (k, c, n)).collect();
        rows.sort_by(|a, b| (&a.1, &a.2).cmp(&(&b.1, &b.2)));
        rows
    }

    /// The pens `category` draws in: the project's own, or what it starts with (a family category: what `component` says).
    pub fn object_style(&self, category: &str) -> ObjectStyle {
        self.object_styles.get(category).copied().unwrap_or_else(|| inherited(&self.object_styles, category))
    }

    /// The pens element `e` draws in: those of its category, a component's those of its family's.
    pub fn object_style_of(&self, e: &Element) -> ObjectStyle {
        self.object_style(crate::graphics::category_of(self, e))
    }
}
