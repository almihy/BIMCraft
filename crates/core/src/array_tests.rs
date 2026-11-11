use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};
use serde_json::{Value, json};

/// A wall along x from 1000 to 3000 with a door on it.
fn session() -> Session {
    let mut s = Session::new();
    s.script("wall {\"points\": [[1000, 0], [3000, 0]]}\ndoor {\"wall\": 100002, \"offset\": 1000}").unwrap();
    s
}

fn ids(v: &Value) -> Vec<Id> {
    v["ids"].as_array().unwrap().iter().map(|i| i.as_u64().unwrap() as Id).collect()
}

fn near(a: P2, x: f64, y: f64) -> bool {
    a.dist(P2(x, y)) < 0.01
}

fn walls_by_start(s: &Session) -> Vec<P2> {
    let mut w: Vec<P2> = s.model.elements.values().filter_map(|e| if let Element::Wall(w) = e { Some(w.start) } else { None }).collect();
    w.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    w
}

fn doors_on(s: &Session, wall: Id) -> usize {
    s.model.elements.values().filter(|e| matches!(e, Element::Door(o) if s.model.same(o.host, wall))).count()
}

#[test]
fn linear_array_to_the_second_steps_by_the_distance_and_brings_the_doors() {
    let mut s = session();
    let r = s.execute("array", &json!({"ids": [100002], "count": 3, "by": [0, 5000]})).unwrap();
    let made = ids(&r);
    // Two walls and their two doors.
    assert_eq!(made.len(), 4);
    assert_eq!(r["members"].as_array().unwrap().len(), 2);
    let starts = walls_by_start(&s);
    assert_eq!(starts.len(), 3);
    assert!(near(starts[0], 1000.0, 0.0) && near(starts[1], 1000.0, 5000.0) && near(starts[2], 1000.0, 10000.0));
    for id in made {
        if s.model.wall(id).is_some() {
            assert_eq!(doors_on(&s, id), 1, "each copied wall has its door");
        }
    }
    assert_eq!(doors_on(&s, 100002), 1);
    assert!(s.model.check().is_ok());
}

#[test]
fn linear_array_to_the_last_shares_the_distance_out() {
    let mut s = session();
    s.execute("array", &json!({"ids": [100002], "count": 3, "move_to": "last", "from": [0, 0], "to": [0, 10000]})).unwrap();
    let starts = walls_by_start(&s);
    assert_eq!(starts.len(), 3);
    assert!(near(starts[1], 1000.0, 5000.0) && near(starts[2], 1000.0, 10000.0));
}

#[test]
fn the_default_is_two_members_by_the_second() {
    let mut s = session();
    s.execute("array", &json!({"ids": [100002], "by": [0, 4000]})).unwrap();
    assert_eq!(walls_by_start(&s).len(), 2);
    assert!(near(walls_by_start(&s)[1], 1000.0, 4000.0));
}

#[test]
fn radial_array_turns_each_copy_about_the_centre_with_its_door() {
    let mut s = session();
    let r = s.execute("array", &json!({"ids": [100002], "mode": "radial", "centre": [0, 0], "angle": 90, "count": 3})).unwrap();
    let starts = walls_by_start(&s);
    assert_eq!(starts.len(), 3);
    // 1000 east turned 90 and 180 degrees.
    assert!(starts.iter().any(|q| near(*q, 0.0, 1000.0)));
    assert!(starts.iter().any(|q| near(*q, -1000.0, 0.0)));
    for id in ids(&r) {
        if s.model.wall(id).is_some() {
            assert_eq!(doors_on(&s, id), 1);
        }
    }
    assert!(s.model.check().is_ok());
}

#[test]
fn radial_array_to_the_last_spans_the_angle_and_360_shares_the_circle() {
    let mut s = session();
    s.execute("array", &json!({"ids": [100002], "mode": "radial", "centre": [0, 0], "angle": 180, "count": 3, "move_to": "last"})).unwrap();
    let starts = walls_by_start(&s);
    assert!(starts.iter().any(|q| near(*q, 0.0, 1000.0)), "the middle one is at 90 degrees");
    assert!(starts.iter().any(|q| near(*q, -1000.0, 0.0)), "the last one is at 180 degrees");

    let mut s = session();
    s.execute("array", &json!({"ids": [100002], "mode": "radial", "centre": [0, 0], "angle": 360, "count": 4, "move_to": "last"})).unwrap();
    let starts = walls_by_start(&s);
    assert_eq!(starts.len(), 4);
    assert!(starts.iter().any(|q| near(*q, 0.0, 1000.0)) && starts.iter().any(|q| near(*q, 0.0, -1000.0)));
    // None sits on the original but the original.
    assert_eq!(starts.iter().filter(|q| near(**q, 1000.0, 0.0)).count(), 1);
}

