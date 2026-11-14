//! A door family's leaf in plan, as Revit's door families draw it: the leaf built shut in its opening is drawn in no plan
//! (its Visibility Settings), and the plan draws it open, a panel as thick as the leaf square to the wall, with its swing,
//! at every detail level. The built-in doors are drawn so; a family made before Visibility Settings is given them as its
//! project is read.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Id, Model};
use crate::prim::Prim;
use crate::render::{self, Drawing};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A 200 mm wall along x from 0 to 8000, its exterior to +y.
fn wall(s: &mut Session) {
    run(s, "wall", json!({"points": [[0, 0], [8000, 0]]}));
}

fn plan_at(s: &mut Session, detail: &str) -> Drawing {
    run(s, "detail", json!({"view": 100000, "detail": detail}));
    render::plan(&s.model, 100000, 4000, 3000)
}

/// What `id` draws, in model mm: its outlined (closed) lines, and its open ones.
fn lines(d: &Drawing, id: Id) -> (Vec<Vec<P2>>, Vec<Vec<P2>>) {
    let (mut closed, mut open) = (Vec::new(), Vec::new());
    for (p, o) in d.prims.iter().zip(&d.owners) {
        if let Prim::Line { pts, closed: c, .. } = p
            && *o == Some(id)
        {
            let pts: Vec<P2> = pts.iter().map(|q| d.view.mm(*q)).collect();
            if *c { closed.push(pts) } else { open.push(pts) }
        }
    }
    (closed, open)
}

/// A ring's box: its width along x and its depth along y.
fn size(ring: &[P2]) -> (f64, f64) {
    let (xs, ys) = (ring.iter().map(|p| p.0), ring.iter().map(|p| p.1));
    let span = |v: Vec<f64>| v.iter().copied().fold(f64::NEG_INFINITY, f64::max) - v.iter().copied().fold(f64::INFINITY, f64::min);
    (span(xs.collect()), span(ys.collect()))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0
}

/// The open leaves `id` draws: rings of five points (the panel round, back to its first) and their sizes.
fn panels(open: &[Vec<P2>]) -> Vec<(f64, f64)> {
    open.iter().filter(|l| l.len() == 5 && l[0].dist(l[4]) < 0.01).map(|l| size(l)).collect()
}

/// The swings `id` draws: open lines of more than five points.
fn swings(open: &[Vec<P2>]) -> usize {
    open.iter().filter(|l| l.len() > 5).count()
}

