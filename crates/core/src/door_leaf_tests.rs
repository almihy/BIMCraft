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
    assert!(f.forms[1..3].iter().all(|x| x.show.is_none()), "its jambs as they were");
    let plate = f.forms[3].show.expect("its kick plate, fixed to the leaf, goes with it");
    assert!(!plate.plan && !plate.plan_top && !plate.plan_cut && plate.front && plate.side, "in no plan, still drawn in 3D: {plate:?}");
    assert_eq!(f.symbols[0].line.len(), 5, "the open leaf a panel");
    s.model = Arc::new(m);
    for detail in ["coarse", "medium", "fine"] {
        let d = plan_at(&mut s, detail);
        let (closed, open) = lines(&d, door);
        // The kick plate, fixed to the leaf, goes with it: nothing is drawn across the opening but its jambs, the leaf's 846 by
        // 40 and the plate's 806 by 2 alike.
        assert!(!closed.iter().any(|r| size(r).0 > 100.0), "{detail}: no leaf, no kick plate shut in the wall: {closed:?}");
        assert_eq!(closed.len(), 2, "{detail}: its jambs cut: {closed:?}");
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
    use crate::model::{FORMAT, LEAF_FORMAT, PARAMETRIC_FORMAT, PREVIOUS_FORMAT, STYLES_FORMAT, TYPES_FORMAT};
    assert_eq!((LEAF_FORMAT, TYPES_FORMAT, PARAMETRIC_FORMAT, STYLES_FORMAT, FORMAT), (22, 23, 24, 25, 30));
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

// Parts fixed to the leaf: the kick plates, levers and roses of a door whose leaf is hidden in plan go with it.

/// The live project's "Room door with lever", an agent's: a 40 mm leaf (form 1) that says `plan: false`, two jambs and a head
/// (forms 2 to 4) saying nothing, two roses and four lever pieces around a metre up (forms 5 to 10) and a kick plate on each face of
/// the leaf (forms 11 and 12), 2 mm thick, which say nothing either. Its ids, GUIDs and material ids are left out.
const ROOM_DOOR: &str = r#"{"category":"door","parameters":[{"name":"Width","kind":"length","value":926},{"name":"Height","kind":"length","value":2100}],"types":{"650x2100":{"Height":2100,"Width":650},"826x2100":{"Height":2100,"Width":826},"926x2100":{"Height":2100,"Width":926}},"forms":[{"plane":"xy","profile":[["-Width / 2 + 40",-20.0],["Width / 2 - 40",-20.0],["Width / 2 - 40",20.0],["-Width / 2 + 40",20.0]],"from":5.0,"to":"Height - 40","show":{"plan":false}},{"plane":"xy","profile":[["-Width / 2","-Wall Thickness / 2 - 12"],["-Width / 2 + 40","-Wall Thickness / 2 - 12"],["-Width / 2 + 40","Wall Thickness / 2 + 12"],["-Width / 2","Wall Thickness / 2 + 12"]],"from":0.0,"to":"Height"},{"plane":"xy","profile":[["Width / 2 - 40","-Wall Thickness / 2 - 12"],["Width / 2","-Wall Thickness / 2 - 12"],["Width / 2","Wall Thickness / 2 + 12"],["Width / 2 - 40","Wall Thickness / 2 + 12"]],"from":0.0,"to":"Height"},{"plane":"xy","profile":[["-Width / 2","-Wall Thickness / 2 - 12"],["Width / 2","-Wall Thickness / 2 - 12"],["Width / 2","Wall Thickness / 2 + 12"],["-Width / 2","Wall Thickness / 2 + 12"]],"from":"Height - 40","to":"Height"},{"plane":"xz","circle":{"centre":["Width / 2 - 110",1000.0],"radius":26.0},"from":20.0,"to":30.0},{"plane":"xz","circle":{"centre":["Width / 2 - 110",1000.0],"radius":26.0},"from":-30.0,"to":-20.0},{"plane":"xy","profile":[["Width / 2 - 250",50.0],["Width / 2 - 100",50.0],["Width / 2 - 100",68.0],["Width / 2 - 250",68.0]],"from":990.0,"to":1010.0},{"plane":"xy","profile":[["Width / 2 - 120",30.0],["Width / 2 - 100",30.0],["Width / 2 - 100",68.0],["Width / 2 - 120",68.0]],"from":990.0,"to":1010.0},{"plane":"xy","profile":[["Width / 2 - 250",-68.0],["Width / 2 - 100",-68.0],["Width / 2 - 100",-50.0],["Width / 2 - 250",-50.0]],"from":990.0,"to":1010.0},{"plane":"xy","profile":[["Width / 2 - 120",-68.0],["Width / 2 - 100",-68.0],["Width / 2 - 100",-30.0],["Width / 2 - 120",-30.0]],"from":990.0,"to":1010.0},{"plane":"xy","profile":[["-Width / 2 + 60",20.0],["Width / 2 - 60",20.0],["Width / 2 - 60",22.0],["-Width / 2 + 60",22.0]],"from":20.0,"to":220.0},{"plane":"xy","profile":[["-Width / 2 + 60",-22.0],["Width / 2 - 60",-22.0],["Width / 2 - 60",-20.0],["-Width / 2 + 60",-20.0]],"from":20.0,"to":220.0}],"symbols":[{"line":[["-Width / 2 + 40",0.0],["-Width / 2 + 40","Width - 80"],["(-Width / 2 + 40) + 40","Width - 80"],["(-Width / 2 + 40) + 40",0.0],["-Width / 2 + 40",0.0]],"show":{}},{"arc":{"centre":["-Width / 2 + 40",0.0],"radius":"Width - 80","from":0.0,"to":90.0},"show":{}}],"operation":"swing"}"#;

/// The live project's "Cubicle door Venesta Infinite": a full-height leaf from 100 mm up (form 1, no plan) with its hinge, lock,
/// indicator and hook on it (forms 2 to 8), and no frame. Its ids, GUIDs and material ids are left out.
const CUBICLE_DOOR: &str = r#"{"category":"door","parameters":[{"name":"Width","kind":"length","value":650},{"name":"Height","kind":"length","value":2350}],"types":{"650 full height":{"Height":2350,"Width":650}},"forms":[{"plane":"xy","profile":[["-Width / 2 + 3",-12.5],["Width / 2 - 3",-12.5],["Width / 2 - 3",12.5],["-Width / 2 + 3",12.5]],"from":100.0,"to":"Height","show":{"plan":false}},{"plane":"xz","circle":{"centre":["Width / 2 - 60",1050.0],"radius":25.0},"from":12.5,"to":20.0},{"plane":"xz","circle":{"centre":["Width / 2 - 60",1050.0],"radius":10.0},"from":20.0,"to":40.0},{"plane":"xz","circle":{"centre":["Width / 2 - 60",1050.0],"radius":25.0},"from":-20.0,"to":-12.5},{"plane":"xy","profile":[["Width / 2 - 80",-36.0],["Width / 2 - 40",-36.0],["Width / 2 - 40",-20.0],["Width / 2 - 80",-20.0]],"from":1040.0,"to":1060.0},{"plane":"xz","profile":[["Width / 2 - 75",1130.0],["Width / 2 - 45",1130.0],["Width / 2 - 45",1150.0],["Width / 2 - 75",1150.0]],"from":12.5,"to":16.0},{"plane":"xz","profile":[[-15.0,1650.0],[15.0,1650.0],[15.0,1680.0],[-15.0,1680.0]],"from":-20.0,"to":-12.5},{"plane":"xy","profile":[[-8.0,-70.0],[8.0,-70.0],[8.0,-20.0],[-8.0,-20.0]],"from":1660.0,"to":1676.0}],"symbols":[{"line":[["-Width / 2",0.0],["-Width / 2","Width"],["(-Width / 2) + 25","Width"],["(-Width / 2) + 25",0.0],["-Width / 2",0.0]],"show":{}},{"arc":{"centre":["-Width / 2",0.0],"radius":"Width","from":0.0,"to":90.0},"show":{}}],"operation":"swing"}"#;

/// A project as the live one is: a wall and a door of the family as it was made, before any of this was read.
fn live(name: &str, family: &str, ty: &str, change: impl Fn(&mut crate::family::Family)) -> (Session, Id) {
    let mut s = Session::new();
    wall(&mut s);
    let mut f: crate::family::Family = serde_json::from_str(family).unwrap();
    change(&mut f);
    s.model_mut().families.insert(name.to_string(), f);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": name, "type": ty})));
    (s, door)
}

/// The project written, as format 22 as the live one is, and read back.
fn read_again(s: &mut Session) {
    let text = s.model.to_json().unwrap();
    assert!(text.contains(&format!("\"format\": {}", crate::model::LEAF_FORMAT)), "a project of format 22");
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    s.model = Arc::new(m);
}

/// The forms a plan draws none of (Plan/RCP off), by their place.
fn hidden_in_plan(s: &Session, family: &str) -> Vec<usize> {
    let f = &s.model.families[family];
    (0..f.forms.len()).filter(|i| f.forms[*i].show.is_some_and(|v| !v.plan)).collect()
}

/// How many triangles a 3D view at Fine draws of `door`.
fn drawn_in_3d(s: &mut Session, door: Id) -> usize {
    let view = run(s, "detail", json!({"view": "{3D}", "detail": "fine"}))["view"].as_u64().unwrap();
    let scene = crate::view3d::Scene::new(&s.model);
    let look = crate::graphics::Look::of(&s.model, view);
    scene.tris.iter().filter(|t| t.owner == door && look.draws(t)).count()
}

/// What a plan of `door` draws at `detail`, across the opening: only the rings its frame cuts are outlined.
fn only_its_jambs(d: &Drawing, door: Id, detail: &str, jambs: usize) {
    let (closed, _) = lines(d, door);
    assert!(closed.iter().all(|r| size(r).0 < 100.0), "{detail}: nothing shut across the opening: {closed:?}");
    assert_eq!(closed.len(), jambs, "{detail}: {closed:?}");
}

#[test]
fn a_family_in_a_file_of_22_gets_its_kick_plates_levers_and_roses_hidden_in_plan_with_its_leaf() {
    use crate::family::Show;
    let name = "Room door with lever";
    let (mut s, door) = live(name, ROOM_DOOR, "926x2100", |_| {});
    // As the live project draws it: the leaf is hidden, but its kick plates (806 by 2, a leaf's thickness apart), roses and
    // levers are outlined seen from above, shut in the opening.
    let (closed, _) = lines(&plan_at(&mut s, "medium"), door);
    assert!(closed.iter().filter(|r| size(r).0 > 700.0).count() >= 2, "two kick plates across the opening: {closed:?}");
    assert!(closed.len() > 6, "and handle marks: {closed:?}");
    let in_3d = drawn_in_3d(&mut s, door);
    assert_eq!(hidden_in_plan(&s, name), vec![0], "only the leaf says so");
    read_again(&mut s);
    // The leaf, its two roses, four lever pieces and two kick plates (forms 1, 5 to 12) say no plan; its frame (forms 2 to 4) is as it was.
    assert_eq!(hidden_in_plan(&s, name), vec![0, 4, 5, 6, 7, 8, 9, 10, 11]);
    let f = &s.model.families[name];
    assert!(f.forms[1..4].iter().all(|x| x.show.is_none()), "its jambs and head keep saying nothing");
    for part in &f.forms[4..] {
        // A plan draws a part below its cut by Top/Bottom alone, so that is off too; the views along the door and 3D still draw it.
        assert_eq!(part.show, Some(Show { plan: false, plan_top: false, plan_cut: false, ..Show::default() }));
    }
    assert_eq!(f.forms[0].show, Some(Show { plan: false, ..Show::default() }), "the leaf as it was");
    for detail in ["coarse", "medium", "fine"] {
        let d = plan_at(&mut s, detail);
        // Its two jambs, 40 by the wall and 12 mm each side, cut; the head is above the cut.
        only_its_jambs(&d, door, detail, 2);
        let (_, open) = lines(&d, door);
        let leaves = panels(&open);
        assert_eq!(leaves.len(), 1, "{detail}: one leaf: {open:?}");
        assert!(near(leaves[0].0, 40.0) && near(leaves[0].1, 846.0), "{detail}: {leaves:?}");
        assert_eq!(swings(&open), 1, "{detail}: one swing");
    }
    assert_eq!(drawn_in_3d(&mut s, door), in_3d, "kick plates and handles are still built in 3D");
    // Read again, it is as it was.
    let once = s.model.families.clone();
    read_again(&mut s);
    assert_eq!(s.model.families, once, "read twice, changed once");
    assert!(!s.model.families[name].clone().parts_hidden_with_leaves());
}

#[test]
fn the_cubicle_door_has_only_parts_fixed_to_its_leaf_and_they_all_go_with_it() {
    let name = "Cubicle door Venesta Infinite";
    let (mut s, door) = live(name, CUBICLE_DOOR, "650 full height", |_| {});
    let (closed, _) = lines(&plan_at(&mut s, "medium"), door);
    assert!(!closed.is_empty(), "its hinge, lock and hook are outlined shut in the opening: {closed:?}");
    let in_3d = drawn_in_3d(&mut s, door);
    read_again(&mut s);
    // The leaf (form 1) says it already; its hinge, lock, indicator and hook (forms 2 to 8) are on the leaf, within its width,
    // its height and a hand's reach of its faces. It has no frame to keep.
    assert_eq!(hidden_in_plan(&s, name), (0..8).collect::<Vec<_>>());
    for detail in ["coarse", "medium", "fine"] {
        let d = plan_at(&mut s, detail);
        only_its_jambs(&d, door, detail, 0);
        let (_, open) = lines(&d, door);
        let leaves = panels(&open);
        assert_eq!(leaves.len(), 1, "{detail}: {open:?}");
        assert!(near(leaves[0].0, 25.0) && near(leaves[0].1, 650.0), "{detail}: {leaves:?}");
        assert_eq!(swings(&open), 1, "{detail}");
    }
    assert_eq!(drawn_in_3d(&mut s, door), in_3d);
}

#[test]
fn a_part_saying_every_view_draws_it_keeps_saying_so_and_what_is_not_on_the_leaf_is_never_hidden() {
    use crate::family::Show;
    let name = "Room door with lever";
    // A kick plate its author set to show everywhere (a form that says so, though it says what the default does), and a
    // threshold across the whole opening, a frame's, beside it.
    let (mut s, door) = live(name, ROOM_DOOR, "926x2100", |f| {
        f.forms[10].show = Some(Show::default());
        let mut sill = f.forms[3].clone();
        sill.from = crate::family::Expr::Number(0.0);
        sill.to = crate::family::Expr::Number(20.0);
        sill.id = 0;
        sill.guid = Default::default();
        f.forms.push(sill);
    });
    read_again(&mut s);
    let f = &s.model.families[name];
    assert_eq!(f.forms[10].show, Some(Show::default()), "what a form says is kept");
    assert_eq!(hidden_in_plan(&s, name), vec![0, 4, 5, 6, 7, 8, 9, 11]);
    assert!(f.forms[12].show.is_none(), "a threshold, wider than the leaf, is no part of it");
    // Its plate is the one thing outlined across the opening; and a sill shut in 20 mm high is cut below nothing.
    let (closed, _) = lines(&plan_at(&mut s, "medium"), door);
    assert_eq!(closed.iter().filter(|r| size(r).0 > 700.0).count(), 2, "its kick plate and its sill: {closed:?}");
}

#[test]
fn a_door_family_made_with_the_family_command_has_its_leafs_parts_hidden_and_one_left_shut_is_warned_of() {
    let name = "Room door with lever";
    // As an agent made it: the parts the family command is given are taken as they come, and say it.
    let mut s = Session::new();
    let mut p: Value = serde_json::from_str(ROOM_DOOR).unwrap();
    p["name"] = json!(name);
    let r = run(&mut s, "family", p);
    assert!(!r["warnings"].to_string().contains("part of a door's leaf"), "{}", r["warnings"]);
    assert_eq!(hidden_in_plan(&s, name), vec![0, 4, 5, 6, 7, 8, 9, 10, 11]);
    // One built another way (a family put into the project as it came, say) is warned of, part by part, not its frame or leaf.
    let (s, _) = live(name, ROOM_DOOR, "926x2100", |_| {});
    let (_, warnings) = s.model.families[name].problems(&|_| true);
    let warnings = warnings.join("\n");
    for form in [5, 6, 7, 8, 9, 10, 11, 12] {
        assert!(warnings.contains(&format!("form {form}: part of a door's leaf, shut in its opening, is drawn in plan")), "{warnings}");
    }
    for form in [1, 2, 3, 4] {
        let part = format!("form {form}: part of a door's leaf");
        assert!(!warnings.contains(&part), "form {form} is the leaf or the frame: {warnings}");
    }
}

#[test]
fn the_built_in_doors_have_no_part_to_hide_for_their_handles_say_their_views() {
    let s = Session::new();
    for name in ["Single Door", "Double Door", "Sliding Door"] {
        let mut f = s.model.families[name].clone();
        assert!(f.leaf_parts().is_empty(), "{name}");
        assert!(!f.parts_hidden_with_leaves(), "{name}");
        assert_eq!(f, s.model.families[name]);
    }
}
