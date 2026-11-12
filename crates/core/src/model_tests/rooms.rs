//! `rooms`'s tests that drive the Session (moved out of `rooms.rs`).

use crate::geom::{self, P2, Region};
use crate::model::{Model, Room, RoomArea, TOL, Unbound, region_holds};
use crate::walls::{self};

use crate::engine::Session;
use crate::model::Element;
use crate::rooms::*;
use serde_json::json;

/// A room's boundary as it was always found: among every wall of the project, its level's lines arranged for it alone,
/// and the room that has its space before it among every element.
fn brute(m: &Model, r: &Room, at: RoomArea) -> Result<Region, Unbound> {
    if let Some(k) = &r.kept {
        return k.at(at).cloned().ok_or(Unbound::NotEnclosed);
    }
    if at == RoomArea::Finish {
        let inside = m
            .elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Wall(w) if m.room_bounding(w, r.level)))
            .any(|(id, _)| geom::contains(&walls::outline_in(m, *id, None), r.point));
        if inside {
            return Err(Unbound::NotEnclosed);
        }
    }
    let g = geom::region_at(&m.room_lines_in(r.level, at, None), r.point, TOL).ok_or(Unbound::NotEnclosed)?;
    let me = m.elements.iter().find(|(_, e)| matches!(e, Element::Room(o) if std::ptr::eq(o, r))).map(|(id, _)| *id);
    let before = m
        .elements
        .iter()
        .take_while(|(id, _)| me.is_none_or(|me| **id < me))
        .find(|(_, e)| matches!(e, Element::Room(o) if m.same(o.level, r.level) && share_a_phase(m, o, r) && region_holds(&g, o.point)))
        .map(|(id, _)| *id);
    before.map_or(Ok(g), |id| Err(Unbound::Shared(id)))
}

/// Every room's boundary, at both measures, as [`Rooms`] finds it and as it was always found.
pub fn agree(m: &Model) {
    let rooms = Rooms::new(m);
    for (id, e) in &m.elements {
        let Element::Room(r) = e else { continue };
        for at in [RoomArea::Centre, RoomArea::Finish] {
            assert_eq!(rooms.bound(r, at), brute(m, r, at), "room {id} ({}) at {at:?}", r.name);
        }
    }
}

#[test]
fn rooms_found_a_level_at_a_time_are_those_found_one_by_one() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    agree(&s.model);
    // A room in a wall, at the finish faces; one not enclosed; one on a level of its own.
    s.execute("room_area", &json!({"at": "finish"})).unwrap();
    let m = s.model_mut();
    let level = m.levels()[0].0;
    let level = m.guid(level);
    for point in [(3000.0, 4000.0), (-5000.0, -5000.0), (0.0, 0.0)] {
        m.insert(Element::Room(Room::new(level, P2(point.0, point.1), "Odd", "9")));
    }
    agree(&s.model);
    let tower = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap();
    agree(&tower);
}