#[test]
fn an_array_is_one_undo_step() {
    let mut s = session();
    let before = s.model.elements.clone();
    s.execute("array", &json!({"ids": [100002], "count": 5, "by": [0, 3000]})).unwrap();
    // The wall and its door, four times over.
    assert_eq!(s.model.elements.len(), before.len() + 2 * 4);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements, before);
}

#[test]
fn hostile_values_are_refused_and_change_nothing() {
    let mut s = session();
    let before = s.model.elements.clone();
    for p in [
        json!({"ids": [100002], "count": 1, "by": [0, 1000]}),
        json!({"ids": [100002], "count": 0, "by": [0, 1000]}),
        json!({"ids": [100002], "count": 1001, "by": [0, 1000]}),
        json!({"ids": [100002], "count": 1e300, "by": [0, 1000]}),
        json!({"ids": [100002], "count": -2, "by": [0, 1000]}),
        json!({"ids": [100002], "count": "many", "by": [0, 1000]}),
        json!({"ids": [100002], "count": 2}),
        json!({"ids": [100002], "by": [0, 0]}),
        json!({"ids": [100002], "from": [5, 5], "to": [5, 5]}),
        json!({"ids": [100002], "from": [5, 5]}),
        json!({"ids": [100002], "by": [0, 1000], "from": [0, 0], "to": [0, 1]}),
        json!({"ids": [100002], "by": [1e300, 0]}),
        json!({"ids": [100002], "by": "x"}),
        json!({"ids": [100002], "by": [0, 1000], "move_to": "middle"}),
        json!({"ids": [100002], "by": [0, 1000], "mode": "spiral"}),
        json!({"ids": [100002], "by": [0, 1000], "angle": 30}),
        json!({"ids": [100002], "by": [0, 1000], "group": true}),
        json!({"ids": [100002], "mode": "radial", "angle": 90}),
        json!({"ids": [100002], "mode": "radial", "centre": [0, 0]}),
        json!({"ids": [100002], "mode": "radial", "centre": [0, 0], "angle": 0}),
        json!({"ids": [100002], "mode": "radial", "centre": [0, 0], "angle": 361}),
        json!({"ids": [100002], "mode": "radial", "centre": [0, 0], "angle": 1e300}),
        json!({"ids": [100002], "mode": "radial", "centre": [1e300, 0], "angle": 45}),
        json!({"ids": [100002], "mode": "radial", "centre": [0, 0], "angle": 45, "by": [1, 1]}),
        json!({"ids": []}),
        json!({"ids": [999999], "by": [0, 1000]}),
        json!({"by": [0, 1000]}),
        json!(null),
    ] {
        assert!(s.execute("array", &p).is_err(), "{p}");
        assert_eq!(s.model.elements, before, "{p}");
    }
}

#[test]
fn an_array_of_a_level_is_refused_whole() {
    let mut s = session();
    let before = s.model.elements.clone();
    let level = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Level(_))).map(|(i, _)| *i).unwrap();
    assert!(s.execute("array", &json!({"ids": [100002, level], "count": 3, "by": [0, 1000]})).is_err());
    assert_eq!(s.model.elements, before);
}

/// A beam along x from 0 to 4000 and a bar in it.
fn beam_with_a_bar(s: &mut Session) -> (Id, Id) {
    let beam = ids(&s.execute("beam", &json!({"points": [[0, 0], [4000, 0]]})).unwrap())[0];
    let bar = s.execute("rebar", &json!({"points": [[50, 0, -40], [3950, 0, -40]], "diameter": 16, "host": beam})).unwrap();
    let bar = bar["id"].as_u64().or_else(|| bar["ids"][0].as_u64()).unwrap() as Id;
    (beam, bar)
}

