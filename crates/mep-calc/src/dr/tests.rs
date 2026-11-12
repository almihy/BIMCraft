//! The circuits NBR 5410:2004 asks a high-sensitivity residual-current device (30 mA at most) for, by the table of its
//! "additional protection", worked by hand.

use super::*;

fn point(place: Place, kind: Kind) -> Point {
    Point { place, kind, rating: None, height: None, outdoor: false }
}

fn circuit(points: Vec<Point>) -> Circuit {
    Circuit { points, residual: None, it_continuity: false }
}

fn required(c: &Circuit, residential: bool) -> bool {
    check(c, residential).required
}

#[test]
fn every_point_where_there_is_a_bath_or_a_shower_needs_it() {
    for kind in [Kind::Outlet, Kind::Lighting, Kind::Fixed] {
        assert!(required(&circuit(vec![point(Place::Bath, kind)]), true), "{kind:?}");
        assert!(required(&circuit(vec![point(Place::Bath, kind)]), false), "{kind:?}");
    }
}

#[test]
fn outlets_in_external_areas_need_it() {
    assert!(required(&circuit(vec![point(Place::External, Kind::Outlet)]), true));
    assert!(required(&circuit(vec![point(Place::External, Kind::Outlet)]), false));
    // Its lighting is not an outlet.
    assert!(!required(&circuit(vec![point(Place::External, Kind::Lighting)]), true));
}

#[test]
fn an_inside_outlet_that_may_feed_equipment_outdoors_needs_it() {
    let mut p = point(Place::Dry, Kind::Outlet);
    assert!(!required(&circuit(vec![p.clone()]), true));
    p.outdoor = true;
    assert!(required(&circuit(vec![p]), true));
}

#[test]
fn in_a_dwelling_every_point_of_the_wet_rooms_needs_it() {
    for place in [Place::Kitchen, Place::Laundry, Place::Service, Place::Garage, Place::Wet] {
        for kind in [Kind::Outlet, Kind::Fixed, Kind::Lighting] {
            assert!(required(&circuit(vec![point(place, kind)]), true), "{place:?} {kind:?}");
        }
    }
    assert!(!required(&circuit(vec![point(Place::Dry, Kind::Outlet)]), true), "a bedroom's outlets, on their own, do not");
}

#[test]
fn a_light_2_5_m_up_or_more_in_a_dwelling_is_left_out() {
    let mut p = point(Place::Kitchen, Kind::Lighting);
    p.height = Some(2500.0);
    assert!(!required(&circuit(vec![p.clone()]), true));
    p.height = Some(2499.0);
    assert!(required(&circuit(vec![p.clone()]), true));
    // It does not reach the outlets, or a bath.
    let mut o = point(Place::Kitchen, Kind::Outlet);
    o.height = Some(3000.0);
    assert!(required(&circuit(vec![o]), true));
    let mut b = point(Place::Bath, Kind::Lighting);
    b.height = Some(3000.0);
    assert!(required(&circuit(vec![b]), true));
}

#[test]
fn outside_a_dwelling_only_the_outlets_of_the_wet_rooms_need_it() {
    assert!(required(&circuit(vec![point(Place::Kitchen, Kind::Outlet)]), false));
    assert!(!required(&circuit(vec![point(Place::Kitchen, Kind::Lighting)]), false));
    assert!(!required(&circuit(vec![point(Place::Laundry, Kind::Fixed)]), false));
}

#[test]
fn an_outlet_above_32_a_is_not_covered() {
    let mut p = point(Place::External, Kind::Outlet);
    p.rating = Some(32.0);
    assert!(required(&circuit(vec![p.clone()]), true));
    p.rating = Some(32.5);
    assert!(!required(&circuit(vec![p]), true));
    // Note 1 reaches every outlet, a bathroom's too; what else is in the circuit still asks for it.
    let mut b = point(Place::Bath, Kind::Outlet);
    b.rating = Some(63.0);
    assert!(!required(&circuit(vec![b.clone()]), true));
    assert!(required(&circuit(vec![b, point(Place::Bath, Kind::Lighting)]), true));
    let mut k = point(Place::Kitchen, Kind::Outlet);
    k.rating = Some(63.0);
    assert!(!required(&circuit(vec![k]), true));
}

#[test]
fn an_it_circuit_that_must_not_be_cut_is_left_out() {
    let mut c = circuit(vec![point(Place::Bath, Kind::Outlet)]);
    c.it_continuity = true;
    let r = check(&c, true);
    assert!(!r.required && r.exempt, "{r:?}");
}

#[test]
fn a_circuit_asks_for_it_if_any_of_its_points_does_and_says_why() {
    let c = circuit(vec![point(Place::Dry, Kind::Outlet), point(Place::External, Kind::Outlet), point(Place::Bath, Kind::Fixed)]);
    let r = check(&c, true);
    assert!(r.required);
    assert_eq!(r.reasons.len(), 2, "{:?}", r.reasons);
    assert!(r.reasons[0].contains("external") && r.reasons[1].contains("bath"), "{:?}", r.reasons);
}

#[test]
fn what_it_has_is_judged_against_30_ma() {
    let mut c = circuit(vec![point(Place::Bath, Kind::Outlet)]);
    assert_eq!(check(&c, true).ok, Some(false), "none given: it is missing");
    c.residual = Some(30.0);
    assert_eq!(check(&c, true).ok, Some(true));
    c.residual = Some(300.0);
    assert_eq!(check(&c, true).ok, Some(false));
    c.residual = Some(0.0);
    assert_eq!(check(&c, true).ok, Some(true), "a 0 mA is nonsense, but no more than 30");
    // A circuit that does not need it has no verdict.
    let free = circuit(vec![point(Place::Dry, Kind::Lighting)]);
    assert_eq!(check(&free, true).ok, None);
}

#[test]
fn places_are_known_by_a_name() {
    for (name, place) in [
        ("bath", Place::Bath),
        ("external", Place::External),
        ("kitchen", Place::Kitchen),
        ("laundry", Place::Laundry),
        ("service", Place::Service),
        ("garage", Place::Garage),
        ("wet", Place::Wet),
        ("dry", Place::Dry),
    ] {
        assert_eq!(Place::parse(name), Some(place));
    }
    assert_eq!(Place::parse("attic"), None);
    assert_eq!(Kind::parse("outlet"), Some(Kind::Outlet));
    assert_eq!(Kind::parse("lamp"), None);
}
