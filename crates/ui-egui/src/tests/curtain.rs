//! Curtain walls in the window, as Revit shows them: the Wall tool's curtain wall types, a curtain wall's, a panel's and a
//! mullion's Properties, their Type Properties, and what the status bar calls them.

use super::*;
use egui_kittest::kittest::NodeT;

const STORE: &str = r#"wall {"points": [[0,0],[6000,0]], "type": "Storefront", "height": 3000}"#;

/// The curtain panels and the mullions of the model.
fn parts(h: &Harness<BimApp>) -> (Vec<Id>, Vec<Id>) {
    (ids(h, "curtainpanel"), ids(h, "mullion"))
}

fn select(h: &mut Harness<BimApp>, id: Id) {
    h.state_mut().selection = vec![id];
    h.run();
}

fn wall_type(h: &Harness<BimApp>, name: &str) -> Value {
    serde_json::to_value(&h.state().session.model.types.wall[name]).unwrap()
}

#[test]
fn the_wall_tool_always_offers_the_curtain_wall_types_and_a_wall_drawn_with_one_is_a_curtain_wall() {
    let mut h = app(BOX);
    tool(&mut h, "Wall");
    press(&mut h, "Type Selector");
    // Under the family each is of, as Revit's Type Selector: the project's basic walls, then the curtain walls.
    for label in ["Basic Wall", "Generic - 200mm", "Exterior Glazing", "Storefront"] {
        assert_eq!(h.query_all_by_label(label).count(), 1, "{label}");
    }
    assert_eq!(h.query_all_by_label("Curtain Wall").count(), 2, "the family's heading and the type named after it");
    press(&mut h, "Storefront");
    assert_eq!(h.state().types.get("wall").map(String::as_str), Some("Storefront"));
    // The project has no such type until a wall is drawn with it: none to edit yet.
    assert!(!h.state().session.model.types.wall.contains_key("Storefront"));
    assert!(h.state().type_subject().is_none());
    click(&mut h, P2(1000.0, 2000.0));
    click(&mut h, P2(7000.0, 2000.0));
    key(&mut h, Key::Escape);
    let m = h.state().session.model.clone();
    assert!(m.types.wall["Storefront"].is_curtain(), "{}", h.state().status.text);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.type_name == "Storefront"));
    assert!(!m.hosted_by(m.guid(wall)).is_empty(), "panels and mullions");
    // Now the project's own, it is listed once, and Edit Type opens it.
    press(&mut h, "Type Selector");
    assert_eq!(h.query_all_by_label("Storefront").count(), 1);
    assert_eq!(h.state().type_subject().map(|(c, n, _)| (c, n)), Some(("wall", "Storefront".to_string())));
}

#[test]
fn a_walls_type_selector_lists_the_curtain_wall_types_the_project_has_and_changes_it_to_one() {
    let mut h = app(&format!("{BOX}\n{STORE}"));
    let basic = ids(&h, "wall")[0];
    select(&mut h, basic);
    press(&mut h, "Type Selector");
    assert!(h.query_by_label("Storefront").is_some() && h.query_by_label("Basic Wall").is_some());
    assert!(h.query_by_label("Exterior Glazing").is_none(), "one the project hasn't made is the Wall tool's to make");
    press(&mut h, "Storefront");
    let m = h.state().session.model.clone();
    assert_eq!(field(&h, basic, "type"), json!("Storefront"), "{}", h.state().status.text);
    assert!(!m.hosted_by(m.guid(basic)).is_empty(), "it is a curtain wall now");
}

