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

impl Model {
    /// The pens `category` draws in: the project's own, or what it starts with (a family category: what `component` says).
    pub fn object_style(&self, category: &str) -> ObjectStyle {
        self.object_styles.get(category).copied().unwrap_or_else(|| inherited(&self.object_styles, category))
    }

    /// The pens element `e` draws in: those of its category, a component's those of its family's.
    pub fn object_style_of(&self, e: &Element) -> ObjectStyle {
        self.object_style(crate::graphics::category_of(self, e))
    }
}
