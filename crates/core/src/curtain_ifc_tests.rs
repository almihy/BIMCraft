//! Curtain walls out to IFC and back: an `IfcCurtainWall` with no body, its panels `IfcPlate`s and its mullions `IfcMember`s
//! aggregated under it, and BIMCraft's own file read back as the curtain wall it was.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::curtain::{self, GridLine};
use crate::engine::Session;
use crate::model::{Element, Id, Model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn storefront(s: &mut Session) -> Id {
    run(s, "wall", json!({"points": [[1000, 2000], [7000, 2000]], "type": "Storefront", "height": 3000}))["ids"][0].as_u64().unwrap()
}

/// A STEP file's entities: each one's class and arguments, by its number.
fn entities(step: &str) -> BTreeMap<u64, (String, String)> {
    step.lines()
        .filter_map(|l| {
            let (id, rest) = l.strip_prefix('#')?.split_once('=')?;
            let (class, args) = rest.split_once('(')?;
            Some((id.parse().ok()?, (class.to_string(), args.trim_end_matches(';').trim_end_matches(')').to_string())))
        })
        .collect()
}

/// The entities `text` refers to, in order.
fn references(text: &str) -> Vec<u64> {
    text.split('#').skip(1).filter_map(|t| t.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()).collect()
}

/// The numbers among an entity's arguments, in order.
fn numbers(args: &str) -> Vec<f64> {
    args.split([',', '(', ')']).filter_map(|t| t.trim().parse::<f64>().ok()).collect()
}

struct Step {
    all: BTreeMap<u64, (String, String)>,
}

impl Step {
    fn of(&self, class: &str) -> Vec<(u64, &str)> {
        self.all.iter().filter(|(_, (c, _))| c == class).map(|(id, (_, a))| (*id, a.as_str())).collect()
    }

    fn args(&self, id: u64) -> &str {
        self.all.get(&id).map_or("", |(_, a)| a.as_str())
    }

    /// The relating and related entities of each relation of `class`: what comes last, and the list before it (or the other
    /// way round, as `IfcRelAggregates` has its whole first).
    fn related(&self, class: &str) -> Vec<Vec<u64>> {
        self.of(class).into_iter().map(|(_, a)| references(a)).collect()
    }

    /// The quantities of the quantity set named `name` that product `id` has.
    fn quantities(&self, id: u64, name: &str) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        for refs in self.related("IFCRELDEFINESBYPROPERTIES").into_iter().filter(|r| r.first() == Some(&id)) {
            let Some(set) = refs.last().filter(|s| self.args(**s).contains(&format!("'{name}'"))) else { continue };
            for q in references(self.args(*set)) {
                let a = self.args(q);
                if let (Some(n), Some(v)) = (a.split('\'').nth(1), numbers(a).last()) {
                    out.insert(n.to_string(), *v);
                }
            }
        }
        out
    }

    /// The body of product `id`: its extrusion's depth and the sizes of its profile (a rectangle's two, a circle's radius).
    fn body(&self, id: u64) -> (f64, Vec<f64>) {
        let shape = references(self.args(id)).into_iter().find(|r| self.all.get(r).is_some_and(|(c, _)| c == "IFCPRODUCTDEFINITIONSHAPE"));
        let solid = shape
            .into_iter()
            .flat_map(|s| references(self.args(s)))
            .flat_map(|rep| references(self.args(rep)))
            .find(|x| self.all.get(x).is_some_and(|(c, _)| c == "IFCEXTRUDEDAREASOLID"))
            .unwrap_or_else(|| panic!("#{id} has no swept body"));
        let depth = *numbers(self.args(solid)).last().unwrap();
        let profile = references(self.args(solid))[0];
        let sizes = numbers(self.args(profile));
        (depth, sizes)
    }
}

fn step(m: &Model) -> Step {
    Step { all: entities(&crate::ifc::export(m)) }
}

