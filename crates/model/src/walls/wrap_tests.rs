//! Layer wrapping, by Autodesk's pictures of it (Revit 2027 help, About Layer Wrapping).

use super::*;
use crate::model::{Function, Layer};

/// Brick 100 and insulation 60 that wrap, block 140, plaster 15 that wraps: 315 thick, its core the block (160 to 300).
fn wall() -> WallType {
    let layer = |function, thickness, wraps| Layer { function, thickness, wraps, ..Layer::default() };
    WallType {
        thickness: 315.0,
        layers: vec![
            layer(Function::Finish, 100.0, true),
            layer(Function::Thermal, 60.0, true),
            layer(Function::Structure, 140.0, false),
            layer(Function::Finish2, 15.0, true),
        ],
        ..WallType::default()
    }
}

fn free(len: f64, ends: Wrapping) -> Vec<Body> {
    bodies(len, &[], [ends, ends], Wrapping::None)
}

#[test]
fn with_no_wrapping_every_layer_runs_the_walls_length() {
    let (pieces, seams) = shape(&wall(), &free(2000.0, Wrapping::None));
    assert_eq!(
        pieces,
        [
            vec![[0.0, 2000.0, 0.0, 100.0]],
            vec![[0.0, 2000.0, 100.0, 160.0]],
            vec![[0.0, 2000.0, 160.0, 300.0]],
            vec![[0.0, 2000.0, 300.0, 315.0]]
        ]
    );
    assert_eq!(seams, [[[0.0, 100.0], [2000.0, 100.0]], [[0.0, 160.0], [2000.0, 160.0]], [[0.0, 300.0], [2000.0, 300.0]]]);
}

#[test]
fn exterior_layers_wrap_the_ends_one_inside_another_to_the_cores_far_side() {
    let (pieces, seams) = shape(&wall(), &free(2000.0, Wrapping::Exterior));
    // The brick runs the whole length and down each end, 100 thick, from its own inside (100) to the plaster (300).
    assert_eq!(pieces[0], [[0.0, 2000.0, 0.0, 100.0], [0.0, 100.0, 100.0, 300.0], [1900.0, 2000.0, 100.0, 300.0]]);
    // The insulation stops inside the brick's returns and turns down inside them, 60 thick.
    assert_eq!(pieces[1], [[100.0, 1900.0, 100.0, 160.0], [100.0, 160.0, 160.0, 300.0], [1840.0, 1900.0, 160.0, 300.0]]);
    // The block is shorter by both returns at each end; the plaster, on the other side, is whole.
    assert_eq!(pieces[2], [[160.0, 1840.0, 160.0, 300.0]]);
    assert_eq!(pieces[3], [[0.0, 2000.0, 300.0, 315.0]]);
    // Between brick and insulation the line runs as far as the insulation does; between block and plaster, as the block.
    assert!(seams.contains(&[[100.0, 100.0], [1900.0, 100.0]]) && seams.contains(&[[160.0, 300.0], [1840.0, 300.0]]));
    // Round the brick's return at the start: its inner side, and its far end on the plaster; it has nothing outside it.
    assert!(seams.contains(&[[100.0, 100.0], [100.0, 300.0]]) && seams.contains(&[[0.0, 300.0], [100.0, 300.0]]));
    assert!(!seams.contains(&[[0.0, 100.0], [0.0, 300.0]]));
    // The insulation's has the brick's outside it: a line between the two.
    assert!(seams.contains(&[[100.0, 160.0], [100.0, 300.0]]));
    // Nothing is covered twice, and nothing left out: the pieces add up to the wall.
    let area: f64 = pieces.iter().flatten().map(|p| (p[1] - p[0]) * (p[3] - p[2])).sum();
    assert!((area - 2000.0 * 315.0).abs() < 1e-6, "{area}");
}

#[test]
fn interior_layers_wrap_up_to_the_cores_exterior_side() {
    let (pieces, _) = shape(&wall(), &free(2000.0, Wrapping::Interior));
    // The plaster turns up each end across the block, to the insulation (160); brick and insulation are whole.
    assert_eq!(pieces[3], [[0.0, 2000.0, 300.0, 315.0], [0.0, 15.0, 160.0, 300.0], [1985.0, 2000.0, 160.0, 300.0]]);
    assert_eq!(pieces[2], [[15.0, 1985.0, 160.0, 300.0]]);
    assert_eq!((pieces[0].clone(), pieces[1].clone()), (vec![[0.0, 2000.0, 0.0, 100.0]], vec![[0.0, 2000.0, 100.0, 160.0]]));
}

#[test]
fn only_layers_ticked_wraps_turn_the_corner() {
    let mut t = wall();
    t.layers[1].wraps = false;
    let (pieces, _) = shape(&t, &free(2000.0, Wrapping::Exterior));
    // The insulation ends square inside the brick's return, and the block with it.
    assert_eq!(pieces[1], [[100.0, 1900.0, 100.0, 160.0]]);
    assert_eq!(pieces[2], [[100.0, 1900.0, 160.0, 300.0]]);
}

#[test]
fn layers_wrap_at_the_jambs_of_an_insert_and_both_sides_meet_in_the_cores_middle() {
    // A window from 800 to 1800 in a wall whose ends join others: its ends are as they were, its jambs wrap.
    let b = bodies(3000.0, &[(800.0, 1800.0)], [Wrapping::None, Wrapping::None], Wrapping::Both);
    assert_eq!(
        b,
        [
            Body { from: 0.0, to: 800.0, start: Wrapping::None, end: Wrapping::Both },
            Body { from: 1800.0, to: 3000.0, start: Wrapping::Both, end: Wrapping::None }
        ]
    );
    let (pieces, _) = shape(&wall(), &b);
    // Left of the window: the brick to the jamb and across to the block's middle (230); the plaster up to it from inside.
    assert!(pieces[0].contains(&[0.0, 800.0, 0.0, 100.0]) && pieces[0].contains(&[700.0, 800.0, 100.0, 230.0]));
    assert!(pieces[3].contains(&[0.0, 800.0, 300.0, 315.0]) && pieces[3].contains(&[785.0, 800.0, 230.0, 300.0]));
    // The block is stepped at the jamb: 160 short where brick and insulation cross it, 15 where the plaster does.
    assert!(pieces[2].contains(&[0.0, 640.0, 160.0, 230.0]) && pieces[2].contains(&[0.0, 785.0, 230.0, 300.0]));
    // Right of it, the same turned round.
    assert!(pieces[0].contains(&[1800.0, 1900.0, 100.0, 230.0]) && pieces[2].contains(&[1960.0, 3000.0, 160.0, 230.0]));
    // An opening past the wall's end leaves no body beyond it.
    assert_eq!(
        bodies(3000.0, &[(2500.0, 3400.0)], [Wrapping::Exterior; 2], Wrapping::Interior),
        [Body { from: 0.0, to: 2500.0, start: Wrapping::Exterior, end: Wrapping::Interior }]
    );
}
