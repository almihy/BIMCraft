//! A selected wall's temporary dimensions to the walls and grids beside it: typed to move the wall, the witness line's grip,
//! and the icon that makes one permanent.

use super::*;

const WITNESS: &str = "Witness line: click for its next line, drag to another reference";
const FAR_WITNESS: &str = "Witness line at the other end: click for its next line, drag to another reference";
const PERMANENT: &str = "Make this temporary dimension permanent";

/// The box of walls with a wall of 4700 inside it, 2000 from the south wall and 4000 from the north one.
fn with_inner_wall() -> (Harness<'static, BimApp>, Id) {
    let mut h = app(&format!("{BOX}\nwall {{\"points\": [[1000, 2000], [5700, 2000]]}}"));
    let inner = ids(&h, "wall")[4];
    click(&mut h, P2(3000.0, 2000.0));
    assert_eq!(h.state().selection, [inner]);
    (h, inner)
}

fn start(h: &Harness<BimApp>, wall: Id) -> P2 {
    h.state().session.model.wall(wall).map(|w| w.start).unwrap_or_default()
}

#[test]
fn a_selected_wall_shows_how_far_it_stands_from_the_walls_either_side_of_it() {
    let (h, _) = with_inner_wall();
    // Its own length, then north to the far wall's centre line and south to the near one's.
    for n in ["4700", "4000", "2000"] {
        assert!(h.query_by_label(&format!("Temporary dimension {n}")).is_some(), "{n}");
    }
}

#[test]
fn typing_a_walls_temporary_dimension_moves_it_so_the_distance_is_met() {
    let (mut h, inner) = with_inner_wall();
    // 500 less from the south wall: it moves 500 south, and the north dimension grows by that.
    retype_dimension(&mut h, "2000", "1500");
    assert_eq!(start(&h, inner), P2(1000.0, 1500.0));
    assert_eq!(h.state().session.history().last(), Some(&"Move"));
    assert!(h.query_by_label("Temporary dimension 4500").is_some());
    assert!(h.query_by_label("Temporary dimension 1500").is_some());
    // From the north wall: 4000 becomes 1000, the wall goes 3500 north of where it stood.
    retype_dimension(&mut h, "4500", "1000");
    assert_eq!(start(&h, inner), P2(1000.0, 5000.0));
    // What isn't a length moves nothing, and says so.
    let steps = h.state().session.history().len();
    retype_dimension(&mut h, "1000", "far");
    assert_eq!(h.state().session.history().len(), steps);
    assert!(h.state().status.error);
}

#[test]
fn the_witness_lines_grip_cycles_the_wall_through_its_centre_and_its_two_faces() {
    let (mut h, _) = with_inner_wall();
    let grips = |h: &Harness<BimApp>| h.get_all_by_label(WITNESS).count();
    assert_eq!(grips(&h), 2, "one on each side");
    // The first grip is on the left, the north side: the exterior face is nearer the north wall by half of 200 mm.
    h.get_all_by_label(WITNESS).next().unwrap().click();
    h.run();
    assert!(h.query_by_label("Temporary dimension 3900").is_some(), "to the exterior face");
    assert!(h.query_by_label("Temporary dimension 2000").is_some(), "the other side keeps its own");
    h.get_all_by_label(WITNESS).next().unwrap().click();
    h.run();
    assert!(h.query_by_label("Temporary dimension 4100").is_some(), "to the interior face");
    h.get_all_by_label(WITNESS).next().unwrap().click();
    h.run();
    assert!(h.query_by_label("Temporary dimension 4000").is_some(), "back to the centre line");
    // Typed from a face, the wall moves so that the face is that far off.
    h.get_all_by_label(WITNESS).next().unwrap().click();
    h.run();
    retype_dimension(&mut h, "3900", "3000");
    assert_eq!(start(&h, ids(&h, "wall")[4]), P2(1000.0, 2900.0));
}

#[test]
fn make_permanent_lays_the_temporary_dimension_as_a_dimension_of_the_plan() {
    let (mut h, inner) = with_inner_wall();
    assert!(ids(&h, "dimension").is_empty());
    h.get_all_by_label(PERMANENT).next().unwrap().click();
    h.run();
    let dims = ids(&h, "dimension");
    assert_eq!(dims.len(), 1);
    // From the inner wall to the north wall, as it showed.
    assert_eq!(field(&h, dims[0], "values"), json!([4000.0]));
    let guid = h.state().session.model.guid(inner);
    assert!(matches!(h.state().session.model.elements.get(dims[0]), Some(Element::Dimension(d)) if d.refs.iter().any(|r| r.id == guid)));
    // The wall is still the selection and shows its temporary dimensions.
    assert_eq!(h.state().selection, [inner]);
    assert!(h.query_by_label("Temporary dimension 2000").is_some());
}