#[test]
fn the_built_in_single_door_draws_its_leaf_open_and_not_shut_at_every_detail_level() {
    let mut s = Session::new();
    wall(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Single Door", "type": "900 x 2100"})));
    for detail in ["coarse", "medium", "fine"] {
        let d = plan_at(&mut s, detail);
        let (closed, open) = lines(&d, door);
        // The leaf: 40 thick, the opening less its jambs (900 - 2 x 50) long, square to the wall (along y).
        let leaves = panels(&open);
        assert_eq!(leaves.len(), 1, "{detail}: one open leaf: {open:?}");
        assert!(near(leaves[0].0, 40.0) && near(leaves[0].1, 800.0), "{detail}: a 40 x 800 panel square to the wall: {leaves:?}");
        assert_eq!(swings(&open), 1, "{detail}: its swing");
        // Nothing drawn shut across the opening: what is cut is its two jambs, at every level (they say nothing of their
        // levels, so Revit's default draws them at Coarse too; its handles say Coarse and Top/Bottom off).
        assert!(closed.iter().all(|r| size(r).0 < 100.0), "{detail}: no leaf shut in the wall: {closed:?}");
        assert_eq!(closed.len(), 2, "{detail}: its jambs cut");
    }
}

#[test]
fn the_built_in_double_and_sliding_doors_draw_their_leaves_as_revits_do() {
    let mut s = Session::new();
    wall(&mut s);
    let double = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Double Door", "type": "1500 x 2100"})));
    let sliding = id_of(&run(&mut s, "door", json!({"at": [5500, 0], "family": "Sliding Door", "type": "2000 x 2100"})));
    for detail in ["coarse", "medium", "fine"] {
        let d = plan_at(&mut s, detail);
        let (closed, open) = lines(&d, double);
        let leaves = panels(&open);
        assert_eq!(leaves.len(), 2, "{detail}: two leaves open");
        assert!(leaves.iter().all(|(w, l)| near(*w, 40.0) && near(*l, 700.0)), "{detail}: each 40 x 700: {leaves:?}");
        assert_eq!(swings(&open), 2, "{detail}: two swings");
        assert!(closed.iter().all(|r| size(r).0 < 100.0), "{detail}: no leaf shut in the wall");
        // A sliding door's two leaves lie in the wall, on their tracks, each a panel along it.
        let (closed, open) = lines(&d, sliding);
        let leaves = panels(&open);
        assert_eq!(leaves.len(), 2, "{detail}: two leaves on their tracks: {open:?}");
        assert!(leaves.iter().all(|(w, t)| near(*w, 975.0) && near(*t, 40.0)), "{detail}: each 975 x 40 along the wall: {leaves:?}");
        assert_eq!(swings(&open), 0, "{detail}");
        assert!(closed.iter().all(|r| size(r).0 < 100.0), "{detail}: the leaves drawn once, by the symbols");
    }
}

/// A door family as one was made before Visibility Settings, as an agent made one: jambs 40 wide, a 40 mm leaf shut across
/// the opening between them, a kick plate low down, and its plan a single line for the open leaf from the middle of the
/// wall and the arc of its swing.
fn old_family(s: &mut Session) -> Value {
    run(
        s,
        "family",
        json!({
            "name": "Room door",
            "category": "door",
            "operation": "swing",
            "parameters": [{"name": "Width", "value": 926}, {"name": "Height", "value": 2100}],
            "types": {"926x2100": {}},
            "forms": [
                {"profile": [["-Width / 2 + 40", -20], ["Width / 2 - 40", -20], ["Width / 2 - 40", 20], ["-Width / 2 + 40", 20]], "from": 5, "to": "Height - 40"},
                {"profile": [["-Width / 2", "-Wall Thickness / 2"], ["-Width / 2 + 40", "-Wall Thickness / 2"], ["-Width / 2 + 40", "Wall Thickness / 2"], ["-Width / 2", "Wall Thickness / 2"]], "from": 0, "to": "Height"},
                {"profile": [["Width / 2 - 40", "-Wall Thickness / 2"], ["Width / 2", "-Wall Thickness / 2"], ["Width / 2", "Wall Thickness / 2"], ["Width / 2 - 40", "Wall Thickness / 2"]], "from": 0, "to": "Height"},
                {"profile": [["-Width / 2 + 60", 20], ["Width / 2 - 60", 20], ["Width / 2 - 60", 22], ["-Width / 2 + 60", 22]], "from": 20, "to": 220}
            ],
            "symbols": [
                {"line": [["-Width / 2 + 40", 0], ["-Width / 2 + 40", "Width - 80"]]},
                {"arc": {"centre": ["-Width / 2 + 40", 0], "radius": "Width - 80", "from": 0, "to": 90}}
            ]
        }),
    )
}

#[test]
fn a_door_family_drawing_its_leaf_shut_beside_its_swing_is_warned_of() {
    let mut s = Session::new();
    let r = old_family(&mut s);
    let warnings = r["warnings"].to_string();
    assert!(warnings.contains("form 1: a door's leaf, shut in its opening, is drawn in plan"), "{warnings}");
    // Its jambs and kick plate are no leaf; the built-in doors draw theirs open, and are not warned of.
    assert!(!warnings.contains("form 2:") && !warnings.contains("form 4:"), "{warnings}");
    for name in ["Single Door", "Double Door", "Sliding Door"] {
        let r = run(&mut s, "family", json!({"name": name}));
        assert!(!r["warnings"].to_string().contains("shut in its opening"), "{name}: {}", r["warnings"]);
    }
}

#[test]
fn a_door_family_from_before_visibility_settings_draws_its_leaf_open_once_its_project_is_read() {
    let mut s = Session::new();
    wall(&mut s);
    old_family(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Room door"})));
    // As a build of format 21 drew it: the leaf shut in the wall, outlined, and a line for the open leaf.
    let before = plan_at(&mut s, "medium");
    let (closed, open) = lines(&before, door);
    assert!(closed.iter().any(|r| size(r).0 > 800.0), "the leaf shut across the opening: {closed:?}");
    assert!(panels(&open).is_empty());
    // Its project, as a file of 21.
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(21);
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let f = m.families.values().find(|f| f.symbols.len() == 2 && f.forms.len() == 4).unwrap();
    assert_eq!(f.forms[0].show.map(|x| (x.plan, x.coarse, x.fine)), Some((false, true, true)), "the leaf, in no plan");
    assert!(f.forms[1..].iter().all(|x| x.show.is_none()), "its jambs and kick plate as they were");
    assert_eq!(f.symbols[0].line.len(), 5, "the open leaf a panel");
    s.model = Arc::new(m);
    for detail in ["coarse", "medium", "fine"] {
        let d = plan_at(&mut s, detail);
        let (closed, open) = lines(&d, door);
        // The kick plate, low down and below the cut, is outlined seen from above (a form that says nothing is drawn at every
        // level); the leaf, 846 by 40, is not.
        assert!(!closed.iter().any(|r| size(r).0 > 800.0 && size(r).1 > 30.0), "{detail}: no leaf shut in the wall: {closed:?}");
        let leaves = panels(&open);
        // The leaf's 40 mm, on the side it shuts to, as long as the arc's radius (926 - 80).
        assert_eq!(leaves.len(), 1, "{detail}: {open:?}");
        assert!(near(leaves[0].0, 40.0) && near(leaves[0].1, 846.0), "{detail}: {leaves:?}");
        assert_eq!(swings(&open), 1, "{detail}");
    }
    // The leaf lies inside the swing, from the hinge toward the jamb it shuts to.
    let d = plan_at(&mut s, "fine");
    let (_, open) = lines(&d, door);
    let panel = open.iter().find(|l| l.len() == 5).unwrap();
    let xs: Vec<f64> = panel.iter().map(|p| p.0).collect();
    let (lo, hi) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let hinge = 2000.0 - 926.0 / 2.0 + 40.0;
    assert!(near(lo, hinge) && near(hi, hinge + 40.0), "{lo}..{hi}");
    // Written again it says so (22), and read again it is as it was.
    let again = s.model.to_json().unwrap();
    assert!(again.contains(&format!("\"format\": {}", crate::model::LEAF_FORMAT)));
    let (m2, _) = Model::read(&again).unwrap();
    assert_eq!(m2.families, s.model.families, "read once, changed once");
}

#[test]
fn a_project_is_written_as_format_22_only_when_it_places_a_door_whose_family_says_which_views_draw_it() {
    let mut s = Session::new();
    wall(&mut s);
    let format = |s: &Session| serde_json::from_str::<Value>(&s.model.to_json().unwrap()).unwrap()["format"].clone();
    // The built-in doors say it, but a project that places none of them draws nothing of theirs: an older build opens it.
    let own = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "type": "Single 900 x 2100"})));
    assert_eq!(format(&s), json!(crate::model::PREVIOUS_FORMAT));
    let door = id_of(&run(&mut s, "door", json!({"at": [5000, 0], "family": "Single Door"})));
    assert_eq!(format(&s), json!(crate::model::LEAF_FORMAT), "a build of 21 would draw its leaf shut in the wall");
    run(&mut s, "delete", json!({"ids": [door, own]}));
    assert_eq!(format(&s), json!(crate::model::PREVIOUS_FORMAT));
}

