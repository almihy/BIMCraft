//! A type's identity data, as Revit's Identity Data group in Type Properties: Type Mark, Description, Manufacturer, Model,
//! Fire Rating, Cost, Keynote, Assembly Code, URL and Type Comments. Every library type holds one (`identity` on a wall,
//! floor, roof, ceiling, door, window, column, railing or run type; a family holds one for each of its types), all of it
//! optional and written to a file only where set. The elements of a type read it as fields (a schedule's, a tag's) and IFC
//! carries it in its property sets.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Element, MAX_TEXT, Model};

/// The most a type's cost may be: a sane bound, so a total can't overflow.
pub const MAX_COST: f64 = 1e12;

/// The identity data keys, as the `type` command takes them and an element's JSON gives them, each with the name Revit's
/// Type Properties (and so a schedule) shows.
pub const FIELDS: [(&str, &str); 10] = [
    ("type_mark", "Type Mark"),
    ("description", "Description"),
    ("manufacturer", "Manufacturer"),
    ("model", "Model"),
    ("fire_rating", "Fire Rating"),
    ("cost", "Cost"),
    ("keynote", "Keynote"),
    ("assembly_code", "Assembly Code"),
    ("url", "URL"),
    ("type_comments", "Type Comments"),
];

/// A type's identity data. Empty text is none, as is no cost.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeIdentity {
    /// Names the type in a schedule and its tags: `D1`, `W-03`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub type_mark: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub manufacturer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fire_rating: String,
    /// What one of its elements costs, in the project's currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keynote: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub assembly_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub type_comments: String,
}

impl TypeIdentity {
    /// Whether it holds nothing (so a file leaves it out).
    pub fn is_empty(&self) -> bool {
        *self == TypeIdentity::default()
    }

    /// The text field `key` names, if it is one.
    fn text(&self, key: &str) -> Option<&String> {
        Some(match key {
            "type_mark" => &self.type_mark,
            "description" => &self.description,
            "manufacturer" => &self.manufacturer,
            "model" => &self.model,
            "fire_rating" => &self.fire_rating,
            "keynote" => &self.keynote,
            "assembly_code" => &self.assembly_code,
            "url" => &self.url,
            "type_comments" => &self.type_comments,
            _ => return None,
        })
    }

    fn text_mut(&mut self, key: &str) -> Option<&mut String> {
        Some(match key {
            "type_mark" => &mut self.type_mark,
            "description" => &mut self.description,
            "manufacturer" => &mut self.manufacturer,
            "model" => &mut self.model,
            "fire_rating" => &mut self.fire_rating,
            "keynote" => &mut self.keynote,
            "assembly_code" => &mut self.assembly_code,
            "url" => &mut self.url,
            "type_comments" => &mut self.type_comments,
            _ => return None,
        })
    }

    /// What field `key` holds, as JSON: text, or the cost as a number; none where it's unset.
    pub fn get(&self, key: &str) -> Option<Value> {
        if key == "cost" {
            return self.cost.map(|c| serde_json::json!(c));
        }
        self.text(key).filter(|t| !t.is_empty()).map(|t| Value::String(t.clone()))
    }

    /// Each field that is set, as `(key, value)`, in [`FIELDS`]' order.
    pub fn set(&self) -> impl Iterator<Item = (&'static str, Value)> + '_ {
        FIELDS.iter().filter_map(|(k, _)| Some((*k, self.get(k)?)))
    }

    /// Changes what `p` gives under the keys of [`FIELDS`]: text as it's typed, trimmed (empty or `null` clears
    /// it), a cost as a number or as text of one. Keys `p` leaves out stay as they are.
    pub fn read(&mut self, p: &Value) -> Result<(), String> {
        for (key, label) in FIELDS {
            let Some(v) = p.get(key) else { continue };
            if key == "cost" {
                self.cost = match v {
                    Value::Null => None,
                    Value::String(t) if t.trim().is_empty() => None,
                    Value::String(t) => Some(t.trim().parse::<f64>().map_err(|_| format!("`{key}` ({label}) is a number, not `{t}`"))?),
                    v => Some(v.as_f64().ok_or_else(|| format!("`{key}` ({label}) is a number"))?),
                };
                continue;
            }
            let typed = match v {
                Value::Null => String::new(),
                Value::String(t) => t.trim().to_string(),
                _ => return Err(format!("`{key}` ({label}) is text")),
            };
            if let Some(slot) = self.text_mut(key) {
                *slot = typed;
            }
        }
        self.check()
    }

    /// Whether it is sound: text of up to [`MAX_TEXT`] characters (one line, except a type's comments and description), a cost
    /// that is finite and from 0 to [`MAX_COST`].
    pub fn check(&self) -> Result<(), String> {
        for (key, label) in FIELDS {
            let Some(t) = self.text(key) else { continue };
            let lines = matches!(key, "type_comments" | "description");
            if t.chars().count() > MAX_TEXT || t.chars().any(|c| c.is_control() && !(lines && c == '\n')) {
                let how = if lines { "text of up to" } else { "one line of up to" };
                return Err(format!("`{key}` ({label}) is {how} {MAX_TEXT} characters"));
            }
        }
        if let Some(c) = self.cost
            && !(c.is_finite() && (0.0..=MAX_COST).contains(&c))
        {
            return Err(format!("`cost` is a number from 0 to {MAX_COST}"));
        }
        Ok(())
    }
}

/// The schedule field name of identity key `key` (`Type Mark`), if it is one.
pub fn label(key: &str) -> Option<&'static str> {
    FIELDS.iter().find(|(k, _)| *k == key).map(|(_, l)| *l)
}