#[test]
fn a_wall_with_no_wall_or_grid_beside_it_has_only_its_length() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}");
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection.len(), 1);
    assert!(h.query_by_label("Temporary dimension 4000").is_some());
    assert_eq!(h.query_all_by_label(WITNESS).count(), 0);
    assert_eq!(h.query_all_by_label(PERMANENT).count(), 0);
}

#[test]
fn a_grid_beside_a_wall_is_measured_to_like_a_wall() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}\ngrid {\"start\": [-500, 700], \"end\": [4500, 700]}");
    click(&mut h, P2(2000.0, 0.0));
    assert!(h.query_by_label("Temporary dimension 700").is_some());
    retype_dimension(&mut h, "700", "1200");
    assert_eq!(walls(&h)[0], (P2(0.0, -500.0), P2(4000.0, -500.0)));
}

#[test]
fn a_walls_temporary_dimensions_measure_to_what_the_plan_shows() {
    let (mut h, inner) = with_inner_wall();
    assert!(h.query_by_label("Temporary dimension 2000").is_some(), "to the south wall");
    // The south wall taken down in the first phase isn't in this plan, so it isn't measured to.
    let south = ids(&h, "wall")[0];
    h.state_mut()
        .session
        .execute("modify", &json!({"id": south, "params": {"Phase Created": "Existing", "Phase Demolished": "Existing"}}))
        .unwrap();
    h.run();
    assert_eq!(h.state().selection, [inner]);
    assert!(h.query_by_label("Temporary dimension 2000").is_none(), "the demolished wall is no neighbour");
    assert!(h.query_by_label("Temporary dimension 4000").is_some(), "the north wall still is");
}

/// Two "Exterior - 300mm" walls (brick, insulation, a 140 mm block core, plaster), the first on y = 0 and the second on y = 4000,
/// the first selected: its exterior face is 150 on its left, the core's lines 0 and 140 on the right of its centre line.
fn with_cores() -> (Harness<'static, BimApp>, Id) {
    let mut h = app(
        "wall {\"points\": [[0, 0], [6000, 0]], \"type\": \"Exterior - 300mm\"}\nwall {\"points\": [[0, 4000], [6000, 4000]], \"type\": \"Exterior - 300mm\"}",
    );
    let a = ids(&h, "wall")[0];
    click(&mut h, P2(3000.0, 0.0));
    assert_eq!(h.state().selection, [a]);
    (h, a)
}

#[test]
fn the_witness_dot_goes_on_through_the_cores_centre_and_faces_for_a_wall_with_a_core() {
    let (mut h, _) = with_cores();
    assert_eq!(h.query_all_by_label(WITNESS).count(), 1, "one side has a neighbour");
    // The centre line, the exterior face, the interior face, the core's centre, its exterior face, its interior face, round.
    for (n, shown) in ["4000", "3850", "4150", "4070", "4000", "4140", "4000"].into_iter().enumerate() {
        assert!(h.query_by_label(&format!("Temporary dimension {shown}")).is_some(), "stop {n} measures {shown}");
        h.get_by_label(WITNESS).click();
        h.run();
    }
}

#[test]
fn the_dot_at_the_other_end_goes_through_the_walls_lines_it_measures_to() {
    let (mut h, _) = with_cores();
    assert_eq!(h.query_all_by_label(FAR_WITNESS).count(), 1);
    // The other wall's centre line, then its exterior face (4150 from this wall's centre line), then its interior face.
    for shown in ["4150", "3850", "3930"] {
        h.get_by_label(FAR_WITNESS).click();
        h.run();
        assert!(h.query_by_label(&format!("Temporary dimension {shown}")).is_some(), "{shown}");
    }
}