#[test]
fn a_curtain_wall_shows_its_grids_and_no_location_line_and_its_number_is_typed_only_for_a_fixed_number() {
    let mut h = app(&format!("{BOX}\n{STORE}"));
    let wall = ids(&h, "wall")[4];
    select(&mut h, wall);
    for label in ["Vertical Grid", "Horizontal Grid", "Base Constraint", "Unconnected Height", "Room Bounding", "Length", "Area"] {
        assert!(h.query_by_label(label).is_some(), "no `{label}`");
    }
    assert!(h.query_by_label("Location Line").is_none(), "a curtain wall has none");
    // Number, Justification and Offset, once for each grid.
    for label in ["Number", "Justification", "Offset"] {
        assert_eq!(
            h.query_all_by_role_and_label(Role::TextInput, label).count() + h.query_all_by_role_and_label(Role::ComboBox, label).count(),
            2,
            "{label}"
        );
    }
    // A Storefront's grids are by Maximum Spacing and Fixed Distance: neither Number is typed.
    assert!(h.query_all_by_role_and_label(Role::TextInput, "Number").all(|n| n.accesskit_node().is_disabled()));
    // Its vertical grid by a Fixed Number: that Number is typed, four lines unless said.
    h.state_mut().run("type", json!({"category": "wall", "name": "Storefront", "curtain": {"vertical": {"layout": "fixed-number"}}}));
    h.run();
    let numbers: Vec<bool> = h.query_all_by_role_and_label(Role::TextInput, "Number").map(|n| n.accesskit_node().is_disabled()).collect();
    assert_eq!(numbers, [false, true], "the vertical grid's alone");
    let panels = |h: &Harness<BimApp>| ids(h, "curtainpanel").len();
    assert_eq!(panels(&h), 5 * 2);
    let number = h.query_all_by_role_and_label(Role::TextInput, "Number").next().unwrap();
    assert_eq!(number.value().as_deref(), Some("4"));
    number.focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.query_all_by_role_and_label(Role::TextInput, "Number").next().unwrap().type_text("2");
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(panels(&h), 3 * 2, "{}", h.state().status.text);
    // Justification: Beginning, Center or End.
    let m = h.state().session.model.clone();
    assert_eq!(m.wall(wall).unwrap().curtain.vertical.number, 2);
    h.query_all_by_role_and_label(Role::ComboBox, "Justification").next().unwrap().click_accesskit();
    h.run();
    h.get_by_label("End").click_accesskit();
    h.run();
    assert_eq!(field(&h, wall, "curtain")["vertical"]["justification"], json!("end"), "{}", h.state().status.text);
}

#[test]
fn a_panel_and_a_mullion_show_what_they_measure_their_host_and_a_type_selector_greyed_while_pinned() {
    let mut h = app(STORE);
    let wall = ids(&h, "wall")[0];
    let (panels, mullions) = parts(&h);
    select(&mut h, panels[0]);
    for label in ["Height", "Width", "Area", "Host"] {
        assert!(h.query_by_label(label).is_some(), "a panel's `{label}`");
    }
    // What its wall placed is pinned, and of the type the wall's type gives it: the Type Selector is greyed.
    assert!(h.get_by_label("Type Selector").accesskit_node().is_disabled());
    assert_eq!(h.state().type_subject().map(|(c, n, _)| (c, n)), Some(("curtainpanel", "Glazed".to_string())));
    // Its host is a link that selects the wall.
    h.get_by_role(Role::Link).click();
    h.run();
    assert_eq!(h.state().selection, [wall]);
    select(&mut h, mullions[0]);
    assert!(h.query_by_label("Length").is_some() && h.query_by_label("Host").is_some());
    assert!(h.get_by_label("Type Selector").accesskit_node().is_disabled());
    // Delete leaves what is pinned, with Revit's words for it.
    h.state_mut().selection = vec![panels[0]];
    h.state_mut().delete_selection();
    h.run();
    assert_eq!(parts(&h).0.len(), panels.len());
    assert!(h.state().warned.iter().any(|w| w.contains("Pinned objects were not deleted")), "{:?}", h.state().warned);
}

#[test]
fn the_status_bar_names_curtain_walls_and_their_parts_by_their_family_as_revit_does() {
    let mut h = app(&format!("{BOX}\n{STORE}"));
    let (panels, mullions) = parts(&h);
    let walls = ids(&h, "wall");
    let mut named = |id: Id| {
        let s = h.state_mut();
        s.canvas.hovered = Some(id);
        s.hover_text().unwrap_or_default()
    };
    assert_eq!(named(walls[4]), "Walls : Curtain Wall : Storefront");
    assert_eq!(named(walls[0]), "Walls : Basic Wall : Generic - 200mm");
    // (The categories' own words, Curtain Panels and Curtain Wall Mullions, are `lang::category`'s.)
    assert!(named(panels[0]).ends_with(" : System Panel : Glazed"), "{}", named(panels[0]));
    assert!(named(mullions[0]).ends_with(" : Rectangular Mullion : 50 x 150mm"), "{}", named(mullions[0]));
    let m = &h.state().session.model;
    assert_eq!(crate::curtain::family(m, "curtainpanel", "Empty"), Some("Empty System Panel"));
    assert_eq!(crate::curtain::family(m, "mullion", "25mm Radius"), Some("Circular Mullion"));
}

