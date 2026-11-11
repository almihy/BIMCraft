//! The Massing & Site tab's site tools (`site.rs`): each makes its element from clicks, as a user makes it; Properties edits
//! one's props through `site_edit`; its grips and Edit Boundary / Edit Path redraw it.

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

#[test]
fn the_tab_is_massing_and_site_with_the_site_panels_after_the_massing_tools() {
    let mut h = app(BOX);
    press(&mut h, "Massing & Site");
    for tool in
        ["Mass", "Toposolid", "Sub-divide", "Road", "Building Pad", "Property Line", "Site Component", "Parking Component", "Parking Area"]
    {
        assert!(h.query_all_by_label(tool).next().is_some(), "{tool}");
    }
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
fn building_pad_and_property_line_sketch_their_outlines() {
    let mut h = app(BOX);
    site_tool(&mut h, "Building Pad");
    type_into(&mut h, "Height Offset:", "500");
    sketch_loop(&mut h, &RECT);
    press(&mut h, "Finish Edit Mode");
    let [pad] = ids(&h, "buildingpad")[..] else { panic!("one pad: {:?}", h.state().status) };
    let Some(Element::BuildingPad(bp)) = h.state().session.model.elements.get(pad) else { panic!() };
    assert_eq!(bp.offset, 500.0);
    assert_eq!(bp.boundary.len(), 4);

    site_tool(&mut h, "Property Line");
    sketch_loop(&mut h, &[P2(-1000.0, -1000.0), P2(9000.0, -1000.0), P2(4000.0, 6500.0)]);
    press(&mut h, "Finish Edit Mode");
    let [pl] = ids(&h, "propertyline")[..] else { panic!("one property line: {:?}", h.state().status) };
    let Some(Element::PropertyLine(line)) = h.state().session.model.elements.get(pl) else { panic!() };
    assert_eq!(line.points.len(), 3);
    assert!((line.area() - 3.75e7).abs() < 1.0, "{}", line.area());
    // Properties: its area and perimeter.
    assert!(h.query_all_by_label("Perimeter").next().is_some());
}

#[test]
fn sub_divide_sketches_an_area_of_its_type_and_its_pitch_places_where_clicked() {
    let mut h = app(BOX);
    site_tool(&mut h, "Sub-divide");
    sketch_loop(&mut h, &RECT);
    press(&mut h, "Finish Edit Mode");
    let (_, paving) = site_of(&h, "paving").unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(paving.use_, "forecourt");
    assert_eq!(paving.props["control"].as_array().map(Vec::len), Some(4));
    assert!(!paving.outline.is_empty());

    // A lawn: the Type's Landscape, its first use.
    site_tool(&mut h, "Sub-divide");
    pick(&mut h, Role::ComboBox, "Type:");
    press(&mut h, "Landscape");
    assert_eq!((h.state().site.subdivide, h.state().site.subdivide_use), ("landscape", "lawn"));
    sketch_loop(&mut h, &[P2(2000.0, 2000.0), P2(4000.0, 2000.0), P2(3000.0, 4000.0)]);
    press(&mut h, "Finish Edit Mode");
    assert_eq!(site_of(&h, "landscape").map(|(_, s)| s.use_), Some("lawn".into()));

    // A pitch has its standard size: the tool places it where clicked, Space turning it.
    site_tool(&mut h, "Sub-divide");
    pick(&mut h, Role::ComboBox, "Type:");
    press(&mut h, "Pitch");
    assert!(h.state().sketch.is_none(), "a pitch is placed, not sketched");
    assert_eq!(h.state().tool, Tool::Site(SiteTool::Subdivide));
    key(&mut h, Key::Space);
    click(&mut h, P2(4000.0, 3000.0));
    let (_, pitch) = site_of(&h, "pitch").unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(pitch.props["at"], json!([4000.0, 3000.0]));
    assert_eq!(pitch.props["angle"], json!(90.0));
}

#[test]
fn road_sketches_its_path_with_the_options_bars_use_width_and_lanes() {
    let mut h = app(BOX);
    site_tool(&mut h, "Road");
    assert_eq!(h.state().site.road_use, "road");
    type_into(&mut h, "Width:", "12000");
    type_into(&mut h, "Lanes:", "2");
    click(&mut h, P2(-4000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    click(&mut h, P2(8000.0, 6500.0));
    // While it is drawn, the road its lines make is drawn under them.
    assert!(h.state().site_sketch_ghost(&h.state().session.model).is_some(), "a preview of the road");
    press(&mut h, "Finish Edit Mode");
    let (id, road) = site_of(&h, "lane").unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(road.use_, "road");
    assert_eq!(road.props["width"], json!(12000.0));
    assert_eq!(road.props["lanes"], json!(2));
    assert_eq!(road.props["path"].as_array().map(Vec::len), Some(3));
    assert!(road.parts.iter().any(|p| p.role == "carriageway"), "a road's cross-section");
    // Properties: its carriageway and pavements, read only.
    assert_eq!(h.state().selection, vec![id]);
    assert!(h.query_all_by_label("Carriageway").next().is_some());
    // Edit Path: its line in the sketch, Finish puts it back.
    press(&mut h, "Edit Path");
    assert_eq!(h.state().sketch.as_ref().map(|s| s.lines.len()), Some(2));
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{:?}", h.state().status);
    assert_eq!(site_of(&h, "lane").map(|(_, s)| s.props["path"].clone()), Some(road.props["path"].clone()));
}

#[test]
fn site_component_places_a_tree_and_a_bike_rack_turned_and_sketches_a_hedge() {
    let mut h = app(BOX);
    site_tool(&mut h, "Site Component");
    type_into(&mut h, "Crown:", "5000");
    click(&mut h, P2(4000.0, 3000.0));
    let (_, tree) = site_of(&h, "tree").unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(tree.props["at"], json!([4000.0, 3000.0]));
    assert_eq!(tree.props["crown"], json!(5000.0));

    pick(&mut h, Role::ComboBox, "Type:");
    press(&mut h, "Bike Rack");
    key(&mut h, Key::Space);
    click(&mut h, P2(2000.0, 2000.0));
    let (_, rack) = site_of(&h, "bike_rack").unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(rack.props["angle"], json!(90.0));
    assert_eq!(rack.use_, "residents");

    pick(&mut h, Role::ComboBox, "Type:");
    press(&mut h, "Hedge");
    assert!(h.state().sketch.is_some(), "a hedge runs along a path: sketched");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(7000.0, 1000.0));
    press(&mut h, "Finish Edit Mode");
    assert!(site_of(&h, "hedge").is_some(), "{:?}", h.state().status);
}

#[test]
fn parking_component_places_a_stall_and_parking_area_lays_out_a_car_park_or_a_row() {
    let mut h = app(BOX);
    site_tool(&mut h, "Parking Component");
    pick(&mut h, Role::ComboBox, "Use:");
    press(&mut h, "Accessible");
    click(&mut h, P2(4000.0, 3000.0));
    let (_, stall) = site_of(&h, "parking_stall").unwrap_or_else(|| panic!("{:?}", h.state().status));
    assert_eq!(stall.use_, "accessible");

    // A car park wants room: a plan forty metres across.
    let mut h = app(r#"wall {"points": [[-2000,-8000],[42000,-8000],[42000,32000],[-2000,32000]], "closed": true}"#);
    site_tool(&mut h, "Parking Area");
    sketch_loop(&mut h, &[P2(0.0, 0.0), P2(40000.0, 0.0), P2(40000.0, 30000.0), P2(0.0, 30000.0)]);
    press(&mut h, "Finish Edit Mode");
    let (id, park) = site_of(&h, "car_park").unwrap_or_else(|| panic!("{:?}", h.state().status));
    let stalls = park.layout.as_ref().and_then(|l| l.result.get("stalls")).and_then(Value::as_f64).unwrap_or(0.0);
    assert!(stalls > 10.0, "laid out: {stalls} stalls");
    // Properties: its stalls, read only.
    assert_eq!(h.state().selection, vec![id]);
    assert!(h.query_all_by_label("Stalls").next().is_some());

    site_tool(&mut h, "Parking Area");
    pick(&mut h, Role::RadioButton, "Row");
    click(&mut h, P2(0.0, -5000.0));
    click(&mut h, P2(30000.0, -5000.0));
    press(&mut h, "Finish Edit Mode");
    assert!(site_of(&h, "parking_band").is_some(), "{:?}", h.state().status);
}

#[test]
fn properties_edit_a_site_elements_props_use_and_phase_through_site_edit() {
    let mut h = app(BOX);
    site_tool(&mut h, "Site Component");
    click(&mut h, P2(4000.0, 3000.0));
    let (tree, _) = site_of(&h, "tree").unwrap();
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(h.state().selection, vec![tree]);
    type_role(&mut h, Role::TextInput, "Crown", "7500");
    assert_eq!(site_of(&h, "tree").map(|(_, s)| s.props["crown"].clone()), Some(json!(7500.0)));
    // Blank, back to the figures' default.
    type_role(&mut h, Role::TextInput, "Crown", "");
    assert_eq!(site_of(&h, "tree").map(|(_, s)| s.props["crown"].clone()), Some(json!(6000.0)));
    // Its phase, by name: the palette scrolled to it, as a hand does.
    h.get_by_role_and_label(Role::ComboBox, "Phase").scroll_to_me();
    h.run();
    pick(&mut h, Role::ComboBox, "Phase");
    press(&mut h, "Existing");
    assert_eq!(site_of(&h, "tree").map(|(_, s)| s.phase), Some("existing".into()));
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
fn a_trees_point_and_a_paths_points_are_grips() {
    let mut h = app(BOX);
    h.state_mut().execute_for_agent("site", &json!({"kind": "tree", "props": {"at": [4000, 3000]}})).unwrap();
    let (tree, _) = site_of(&h, "tree").unwrap();
    h.state_mut().selection = vec![tree];
    h.run();
    drag(&mut h, P2(4000.0, 3000.0), P2(5500.0, 3500.0));
    let at = site_of(&h, "tree").map(|(_, s)| s.props["at"].clone()).unwrap();
    assert_eq!(at, json!([5500.0, 3500.0]), "the tree moved by its grip");

    h.state_mut()
        .execute_for_agent("site", &json!({"kind": "lane", "use": "footpath", "props": {"path": [[0, 3000], [8000, 3000]]}}))
        .unwrap();
    let (lane, _) = site_of(&h, "lane").unwrap();
    h.state_mut().selection = vec![lane];
    h.run();
    drag(&mut h, P2(8000.0, 3000.0), P2(8000.0, 5000.0));
    let path = site_of(&h, "lane").map(|(_, s)| s.props["path"].clone()).unwrap();
    assert_eq!(path[1], json!([8000.0, 5000.0, 0.0]), "{path}");
}

#[test]
fn the_new_categories_have_names_where_categories_are_listed() {
    for (c, one, many) in [
        ("toposolid", "Toposolid", "Toposolids"),
        ("buildingpad", "Building Pad", "Building Pads"),
        ("propertyline", "Property Line", "Property Lines"),
        ("site", "Site", "Site"),
    ] {
        assert_eq!(crate::lang::category(c, false), one);
        assert_eq!(crate::lang::category(c, true), many);
    }
    let mut h = app(BOX);
    shortcut(&mut h, "VG");
    assert!(h.query_all_by_label("Toposolids").next().is_some(), "Visibility/Graphics lists them");
    assert!(h.query_all_by_label("buildingpad").next().is_none());
}
