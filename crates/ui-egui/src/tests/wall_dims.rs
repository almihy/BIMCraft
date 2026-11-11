//! A selected wall's temporary dimensions to the walls and grids beside it: typed to move the wall, the witness line's grip,
//! and the icon that makes one permanent.

use super::*;

const WITNESS: &str = "Witness line: centre line, exterior face or interior face";
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
    h.get_all_by_label(WITNESS).next().map(|g| g.click());
    h.run();
    assert!(h.query_by_label("Temporary dimension 3900").is_some(), "to the exterior face");
    assert!(h.query_by_label("Temporary dimension 2000").is_some(), "the other side keeps its own");
    h.get_all_by_label(WITNESS).next().map(|g| g.click());
    h.run();
    assert!(h.query_by_label("Temporary dimension 4100").is_some(), "to the interior face");
    h.get_all_by_label(WITNESS).next().map(|g| g.click());
    h.run();
    assert!(h.query_by_label("Temporary dimension 4000").is_some(), "back to the centre line");
    // Typed from a face, the wall moves so that the face is that far off.
    h.get_all_by_label(WITNESS).next().map(|g| g.click());
    h.run();
    retype_dimension(&mut h, "3900", "3000");
    assert_eq!(start(&h, ids(&h, "wall")[4]), P2(1000.0, 2900.0));
}

#[test]
fn make_permanent_lays_the_temporary_dimension_as_a_dimension_of_the_plan() {
    let (mut h, inner) = with_inner_wall();
    assert!(ids(&h, "dimension").is_empty());
    h.get_all_by_label(PERMANENT).next().map(|g| g.click());
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
    assert_eq!(h.get_all_by_label(WITNESS).count(), 0);
    assert_eq!(h.get_all_by_label(PERMANENT).count(), 0);
}

#[test]
fn a_grid_beside_a_wall_is_measured_to_like_a_wall() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}\ngrid {\"start\": [-500, 700], \"end\": [4500, 700]}");
    click(&mut h, P2(2000.0, 0.0));
    assert!(h.query_by_label("Temporary dimension 700").is_some());
    retype_dimension(&mut h, "700", "1200");
    assert_eq!(walls(&h)[0], (P2(0.0, -500.0), P2(4000.0, -500.0)));
}
