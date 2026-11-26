//! An element as agents read it: its fields (what it points at by GUID), its `id` and `guid`, and the values worked out
//! from it (lengths, areas, volumes, its room, a stair's runs). The commands answer with it, and a schedule's fields and a
//! tag's text read from it.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::geom::{self, P2, micron};
use crate::library::Kind as LibKind;
use crate::model::{Column, Element, Floor, Id, Mass, Model, Unbound, Wall, paper};
use crate::rebar;
use crate::sketch;
use crate::stairs;
use crate::units::m2;
use crate::walls::Axis;

/// An element as agents read it: its fields (what it points at by GUID) plus its `id`, its `guid` and derived values.
pub fn element_json(m: &Model, id: Id) -> Value {
    element_json_with(m, id, &crate::rooms::Rooms::new(m))
}

/// [`element_json`], a room's boundary found among `rooms`: what one look at many elements shares.
pub fn element_json_with(m: &Model, id: Id, rooms: &crate::rooms::Rooms) -> Value {
    json_of(m, id, Some(rooms), true)
}

/// [`element_json_with`] less what is measured from the element's solids ([`measured`]: a wall's area and volume, a floor's, a roof's
/// and a ceiling's volume), which is what costs: a schedule or a tag reads those only where it shows them, and asks [`measured`] for
/// them then.
pub fn element_json_light(m: &Model, id: Id, rooms: &crate::rooms::Rooms) -> Value {
    json_of(m, id, Some(rooms), false)
}

/// [`element_json`] less what is found among the rooms of its level: a component's room, a door's or window's From Room,
/// To Room and the room it swings into. What wants an element's name, type or mark, often (the status bar, a tag that
/// shows no room, IDs of Selection), and not its rooms nor what is measured from its solids ([`measured`]); a room's own boundary
/// and a tag's text are still found.
pub fn element_json_bare(m: &Model, id: Id) -> Value {
    json_of(m, id, None, false)
}

/// Whether `key` of an element's JSON is one [`measured`] gives.
pub fn is_measured(key: &str) -> bool {
    matches!(key, "area_m2" | "volume_m3")
}

/// What element `id`'s JSON says that is measured from its solids (as the model keeps them, [`Model::solids`]): a wall's `area_m2`
/// and `volume_m3`, a floor's, roof's and ceiling's `volume_m3`. [`element_json_with`] has them, [`element_json_light`] and
/// [`element_json_bare`] leave them out.
pub fn measured(m: &Model, id: Id) -> Map<String, Value> {
    let mut out = Map::new();
    match m.elements.get(id) {
        // One side's area and the volume, less the doors and windows in it, as Revit's wall Area and Volume.
        Some(Element::Wall(_)) => {
            if let Some(q) = crate::quantities::wall(m, id) {
                out.insert("area_m2".into(), json!(m2(q.net_side)));
                out.insert("volume_m3".into(), json!(m3(q.net_volume)));
            }
        }
        // What a floor holds, less its openings and the shafts through it: under a variable layer, less where its top falls.
        Some(Element::Floor(f) | Element::Toposolid(f)) => {
            out.insert("volume_m3".into(), json!(m3(crate::quantities::floor_at(m, id, f).net_volume)));
        }
        // What a roof holds through its thickness.
        Some(Element::Roof(r)) => {
            out.insert("volume_m3".into(), json!(m3(crate::quantities::roof_at(m, id, r).volume)));
        }
        Some(Element::Ceiling(c)) => {
            out.insert("volume_m3".into(), json!(m3(crate::quantities::ceiling_at(m, id, c).volume)));
        }
        _ => {}
    }
    out
}