#[test]
fn a_witness_line_dragged_lands_on_the_reference_it_is_let_go_on() {
    let (mut h, _) = with_cores();
    // The far dot stands a third of the way along, on the other wall's centre line; its interior face is 150 nearer.
    drag(&mut h, P2(2000.0, 4000.0), P2(2000.0, 3850.0));
    assert!(h.query_by_label("Temporary dimension 3850").is_some());
    assert_eq!(h.state().selection.len(), 1, "the drag moved the dot, not the selection");
    assert!(h.state().session.history().last() != Some(&"Move"), "no wall moved");
    // The dot at the wall, dragged to the wall's own interior face (150 on the other side of its centre line).
    drag(&mut h, P2(2000.0, 0.0), P2(2000.0, -150.0));
    assert!(h.query_by_label("Temporary dimension 4000").is_some(), "from y = -150 to y = 3850 is 4000");
}

#[test]
fn move_witness_line_in_the_dots_menu_takes_the_next_click_as_the_line() {
    let (mut h, a) = with_cores();
    let dot = h.get_by_label(FAR_WITNESS).rect().center();
    right_click_at(&mut h, dot);
    choose_item(&mut h, "Move Witness Line");
    assert!(matches!(h.state().tool, Tool::MoveWitness { .. }), "{:?}", h.state().tool);
    assert!(h.state().prompt().contains("witness line"), "the status bar says what to click");
    // The other wall's exterior face, nearest the click, is where it goes; the tool ends and the wall stays selected.
    click(&mut h, P2(2500.0, 4140.0));
    assert_eq!(h.state().tool, Tool::Modify);
    assert_eq!(h.state().selection, [a]);
    assert!(h.query_by_label("Temporary dimension 4150").is_some());
    // Esc leaves it as it was.
    let dot = h.get_by_label(FAR_WITNESS).rect().center();
    right_click_at(&mut h, dot);
    choose_item(&mut h, "Move Witness Line");
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.query_by_label("Temporary dimension 4150").is_some());
    // What a person chose is kept for the wall: select another wall and come back.
    let b = ids(&h, "wall")[1];
    click(&mut h, P2(3000.0, 4000.0));
    assert_eq!(h.state().selection, [b]);
    click(&mut h, P2(3000.0, 0.0));
    assert!(h.query_by_label("Temporary dimension 4150").is_some());
}

#[test]
fn manage_temporary_dimensions_sets_which_lines_the_dimensions_measure_and_the_project_keeps_it() {
    let (mut h, _) = with_cores();
    assert!(h.query_by_label("Temporary dimension 4000").is_some(), "centerlines to start with");
    annotation(&mut h, "Temporary Dimensions");
    assert!(h.state().temp_dim_dialog.is_some());
    pick(&mut h, Role::RadioButton, "Faces of Core");
    press(&mut h, "OK");
    assert!(h.state().temp_dim_dialog.is_none());
    assert_eq!(h.state().session.history().last(), Some(&"Temporary Dimension Properties"));
    assert_eq!(h.state().session.model.temporary_dimensions.walls.name(), "core-faces");
    // The core's own faces, toward each other: 0 to 3860.
    assert!(h.query_by_label("Temporary dimension 3860").is_some());
    // Cancel changes nothing.
    annotation(&mut h, "Temporary Dimensions");
    h.get_all_by_role_and_label(Role::RadioButton, "Centerlines").next().unwrap().click();
    h.run();
    press(&mut h, "Cancel");
    assert_eq!(h.state().session.model.temporary_dimensions.walls.name(), "core-faces");
}

#[test]
fn a_doors_dimensions_stop_at_its_jambs_when_the_project_says_openings() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    assert!(h.query_by_label("Temporary dimension 1900").is_some() && h.query_by_label("Temporary dimension 5900").is_some());
    let half = {
        let m = &h.state().session.model;
        m.opening_type(m.elements.get(door).unwrap()).unwrap().width / 2.0
    };
    annotation(&mut h, "Temporary Dimensions");
    pick(&mut h, Role::RadioButton, "Openings");
    press(&mut h, "OK");
    let shown = |n: f64| format!("Temporary dimension {}", n);
    assert!(h.query_by_label(&shown(1900.0 - half)).is_some(), "to the near jamb");
    assert!(h.query_by_label(&shown(5900.0 - half)).is_some(), "from the far jamb");
    // Typed, the jamb is that far from the face: the door's middle is the jamb's half a width on.
    retype_dimension(&mut h, &(1900.0 - half).to_string(), "2000");
    assert_eq!(field(&h, door, "offset"), json!(100.0 + 2000.0 + half));
}
