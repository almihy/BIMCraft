//! Layer join cleanup, by the pictures Revit draws of it.

use super::*;

const CORE: Rank = Rank { shell: false, priority: 1 };
const FINISH: Rank = Rank { shell: true, priority: 4 };
const FINISH2: Rank = Rank { shell: true, priority: 5 };
const THERMAL: Rank = Rank { shell: true, priority: 3 };

/// A wall that ends at the join, crossing the other's layers in `order`.
fn ending(ranks: &[Rank], order: &[usize]) -> Side {
    Side { ranks: ranks.to_vec(), arrives: vec![order.to_vec()] }
}

/// A wall that runs on through the join, crossing the other's `n` layers from either side.
fn through(ranks: &[Rank], n: usize) -> Side {
    Side { ranks: ranks.to_vec(), arrives: vec![(0..n).collect(), (0..n).rev().collect()] }
}

/// What each layer holds, one way in.
fn one(r: &Reach) -> Vec<usize> {
    r.iter().map(|l| l[0]).collect()
}

#[test]
fn at_a_butt_corner_the_finishes_wrap_and_the_cores_join_as_an_l() {
    // Finish, core, finish in both, exterior first. Each comes to the corner meeting the other's interior side first.
    let wall = [FINISH, CORE, FINISH];
    let (a, b) = solve(&ending(&wall, &[2, 1, 0]), &ending(&wall, &[2, 1, 0]), true);
    // The wall running through: its exterior finish to the outer corner (all three cells), its core through the other's
    // interior finish and core (two), its interior finish the inner corner's cell.
    assert_eq!(one(&a), [3, 2, 1]);
    // The one that stops: its exterior finish up the outside past the other's interior finish and core, to under the other's
    // exterior finish; its core through the other's interior finish to its core; its interior finish at the inner face.
    assert_eq!(one(&b), [2, 1, 0]);
    // The other wall through: the same, the other way round.
    let (a, b) = solve(&ending(&wall, &[2, 1, 0]), &ending(&wall, &[2, 1, 0]), false);
    assert_eq!((one(&a), one(&b)), (vec![2, 1, 0], vec![3, 2, 1]));
}

#[test]
fn at_a_t_the_ending_walls_core_passes_through_the_finish_to_the_core() {
    let wall = [FINISH, CORE, FINISH];
    // The ending wall comes to the continuous wall's interior side.
    let (a, b) = solve(&through(&wall, 3), &ending(&wall, &[2, 1, 0]), true);
    // The continuous wall: its exterior finish whole (three cells from either side), its core whole (to the ending wall's core
    // and into it from either side), its interior finish broken: one cell from either side, the ending wall's core between.
    assert_eq!(a, [vec![3, 3], vec![2, 2], vec![1, 1]]);
    // The ending wall: its finishes stop at the face, its core runs one layer in.
    assert_eq!(one(&b), [0, 1, 0]);
}

#[test]
fn a_partition_of_one_layer_meets_the_core_of_a_wall_it_ends_on() {
    // A wall without layers is its core: it passes the plaster and stops on the block.
    let (a, b) = solve(&through(&[FINISH, CORE, FINISH], 1), &ending(&[CORE], &[2, 1, 0]), true);
    assert_eq!(a, [vec![1, 1], vec![1, 1], vec![0, 0]]);
    assert_eq!(one(&b), [1]);
}

#[test]
fn a_layer_does_not_pass_one_of_its_own_or_a_better_priority() {
    // Brick (finish 1), insulation, block, plaster (finish 2), against the same: at a corner the insulation passes the
    // other's plaster, meets no insulation on the way in, and stops under the other's block.
    let wall = [FINISH, THERMAL, CORE, FINISH2];
    let (a, b) = solve(&ending(&wall, &[3, 2, 1, 0]), &ending(&wall, &[3, 2, 1, 0]), true);
    // Through: brick to the corner (4); insulation past plaster and block's column is its own core's... the other's core
    // column is free above the through core, then it meets the other's insulation (4: plaster, block, insulation's cell won);
    // block through plaster and the other's block (2); plaster the inner cell (1).
    assert_eq!(one(&a), [4, 3, 2, 1]);
    // Stopping: brick past plaster, block and insulation rows (3); insulation past plaster and block rows, stopped at the
    // other's insulation (2); block through the plaster (1); plaster at the face (0).
    assert_eq!(one(&b), [3, 2, 1, 0]);
}

#[test]
fn a_core_of_any_function_outranks_a_finish_outside_a_core() {
    // Revit: layers in the core pass through layers outside another wall's core, even set to Finish 2.
    let odd = [FINISH, Rank { shell: false, priority: 5 }, FINISH];
    let (_, b) = solve(&through(&[FINISH, CORE, FINISH], 3), &ending(&odd, &[2, 1, 0]), true);
    assert_eq!(one(&b), [0, 1, 0]);
}

#[test]
fn ranks_come_from_the_types_core_boundaries() {
    use crate::model::{Core, Layer};
    let layer = |function| Layer { function, thickness: 10.0, ..Layer::default() };
    let mut t = WallType {
        thickness: 40.0,
        layers: vec![layer(Function::Finish), layer(Function::Substrate), layer(Function::Structure), layer(Function::Finish2)],
        ..WallType::default()
    };
    assert_eq!(Rank::of(&t), [FINISH, Rank { shell: true, priority: 2 }, CORE, FINISH2]);
    // The substrate taken into the core.
    t.core = Some(Core { exterior: 1, interior: 1 });
    assert_eq!(Rank::of(&t), [FINISH, Rank { shell: false, priority: 2 }, CORE, FINISH2]);
    assert_eq!(Rank::of(&WallType { thickness: 200.0, ..WallType::default() }), [CORE]);
    // Edit Assembly's Priority column, apart from the function; and a core holding no layer.
    t.layers[0].priority = Some(2);
    t.core = Some(Core { exterior: 0, interior: 4 });
    assert_eq!(Rank::of(&t).iter().map(|r| (r.shell, r.priority)).collect::<Vec<_>>(), [(true, 2), (true, 2), (true, 1), (true, 5)]);
}
