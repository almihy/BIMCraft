//! The Wall Joins tool, as Revit's: click the square over a join of two walls, then its kind, its order and whether its
//! ends join are the options'.
use super::*;
use bimcraft_core::model::{JoinDisplay, JoinKind};
use egui_kittest::kittest::NodeT;

/// The two walls of BOX that meet at (8000, 0): the south wall and the east one.
fn corner(h: &Harness<BimApp>) -> (Id, Id) {
    let w = ids(h, "wall");
    (w[0], w[1])
}

fn plan_area(h: &Harness<BimApp>, id: Id) -> f64 {
    bimcraft_core::geom::area(&bimcraft_core::walls::outline(&h.state().session.model, id)).abs()
}

#[test]
fn the_wall_joins_tool_picks_a_join_and_changes_its_kind_and_order() {
    let mut h = app(BOX);
    let (south, east) = corner(&h);
    // The default Butt, the wall made first through: the south wall runs 100 past each of its corners.
    assert_eq!(plan_area(&h, south), 8200.0 * 200.0);
    topmost(&mut h, "Modify");
    press(&mut h, "Wall Joins");
    assert!(matches!(h.state().tool, Tool::WallJoins(None)));
    // No join picked, no options yet; a click on the south wall near its east end picks the corner there.
    assert!(h.query_by_label("Miter").is_none());
    click(&mut h, P2(7800.0, 0.0));
    assert!(matches!(h.state().tool, Tool::WallJoins(Some(j)) if j == (south, east)), "the join of the two walls");
    for label in ["Butt", "Miter", "Square off", "Previous", "Next", "Disallow Join"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // Next: the other wall runs through. One undo step, and the join stays picked.
    press(&mut h, "Next");
    assert_eq!(
        plan_area(&h, south),
        8000.0 * 200.0,
        "the east wall runs through that corner now: 100 past the west, 100 short of the east"
    );
    assert!(matches!(h.state().tool, Tool::WallJoins(Some(_))));
    // A Mitre: the two cut on the corner's diagonal; it has one order, so Previous and Next are off.
    press(&mut h, "Miter");
    let kept = h.state().session.model.wall_joins.clone();
    assert_eq!(kept.len(), 1);
    assert_eq!((kept[0].kind, kept[0].display), (JoinKind::Mitre, JoinDisplay::ViewSetting));
    assert!(h.get_by_label("Next").accesskit_node().is_disabled());
    key(&mut h, Key::Escape);
    assert!(!matches!(h.state().tool, Tool::WallJoins(_)), "Escape leaves the tool");
}

#[test]
fn disallow_join_frees_both_ends_of_the_join() {
    let mut h = app(BOX);
    let (south, east) = corner(&h);
    topmost(&mut h, "Modify");
    press(&mut h, "Wall Joins");
    click(&mut h, P2(7800.0, 0.0));
    press(&mut h, "Disallow Join");
    let m = &h.state().session.model;
    let (s, e) = (m.wall(south).unwrap(), m.wall(east).unwrap());
    assert!(!s.join_end && s.join_start && !e.join_start && e.join_end, "the two ends at the corner, and only they");
    // Each stands square at the node; the south wall's other corner joins still.
    assert_eq!(plan_area(&h, south), 8100.0 * 200.0);
    assert!(matches!(h.state().tool, Tool::WallJoins(None)), "no join is left there to have picked");
}

#[test]
fn disallow_join_on_a_walls_end_grip_is_that_ends_alone() {
    let mut h = app(BOX);
    let south = ids(&h, "wall")[0];
    click(&mut h, P2(4000.0, 0.0));
    // The right click on its east end: that end refuses, its west end joins on.
    let east = h.state().screen(P2(8000.0, 0.0));
    assert_eq!(offered(&mut h, east, &["Disallow Join", "Allow Join"]), ["Disallow Join"]);
    choose_item(&mut h, "Disallow Join");
    let w = h.state().session.model.wall(south).cloned().unwrap();
    assert!(w.join && w.join_start && !w.join_end, "{w:?}");
    assert_eq!(plan_area(&h, south), 8100.0 * 200.0, "square at the east, through at the west");
    // That end now offers Allow Join, the other still Disallow Join.
    assert_eq!(offered(&mut h, east, &["Disallow Join", "Allow Join"]), ["Allow Join"]);
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 0.0));
    let west = h.state().screen(P2(0.0, 0.0));
    assert_eq!(offered(&mut h, west, &["Disallow Join", "Allow Join"]), ["Disallow Join"]);
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(offered(&mut h, east, &["Disallow Join", "Allow Join"]), ["Allow Join"]);
    choose_item(&mut h, "Allow Join");
    assert!(h.state().session.model.wall(south).is_some_and(|w| w.join_end));
}

#[test]
fn the_wall_joins_tool_picks_a_t_join_from_either_wall_and_sets_only_how_it_shows() {
    // A wall up from the middle of the south wall: its start ends on the south wall's side.
    let mut h = app(&format!(
        "{BOX}
wall {{\"points\": [[4000, 0], [4000, 3000]]}}"
    ));
    let south = ids(&h, "wall")[0];
    let stem = *ids(&h, "wall").last().unwrap();
    topmost(&mut h, "Modify");
    press(&mut h, "Wall Joins");
    // A click on the wall that ends there, near that end.
    click(&mut h, P2(4000.0, 400.0));
    assert!(matches!(h.state().tool, Tool::WallJoins(Some(j)) if j == (stem, south)), "{:?}", h.state().tool);
    // A T join is a butt, the wall run into through: nothing else to choose and no order to step.
    for label in ["Miter", "Square off", "Previous", "Next"] {
        assert!(h.get_by_label(label).accesskit_node().is_disabled(), "{label}");
    }
    key(&mut h, Key::Escape);
    topmost(&mut h, "Modify");
    press(&mut h, "Wall Joins");
    // A click on the wall run into, beside where the other ends on it: the same join.
    click(&mut h, P2(4300.0, 0.0));
    assert!(matches!(h.state().tool, Tool::WallJoins(Some(j)) if j == (stem, south)), "{:?}", h.state().tool);
    // How it shows is kept with the model.
    pick(&mut h, Role::ComboBox, "Display:");
    press(&mut h, "Don't Clean Join");
    let m = &h.state().session.model;
    assert_eq!(m.tee_join((m.guid(stem), false), m.guid(south)).map(|j| j.display), Some(JoinDisplay::DontClean));
    // Disallow Join frees the end that ends there, and only it.
    press(&mut h, "Disallow Join");
    let m = &h.state().session.model;
    assert!(!m.wall(stem).unwrap().join_start && m.wall(stem).unwrap().join_end);
    assert!(m.wall(south).unwrap().join_start && m.wall(south).unwrap().join_end);
    assert!(matches!(h.state().tool, Tool::WallJoins(None)));
}