#[test]
fn a_curtain_wall_types_properties_are_revits_and_change_its_grid_its_panel_and_its_mullions() {
    let mut h = app(STORE);
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.state_mut().edit_type();
    h.run();
    assert!(h.query_by_label("System Family: Curtain Wall").is_some());
    for label in ["Construction", "Vertical Grid", "Horizontal Grid", "Vertical Mullions", "Horizontal Mullions", "Identity Data"] {
        assert!(h.query_all_by_label(label).next().is_some(), "no group `{label}`");
    }
    for label in ["Function", "Curtain Panel", "Join Condition"] {
        assert!(h.query_by_role_and_label(Role::ComboBox, label).is_some(), "no `{label}`");
    }
    for label in ["Layout", "Interior Type", "Border 1 Type", "Border 2 Type"] {
        assert_eq!(h.query_all_by_role_and_label(Role::ComboBox, label).count(), 2, "`{label}`, each way");
    }
    // No layers: none of a basic wall's rows, and no Edit Assembly.
    for label in ["Wrapping at Inserts", "Coarse Scale Fill Pattern", "Structural Material", "Edit…"] {
        assert!(h.query_by_label(label).is_none(), "`{label}` is a basic wall's");
    }
    // Spacing is typed for a layout that spaces by it: the vertical grid by a Fixed Number has none.
    let spacings = |h: &Harness<BimApp>| -> Vec<bool> {
        h.query_all_by_role_and_label(Role::TextInput, "Spacing").map(|n| n.accesskit_node().is_disabled()).collect()
    };
    assert_eq!(spacings(&h), [false, false]);
    h.query_all_by_role_and_label(Role::ComboBox, "Layout").next().unwrap().click();
    h.run();
    press(&mut h, "Fixed Number");
    assert_eq!(spacings(&h), [true, false]);
    // No mullions on the vertical grid's lines between its borders, and solid panels.
    h.query_all_by_role_and_label(Role::ComboBox, "Interior Type").next().unwrap().click();
    h.run();
    press(&mut h, "None");
    h.get_by_role_and_label(Role::ComboBox, "Curtain Panel").click();
    h.run();
    press(&mut h, "System Panel : Solid");
    h.get_by_role_and_label(Role::ComboBox, "Join Condition").click();
    h.run();
    press(&mut h, "Horizontal Grid Continuous");
    press(&mut h, "OK");
    assert!(h.state().type_dialog.is_none(), "{}", h.state().status.text);
    let t = wall_type(&h, "Storefront");
    assert_eq!(t["curtain"]["vertical"]["layout"], "fixed-number");
    assert_eq!(t["curtain"]["vertical"]["spacing"], 1500.0, "kept");
    assert_eq!(t["curtain"]["vertical_mullions"], json!({"border1": "50 x 150mm", "border2": "50 x 150mm"}));
    assert_eq!(t["curtain"]["horizontal_mullions"]["interior"], "50 x 150mm", "as it was");
    assert_eq!((&t["curtain"]["panel"], &t["curtain"]["join_condition"]), (&json!("Solid"), &json!("horizontal-grid-continuous")));
    let m = h.state().session.model.clone();
    assert!(parts(&h).0.iter().all(|id| matches!(m.elements.get(*id), Some(Element::CurtainPanel(p)) if p.type_name == "Solid")));
}

#[test]
fn a_duplicate_of_a_curtain_wall_type_has_its_grid_its_mullions_and_its_identity_data() {
    let mut h = app(STORE);
    let wall = ids(&h, "wall")[0];
    h.state_mut().run("type", json!({"category": "wall", "name": "Storefront", "type_mark": "CW1", "function": "exterior"}));
    h.state_mut().selection = vec![wall];
    h.state_mut().edit_type();
    h.run();
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Shopfront");
    press(&mut h, "Create");
    let (was, copy) = (wall_type(&h, "Storefront"), wall_type(&h, "Shopfront"));
    assert_eq!(copy, was, "{}", h.state().status.text);
    assert_eq!(copy["identity"]["type_mark"], "CW1");
    // The Type list is the curtain wall types', and OK puts the wall on the copy shown.
    press(&mut h, "OK");
    assert_eq!(field(&h, wall, "type"), json!("Shopfront"));
}

