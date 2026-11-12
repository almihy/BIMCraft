//! Visibility/Graphics and Object Styles by the category of a family: a luminaire is Lighting, an outlet Outlets, and so on,
//! where every family component was one `component` row. `component` stays, naming all of them.

use crate::graphics::{COMPONENT, COMPONENTS, MODEL, ViewKind};
use crate::language::Language;
use crate::model::Model;
use serde_json::json;

#[test]
fn model_categories_hold_each_family_category_and_no_component_row() {
    assert!(COMPONENTS.iter().all(|c| MODEL.contains(c)), "every family category is a row");
    assert!(!MODEL.contains(&COMPONENT), "`component` is the name of them all, not a row of its own");
    assert!(crate::graphics::drawn(COMPONENT) && COMPONENTS.iter().all(|c| crate::graphics::drawn(c)));
    // The keys are the names a family's category goes by in a file, and every category a component may be of has one.
    for c in crate::family::Category::PLACED.into_iter().chain([crate::family::Category::Casework]) {
        let named = serde_json::to_value(c).unwrap();
        let key = crate::graphics::component_category(c).unwrap_or_else(|| panic!("{named} has no row"));
        assert_eq!(named, json!(key));
        assert!(COMPONENTS.contains(&key), "{key}");
    }
    // A door's or a window's family is placed in a wall, as a door or a window: it is no component.
    assert!(crate::graphics::component_category(crate::family::Category::Door).is_none());
    assert!(crate::graphics::component_category(crate::family::Category::Window).is_none());
    assert_eq!(COMPONENTS.len(), 18);
}

#[test]
fn a_new_project_starts_with_the_electrical_templates_named_in_its_language() {
    for l in Language::ALL {
        let m = Model::new_in("p", l);
        for name in ["Electrical Power", "Electrical Lighting", "Electrical Ceiling"] {
            let t = m.view_templates.get(l.word(name)).unwrap_or_else(|| panic!("{l:?}: no {name}"));
            assert_eq!(t.kind, Some(ViewKind::Plan), "{name}");
            assert_eq!(m.check(), Ok(()));
        }
        assert_eq!(m.view_templates.len(), 3);
    }
}
