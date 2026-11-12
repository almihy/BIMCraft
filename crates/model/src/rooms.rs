//! Rooms' boundaries for one look at a model: what a plan, a schedule, `inspect` or an IFC export asks of many rooms.
//!
//! A room's boundary is the smallest loop of its level's room-bounding lines around its point, less the islands inside it
//! ([`geom::region_in`]). Finding those lines takes each bounding wall's joins, and arranging them into loops is the costly
//! part; both are the same for every room of a level. [`Rooms`] does them once a level and a measure, and finds the walls'
//! joins through a plan index ([`walls::Index`]) rather than among every wall of the project. It borrows the model, so
//! what it holds can't go stale: the borrow ends before anything can change the model.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::geom::{self, P2, Region};
use crate::model::{Element, Guid, Id, Model, ROOM_PROBE, Room, RoomArea, TOL, Unbound, region_holds};
use crate::phasing;
use crate::walls::{self, Index};

/// What a level's rooms are found among, at one measure.
struct Plan {
    /// The loops its room-bounding lines make.
    faces: Vec<Vec<P2>>,
    /// At the finish faces, its bounding walls' outlines, by id: a point in one is in a wall, not a room.
    walls: Vec<(Id, Vec<P2>)>,
}

/// A level's rooms, in order of id.
type Placed<'a> = Rc<Vec<(Id, &'a Room)>>;

thread_local! {
    /// How many times this thread has looked up a door's or a window's rooms: see [`openings_looked_up`].
    static LOOKED_UP: Cell<usize> = const { Cell::new(0) };
}

/// How many times this thread has looked up the rooms either side of a door or a window ([`Rooms::from_to`],
/// [`Rooms::swings_into`]): what tests read to know that what wants an element's name, mark or tag, or a palette drawn
/// again, doesn't look for them.
pub fn openings_looked_up() -> usize {
    LOOKED_UP.with(Cell::get)
}

/// The phases a room stands in, as Revit's rooms are each of a phase: from the one it was made in up to, not into, the one
/// it comes down in (a room made and taken down in one phase stands in that one), by their places in the project's order.
fn standing(m: &Model, r: &Room) -> (usize, usize) {
    let at = |key: &str| r.params.get(key).and_then(serde_json::Value::as_str).and_then(|n| phasing::index(m, n));
    let made = at(phasing::CREATED).unwrap_or(0);
    let gone = at(phasing::DEMOLISHED).map_or(usize::MAX, |d| d.max(made + 1));
    (made, gone)
}

/// Whether rooms `a` and `b` stand in a phase together ([`standing`]): only then may one have the other's space, as Revit's
/// redundant rooms are of one phase.
pub fn share_a_phase(m: &Model, a: &Room, b: &Room) -> bool {
    let ((a0, a1), (b0, b1)) = (standing(m, a), standing(m, b));
    a0 < b1 && b0 < a1
}

/// The boundaries of a model's rooms, each level's found once.
pub struct Rooms<'a> {
    m: &'a Model,
    index: OnceCell<Index>,
    plans: RefCell<HashMap<(Guid, bool), Rc<Plan>>>,
    /// Each level's rooms, in order of id.
    placed: RefCell<HashMap<Guid, Placed<'a>>>,
    /// Each room's boundary at the walls' centre lines, once found: what [`Rooms::room_at`] asks of every room of a level.
    centre: RefCell<HashMap<Id, Option<Region>>>,
    /// Each level's room-bounding walls, once found: what a door's rooms are looked for past ([`Rooms::past_walls`]).
    bounding: RefCell<HashMap<Guid, Rc<Vec<Id>>>>,
    /// Where the lines are looked for, when only those near one place are: the box `[x0, y0, x1, y1]` the loop must lie in,
    /// and the walls, separation lines and column faces that meet it grown by the second.
    window: Option<([f64; 4], f64)>,
}

impl<'a> Rooms<'a> {
    pub fn new(m: &'a Model) -> Self {
        Rooms {
            m,
            index: OnceCell::new(),
            plans: RefCell::new(HashMap::new()),
            placed: RefCell::new(HashMap::new()),
            centre: RefCell::new(HashMap::new()),
            bounding: RefCell::new(HashMap::new()),
            window: None,
        }
    }