#[test]
fn a_curtain_wall_goes_out_as_a_curtain_wall_of_plates_and_members() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    run(&mut s, "wall", json!({"points": [[0, 9000], [6000, 9000]], "height": 3000}));
    let f = step(&s.model);
    // The wall: one IfcCurtainWall of an IfcCurtainWallType, in the storey, with its axis and no body.
    let walls = f.of("IFCCURTAINWALL");
    assert_eq!(walls.len(), 1);
    assert_eq!(f.of("IFCWALL").len(), 1, "the basic wall beside it stays a wall");
    let (wall, args) = walls[0];
    assert!(args.ends_with(&format!("'{w}',.NOTDEFINED.")), "{args}");
    assert!(args.contains("'Storefront'"));
    let bodies: Vec<&str> =
        references(args).into_iter().flat_map(|r| references(f.args(r))).map(|r| f.args(r)).filter(|a| a.contains("'Body'")).collect();
    assert!(bodies.is_empty(), "no body of its own: {bodies:?}");
    assert_eq!(f.of("IFCCURTAINWALLTYPE").len(), 1);
    let contained: Vec<u64> = f.related("IFCRELCONTAINEDINSPATIALSTRUCTURE").into_iter().flatten().collect();
    assert!(contained.contains(&wall));
    // Its parts: eight plates and twenty-two members, aggregated under it and not in the storey too.
    let (plates, members) = (f.of("IFCPLATE"), f.of("IFCMEMBER"));
    assert_eq!((plates.len(), members.len()), (8, 22));
    assert!(plates.iter().all(|(_, a)| a.ends_with(".CURTAIN_PANEL.")) && members.iter().all(|(_, a)| a.ends_with(".MULLION.")));
    let parts: BTreeSet<u64> = f.related("IFCRELAGGREGATES").into_iter().filter(|r| r.first() == Some(&wall)).flatten().skip(1).collect();
    let both: BTreeSet<u64> = plates.iter().chain(&members).map(|(id, _)| *id).collect();
    assert_eq!(parts, both);
    assert!(both.iter().all(|id| !contained.contains(id)), "a part is in its whole, not in the storey");
    assert_eq!((f.of("IFCPLATETYPE").len(), f.of("IFCMEMBERTYPE").len()), (1, 1));
    assert!(f.of("IFCPLATETYPE")[0].1.ends_with(".CURTAIN_PANEL.") && f.of("IFCMEMBERTYPE")[0].1.ends_with(".MULLION."));
    // Each part is made of its type's material, and says so.
    let made: Vec<u64> = f.related("IFCRELASSOCIATESMATERIAL").into_iter().flat_map(|r| r[..r.len() - 1].to_vec()).collect();
    assert!(both.iter().all(|id| made.contains(id)));
    // The property sets: IsExternal on each, as the type's Function says.
    for (class, set) in [("IFCCURTAINWALL", "Pset_CurtainWallCommon"), ("IFCPLATE", "Pset_PlateCommon"), ("IFCMEMBER", "Pset_MemberCommon")]
    {
        let sets = f.of("IFCPROPERTYSET").into_iter().filter(|(_, a)| a.contains(&format!("'{set}'"))).count();
        assert_eq!(sets, f.of(class).len(), "{set}");
    }
    assert_eq!(f.of("IFCPROPERTYSET").iter().filter(|(_, a)| a.contains("'Pset_WallCommon'")).count(), 1);
    // Every GlobalId in the file is its own: the wall's, its parts', their types', their sets' and their relations'.
    let ids: Vec<&str> = f.all.values().filter_map(|(_, a)| a.strip_prefix('\'')?.split('\'').next()).filter(|g| g.len() == 22).collect();
    assert!(ids.len() > 150, "{}", ids.len());
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len());
    // The wall's quantities: its length, height and depth and its face.
    let q = f.quantities(wall, "Qto_CurtainWallQuantities");
    assert_eq!((q["Length"], q["Height"], q["Width"], q["GrossSideArea"]), (6000.0, 3000.0, 150.0, 18.0));
}

#[test]
fn a_panel_and_a_mullion_hold_what_a_hand_calculation_gives() {
    let mut s = Session::new();
    storefront(&mut s);
    let f = step(&s.model);
    // A panel of the low row: 1500 wide, 2400 tall, 25 thick.
    let low: Vec<u64> = f.of("IFCPLATE").into_iter().map(|(id, _)| id).filter(|id| f.body(*id).0 == 2400.0).collect();
    assert_eq!(low.len(), 4);
    let (depth, sizes) = f.body(low[0]);
    assert_eq!((depth, sizes.as_slice()), (2400.0, &[1500.0, 25.0][..]));
    let q = f.quantities(low[0], "Qto_PlateBaseQuantities");
    assert_eq!((q["Width"], q["Perimeter"], q["GrossArea"], q["NetArea"]), (25.0, 7800.0, 3.6, 3.6));
    assert!((q["GrossVolume"] - 1.5 * 2.4 * 0.025).abs() < 1e-9, "{q:?}");
    // The mullions: 50 x 150 in section, and as long as their segments less where they stop at one that runs through. A
    // border's standing mullion runs the wall's whole height in two pieces.
    let mut lengths = 0.0;
    for (id, _) in f.of("IFCMEMBER") {
        let (length, sizes) = f.body(id);
        assert_eq!(sizes, [50.0, 150.0]);
        let q = f.quantities(id, "Qto_MemberBaseQuantities");
        assert_eq!(q["Length"], length);
        assert!((q["CrossSectionArea"] - 0.0075).abs() < 1e-12 && (q["GrossVolume"] - 0.0075 * length / 1000.0).abs() < 1e-6, "{q:?}");
        lengths += length;
    }
    let own: f64 = s.model.elements.values().filter_map(|e| if let Element::Mullion(x) = e { Some(x.length()) } else { None }).sum();
    assert!((lengths - own).abs() < 1e-6);
    // By hand, a lying mullion between two standing ones that run through it: its segment's 1500 less 25 at each end,
    // and 50 by 150 by 1450 mm is 0.010875 m³.
    let lying: Vec<u64> = f.of("IFCMEMBER").into_iter().map(|(id, _)| id).filter(|id| f.body(*id).0 == 1450.0).collect();
    assert!(!lying.is_empty());
    assert!((f.quantities(lying[0], "Qto_MemberBaseQuantities")["GrossVolume"] - 0.010875).abs() < 1e-6);
}

