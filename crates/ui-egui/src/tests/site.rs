//! The Massing & Site tab's site tools (`site.rs`): each makes its element from clicks, as a user makes it; Properties edits
//! what it has; its grips and Edit Boundary / Edit Path redraw it.

use super::*;
use crate::site::SiteTool;

/// The site element of `kind`, the one made last.
fn site_of(h: &Harness<BimApp>, kind: &str) -> Option<(Id, bimcraft_core::masterplan::Site)> {
    h.state().session.model.elements.iter().rev().find_map(|(id, e)| match e {
        Element::Site(st) if st.kind == kind => Some((*id, (**st).clone())),
        _ => None,
    })
}

/// The Massing & Site tab, and a tool on it.
fn site_tool(h: &mut Harness<BimApp>, label: &str) {
    key(h, Key::Escape);
    press(h, "Massing & Site");
    press(h, label);
}

/// A loop of sketch lines through `pts`, back to the first.
fn sketch_loop(h: &mut Harness<BimApp>, pts: &[P2]) {
    for p in pts.iter().chain(pts.first()) {
        click(h, *p);
    }
}

const RECT: [P2; 4] = [P2(1000.0, 1000.0), P2(7000.0, 1000.0), P2(7000.0, 5000.0), P2(1000.0, 5000.0)];

/// The ground under the tools: a toposolid round the box.
fn with_ground(h: &mut Harness<BimApp>) -> Id {
    let r = h
        .state_mut()
        .execute_for_agent("toposolid", &json!({"boundary": [[-10000, -10000], [20000, -10000], [20000, 20000], [-10000, 20000]]}))
        .unwrap();
    h.run();
    serde_json::from_value(r["id"].clone()).unwrap()
}

#[test]
fn the_tab_is_massing_and_site_with_revits_site_tools_after_the_massing_tools() {
    let mut h = app(BOX);
    press(&mut h, "Massing & Site");
    for tool in [
        "Mass",
        "Toposolid",
        "Sub-divide",
        "Road",
        "Property Line",
        "Excavate",
        "Graded Region",
        "Site Component",
        "Planting",
        "Parking Component",
        "Parking Area",
    ] {
        assert!(h.query_all_by_label(tool).next().is_some(), "{tool}");
    }
    assert!(h.query_all_by_label("Building Pad").next().is_none(), "Revit dropped it for Excavate");
}

#[test]
fn toposolid_sketches_the_grounds_outline_and_finish_makes_it() {
    let mut h = app(BOX);
    site_tool(&mut h, "Toposolid");
    assert!(matches!(h.state().sketch.as_ref().map(|s| s.of), Some(crate::sketch::Of::Site { tool: SiteTool::Toposolid, .. })));
    sketch_loop(&mut h, &RECT);
    press(&mut h, "Finish Edit Mode");
    let [topo] = ids(&h, "toposolid")[..] else { panic!("one toposolid: {:?}", h.state().status) };
    let Some(Element::Toposolid(f)) = h.state().session.model.elements.get(topo) else { panic!() };
    assert!((bimcraft_core::geom::area(&f.boundary).abs() - 2.4e7).abs() < 1.0);
    assert_eq!(h.state().selection, vec![topo]);
    // Edit Boundary sketches it again, as a floor's: the core's edit_boundary.
    press(&mut h, "Edit Boundary");
    assert_eq!(h.state().sketch.as_ref().map(|s| s.lines.len()), Some(4));
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{:?}", h.state().status);
}

#[test]
fn property_line_sketches_its_outline_and_shows_its_buildable_area() {
    let mut h = app(BOX);
    site_tool(&mut h, "Property Line");
    // Revit's Create Property Line asks how it is made first.
    press(&mut h, "Create by sketching");
    sketch_loop(&mut h, &[P2(-1000.0, -1000.0), P2(9000.0, -1000.0), P2(9000.0, 6000.0)]);
    press(&mut h, "Finish Edit Mode");
    let [pl] = ids(&h, "propertyline")[..] else { panic!("one property line: {:?}", h.state().status) };
    let Some(Element::PropertyLine(line)) = h.state().session.model.elements.get(pl) else { panic!() };
    assert_eq!(line.points.len(), 3);
    assert!((line.area() - 3.5e7).abs() < 1.0, "{}", line.area());
    // Properties: its area, perimeter and buildable area.
    assert!(h.query_all_by_label("Perimeter").next().is_some());
    assert!(h.query_all_by_label("Buildable Area").next().is_some());
}

#[test]
fn sub_divide_sketches_a_part_of_the_toposolid_of_its_type_and_height_offset() {
    let mut h = app(BOX);
    let topo = with_ground(&mut h);
    site_tool(&mut h, "Sub-divide");
    pick(&mut h, Role::ComboBox, "Type:");
    press(&mut h, "Grass");
    type_into(&mut h, "Height Offset:", "100");
    sketch_loop(&mut h, &RECT);
    press(&mut h, "Finish Edit Mode");
    let [sd] = ids(&h, "subdivision")[..] else { panic!("one sub-division: {:?}", h.state().status) };
    let Some(Element::SubDivision(x)) = h.state().session.model.elements.get(sd) else { panic!() };
    assert_eq!((x.type_name.as_str(), x.offset), ("Grass", 100.0));
    assert_eq!(h.state().session.model.elements.id(x.host), Some(topo));
}