    /// Rooms found among the lines near `window` alone (those that meet it grown by `pad`), for [`Rooms::loop_near`].
    pub fn within(m: &'a Model, window: [f64; 4], pad: f64) -> Self {
        Rooms { window: Some((window, pad)), ..Rooms::new(m) }
    }

    /// Whether a box meets the window grown by its pad (any box does, with no window).
    fn near(&self, b: [f64; 4]) -> bool {
        self.window.is_none_or(|(w, pad)| b[0] <= w[2] + pad && b[2] >= w[0] - pad && b[1] <= w[3] + pad && b[3] >= w[1] - pad)
    }

    /// Whether wall `id` may touch the window.
    fn wall_near(&self, id: Id) -> bool {
        self.window.is_none() || self.m.wall(id).and_then(|w| walls::reach(self.m, w)).is_none_or(|b| self.near(b))
    }

    /// Room `r`'s loop, as [`Rooms::loop_at`] finds it, among the lines near the window alone, and whether its point is in a
    /// room-bounding wall's body. `None` when that cannot be said: there is no window, the room is frozen, or no loop closes
    /// round its point within the window. Where the loop lies wholly in the window nothing outside it could change it.
    pub fn loop_near(&self, r: &Room, at: RoomArea) -> Option<(Region, bool)> {
        let (window, _) = self.window?;
        if r.kept.is_some() {
            return None;
        }
        let plan = self.plan(r.level, at);
        let g = geom::region_in(&plan.faces, r.point, TOL)?;
        let inside =
            g.outer.iter().all(|p| p.0 >= window[0] - TOL && p.0 <= window[2] + TOL && p.1 >= window[1] - TOL && p.1 <= window[3] + TOL);
        inside.then(|| (g, plan.walls.iter().any(|(_, ring)| geom::contains(ring, r.point))))
    }