/// What a curtain wall is, to compare one model's with another's: its line and heights, its own grid settings, and each of
/// its lines, panels and mullions by what it is (never by its id, which a file gives anew).
fn facade(m: &Model, wall: Id) -> Value {
    let host = m.guid(wall);
    let w = m.wall(wall).unwrap();
    let mut parts: Vec<Value> = m
        .hosted_by(host)
        .into_iter()
        .filter_map(|id| match m.elements.get(id)? {
            Element::CurtainGrid(l) => Some(json!({"line": m.guid(id), "vertical": l.vertical, "at": l.at})),
            Element::CurtainPanel(p) => Some(json!({"panel": p.key, "type": p.type_name, "span": p.span, "free": p.free, "params": p.params})),
            Element::Mullion(x) => Some(json!({
                "mullion": x.key, "type": x.type_name, "at": x.at, "from": x.from, "to": x.to, "trim": x.trim, "free": x.free, "params": x.params,
            })),
            _ => None,
        })
        .collect();
    parts.sort_by_key(Value::to_string);
    json!({
        "guid": host, "type": w.type_name, "start": w.start, "end": w.end, "arc": w.arc, "height": w.height,
        "base_offset": w.base_offset, "curtain": w.curtain, "params": w.params, "level": m.level(w.level).map(|l| l.name.clone()),
        "parts": parts,
    })
}

#[test]
fn bimcrafts_own_curtain_wall_comes_back_from_ifc_as_it_was() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let curved = run(&mut s, "wall", json!({"points": [[0, -5000], [6000, -5000]], "arc": 60, "type": "Exterior Glazing", "height": 4000}));
    let curved = curved["ids"][0].as_u64().unwrap();
    run(&mut s, "type", json!({"category": "curtainpanel", "name": "Glazed", "type_mark": "GL1"}));
    run(&mut s, "parameter", json!({"name": "Finish", "categories": ["curtainpanel", "mullion"]}));
    // A line drawn by hand, a segment of the type's removed, a mullion deleted, a panel and a mullion unpinned and given
    // other types (one of them the empty panel, which has no body), and a parameter on a panel the wall placed.
    let host = s.model.guid(w);
    {
        let m = s.model_mut();
        m.insert(Element::CurtainGrid(GridLine { host, vertical: true, at: 700.0 }));
        curtain::keep_all(m);
        let g = curtain::grid(m, w).unwrap();
        let keys: Vec<String> = m
            .hosted_by(host)
            .into_iter()
            .filter_map(|id| match m.elements.get(id)? {
                Element::Mullion(x) if x.vertical && x.border == 0 => Some(x.key.clone()),
                _ => None,
            })
            .collect();
        let Some(Element::Wall(wall)) = m.elements.get_mut(w) else { panic!() };
        wall.curtain.merged.insert(curtain::segment_key(&g.u[3].key, &g.v[0].key));
        wall.curtain.no_mullion.insert(keys[0].clone());
        wall.curtain.horizontal.offset = 150.0;
        curtain::keep_all(m);
        let parts = m.hosted_by(host);
        let mut freed = (false, false);
        for id in parts {
            match m.elements.get_mut(id) {
                Some(Element::CurtainPanel(p)) if !freed.0 => {
                    (p.free, p.type_name, freed.0) = (true, "Empty".into(), true);
                }
                Some(Element::CurtainPanel(p)) if p.params.is_empty() && p.span[0] > 3000.0 => {
                    p.params.insert("Finish".into(), json!("Fritted"));
                }
                Some(Element::Mullion(x)) if !freed.1 && !x.vertical => {
                    (x.free, x.type_name, freed.1) = (true, "30mm Square".into(), true);
                }
                _ => {}
            }
        }
        curtain::keep_all(m);
        m.check().unwrap();
    }
    let text = crate::ifc::export(&s.model);
    let back = crate::import::ifc(text.as_bytes(), "x").unwrap();
    assert_eq!(back.skipped, Vec::<String>::new());
    let t = &back.model;
    t.check().unwrap();
    // The same walls, by their GUIDs, each with the same lines, panels and mullions.
    for wall in [w, curved] {
        let again = t.elements.id(s.model.guid(wall)).expect("the wall, by its GUID");
        assert_eq!(facade(t, again), facade(&s.model, wall));
    }
    // The same types, and nothing generic standing in for a part.
    // (The panel and mullion types the walls and their parts name: a type nothing uses is not in the file.)
    assert_eq!(t.types.curtain_panel.keys().collect::<Vec<_>>(), ["Empty", "Glazed"]);
    assert_eq!(t.types.mullion.keys().collect::<Vec<_>>(), ["30mm Square", "50 x 150mm"]);
    assert!(t.types.curtain_panel.iter().all(|(n, x)| s.model.types.curtain_panel.get(n) == Some(x)));
    assert!(t.types.mullion.iter().all(|(n, x)| s.model.types.mullion.get(n) == Some(x)));
    for name in ["Storefront", "Exterior Glazing"] {
        assert_eq!(t.types.wall[name], s.model.types.wall[name]);
    }
    assert!(t.elements.values().all(|e| !matches!(e, Element::Component(_))));
    let count = |m: &Model, c: &str| m.elements.values().filter(|e| e.category() == c).count();
    for c in ["wall", "curtainpanel", "mullion", "curtaingrid"] {
        assert_eq!(count(t, c), count(&s.model, c), "{c}");
    }
    // (A parameter comes back for the categories of what holds a value of it.)
    assert_eq!(t.parameters["Finish"].categories, ["curtainpanel"]);
    // And out again, the same file but for its date.
    let again = crate::ifc::export(t);
    assert_eq!(entities(&again).len(), entities(&text).len());
}