fn json_of(m: &Model, id: Id, rooms: Option<&crate::rooms::Rooms>, measures: bool) -> Value {
    let Some(e) = m.elements.get(id) else { return Value::Null };
    let mut extra = Map::new();
    extra.insert("id".into(), json!(id));
    extra.insert("guid".into(), json!(m.guid(id)));
    if m.is_pinned(id) {
        extra.insert("pinned".into(), json!(true));
    }
    // Room Bounding, which a file leaves out while it is on.
    if let Element::Wall(Wall { room_bounding, .. })
    | Element::Column(Column { room_bounding, .. })
    | Element::StructuralColumn(Column { room_bounding, .. }) = e
    {
        extra.insert("room_bounding".into(), json!(room_bounding));
    }
    // Its type's and its family's GUIDs, beside their names.
    e.clone().lib_refs_mut(&mut |kind, name| {
        let key = match kind {
            LibKind::Type(_) => "type_guid",
            LibKind::Family => "family_guid",
            _ => return,
        };
        if let Some(g) = m.library.guid(kind, name) {
            extra.insert(key.into(), json!(g));
        }
    });
    // Its type's identity data (Type Mark, Manufacturer…): what a schedule's and a tag's fields of that name read.
    if let Some(identity) = m.type_identity(e) {
        extra.extend(identity.set().map(|(k, v)| (k.to_string(), v)));
    }
    // The room a component stands in, as Revit's Room parameter: by GUID, with its number and name for schedules and tags.
    if let (Element::Component(c), Some(rooms)) = (e, rooms)
        && let Some(id) = rooms.room_at(c.level, c.at)
        && let Some(Element::Room(r)) = m.elements.get(id)
    {
        extra.insert("room".into(), json!(m.guid(id)));
        extra.insert("room_number".into(), json!(r.number));
        extra.insert("room_name".into(), json!(r.name));
    }
    match e {
        Element::CadImport(ci) => {
            if let Some(cad) = m.cads.get(&ci.cad) {
                extra.insert("source".into(), json!(cad.source()));
                extra.insert("entities".into(), json!(cad.dxf().count()));
                extra.insert("layers".into(), json!(cad.dxf().layers.len()));
                extra.insert("unit".into(), json!(cad.dxf().unit));
                if let Some(path) = cad.path() {
                    extra.insert("linked".into(), json!(path));
                }
                // A drawing not read (yet): why, in place of what it draws.
                if let Some(why) = cad.unread() {
                    extra.insert("unread".into(), json!(why));
                }
            }
            if let Some(b) = m.cad_box(id) {
                extra.insert("extents".into(), json!(b));
            }
        }
        Element::RoomSeparator(rs) => {
            extra.insert("length".into(), json!(hundredth(rs.start.dist(rs.end))));
        }
        Element::Image(im) => {
            if let Some(t) = m.image_types.get(&im.type_name) {
                let [sx, sy] = im.scales(t);
                extra.insert("source".into(), json!(t.source()));
                extra.insert("horizontal_scale".into(), json!((sx * 1e6).round() / 1e6));
                extra.insert("vertical_scale".into(), json!((sy * 1e6).round() / 1e6));
                extra.insert("lines".into(), json!(t.lines().map_or(0, |c| c.dxf().count())));
            }
            extra.insert("corners".into(), json!(im.corners().map(read_point)));
        }
        Element::Rebar(rb) => {
            let length = rebar::length(&rb.path);
            extra.insert("length".into(), json!(hundredth(length)));
            extra.insert("weight_kg".into(), json!((rebar::weight_kg(rb.diameter, length) * 1000.0).round() / 1000.0));
        }
        Element::PointCloud(pc) => {
            if let Some(cloud) = m.clouds.get(&pc.cloud) {
                extra.insert("points".into(), json!(cloud.len()));
                extra.insert("source".into(), json!(cloud.source()));
                if !cloud.stations().is_empty() {
                    extra.insert("stations".into(), json!(cloud.stations()));
                }
            }
        }
        Element::Wall(w) => {
            let axis = Axis::of(w);
            extra.insert("length".into(), json!(hundredth(axis.length() * 1e4) / 1e4));
            extra.insert("height".into(), json!(hundredth(m.top_z(w) - m.base_z(w))));
            extra.insert("thickness".into(), json!(m.thickness(w)));
            if let Some(r) = axis.radius() {
                extra.insert("radius".into(), json!(hundredth(r)));
            }
            // The walls each end is joined to (by id), as Revit's Elements At Join: those ending there with it and one it
            // ends on. A curtain wall joins none.
            if !crate::curtain::is_curtain(m, w) {
                extra.insert("joined_at_start".into(), json!(crate::walls::joined_at(m, id, false)));
                extra.insert("joined_at_end".into(), json!(crate::walls::joined_at(m, id, true)));
            }
            // A curtain wall says so, with how many panels and mullions it has and where its grid lines are (mm along it
            // from its start, and mm over its base, the borders first and last): a facade read without listing its parts.
            if let Some(g) = crate::curtain::grid(m, id) {
                let count = |want: fn(&Element) -> bool| {
                    m.hosted_by(m.guid(id)).into_iter().filter(|c| m.elements.get(*c).is_some_and(want)).count()
                };
                extra.insert("curtain_wall".into(), json!(true));
                extra.insert("panels".into(), json!(count(|e| matches!(e, Element::CurtainPanel(_)))));
                extra.insert("mullions".into(), json!(count(|e| matches!(e, Element::Mullion(_)))));
                // The doors that fill cells of it (`curtain_door`), where it has any.
                let doors = crate::curtain::doors_of(m, m.guid(id));
                if !doors.is_empty() {
                    extra.insert("doors".into(), json!(doors.len()));
                    extra.insert("door_ids".into(), json!(doors));
                }
                // The basic walls it is embedded in (Automatically Embed), by id.
                if let Some(Element::Wall(cw)) = m.elements.get(id)
                    && crate::curtain::curtain_of(m, cw).is_some_and(|c| c.embed)
                {
                    let mut hosts: Vec<Id> = Vec::new();
                    if let Some(b) = crate::walls::reach(m, cw) {
                        for other in m.lookup().walls_near_at(&[(b, m.base_z(cw), m.top_z(cw))], 1.0) {
                            if other != id && m.wall(other).is_some_and(|host| crate::curtain::embeds(m, cw, host).is_some()) {
                                hosts.push(other);
                            }
                        }
                    }
                    hosts.sort_unstable();
                    extra.insert("embedded_in".into(), json!(hosts));
                }
                extra.insert("vertical_grid".into(), json!(g.u.iter().map(|l| hundredth(l.at)).collect::<Vec<_>>()));
                extra.insert("horizontal_grid".into(), json!(g.v.iter().map(|l| hundredth(l.at)).collect::<Vec<_>>()));
            }
        }
        Element::Door(o) | Element::Window(o) => {
            if let (Some(t), Some(w)) = (m.opening_type(e), m.wall(o.host)) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("height".into(), json!(t.height));
                if let Some(op) = t.operation {
                    extra.insert("operation".into(), json!(op.name()));
                }
                extra.insert("level".into(), json!(w.level));
                extra.insert("at".into(), json!(m.anchor(id)));
                // A curtain wall's door says so: it fills a cell of its wall's grid, and its width and height are the cell's.
                if o.cell_size().is_some() {
                    extra.insert("curtain_door".into(), json!(true));
                }
                // The rooms either side, as Revit's From Room and To Room (set as it was placed: a door's To Room, the one it
                // swung into), the room a door swings into now, and its hinge and the side of its wall it swings to: what its
                // flips mean, said back.
                let room = |id: Option<Id>| match id.and_then(|id| m.elements.get(id).map(|r| (id, r))) {
                    Some((id, Element::Room(r))) => json!({"id": id, "guid": m.guid(id), "number": r.number, "name": r.name}),
                    _ => Value::Null,
                };
                if let Some(rooms) = rooms {
                    if let Some((from, to)) = rooms.from_to(e) {
                        extra.insert("from_room".into(), room(from));
                        extra.insert("to_room".into(), room(to));
                    }
                    if let Some(into) = rooms.swings_into(e) {
                        extra.insert("swings_into".into(), room(into));
                    }
                }
                if let Element::Door(_) = e {
                    if let Some(h) = m.door_hinge(e) {
                        extra.insert("hinge".into(), json!([hundredth(h.0), hundredth(h.1)]));
                    }
                    if let Some(way) = m.opening_way(e) {
                        extra.insert("swing".into(), json!(if way.to > 0.0 { "left" } else { "right" }));
                    }
                    if o.family.is_none() {
                        extra.insert("thickness".into(), json!(t.leaf()));
                    }
                }
                // Of a family: every parameter's value, worked out.
                if let Some(v) = o
                    .family
                    .as_ref()
                    .and_then(|n| m.families.get(n))
                    .and_then(|f| f.values_in(&o.type_name, &o.params, m.thickness(w)).ok())
                {
                    extra.insert("values".into(), values_json(v));
                }
            }
        }
        Element::Dimension(d) => {
            extra.insert("values".into(), json!(m.dimension_values(d).into_iter().map(hundredth).collect::<Vec<_>>()));
            extra.insert("labels".into(), json!(m.dimension_labels(d)));
        }
        // Every parameter's value, worked out, and how high it reaches over its base.
        Element::Component(cp) => {
            if let Some(v) = m.families.get(&cp.family).and_then(|f| f.values(&cp.type_name, &cp.params).ok()) {
                extra.insert("values".into(), values_json(v));
            }
            // What it draws and where it joins a system, as its family's connectors say for its type, placed where it
            // stands: none for a family with none.
            let cs = m.component_connectors(cp);
            if !cs.is_empty() {
                extra.insert("connectors".into(), Value::Array(cs.iter().map(connector_json).collect()));
            }
            let base = m.elevation(cp.level) + cp.offset;
            let top = m.component_pieces(cp).iter().map(|p| p.top).fold(base, f64::max);
            extra.insert("height".into(), json!(hundredth(top - base)));
            // A parking space: its number, its size and its sides' angle (its type's), and what it stands on.
            if crate::carpark::is_bay(m, cp) {
                let mark = cp.params.get(m.language.word(crate::family::site::MARK)).cloned().unwrap_or(json!(""));
                extra.insert("mark".into(), mark);
                if let Some((w, d, a)) = crate::carpark::bay_size(m, cp) {
                    extra.insert("width".into(), json!(w));
                    extra.insert("depth".into(), json!(d));
                    extra.insert("stall_angle".into(), json!(a));
                }
                let (host, _) = crate::carpark::surface_at(m, cp.level, cp.at);
                extra.insert("host".into(), json!(host.and_then(|g| m.elements.id(g))));
            }
        }
        Element::Column(col) | Element::StructuralColumn(col) | Element::Footing(col) => {
            let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
            extra.insert("height".into(), json!(top - bottom));
            if let Some(t) = m.column_type(e) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("depth".into(), json!(t.depth));
                extra.insert("round".into(), json!(t.round));
            }
        }
        // Its section, how long it runs along its axis, and the heights of its centre line's ends.
        crate::model::run_arm!(_) => {
            if let (Some(t), Some(g)) = (m.run_type(e), m.run_geom(e)) {
                extra.insert("width".into(), json!(t.width));
                extra.insert("depth".into(), json!(t.depth));
                extra.insert("round".into(), json!(t.round));
                extra.insert("length".into(), json!(hundredth(g.length())));
                extra.insert("axis".into(), json!([g.a, g.b].map(|p| p.map(hundredth))));
            }
        }
        Element::Stair(st) => {
            let (n, each, length) = m.flight(st);
            let (bottom, top) = m.span(st.level, 0.0, st.top_level, st.height);
            extra.insert("height".into(), json!(top - bottom));
            extra.insert("risers".into(), json!(n));
            extra.insert("riser_height".into(), json!(hundredth(each)));
            extra.insert("length".into(), json!(length));
            runs(&mut extra, &stairs::stair(m, st));
        }
        Element::Ramp(r) => {
            let (bottom, top) = m.span(r.level, 0.0, r.top_level, r.height);
            let l = stairs::ramp(m, r);
            extra.insert("height".into(), json!(top - bottom));
            extra.insert("length".into(), json!(hundredth(l.runs.iter().map(|r| r.length).sum())));
            // Each run's slope as it climbs, percent: a sketched ramp's are worked out from its path and its rise.
            let grades: Vec<f64> = l
                .runs
                .iter()
                .map(|run| if run.length > 1e-9 { hundredth((run.z.1 - run.z.0) / run.length * 100.0) } else { 0.0 })
                .collect();
            extra.insert("run_slopes".into(), json!(grades));
            runs(&mut extra, &l);
        }
        Element::Railing(rl) => {
            let path: Vec<P2> = stairs::railing(m, rl).into_iter().map(|(p, _)| micron(p)).collect();
            extra.insert("path".into(), json!(path));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("height".into(), json!(m.types.railing.get(&rl.type_name).map(|t| t.height)));
        }
        Element::Roof(r) => {
            let faces = m.roof_faces(r).unwrap_or_default();
            let ridge = faces
                .iter()
                .flat_map(|(loops, p)| loops.iter().flatten().map(|q| p.at(*q)))
                .fold(m.elevation(r.level) + r.offset, f64::max);
            let outline = m.roof_outline(r);
            let q = crate::quantities::roof_areas_at(m, r);
            extra.insert("ridge".into(), json!(hundredth(ridge)));
            extra.insert("area_m2".into(), json!(m2(q.plan_area)));
            // Its surface, as Revit's roof Area.
            extra.insert("surface_area_m2".into(), json!(m2(q.surface_area)));
            extra.insert("perimeter_m".into(), json!(m_of(q.perimeter)));
            extra.insert("outline".into(), json!(outline.into_iter().map(micron).collect::<Vec<_>>()));
        }
        Element::Sheet(sh) => {
            // Which title block it's drawn with, the built-in one ("") too, which the file leaves out.
            extra.insert("title_block".into(), json!(sh.title_block));
            extra.insert("paper".into(), json!(paper(&sh.size)));
            // What its title block's fields say, filled in; and which of them are its own to give.
            extra.insert("title_values".into(), json!(crate::titleblock::sheet_values(m, id)));
            if let Some(tb) = m.title_blocks.get(&sh.title_block) {
                let own: Vec<&str> =
                    tb.fields.iter().filter(|f| f.scope == crate::titleblock::Scope::Sheet).map(|f| f.key.as_str()).collect();
                extra.insert("sheet_fields".into(), json!(own));
            }
            let on: Vec<Id> =
                m.elements.iter().filter(|(_, e)| matches!(e, Element::Viewport(v) if m.same(v.sheet, id))).map(|(id, _)| *id).collect();
            extra.insert("viewports".into(), json!(on));
        }
        Element::Viewport(v) => {
            extra.insert("title".into(), json!(m.view_name(v.view)));
            // Its size on the sheet is found by drawing it: `bimcraft-core`'s `element_json` adds it.
        }
        Element::Section(v) | Element::Elevation(v) => {
            let l = v.looking();
            extra.insert("length".into(), json!(v.start.dist(v.end)));
            // `+ 0.0`: no `-0`.
            extra.insert("looking".into(), json!([l.0 + 0.0, l.1 + 0.0]));
        }
        Element::Tag(t) => {
            let text = rooms.map_or_else(|| m.tag_text(t), |rooms| m.tag_text_with(t, rooms));
            extra.insert("text".into(), json!(text));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("at".into(), json!(m.anchor(t.host).map(|a| a + t.offset)));
        }
        Element::Site(st) => {
            // Its area: its outline's, less its holes.
            let a: f64 =
                st.outline.iter().map(|p| geom::area(&p.outer).abs() - p.holes.iter().map(|h| geom::area(h).abs()).sum::<f64>()).sum();
            extra.insert("area_m2".into(), json!(m2(a)));
            if let Some(n) = st.props.get("result").and_then(|r| r.get("stalls")) {
                extra.insert("stalls".into(), n.clone());
            }
        }
        Element::PropertyLine(pl) => {
            extra.insert("area_m2".into(), json!(m2(pl.area())));
            let ring = pl.ring();
            let perimeter: f64 = ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| a.dist(*b)).sum();
            extra.insert("perimeter_m".into(), json!((perimeter / 10.0).round() / 100.0));
            // The buildable line its setbacks draw, and its area.
            let buildable = pl.buildable();
            let area: f64 = buildable.iter().map(|r| geom::area(r)).sum::<f64>().max(0.0);
            extra.insert("buildable".into(), json!(buildable));
            extra.insert("buildable_area_m2".into(), json!(m2(area)));
        }
        Element::SubDivision(sd) => {
            extra.insert("area_m2".into(), json!(m2(m.subdivision_area(sd))));
            if let Some(level) = m.subdivision_level(sd) {
                extra.insert("level".into(), json!(level));
            }
            extra.insert("offset".into(), json!(sd.offset));
            extra.insert("region".into(), json!(m.subdivision_region(sd)));
        }
        // A panel and a mullion: their wall's level, what they measure and what they are made of (`quantities.rs`).
        Element::CurtainPanel(p) => {
            let q = crate::quantities::panel(m, p);
            extra.insert("width".into(), json!(q.width));
            extra.insert("height".into(), json!(q.height));
            extra.insert("area_m2".into(), json!(m2(q.area)));
            extra.insert("volume_m3".into(), json!(fine_m3(q.volume)));
            extra.insert("pinned".into(), json!(!p.free));
            if let Some(w) = m.wall(p.host) {
                extra.insert("level".into(), json!(w.level));
            }
            if let Some(t) = m.types.curtain_panel.get(&p.type_name) {
                extra.insert("thickness".into(), json!(t.thickness));
                extra.insert("material".into(), json!(t.material));
            }
        }
        Element::Mullion(x) => {
            extra.insert("length".into(), json!(x.length()));
            extra.insert("pinned".into(), json!(!x.free));
            if let Some(w) = m.wall(x.host) {
                extra.insert("level".into(), json!(w.level));
            }
            if let (Some(q), Some(t)) = (crate::quantities::mullion(m, x), m.types.mullion.get(&x.type_name)) {
                extra.insert("thickness".into(), json!(q.thickness));
                extra.insert("width".into(), json!(q.width));
                extra.insert("volume_m3".into(), json!(fine_m3(q.volume)));
                extra.insert("material".into(), json!(t.material));
            }
        }
        Element::CurtainGrid(_) => {}
        Element::WallOpening(o) => {
            if let Some(w) = m.wall(o.host) {
                extra.insert("level".into(), json!(w.level));
                let ((s0, s1), (z0, z1)) = o.cut(m, w);
                let axis = crate::walls::Axis::of(w);
                let round = |v: f64| (v * 1000.0).round() / 1000.0;
                extra.insert("at".into(), json!(axis.at(o.offset, 0.0)));
                extra.insert("from".into(), json!(round(s0)));
                extra.insert("to".into(), json!(round(s1)));
                extra.insert("bottom".into(), json!(round(z0)));
                extra.insert("top".into(), json!(round(z1)));
                extra.insert("area_m2".into(), json!(round((s1 - s0) * (z1 - z0) / 1e6)));
            }
        }
        Element::SurveyPoint(b) | Element::ProjectBasePoint(b) => {
            let [east, north, height] = m.shared(b.at, b.elevation);
            let round = |v: f64| (v * 1000.0).round() / 1000.0;
            extra.insert("shared".into(), json!({"east": round(east), "north": round(north), "height": round(height)}));
            extra.insert("angle_to_true_north".into(), json!(m.angle_to_true_north()));
            // The survey point's clip, shown whichever way it is (a file leaves it out when clipped).
            if let Element::SurveyPoint(_) = e {
                extra.insert("clipped".into(), json!(b.clipped));
            }
        }
        // Areas less the openings and shafts through them.
        Element::Floor(f) | Element::Toposolid(f) => {
            let q = crate::quantities::floor_areas_at(m, f);
            extra.insert("area_m2".into(), json!(m2(q.net_area)));
            extra.insert("thickness".into(), json!(m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness)));
            extra.insert("perimeter_m".into(), json!(m_of(q.perimeter)));
            // How high its top is, at its lowest and its highest, its shape's points too.
            extra.insert("top".into(), top_range(m, f));
            // Where its top's model pattern starts and how far it's turned: shown at their defaults too, to be set by modify.
            extra.insert("pattern_origin".into(), json!(f.pattern_origin));
            extra.insert("pattern_angle".into(), json!(f.pattern_angle));
            // A graded region's cut and fill against the existing ground (Revit's Cut, Fill and Net Cut/Fill), m³.
            if let Element::Toposolid(_) = e
                && let Some((cut, fill)) = m.cut_fill(f)
            {
                let r = |v: f64| (v * 1000.0).round() / 1000.0;
                extra.insert("cut_m3".into(), json!(r(cut)));
                extra.insert("fill_m3".into(), json!(r(fill)));
                extra.insert("net_m3".into(), json!(r(cut - fill)));
            }
        }
        Element::Ceiling(cl) => {
            let q = crate::quantities::ceiling_areas_at(m, cl);
            extra.insert("area_m2".into(), json!(m2(q.net_area)));
            extra.insert("perimeter_m".into(), json!(m_of(q.perimeter)));
            if let Some(t) = m.types.ceiling.get(&cl.type_name) {
                extra.insert("thickness".into(), json!(t.thickness));
                extra.insert("grid".into(), json!(t.grid));
            }
        }
        Element::Shaft(sh) => {
            let (bottom, top) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
            extra.insert("height".into(), json!(top - bottom));
            extra.insert("area_m2".into(), json!(m2(geom::area(&sh.boundary).abs())));
        }
        Element::Mass(ms) => {
            let (area, height, volume) = mass_size(m, ms);
            extra.insert("area_m2".into(), json!(area));
            extra.insert("height".into(), json!(height));
            extra.insert("volume_m3".into(), json!(volume));
            let gross: f64 = m.mass_slices(ms).iter().map(|(_, z, _)| geom::area(&m.mass_outline(ms, *z)).abs()).sum();
            extra.insert("gross_floor_area_m2".into(), json!(m2(gross)));
            extra.insert("levels".into(), json!(m.mass_slices(ms).iter().map(|(l, _, _)| *l).collect::<Vec<Id>>()));
        }
        Element::Room(r) => {
            let bound = match rooms {
                Some(rooms) => rooms.bound(r, m.room_area),
                None => crate::rooms::Rooms::new(m).bound(r, m.room_area),
            };
            if let Err(Unbound::Shared(other)) = bound {
                extra.insert("shares_space_with".into(), json!(other));
            }
            let region = bound.ok();
            extra.insert("enclosed".into(), json!(region.is_some()));
            extra.insert("area_m2".into(), json!(region.as_ref().map_or(0.0, |g| m2(g.area()))));
            extra.insert("perimeter_m".into(), json!(region.as_ref().map_or(0.0, |g| (g.perimeter() / 10.0).round() / 100.0)));
            let height = m.room_height(r);
            extra.insert("unbounded_height".into(), json!(height));
            extra.insert("volume_m3".into(), json!(region.as_ref().map_or(0.0, |g| (g.area() * height / 1e7).round() / 100.0)));
        }
        Element::Text(n) => {
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("size".into(), json!(m.text_types.get(&n.type_name).map(|t| t.size)));
        }
        Element::DetailGroup(g) => {
            extra.insert("members".into(), json!(m.detail_groups.get(&g.type_name).map_or(0, |t| t.members.len())));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::RepeatingDetail(r) => {
            extra.insert("count".into(), json!(r.places().map_or(0, |v| v.len())));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::FilledRegion(r) => {
            let area = sketch::solve(&r.sketch).map_or(0.0, |p| p.area());
            extra.insert("area_m2".into(), json!(m2(area)));
            extra.insert("level".into(), json!(m.level_of(e)));
            extra.insert("pattern_origin".into(), json!(r.pattern_origin));
            extra.insert("pattern_angle".into(), json!(r.pattern_angle));
        }
        Element::DetailLine(l) => {
            let length = l.curve.length();
            extra.insert("shape".into(), json!(l.curve.kind()));
            extra.insert("length".into(), json!(hundredth(length)));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::Spot(sp) if sp.slope => {
            let read = m.spot_slope(sp);
            extra.insert("slope".into(), json!(read.map(|(f, _)| hundredth(f))));
            extra.insert("toward".into(), json!(read.and_then(|(_, d)| d).map(|d| [hundredth(d.0), hundredth(d.1)])));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::Spot(sp) => {
            extra.insert("elevation".into(), json!(m.spot_value(sp).map(hundredth)));
            extra.insert("level".into(), json!(m.level_of(e)));
        }
        Element::Link(l) => {
            extra.insert("loaded".into(), json!(m.loaded.contains_key(&l.path)));
            extra.insert("elements".into(), json!(m.loaded.get(&l.path).map(|o| o.elements.len())));
        }
        // Its Computation Height, which its JSON leaves out at the usual 1200 mm.
        Element::Level(l) => {
            extra.insert("computation_height".into(), json!(l.computation_height));
        }
        Element::Dimension3D(d) => {
            extra.insert("length".into(), json!(hundredth(d.length())));
        }
        Element::Grid(_) | Element::Schedule(_) | Element::View3D(_) | Element::PlanView(_) => {}
    }
    if measures {
        extra.extend(measured(m, id));
    }
    let mut v = serde_json::to_value(e).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.extend(extra);
    }
    v
}

/// The families, each its category, its types and its parameters' names.
/// A connector, worked out, as JSON: an electrical one's load in VA in all and in each phase; a pipe one's domain, flow and
/// size; and, placed, where it is and the way it faces in the model.
pub fn connector_json((c, at, direction): &crate::family::PlacedConnector) -> Value {
    let mut o = json!({"id": c.id, "guid": c.guid, "kind": c.kind});
    if !c.name.is_empty() {
        o["name"] = json!(c.name);
    }
    if c.kind.electrical() {
        for (k, v) in [
            ("system", json!(c.system)),
            ("poles", json!(c.poles)),
            ("voltage", json!(c.voltage)),
            ("load_va", json!(c.load_va)),
            ("load_va_by_phase", json!(c.load_va_by_phase)),
            ("power_factor", json!(c.power_factor)),
            ("classification", json!(c.classification)),
        ] {
            o[k] = v;
        }
    }
    if let Some(d) = c.domain {
        o["domain"] = json!(d);
    }
    if c.kind == crate::family::ConnectorKind::Pipe || c.flow != crate::family::Flow::Both {
        o["flow"] = json!(c.flow);
    }
    if let Some(x) = c.size {
        o["size"] = json!(x);
    }
    if let Some(p) = at {
        o["at"] = json!(p);
    }
    if let Some(d) = direction {
        o["direction"] = json!(d);
    }
    o
}

pub fn hundredth(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// mm³ as m³, to the litre.
pub fn m3(mm3: f64) -> f64 {
    (mm3 / 1e6).round() / 1e3
}

/// mm³ as m³, to the cubic centimetre: a panel's and a mullion's, which a litre would round by a tenth.
pub fn fine_m3(mm3: f64) -> f64 {
    (mm3 / 1e3).round() / 1e6
}

/// mm as m, to the centimetre.
pub fn m_of(mm: f64) -> f64 {
    (mm / 10.0).round() / 100.0
}

/// A mass's footprint area (m²), height (mm) and volume (m³).
pub fn mass_size(m: &Model, ms: &Mass) -> (f64, f64, f64) {
    let (bottom, top) = m.span(ms.level, ms.base_offset, ms.top_level, ms.height);
    (m2(geom::area(&ms.boundary).abs()), top - bottom, (m.mass_volume(ms, bottom, top) / 1e7).round() / 100.0)
}

/// A stair's or a ramp's runs, each from its foot to its top, and where its last one ends.
pub fn runs(extra: &mut Map<String, Value>, l: &stairs::Layout) {
    let runs: Vec<[P2; 2]> = l.runs.iter().map(|r| [micron(r.start), micron(r.end())]).collect();
    extra.insert("end".into(), json!(runs.last().map(|r| r[1])));
    extra.insert("runs".into(), json!(runs));
}

/// A floor's top at its lowest and its highest, mm above the project's zero, to the hundredth.
pub fn top_range(m: &Model, f: &Floor) -> Value {
    let (lo, hi) = m
        .floor_faces(f)
        .iter()
        .flat_map(|(ring, plane)| ring.iter().map(move |q| plane.at(*q)))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
    json!([hundredth(lo), hundredth(hi)])
}

/// A family's parameters' values, worked out, as JSON.
pub fn values_json(v: BTreeMap<String, crate::family::Val>) -> Value {
    Value::Object(
        v.into_iter()
            .map(|(k, v)| match v {
                crate::family::Val::Number(x) => (k, json!(x)),
                crate::family::Val::Text(t) => (k, json!(t)),
            })
            .collect::<Map<String, Value>>(),
    )
}

pub fn read_point(p: P2) -> Value {
    json!([read_mm(p.0), read_mm(p.1)])
}

/// A length or a coordinate as `cad_read` gives it: mm to the hundredth, never -0.
pub fn read_mm(x: f64) -> f64 {
    hundredth(x) + 0.0
}