fn host_of(s: &Session, bar: Id) -> Option<crate::model::Guid> {
    match s.model.elements.get(bar) {
        Some(Element::Rebar(b)) => b.host,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_copy_of_a_beam_and_its_bars_keeps_each_bar_on_the_beams_copy() {
    let mut s = Session::new();
    let (beam, bar) = beam_with_a_bar(&mut s);
    let r = s.execute("copy", &json!({"ids": [beam, bar], "by": [0, 3000]})).unwrap();
    let made = ids(&r);
    let (new_beam, new_bar) = (
        *made.iter().find(|id| s.model.elements.get(**id).is_some_and(|e| e.category() == "beam")).unwrap(),
        *made.iter().find(|id| s.model.elements.get(**id).is_some_and(|e| e.category() == "rebar")).unwrap(),
    );
    assert_eq!(host_of(&s, new_bar), Some(s.model.guid(new_beam)), "the copied bar is in the copied beam");
    assert_eq!(host_of(&s, bar), Some(s.model.guid(beam)), "the original bar stays in the original beam");
}

#[test]
fn an_array_of_a_beam_and_its_bars_puts_each_members_bar_on_that_members_beam() {
    let mut s = Session::new();
    let (beam, bar) = beam_with_a_bar(&mut s);
    let r = s.execute("array", &json!({"ids": [beam, bar], "count": 3, "by": [0, 3000]})).unwrap();
    let made = ids(&r);
    let of = |category: &str| -> Vec<Id> {
        made.iter().copied().filter(|id| s.model.elements.get(*id).is_some_and(|e| e.category() == category)).collect()
    };
    let (beams, bars) = (of("beam"), of("rebar"));
    assert_eq!((beams.len(), bars.len()), (2, 2), "{r}");
    let hosts: Vec<_> = bars.iter().map(|b| host_of(&s, *b)).collect();
    for b in &beams {
        assert_eq!(hosts.iter().filter(|h| **h == Some(s.model.guid(*b))).count(), 1, "one bar in each new beam: {hosts:?}");
    }
    assert!(hosts.iter().all(|h| *h != Some(s.model.guid(beam))), "no new bar in the original beam");
}

#[test]
fn a_copy_of_a_beam_alone_brings_its_bars_to_stand_on_the_copy() {
    let mut s = Session::new();
    let (beam, bar) = beam_with_a_bar(&mut s);
    let r = s.execute("copy", &json!({"ids": [beam], "by": [0, 3000]})).unwrap();
    let made = ids(&r);
    assert_eq!(made.len(), 2, "the beam and its bar: {r}");
    let new_beam = *made.iter().find(|id| s.model.elements.get(**id).is_some_and(|e| e.category() == "beam")).unwrap();
    let new_bar = *made.iter().find(|id| s.model.elements.get(**id).is_some_and(|e| e.category() == "rebar")).unwrap();
    assert_eq!(host_of(&s, new_bar), Some(s.model.guid(new_beam)));
    assert_eq!(host_of(&s, bar), Some(s.model.guid(beam)), "the original's stays");
    // The bar went where the beam went: 3000 along y.
    match s.model.elements.get(new_bar) {
        Some(Element::Rebar(b)) => assert!(b.path.iter().all(|q| (q[1] - 3000.0).abs() < 0.01), "{:?}", b.path),
        other => panic!("{other:?}"),
    }
    // Given both, the bar is copied once.
    let r = s.execute("copy", &json!({"ids": [beam, bar], "by": [0, 6000]})).unwrap();
    assert_eq!(ids(&r).len(), 2, "{r}");
    // A mirror brings them too.
    let r = s.execute("mirror", &json!({"ids": [beam], "axis": [[0, 10000], [4000, 10000]], "copy": true})).unwrap();
    assert_eq!(ids(&r).len(), 2, "{r}");
    assert!(s.model.check().is_ok());
}

#[test]
fn an_array_of_a_beam_alone_brings_each_members_bars() {
    let mut s = Session::new();
    let (beam, _) = beam_with_a_bar(&mut s);
    let r = s.execute("array", &json!({"ids": [beam], "count": 3, "by": [0, 3000]})).unwrap();
    let made = ids(&r);
    let of = |category: &str| -> Vec<Id> {
        made.iter().copied().filter(|id| s.model.elements.get(*id).is_some_and(|e| e.category() == category)).collect()
    };
    let (beams, bars) = (of("beam"), of("rebar"));
    assert_eq!((beams.len(), bars.len()), (2, 2), "{r}");
    let hosts: Vec<_> = bars.iter().map(|b| host_of(&s, *b)).collect();
    for b in &beams {
        assert_eq!(hosts.iter().filter(|h| **h == Some(s.model.guid(*b))).count(), 1, "one bar in each new beam: {hosts:?}");
    }
}