/// The box round each element's solids, in whole millimetres, in order.
fn boxes(m: &Model) -> Vec<[i64; 6]> {
    let mut by: BTreeMap<Id, [f64; 6]> = BTreeMap::new();
    for solid in crate::solids::solids(m) {
        let b = by.entry(solid.owner()).or_insert([f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN]);
        for c in solid.corners() {
            for k in 0..3 {
                (b[k], b[k + 3]) = (b[k].min(c[k]), b[k + 3].max(c[k]));
            }
        }
    }
    let mut out: Vec<[i64; 6]> = by.into_values().map(|b| b.map(|x| x.round() as i64)).collect();
    out.sort_unstable();
    out
}

#[test]
fn another_programs_curtain_wall_and_a_damaged_set_keep_coming_in_as_generic_parts() {
    let mut s = Session::new();
    storefront(&mut s);
    let text = crate::ifc::export(&s.model);
    // Without BIMCraft's own set, the file is any program's: its plates and members come in as components that remember
    // their class, where their boxes are, and the curtain wall is left out as the whole of them.
    let foreign = text.replace("'BIMCraft_CurtainWall'", "'Other_CurtainWall'");
    let damaged = text.replace(r#"{"type":"Storefront""#, r#"{"type":7"#);
    assert_ne!(damaged, text);
    let boxes = boxes(&s.model);
    assert_eq!(boxes.len(), 30);
    // By hand: the first panel of the low row, 25 mm of glass about the wall's line, between the first two grid lines;
    // and the mullion on the wall's start, 50 wide inside the wall and 150 deep about its line, as tall as the low row.
    assert!(boxes.contains(&[1000, 1988, 0, 2500, 2013, 2400]) || boxes.contains(&[1000, 1987, 0, 2500, 2012, 2400]), "{boxes:?}");
    assert!(boxes.contains(&[1000, 1925, 0, 1050, 2075, 2425]), "{boxes:?}");
    for file in [foreign, damaged] {
        let back = crate::import::ifc(file.as_bytes(), "x").unwrap();
        let t = &back.model;
        t.check().unwrap();
        assert!(t.elements.values().all(|e| !matches!(e, Element::Wall(_) | Element::CurtainPanel(_) | Element::Mullion(_))));
        let components: Vec<_> = t.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c) } else { None }).collect();
        assert_eq!(components.len(), 30);
        let classes: BTreeSet<String> =
            components.iter().filter_map(|c| t.families.get(&c.family)?.ifc.as_ref().map(|i| i.class.clone())).collect();
        assert_eq!(classes, BTreeSet::from(["IFCMEMBER".to_string(), "IFCPLATE".to_string()]));
        assert!(back.skipped.iter().any(|l| l.contains("IFCCURTAINWALL")), "{:?}", back.skipped);
        // Each stands where the panel or mullion it was does: the bodies and the placements the file gave are theirs.
        assert_eq!(self::boxes(t), boxes);
        // Out again as the plates and members they were, the wall no more.
        let f = Step { all: entities(&crate::ifc::export(t)) };
        assert_eq!((f.of("IFCPLATE").len(), f.of("IFCMEMBER").len(), f.of("IFCCURTAINWALL").len()), (8, 22, 0));
    }
}