#[test]
fn a_panel_types_and_a_mullion_types_properties_are_revits_and_change_them() {
    let mut h = app(STORE);
    let (panels, mullions) = parts(&h);
    // System Panel: its offset, its material and its thickness.
    h.state_mut().selection = vec![panels[0]];
    h.state_mut().edit_type();
    h.run();
    assert!(h.query_by_label("System Family: System Panel").is_some());
    for label in ["Constraints", "Materials and Finishes", "Dimensions", "Identity Data", "Offset", "Material", "Thickness"] {
        assert!(h.query_all_by_label(label).next().is_some(), "a panel type's `{label}`");
    }
    type_into(&mut h, "Thickness", "30");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.types.curtain_panel["Glazed"].thickness, 30.0, "{}", h.state().status.text);
    // Rectangular Mullion: its offset, its thickness, its material and its width to each side of its grid line.
    h.state_mut().selection = vec![mullions[0]];
    h.state_mut().edit_type();
    h.run();
    assert!(h.query_by_label("System Family: Rectangular Mullion").is_some());
    for label in ["Constraints", "Construction", "Materials and Finishes", "Dimensions", "Offset", "Thickness", "Material"] {
        assert!(h.query_all_by_label(label).next().is_some(), "a mullion type's `{label}`");
    }
    type_into(&mut h, "Width on side 1", "40");
    type_into(&mut h, "Width on side 2", "20");
    press(&mut h, "OK");
    let t = h.state().session.model.types.mullion["50 x 150mm"].clone();
    assert_eq!((t.side1, t.side2, t.thickness), (40.0, 20.0, 150.0), "{}", h.state().status.text);
    h.state_mut().edit_type();
    h.run();
    // A copy is a rectangular mullion too, with what was typed.
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "60 x 150mm");
    press(&mut h, "Create");
    press(&mut h, "Cancel");
    let m = h.state().session.model.clone();
    assert_eq!(serde_json::to_value(&m.types.mullion["60 x 150mm"]).unwrap(), serde_json::to_value(&t).unwrap());
    // Circular Mullion: a radius, and no widths; Empty System Panel: nothing but what it is called.
    h.state_mut().type_dialog = Some(crate::properties::TypeDialog::new(&m, "mullion", "25mm Radius".into(), Vec::new()));
    h.run();
    assert!(h.query_by_label("System Family: Circular Mullion").is_some());
    assert!(h.query_by_label("Radius").is_some() && h.query_by_label("Width on side 1").is_none());
    type_into(&mut h, "Radius", "30");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.types.mullion["25mm Radius"].thickness, 60.0, "{}", h.state().status.text);
    h.state_mut().type_dialog = Some(crate::properties::TypeDialog::new(&m, "curtainpanel", "Empty".into(), Vec::new()));
    h.run();
    assert!(h.query_by_label("System Family: Empty System Panel").is_some());
    assert!(h.query_by_label("Thickness").is_none() && h.query_all_by_label("Identity Data").next().is_some());
}

#[test]
fn the_project_browser_lists_curtain_wall_types_under_curtain_wall_and_the_panel_and_mullion_types() {
    let mut h = app(&format!("{BOX}\n{STORE}"));
    press(&mut h, "List Families");
    // Under Walls: the basic wall types, and the curtain wall types under their family.
    h.get_by_label("Walls").click_accesskit();
    h.run();
    assert!(h.query_by_label("Generic - 200mm").is_some() && h.query_by_label("Storefront").is_none());
    h.get_by_label("Curtain Wall").click_accesskit();
    h.run();
    // A click makes it the Wall tool's type, as any type's does; its menu is any type's.
    h.get_by_label("Storefront").click_accesskit();
    h.run();
    assert_eq!(h.state().types.get("wall").map(String::as_str), Some("Storefront"));
    // Curtain panel types and mullion types are listed as the other system types are: each under its category's group.
    // (The groups' names, Curtain Panels and Curtain Wall Mullions, are `lang::category`'s.)
    for (category, ty) in [("curtainpanel", "Glazed"), ("mullion", "50 x 150mm")] {
        assert!(h.query_by_label(ty).is_none());
        h.get_by_label(&crate::lang::category(category, true)).click_accesskit();
        h.run();
        assert!(h.query_by_label(ty).is_some(), "{category}: no `{ty}`");
    }
}