impl Model {
    /// The identity data of the type element `e` is of: its category's type, or its family's type. None for what has no type
    /// (a stair, a room), or whose type isn't in the project.
    pub fn type_identity(&self, e: &Element) -> Option<&TypeIdentity> {
        let t = &self.types;
        match e {
            Element::Component(c) => self.families.get(&c.family)?.identity.get(&c.type_name),
            Element::Door(o) | Element::Window(o) => match &o.family {
                Some(f) => self.families.get(f)?.identity.get(&o.type_name),
                None if matches!(e, Element::Door(_)) => t.door.get(&o.type_name).map(|x| &x.identity),
                None => t.window.get(&o.type_name).map(|x| &x.identity),
            },
            Element::Wall(w) => t.wall.get(&w.type_name).map(|x| &x.identity),
            Element::Floor(f) | Element::Toposolid(f) => t.floor.get(&f.type_name).map(|x| &x.identity),
            Element::Roof(r) => t.roof.get(&r.type_name).map(|x| &x.identity),
            Element::Ceiling(c) => t.ceiling.get(&c.type_name).map(|x| &x.identity),
            Element::Railing(r) => t.railing.get(&r.type_name).map(|x| &x.identity),
            Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) => self.column_type(e).map(|x| &x.identity),
            _ => self.run_type(e).map(|x| &x.identity),
        }
    }

    /// Whether every type's identity data is sound, and a family's is of types it has.
    pub fn check_identity(&self) -> Result<(), String> {
        let t = &self.types;
        let mut each: Vec<(String, &TypeIdentity)> = Vec::new();
        macro_rules! of {
            ($what:expr, $map:expr) => {
                each.extend($map.iter().map(|(n, x)| (format!("{} type `{n}`", $what), &x.identity)))
            };
        }
        of!("wall", t.wall);
        of!("floor", t.floor);
        of!("roof", t.roof);
        of!("ceiling", t.ceiling);
        of!("railing", t.railing);
        of!("door", t.door);
        of!("window", t.window);
        of!("column", t.column);
        of!("structural column", t.structural_column);
        of!("footing", t.footing);
        for (category, types) in &t.runs {
            of!(category, types);
        }
        for (family, f) in &self.families {
            for (ty, identity) in &f.identity {
                if !f.types.contains_key(ty) {
                    return Err(format!("family `{family}`: identity data of type `{ty}`, which it doesn't have"));
                }
                each.push((format!("family `{family}` type `{ty}`"), identity));
            }
        }
        each.into_iter().try_for_each(|(who, identity)| identity.check().map_err(|e| format!("{who}: {e}")))
    }

    /// Type `name` of `category`'s identity data, for the `type` command and Edit Type: a category's own types, as the
    /// project's `Types` holds them.
    pub fn identity_of(&self, category: &str, name: &str) -> Option<&TypeIdentity> {
        let t = &self.types;
        match category {
            "wall" => t.wall.get(name).map(|x| &x.identity),
            "floor" => t.floor.get(name).map(|x| &x.identity),
            "roof" => t.roof.get(name).map(|x| &x.identity),
            "ceiling" => t.ceiling.get(name).map(|x| &x.identity),
            "railing" => t.railing.get(name).map(|x| &x.identity),
            "door" => t.door.get(name).map(|x| &x.identity),
            "window" => t.window.get(name).map(|x| &x.identity),
            "column" => t.column.get(name).map(|x| &x.identity),
            "structuralcolumn" => t.structural_column.get(name).map(|x| &x.identity),
            "footing" => t.footing.get(name).map(|x| &x.identity),
            run => t.runs.get(run)?.get(name).map(|x| &x.identity),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn it_is_empty_until_set_and_a_file_leaves_out_what_is_not() {
        let mut t = TypeIdentity::default();
        assert!(t.is_empty() && serde_json::to_value(&t).unwrap() == json!({}));
        t.read(&json!({"type_mark": " D1 ", "cost": "120.5", "url": null})).unwrap();
        assert_eq!(serde_json::to_value(&t).unwrap(), json!({"type_mark": "D1", "cost": 120.5}));
        assert_eq!(t.get("type_mark"), Some(json!("D1")));
        assert_eq!(t.get("model"), None);
        t.read(&json!({"type_mark": "", "cost": null})).unwrap();
        assert!(t.is_empty());
    }

    #[test]
    fn what_cannot_be_held_is_refused_with_what_to_fix() {
        let mut t = TypeIdentity::default();
        assert!(t.read(&json!({"cost": -1})).unwrap_err().contains("cost"));
        assert!(t.read(&json!({"cost": "cheap"})).unwrap_err().contains("number"));
        assert!(t.read(&json!({"manufacturer": 4})).unwrap_err().contains("text"));
        assert!(t.read(&json!({"model": "a\nb"})).unwrap_err().contains("one line"));
        assert!(TypeIdentity::default().read(&json!({"type_comments": "a\nb"})).is_ok());
        assert!(t.read(&json!({"keynote": "x".repeat(MAX_TEXT + 1)})).is_err());
    }
}
