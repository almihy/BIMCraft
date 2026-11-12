//! `component_geometry`'s tests that drive the Session (moved out of `component_geometry.rs`).

use crate::family::Piece;

use crate::component_geometry::*;
use crate::engine::Session;
use serde_json::json;

#[test]
fn repeated_components_keep_their_placement_mirroring_and_instance_parameters() {
    let mut s = Session::new();
    s.execute(
        "family",
        &json!({"name": "Probe", "parameters": [{"name": "Height", "instance": true, "value": 750}],
        "types": {"A": {}}, "forms": [{"profile": [[0,0],[800,0],[800,400],[0,400]], "to": "Height"}]}),
    )
    .unwrap();
    for (angle, mirrored, params) in
        [(0, false, json!({})), (45, false, json!({})), (90, true, json!({})), (180, false, json!({"Height": 1200}))]
    {
        s.execute(
            "component",
            &json!({"family":"Probe", "at":[angle*10,1000], "angle": angle,
            "offset": 100, "mirrored":mirrored, "params": params}),
        )
        .unwrap();
    }
    let first = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Component(_))).map(|(id, _)| *id).unwrap();
    s.execute("material", &json!({"name": "Test paint", "color": [90, 120, 150]})).unwrap();
    s.execute("paint", &json!({"id": first, "face": "all", "material": "Test paint"})).unwrap();
    let mut cache = Components::new(&s.model);
    for _ in 0..2 {
        for e in s.model.elements.values() {
            if let crate::model::Element::Component(c) = e {
                let a = s.model.component_pieces(c);
                let b = cache.pieces(c);
                assert_eq!(a, b);
                let heights = a.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.bottom), hi.max(p.top)));
                assert_eq!(cache.heights(c), heights);
            }
        }
    }
    let shared = SharedComponents::new(&s.model, &|| false).unwrap();
    assert_eq!(shared.shapes.len(), 3);
    assert_eq!(shared.instances.len(), 4);
    for instance in &shared.instances {
        let Some(crate::model::Element::Component(c)) = s.model.elements.get(instance.owner) else { panic!("component") };
        let placed = s.model.component_pieces(c);
        let local = &shared.shapes[instance.shape];
        let bounds = instance.bounds(shared.bounds[instance.shape].unwrap());
        assert_eq!(placed.len(), local.len());
        for (p, shape) in placed.iter().zip(local) {
            assert_eq!(p.tris, shape.tris.iter().map(|t| t.map(|q| instance.place(q))).collect::<Vec<_>>());
            for (world, local) in p.tris.iter().zip(&shape.tris) {
                for point in world {
                    for axis in 0..3 {
                        assert!(point[axis] >= bounds[0][axis] - 1e-9 && point[axis] <= bounds[1][axis] + 1e-9);
                    }
                }
                let normal = |[a, b, c]: &[[f64; 3]; 3]| {
                    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
                };
                let expected = instance.normal(normal(local));
                let actual = crate::geom::v3::unit(normal(world));
                for axis in 0..3 {
                    assert!((actual[axis] - expected[axis]).abs() <= expected[axis].abs().max(1.0) * 1e-9);
                }
            }
        }
    }
}

#[test]
#[ignore = "measures the large tower fixture"]
fn shared_component_shape_count_in_the_ten_storey_tower() {
    let model = crate::sample::tower::annotated(crate::sample::tower::Plate::REFERENCE, 10).unwrap();
    let shared = SharedComponents::new(&model, &|| false).unwrap();
    let tris = |shape: &Vec<Piece>| shape.iter().map(|p| p.tris.len()).sum::<usize>();
    let unique = shared.shapes.iter().map(&tris).sum::<usize>();
    let placed = shared.instances.iter().map(|i| tris(&shared.shapes[i.shape])).sum::<usize>();
    eprintln!(
        "{} instances, {} shapes, {} placed triangles, {} unique triangles",
        shared.instances.len(),
        shared.shapes.len(),
        placed,
        unique
    );
    assert!(placed > unique * 10);
}