#[test]
fn road_sketches_its_path_and_makes_its_sub_divisions() {
    let mut h = app(BOX);
    with_ground(&mut h);
    site_tool(&mut h, "Road");
    assert_eq!(h.state().site.road_use, "road");
    type_into(&mut h, "Width:", "12000");
    click(&mut h, P2(-4000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    click(&mut h, P2(8000.0, 6500.0));
    // While it is drawn, the road its lines make is drawn under them.
    assert!(h.state().site_sketch_ghost(&h.state().session.model).is_some(), "a preview of the road");
    press(&mut h, "Finish Edit Mode");
    assert!(!ids(&h, "subdivision").is_empty(), "{:?}", h.state().status);
}

#[test]
fn planting_and_site_component_place_the_site_families_on_the_ground() {
    let mut h = app(BOX);
    with_ground(&mut h);
    site_tool(&mut h, "Planting");
    click(&mut h, P2(4000.0, 3000.0));
    let comps = ids(&h, "component");
    let [tree] = comps[..] else { panic!("a tree: {:?}", h.state().status) };
    let Some(Element::Component(c)) = h.state().session.model.elements.get(tree) else { panic!() };
    assert_eq!(c.family, "Tree");
    site_tool(&mut h, "Site Component");
    key(&mut h, Key::Space);
    click(&mut h, P2(2000.0, 2000.0));
    let made: Vec<String> = ids(&h, "component")
        .iter()
        .filter_map(|id| match h.state().session.model.elements.get(*id) {
            Some(Element::Component(c)) => Some(c.family.clone()),
            _ => None,
        })
        .collect();
    assert!(made.contains(&"Bike Rack".to_string()), "{made:?}");
}

#[test]
fn excavate_and_graded_region_act_on_the_selection() {
    let mut h = app(BOX);
    let topo = with_ground(&mut h);
    let f = h
        .state_mut()
        .execute_for_agent("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "offset": -3000}))
        .unwrap();
    let floor: Id = serde_json::from_value(f["id"].clone()).unwrap();
    h.state_mut().selection = vec![topo, floor];
    h.run();
    press(&mut h, "Massing & Site");
    press(&mut h, "Excavate");
    let Some(Element::Toposolid(t)) = h.state().session.model.elements.get(topo) else { panic!() };
    assert_eq!(t.excavated_by.len(), 1, "{:?}", h.state().status);
    h.state_mut().selection = vec![topo];
    h.run();
    press(&mut h, "Graded Region");
    assert_eq!(ids(&h, "toposolid").len(), 2, "{:?}", h.state().status);
}

#[test]
fn parking_component_places_a_parking_space_and_parking_area_lays_out_a_car_park() {
    let mut h = app(BOX);
    site_tool(&mut h, "Parking Component");
    pick(&mut h, Role::ComboBox, "Type:");
    press(&mut h, "Accessible");
    click(&mut h, P2(4000.0, 3000.0));
    let bay = h.state().session.model.elements.values().find_map(|e| match e {
        Element::Component(c) if c.family == "Parking Space" => Some(c.clone()),
        _ => None,
    });
    let bay = bay.unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(bay.type_name, "Accessible");

    // A car park wants room: a plan forty metres across.
    let mut h = app(r#"wall {"points": [[-2000,-8000],[42000,-8000],[42000,32000],[-2000,32000]], "closed": true}"#);
    site_tool(&mut h, "Parking Area");
    sketch_loop(&mut h, &[P2(0.0, 0.0), P2(40000.0, 0.0), P2(40000.0, 30000.0), P2(0.0, 30000.0)]);
    press(&mut h, "Finish Edit Mode");
    let (id, park) = site_of(&h, "car_park").unwrap_or_else(|| panic!("{:?}", h.state().status));
    let stalls = park.props.get("result").and_then(|r| r.get("stalls")).and_then(Value::as_f64).unwrap_or(0.0);
    assert!(stalls > 10.0, "laid out: {stalls} stalls");
    let bays =
        h.state().session.model.elements.values().filter(|e| matches!(e, Element::Component(c) if c.family == "Parking Space")).count();
    assert_eq!(bays as f64, stalls, "as parking spaces");
    // Properties: its stalls, read only; and Run on the ribbon.
    h.state_mut().selection = vec![id];
    h.run();
    assert!(h.query_all_by_label("Stalls").next().is_some());
    press(&mut h, "Run");
    assert!(site_of(&h, "car_park").is_some(), "{:?}", h.state().status);
}

#[test]
fn the_ramp_tool_sketches_a_run_with_lines_and_arcs() {
    let mut h = app(BOX);
    h.state_mut().set_tool(crate::Tool::Ramp(Vec::new()));
    h.run();
    press(&mut h, "Sketch Run");
    click(&mut h, P2(-4000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    press(&mut h, "Finish Edit Mode");
    let ramp = h.state().session.model.elements.values().find_map(|e| match e {
        Element::Ramp(r) => Some(r.clone()),
        _ => None,
    });
    let ramp = ramp.unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(ramp.path.len(), 2);
}

#[test]
fn a_mass_shows_its_masterplan_figures_and_storeys_set_its_height() {
    let mut h = app(r#"mass {"boundary": [[1000, 1000], [5000, 1000], [5000, 4000], [1000, 4000]], "height": 3000}"#);
    let mass = ids(&h, "mass")[0];
    h.state_mut().selection = vec![mass];
    h.run();
    type_role(&mut h, Role::TextInput, "Storeys", "4");
    let Some(Element::Mass(ms)) = h.state().session.model.elements.get(mass) else { panic!() };
    assert_eq!(ms.site.as_ref().map(|s| s.props["storeys"].clone()), Some(json!(4)));
    assert_eq!(ms.height, 12000.0, "four storeys of 3 m");
}

#[test]
fn the_new_categories_have_names_where_categories_are_listed() {
    for (c, one, many) in [
        ("toposolid", "Toposolid", "Toposolids"),
        ("subdivision", "Sub-division", "Sub-divisions"),
        ("propertyline", "Property Line", "Property Lines"),
        ("planting", "Planting", "Planting"),
        ("sitecomponent", "Site", "Site"),
        ("site", "Car Park", "Car Parks"),
        ("parking", "Parking", "Parking"),
    ] {
        assert_eq!(crate::lang::category(c, false), one);
        assert_eq!(crate::lang::category(c, true), many);
    }
    let mut h = app(BOX);
    shortcut(&mut h, "VG");
    assert!(h.query_all_by_label("Toposolids").next().is_some(), "Visibility/Graphics lists them");
    assert!(h.query_all_by_label("buildingpad").next().is_none());
}

#[test]
fn property_line_by_distances_and_bearings_closes_and_its_setbacks_draw_the_buildable_line() {
    use crate::pltable::{PlDialog, Row};
    let mut h = app(BOX);
    site_tool(&mut h, "Property Line");
    press(&mut h, "Create by entering distances and bearings");
    // Revit's Edit Table: 20 m east, 15 m north; Add Line to Close adds 20 m west, and OK closes it with the last line south.
    let row = |d: &str, b: &str| Row { distance: d.into(), bearing: b.into(), ..Row::default() };
    match h.state_mut().pl_dialog.as_mut() {
        Some(PlDialog::Table(t)) => t.rows = vec![row("20000", "N 90°00'00\" E"), row("15000", "N 0 E")],
        other => panic!("the Edit Table: {other:?}"),
    }
    h.run();
    press(&mut h, "Add Line to Close");
    match h.state().pl_dialog.as_ref() {
        Some(PlDialog::Table(t)) => {
            assert_eq!(t.rows.len(), 3);
            assert!(t.rows[2].bearing.starts_with("S 53°07'"), "{:?}", t.rows[2]);
        }
        other => panic!("{other:?}"),
    }
    press(&mut h, "OK");
    assert!(h.state().pl_dialog.is_none(), "{:?}", h.state().status);
    let [pl] = ids(&h, "propertyline")[..] else { panic!("one property line: {:?}", h.state().status) };
    let Some(Element::PropertyLine(line)) = h.state().session.model.elements.get(pl) else { panic!() };
    assert!((line.area() - 1.5e8).abs() < 10.0, "{}", line.area());
    // Edit Table reads it back; Setbacks gives each segment its own.
    h.state_mut().selection = vec![pl];
    h.run();
    press(&mut h, "Edit Table");
    assert!(
        matches!(h.state().pl_dialog.as_ref(), Some(PlDialog::Table(t)) if t.rows.len() == 3 && t.id == Some(pl)),
        "{:?} {:?} {:?}",
        h.state().pl_dialog,
        h.state().status,
        h.state().selection
    );
    press(&mut h, "Cancel");
    press(&mut h, "Setbacks");
    match h.state_mut().pl_dialog.as_mut() {
        Some(PlDialog::Setbacks { rows, .. }) => {
            assert_eq!(rows.len(), 3);
            rows[0].setback = "3000".into();
            rows[0].kind = "street".into();
            rows[0].street = "High Street".into();
        }
        other => panic!("{other:?}"),
    }
    h.run();
    press(&mut h, "OK");
    let Some(Element::PropertyLine(line)) = h.state().session.model.elements.get(pl) else { panic!() };
    assert_eq!((line.edge(0).setback, line.edge(0).street.as_str()), (3000.0, "High Street"), "{:?}", h.state().status);
    let buildable: f64 = line.buildable().iter().map(|r| bimcraft_core::geom::area(r)).sum();
    assert!(buildable < line.area() - 1.0e7, "{buildable}");
}