    fn plan(&self, level: Guid, at: RoomArea) -> Rc<Plan> {
        let key = (level, at == RoomArea::Finish);
        if let Some(plan) = self.plans.borrow().get(&key) {
            return Rc::clone(plan);
        }
        // The walls' joins through the model's lookup when it is made (its tiles and heights), else an index of every wall.
        let m = self.m;
        let index = match m.derived.made(&m.elements) {
            Some(_) => None,
            None => Some(self.index.get_or_init(|| Index::new(m))),
        };
        let walls: Vec<(Id, Vec<P2>)> = match at {
            RoomArea::Finish => {
                m.bounding_walls(&level, true).filter(|id| self.wall_near(*id)).map(|id| (id, walls::outline_in(m, id, index))).collect()
            }
            RoomArea::Centre => Vec::new(),
        };
        let lines = match at {
            RoomArea::Finish => {
                let mut lines: Vec<(P2, P2)> = walls.iter().flat_map(|(_, ring)| geom::ring_edges(ring)).collect();
                lines.extend(
                    m.separators(level).into_iter().filter(|(a, b)| self.near([a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)])),
                );
                lines
            }
            // (Room-bounding columns' faces are among them.)
            RoomArea::Centre if self.window.is_some() => m.room_lines_near(level, index, &|id| self.wall_near(id), &|b| self.near(b)),
            RoomArea::Centre => m.room_lines_in(level, at, index),
        };
        let lines = match at {
            RoomArea::Finish => {
                let mut lines = lines;
                for o in m.bounding_columns(level) {
                    let b = o.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, p| {
                        [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)]
                    });
                    if self.near(b) {
                        lines.extend(geom::ring_edges(&o));
                    }
                }
                lines
            }
            RoomArea::Centre => lines,
        };
        let plan = Rc::new(Plan { faces: geom::faces(&lines, TOL), walls });
        self.plans.borrow_mut().insert(key, Rc::clone(&plan));
        plan
    }

    /// Room `r`'s boundary where the project measures rooms; `None` when it isn't enclosed.
    pub fn region(&self, r: &Room) -> Option<Region> {
        self.region_at(r, self.m.room_area)
    }

    /// What room `r` covers in plan ([`Model::room_outline`]), for asking of many rooms.
    pub fn outline(&self, r: &Room) -> (Vec<P2>, bool) {
        match self.region(r) {
            Some(g) => (g.outer, true),
            None => (crate::model::unbounded_square(r.point), false),
        }
    }

    /// Room `r`'s boundary measured at `at`.
    pub fn region_at(&self, r: &Room, at: RoomArea) -> Option<Region> {
        self.bound(r, at).ok()
    }

    /// Room `r`'s boundary measured at `at`, or why it has none ([`Model::room_bound`]): a frozen room's, the one it keeps;
    /// else the loop round its point, unless its point is in a wall or a room placed before it has the space.
    pub fn bound(&self, r: &Room, at: RoomArea) -> Result<Region, Unbound> {
        if let Some(k) = &r.kept {
            return k.at(at).cloned().ok_or(Unbound::NotEnclosed);
        }
        let plan = self.plan(r.level, at);
        if plan.walls.iter().any(|(_, ring)| geom::contains(ring, r.point)) {
            return Err(Unbound::NotEnclosed);
        }
        let g = geom::region_in(&plan.faces, r.point, TOL).ok_or(Unbound::NotEnclosed)?;
        self.before(r, &g).map_or(Ok(g), |id| Err(Unbound::Shared(id)))
    }

    /// The room on `level` that point `p` stands in, as Revit's Room parameter of a family instance: the first, in order of id,
    /// whose boundary at the walls' centre lines holds it. The centre lines, whatever the project measures rooms at, so a device
    /// on a wall's face (an outlet, a switch) is in the room on that side and not in the wall; `None` outside every room.
    pub fn room_at(&self, level: Guid, p: P2) -> Option<Id> {
        let rooms = self.placed(level);
        rooms.iter().find(|(id, r)| self.centre_of(*id, r).is_some_and(|g| region_holds(&g, p))).map(|(id, _)| *id)
    }

    /// The rooms door or window `e` is between, as Revit's From Room and To Room: the room on each side of its wall at its
    /// middle ([`Model::opening_sides`], [`Rooms::room_at`]), on the sides its `to_room_side` says (set as it was placed: a
    /// door's To Room the room it swung into, a window's the one it faced, its exterior), from, then to; none on a side
    /// outside every room. As Revit's FamilyInstance.FromRoom and ToRoom, of the project's last phase: among the rooms
    /// standing in it ([`Rooms::room_in_last_phase`]). None for anything else.
    pub fn from_to(&self, e: &Element) -> Option<(Option<Id>, Option<Id>)> {
        let (Element::Door(o) | Element::Window(o)) = e else { return None };
        LOOKED_UP.with(|n| n.set(n.get() + 1));
        let w = self.m.wall(o.host)?;
        let (from, to) = self.m.opening_sides(e)?;
        let found = |p: P2| self.room_in_last_phase(w.level, self.past_walls(o.host, w, o.offset, p));
        Some((found(from), found(to)))
    }

    /// [`Rooms::room_at`] among the rooms standing in the project's last phase ([`standing`]): what a door's or window's
    /// rooms are, as Revit's.
    pub fn room_in_last_phase(&self, level: Guid, p: P2) -> Option<Id> {
        let last = self.m.phases.len().saturating_sub(1);
        let rooms = self.placed(level);
        rooms
            .iter()
            .filter(|(_, r)| {
                let (made, gone) = standing(self.m, r);
                made <= last && last < gone
            })
            .find(|(id, r)| self.centre_of(*id, r).is_some_and(|g| region_holds(&g, p)))
            .map(|(id, _)| *id)
    }

    /// The room door `e` opens into now, on the side of its wall it swings to, whatever its From Room and To Room say
    /// ([`Model::swing_point`]); of the project's last phase, as [`Rooms::from_to`]. None for a window.
    pub fn swings_into(&self, e: &Element) -> Option<Option<Id>> {
        let Element::Door(o) = e else { return None };
        LOOKED_UP.with(|n| n.set(n.get() + 1));
        let w = self.m.wall(o.host)?;
        let p = self.past_walls(o.host, w, o.offset, self.m.swing_point(e)?);
        Some(self.room_in_last_phase(w.level, p))
    }

    /// Point `p`, a door's or window's room looked for past the face of its wall `w` (of GUID `host`) at `along`, taken on
    /// out of every other room-bounding wall of its level it lies in and the probe on past that one's far face: a lining
    /// wall built against the door's wall, however thick, is not the room beyond it. Straight out from `w`, up to 10 m.
    fn past_walls(&self, host: Guid, w: &crate::model::Wall, along: f64, p: P2) -> P2 {
        let out = (p - walls::Axis::of(w).at(along, 0.0)).unit();
        let ids = self.bounding(w.level);
        let host = self.m.elements.id(host);
        let inside = |q: P2| {
            ids.iter().filter(|id| Some(**id) != host).filter_map(|id| self.m.wall(*id)).any(|o| {
                let a = walls::Axis::of(o);
                let (s, off) = a.project(q);
                let (left, right) = walls::faces(self.m, o);
                a.at(s, off).dist(q) < 0.5 && right - TOL < off && off < left + TOL
            })
        };
        let (step, mut left) = (ROOM_PROBE / 10.0, 1000);
        let mut q = p;
        while inside(q) && left > 0 {
            while inside(q) && left > 0 {
                q = q + out * step;
                left -= 1;
            }
            q = q + out * ROOM_PROBE;
        }
        q
    }

    /// The room-bounding walls of `level`, found once.
    fn bounding(&self, level: Guid) -> Rc<Vec<Id>> {
        if let Some(ids) = self.bounding.borrow().get(&level) {
            return Rc::clone(ids);
        }
        let ids: Rc<Vec<Id>> = Rc::new(self.m.bounding_walls(&level, true).collect());
        self.bounding.borrow_mut().insert(level, Rc::clone(&ids));
        ids
    }

    /// Room `r`'s (of id `id`) boundary at the walls' centre lines, found once.
    fn centre_of(&self, id: Id, r: &Room) -> Option<Region> {
        if let Some(g) = self.centre.borrow().get(&id) {
            return g.clone();
        }
        let g = self.region_at(r, RoomArea::Centre);
        self.centre.borrow_mut().insert(id, g.clone());
        g
    }

    /// The room on `r`'s level placed before it (of a lower id; any, for a room not in the model) whose point lies in `g`
    /// and that stands in a phase `r` stands in ([`Model::room_before`]): as Revit's, each phase has its rooms, so a room of
    /// a later phase in the space of one taken down before it has the space to itself.
    pub fn before(&self, r: &Room, g: &Region) -> Option<Id> {
        let rooms = self.placed(r.level);
        let me = rooms.iter().find(|(_, o)| std::ptr::eq(*o, r)).map(|(id, _)| *id);
        let m = self.m;
        rooms
            .iter()
            .take_while(|(id, _)| me.is_none_or(|me| *id < me))
            .filter(|(_, o)| share_a_phase(m, o, r))
            .find(|(_, o)| m.same(o.level, r.level) && region_holds(g, o.point))
            .map(|(id, _)| *id)
    }

    /// The rooms on `level`, in order of id: what points at it, from the lookup when it is made, else among every element.
    pub fn placed(&self, level: Guid) -> Placed<'a> {
        if let Some(rooms) = self.placed.borrow().get(&level) {
            return Rc::clone(rooms);
        }
        let m = self.m;
        let room = |id: Id| match m.elements.get(id) {
            Some(Element::Room(o)) if m.same(o.level, level) => Some((id, o)),
            _ => None,
        };
        let rooms: Vec<(Id, &'a Room)> = match m.derived.made(&m.elements) {
            Some(look) => {
                let ids: std::collections::BTreeSet<Id> = look.referrers(&level).collect();
                ids.into_iter().filter_map(room).collect()
            }
            None => m.elements.keys().filter_map(|id| room(*id)).collect(),
        };
        let rooms = Rc::new(rooms);
        self.placed.borrow_mut().insert(level, Rc::clone(&rooms));
        rooms
    }

    /// The loop of `level`'s room-bounding lines, measured at `at`, around `point`, less its islands, whether or not the point
    /// lies in a wall: what a plan's room label measures.
    pub fn loop_at(&self, level: Guid, at: RoomArea, point: P2) -> Option<Region> {
        geom::region_in(&self.plan(level, at).faces, point, TOL)
    }

    /// The room-bounding wall whose body room `r`'s point lies in, measured at `at` (only the finish faces close a wall's
    /// body into a loop of its own).
    pub fn holding(&self, r: &Room, at: RoomArea) -> Option<Id> {
        if at != RoomArea::Finish {
            return None;
        }
        self.plan(r.level, at).walls.iter().find(|(_, ring)| geom::contains(ring, r.point)).map(|(id, _)| *id)
    }
}
