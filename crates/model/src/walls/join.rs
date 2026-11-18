//! How the layers of two joined walls clean up, as Revit's layer join cleanup: which wall's layer holds each cell of the patch
//! where the two walls overlap. A cell is the crossing of one layer of each wall. A layer's rank is whether it is in its
//! wall's core (the core first), then its priority (1 to 5: its function's, Structure 1 to Finish 2 5, unless Edit Assembly's Priority
//! column gives it another). Layers are settled best rank
//! first: a layer runs from its own wall into the patch, cell after cell, through the other wall's layers of a worse rank;
//! it stops before a cell a better layer has taken; and where it reaches a layer of its own rank that reaches it too, the two
//! join there and both stop, the cell going to the wall that runs through the join. Nothing here knows where the walls are:
//! [`solve`] takes the order each wall's layers cross the other's in, and says how far each gets.

use std::collections::BTreeSet;

use crate::model::{Function, WallType};

/// A layer's standing where walls join: in the core before outside it, then by its function's priority (1 the best).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rank {
    pub shell: bool,
    pub priority: u8,
}

impl Rank {
    /// The rank of each layer of type `t`, exterior first; a type without layers is one structure layer, its core. A membrane
    /// has no priority of its own: the worst there is, and no thickness to hold a cell with.
    pub fn of(t: &WallType) -> Vec<Rank> {
        if t.layers.is_empty() {
            return vec![Rank { shell: false, priority: 1 }];
        }
        let core = t.core_layers();
        t.layers
            .iter()
            .enumerate()
            .map(|(k, l)| Rank { shell: !core.contains(&k), priority: l.join_priority().unwrap_or(Function::ALL.len() as u8) })
            .collect()
    }
}

/// One of the two walls at a join: its layers' ranks, exterior first, and each way it comes into the patch (one for a wall
/// that ends there, two for a wall that runs on through it, from either side): the other wall's layers, by index, in the
/// order its own layers cross them.
#[derive(Clone, Debug)]
pub struct Side {
    pub ranks: Vec<Rank>,
    pub arrives: Vec<Vec<usize>>,
}

/// How far each layer of a wall gets into the patch: for each layer, for each way its wall comes in, how many of the other
/// wall's layers it crosses and holds, counted along that way.
pub type Reach = Vec<Vec<usize>>;

/// A layer of one of the two walls coming in one way.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Runner {
    first: bool,
    layer: usize,
    way: usize,
}

/// How far each layer of `a` and of `b` gets. Where two layers of one rank meet, the cell is `a`'s with `a_through`, else
/// `b`'s.
pub fn solve(a: &Side, b: &Side, a_through: bool) -> (Reach, Reach) {
    let side = |first: bool| if first { a } else { b };
    // The cell a runner is in after `k` steps, as (layer of a, layer of b).
    let cell = |r: Runner, k: usize| -> Option<(usize, usize)> {
        let other = *side(r.first).arrives.get(r.way)?.get(k)?;
        Some(if r.first { (r.layer, other) } else { (other, r.layer) })
    };
    let rank = |first: bool, layer: usize| side(first).ranks.get(layer).copied();
    let mut held: [Reach; 2] = [a, b].map(|s| vec![vec![0; s.arrives.len()]; s.ranks.len()]);
    let at = |first: bool| usize::from(!first);
    let mut claimed: BTreeSet<(usize, usize)> = BTreeSet::new();
    let ranks: BTreeSet<Rank> = a.ranks.iter().chain(&b.ranks).copied().collect();
    for r in ranks {
        let runners: Vec<Runner> = [true, false]
            .into_iter()
            .flat_map(|first| {
                let s = side(first);
                (0..s.ranks.len())
                    .filter(move |k| s.ranks.get(*k) == Some(&r))
                    .flat_map(move |layer| (0..s.arrives.len()).map(move |way| Runner { first, layer, way }))
            })
            .collect();
        // How far each could go for the better layers alone.
        let free: Vec<usize> =
            runners.iter().map(|x| (0..).take_while(|k| cell(*x, *k).is_some_and(|c| !claimed.contains(&c))).count()).collect();
        // How many cells each gets to (a cell it meets its like in and loses, too), and how many of them it holds. Settled
        // by going round until nothing changes: one layer stopping short may let another run on.
        let (mut gets, mut holds) = (free.clone(), free.clone());
        for _ in 0..=runners.len() + 1 {
            let (mut next_gets, mut next_holds) = (free.clone(), free.clone());
            for (n, x) in runners.iter().enumerate() {
                for k in 0..free.get(n).copied().unwrap_or(0) {
                    let Some(c) = cell(*x, k) else { break };
                    let other = if x.first { c.1 } else { c.0 };
                    // Its like in the other wall, getting to this cell some way it comes in.
                    let met = rank(!x.first, other) == Some(r)
                        && runners
                            .iter()
                            .zip(&gets)
                            .any(|(y, got)| y.first != x.first && y.layer == other && (0..*got).any(|j| cell(*y, j) == Some(c)));
                    if met {
                        let wins = x.first == a_through;
                        if let (Some(g), Some(h)) = (next_gets.get_mut(n), next_holds.get_mut(n)) {
                            *g = k + 1;
                            *h = k + usize::from(wins);
                        }
                        break;
                    }
                }
            }
            if next_gets == gets && next_holds == holds {
                break;
            }
            (gets, holds) = (next_gets, next_holds);
        }
        for (x, h) in runners.iter().zip(&holds) {
            claimed.extend((0..*h).filter_map(|k| cell(*x, k)));
            if let Some(slot) = held.get_mut(at(x.first)).and_then(|w| w.get_mut(x.layer)).and_then(|l| l.get_mut(x.way)) {
                *slot = *h;
            }
        }
    }
    let [ra, rb] = held;
    (ra, rb)
}

#[cfg(test)]
#[path = "join_tests.rs"]
mod tests;