#[test]
fn the_family_command_sets_a_forms_and_a_symbols_visibility_settings_by_id() {
    use crate::family::{Levels, Show};
    let mut s = Session::new();
    wall(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Single Door", "type": "900 x 2100"})));
    let f = &s.model.families["Single Door"];
    let leaf = f.forms.iter().position(|x| x.show.is_some_and(|v| !v.plan)).expect("the leaf");
    let (form, symbol) = (f.forms[leaf].id, f.symbols[0].guid);
    let shut = |s: &mut Session| lines(&plan_at(s, "medium"), door).0.len();
    let before = shut(&mut s);
    // `show` merges into what the form says: Plan/RCP ticked, the rest as it was.
    run(&mut s, "family", json!({"name": "Single Door", "form": {"id": form, "show": {"plan": true, "coarse": false}}}));
    assert_eq!(s.model.families["Single Door"].forms[leaf].show, Some(Show { coarse: false, ..Show::default() }));
    assert_eq!(shut(&mut s), before + 1, "the plan draws the leaf cut, shut in the opening");
    run(&mut s, "family", json!({"name": "Single Door", "symbol": {"guid": symbol, "show": {"fine": false}}}));
    assert_eq!(s.model.families["Single Door"].symbols[0].show, Some(Levels { fine: false, ..Levels::default() }));
    // A null takes them away: drawn as a form or a symbol that says nothing is.
    run(&mut s, "family", json!({"name": "Single Door", "form": {"id": form, "show": null}}));
    assert_eq!(s.model.families["Single Door"].forms[leaf].show, None);
    // And a misspelled one is an error.
    assert!(s.execute("family", &json!({"name": "Single Door", "form": {"id": form, "show": {"plann": true}}})).is_err());
}

#[test]
fn a_door_family_form_saying_nothing_is_drawn_in_a_coarse_plan_and_in_3d_as_revits_default() {
    use crate::graphics::Look;
    let mut s = Session::new();
    wall(&mut s);
    // One jamb, 50 by 200 in plan, saying nothing of the views and detail levels that draw it.
    run(
        &mut s,
        "family",
        json!({
            "name": "Plain door",
            "category": "door",
            "operation": "swing",
            "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 2100}],
            "types": {"900x2100": {}},
            "forms": [{"profile": [["-Width / 2", -100], ["-Width / 2 + 50", -100], ["-Width / 2 + 50", 100], ["-Width / 2", 100]], "from": 0, "to": "Height"}]
        }),
    );
    assert_eq!(s.model.families["Plain door"].forms[0].show, None);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Plain door"})));
    // Revit's default is every view at every level: the plan outlines the jamb where it cuts, at Coarse too (the built-in
    // doors say their forms are not drawn at Coarse, and are drawn so).
    for detail in ["coarse", "medium", "fine"] {
        let (closed, _) = lines(&plan_at(&mut s, detail), door);
        assert_eq!(closed.iter().filter(|r| near(size(r).0, 50.0) && near(size(r).1, 200.0)).count(), 1, "{detail}: {closed:?}");
    }
    // And a 3D view at Coarse draws it.
    let view = run(&mut s, "detail", json!({"view": "{3D}", "detail": "coarse"}))["view"].as_u64().unwrap();
    let scene = crate::view3d::Scene::new(&s.model);
    let look = Look::of(&s.model, view);
    assert!(scene.tris.iter().any(|t| t.owner == door && look.draws(t)), "the jamb in 3D at Coarse");
}

#[test]
fn a_project_is_written_as_22_for_a_component_whose_form_says_its_views_23_for_per_type_forms_and_24_for_a_parametric_family_25_for_styles()
{
    use crate::model::{FORMAT, LEAF_FORMAT, PARAMETRIC_FORMAT, PREVIOUS_FORMAT, TYPES_FORMAT};
    assert_eq!((LEAF_FORMAT, TYPES_FORMAT, PARAMETRIC_FORMAT, FORMAT), (22, 23, 24, 25));
    let mut s = Session::new();
    let format = |s: &Session| serde_json::from_str::<Value>(&s.model.to_json().unwrap()).unwrap()["format"].clone();
    // A family whose form says which views draw it, placed nowhere: nothing a build of 21 would misread is drawn.
    run(
        &mut s,
        "family",
        json!({"name": "Plinth", "category": "furniture", "types": {"A": {}},
               "forms": [{"profile": [[0, 0], [100, 0], [100, 100]], "to": 100, "show": {"side": false}}]}),
    );
    assert_eq!(format(&s), json!(PREVIOUS_FORMAT));
    let c = id_of(&run(&mut s, "component", json!({"family": "Plinth", "type": "A", "at": [0, 0]})));
    assert_eq!(format(&s), json!(LEAF_FORMAT), "a component of a family whose form says its views");
    // A form of some types only is per-type: 23, which holds what 22 does.
    run(
        &mut s,
        "family",
        json!({"name": "Plinth", "forms": [{"profile": [[0, 0], [100, 0], [100, 100]], "to": 100, "show": {"side": false}, "types": ["A"]}]}),
    );
    assert_eq!(format(&s), json!(TYPES_FORMAT));
    // A family that stays parametric is 24, over both.
    run(&mut s, "family", json!({"name": "Plinth", "references": [{"name": "Base", "axis": "z", "at": 0}]}));
    assert_eq!(format(&s), json!(PARAMETRIC_FORMAT));
    run(&mut s, "delete", json!({"ids": [c]}));
    assert_eq!(format(&s), json!(PARAMETRIC_FORMAT), "parametric whether or not it is placed");
}
