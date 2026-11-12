//! Solids cut from solids, for a family's voids: convex polygons in binary space partitions, as
//! csg.js does it. A tree is built and walked with a worklist rather than by recursion (a smooth
//! surface of many faces makes a tree as deep as it has faces), and everything done is counted
//! against a [`Meter`], so a hostile shape ends in an error, not a hang.

use crate::geom::v3::{P3, add, cross, dot, mul, sub};

/// Triangles either solid may have.
pub const MAX_TRIANGLES: usize = 20_000;
/// How near a plane, mm, a point lies on it.
const EPS: f64 = 1e-5;
/// A triangle with less area than this, mm², has no plane worth cutting by.
const SLIVER: f64 = 1e-6;

/// What cutting may cost: corners checked against planes, and polygons split in two.
pub struct Meter {
    work: u64,
    splits: u64,
}

impl Default for Meter {
    fn default() -> Meter {
        Meter { work: 40_000_000, splits: 400_000 }
    }
}

const TOO_MUCH: &str = "it has too many faces to cut: give it fewer sides, or fewer triangles";

impl Meter {
    fn work(&mut self, corners: usize) -> Result<(), String> {
        self.work = self.work.checked_sub(corners as u64 + 1).ok_or(TOO_MUCH)?;
        Ok(())
    }

    fn split(&mut self) -> Result<(), String> {
        self.splits = self.splits.checked_sub(1).ok_or(TOO_MUCH)?;
        Ok(())
    }
}

/// A convex polygon, its corners counter-clockwise seen from where its normal `n` points, in the
/// plane of the points `p` where `n · p = w`.
#[derive(Clone)]
struct Poly {
    v: Vec<P3>,
    n: P3,
    w: f64,
}

impl Poly {
    /// A triangle as a polygon; none for one too small to have a plane.
    fn of(t: &[P3; 3]) -> Option<Poly> {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        let len = dot(n, n).sqrt();
        (len > 2.0 * SLIVER).then(|| {
            let n = mul(n, 1.0 / len);
            Poly { v: t.to_vec(), n, w: dot(n, t[0]) }
        })
    }

    fn flip(&mut self) {
        self.v.reverse();
        self.n = mul(self.n, -1.0);
        self.w = -self.w;
    }

    /// Its triangles, fanned from its first corner; those with no area left out.
    fn triangles(&self) -> impl Iterator<Item = [P3; 3]> + '_ {
        let first = self.v.first().copied().unwrap_or_default();
        self.v.windows(2).skip(1).map(move |w| [first, w[0], w[1]]).filter(|[a, b, c]| {
            let n = cross(sub(*b, *a), sub(*c, *a));
            dot(n, n) > (2.0 * SLIVER).powi(2)
        })
    }
}

/// What a plane makes of a polygon.
enum Side {
    /// All of it in the plane, its normal the same way as the plane's (true) or the other.
    On(Poly, bool),
    Front(Poly),
    Back(Poly),
    /// Across it: the part in front and the part behind, either too small to hold gone.
    Across(Option<Poly>, Option<Poly>),
}

/// `p` against the plane `n · x = w`.
fn divide(n: P3, w: f64, p: Poly) -> Side {
    let kinds: Vec<i8> =
        p.v.iter()
            .map(|c| {
                let d = dot(n, *c) - w;
                if d > EPS {
                    1
                } else if d < -EPS {
                    -1
                } else {
                    0
                }
            })
            .collect();
    match (kinds.contains(&1), kinds.contains(&-1)) {
        (false, false) => {
            let same = dot(n, p.n) > 0.0;
            Side::On(p, same)
        }
        (true, false) => Side::Front(p),
        (false, true) => Side::Back(p),
        (true, true) => {
            let (mut front, mut back) = (Vec::new(), Vec::new());
            let m = p.v.len();
            for i in 0..m {
                let (Some(&a), Some(&b), Some(&ka), Some(&kb)) = (p.v.get(i), p.v.get((i + 1) % m), kinds.get(i), kinds.get((i + 1) % m))
                else {
                    continue;
                };
                if ka != -1 {
                    front.push(a);
                }
                if ka != 1 {
                    back.push(a);
                }
                // One corner in front and the next behind: the edge between them crosses the plane.
                if ka * kb == -1 {
                    let c = add(a, mul(sub(b, a), (w - dot(n, a)) / dot(n, sub(b, a))));
                    front.push(c);
                    back.push(c);
                }
            }
            let part = |v: Vec<P3>| (v.len() >= 3).then_some(Poly { v, n: p.n, w: p.w });
            Side::Across(part(front), part(back))
        }
    }
}

struct Node {
    n: P3,
    w: f64,
    /// The polygons lying in its plane.
    polys: Vec<Poly>,
    front: Option<usize>,
    back: Option<usize>,
}

/// A solid's surface as a tree of planes, in front of each what's outside it, behind what's in.
struct Bsp {
    nodes: Vec<Node>,
}

impl Bsp {
    fn of(tris: &[[P3; 3]], meter: &mut Meter) -> Result<Bsp, String> {
        let mut bsp = Bsp { nodes: Vec::new() };
        bsp.build(tris.iter().filter_map(Poly::of).collect(), meter)?;
        Ok(bsp)
    }

    fn node(first: &Poly) -> Node {
        Node { n: first.n, w: first.w, polys: Vec::new(), front: None, back: None }
    }

    /// `polys` added to the tree: each goes down it as far as the node whose plane it lies in, and
    /// a node is made, on the first polygon there, where it runs off the tree.
    fn build(&mut self, polys: Vec<Poly>, meter: &mut Meter) -> Result<(), String> {
        let Some(first) = polys.first() else { return Ok(()) };
        if self.nodes.is_empty() {
            self.nodes.push(Bsp::node(first));
        }
        let mut stack = vec![(0, polys)];
        while let Some((i, polys)) = stack.pop() {
            let Some((n, w)) = self.nodes.get(i).map(|x| (x.n, x.w)) else { continue };
            let (mut here, mut front, mut back) = (Vec::new(), Vec::new(), Vec::new());
            for p in polys {
                meter.work(p.v.len())?;
                match divide(n, w, p) {
                    Side::On(p, _) => here.push(p),
                    Side::Front(p) => front.push(p),
                    Side::Back(p) => back.push(p),
                    Side::Across(f, b) => {
                        meter.split()?;
                        front.extend(f);
                        back.extend(b);
                    }
                }
            }
            if let Some(node) = self.nodes.get_mut(i) {
                node.polys.extend(here);
            }
            for (list, ahead) in [(front, true), (back, false)] {
                let Some(first) = list.first() else { continue };
                let known = self.nodes.get(i).and_then(|x| if ahead { x.front } else { x.back });
                let child = known.unwrap_or_else(|| {
                    let c = self.nodes.len();
                    self.nodes.push(Bsp::node(first));
                    c
                });
                if let Some(node) = self.nodes.get_mut(i) {
                    *(if ahead { &mut node.front } else { &mut node.back }) = Some(child);
                }
                stack.push((child, list));
            }
        }
        Ok(())
    }

    /// The parts of `polys` outside the solid: what ends in front of a leaf stays, what ends behind one is in it.
    fn clip(&self, polys: Vec<Poly>, meter: &mut Meter) -> Result<Vec<Poly>, String> {
        if self.nodes.is_empty() {
            return Ok(polys);
        }
        let mut kept = Vec::new();
        let mut stack = vec![(0, polys)];
        while let Some((i, polys)) = stack.pop() {
            let Some(node) = self.nodes.get(i) else { continue };
            let (mut front, mut back) = (Vec::new(), Vec::new());
            for p in polys {
                meter.work(p.v.len())?;
                match divide(node.n, node.w, p) {
                    Side::On(p, true) | Side::Front(p) => front.push(p),
                    Side::On(p, false) | Side::Back(p) => back.push(p),
                    Side::Across(f, b) => {
                        meter.split()?;
                        front.extend(f);
                        back.extend(b);
                    }
                }
            }
            match node.front {
                Some(c) => stack.push((c, front)),
                None => kept.extend(front),
            }
            if let Some(c) = node.back {
                stack.push((c, back));
            }
        }
        Ok(kept)
    }

    /// Its own polygons clipped to what's outside `other`'s solid.
    fn clip_to(&mut self, other: &Bsp, meter: &mut Meter) -> Result<(), String> {
        for node in &mut self.nodes {
            node.polys = other.clip(std::mem::take(&mut node.polys), meter)?;
        }
        Ok(())
    }

    /// Inside out: what was the solid is all that isn't.
    fn invert(&mut self) {
        for node in &mut self.nodes {
            node.polys.iter_mut().for_each(Poly::flip);
            node.n = mul(node.n, -1.0);
            node.w = -node.w;
            std::mem::swap(&mut node.front, &mut node.back);
        }
    }

    fn polygons(&self) -> Vec<Poly> {
        self.nodes.iter().flat_map(|n| n.polys.iter().cloned()).collect()
    }
}

/// The corners of the box round `tris`.
fn bounds(tris: &[[P3; 3]]) -> (P3, P3) {
    tris.iter().flatten().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(lo, hi), p| {
        (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))
    })
}

/// `solid` less what `void` takes of it: each a closed surface of triangles turned outward; what's
/// left, as triangles, none if the void swallows it. A void away from it leaves it as it was.
pub fn subtract(solid: &[[P3; 3]], void: &[[P3; 3]], meter: &mut Meter) -> Result<Vec<[P3; 3]>, String> {
    for (what, n) in [("the form cut", solid.len()), ("the void", void.len())] {
        if n > MAX_TRIANGLES {
            return Err(format!("{what} has {n} triangles; it can't have more than {MAX_TRIANGLES} to cut"));
        }
    }
    let ((lo_s, hi_s), (lo_v, hi_v)) = (bounds(solid), bounds(void));
    if (0..3).any(|k| lo_v[k] > hi_s[k] + EPS || hi_v[k] < lo_s[k] - EPS) {
        return Ok(solid.to_vec());
    }
    let (mut a, mut b) = (Bsp::of(solid, meter)?, Bsp::of(void, meter)?);
    a.invert();
    a.clip_to(&b, meter)?;
    b.clip_to(&a, meter)?;
    b.invert();
    b.clip_to(&a, meter)?;
    b.invert();
    a.build(b.polygons(), meter)?;
    a.invert();
    Ok(a.polygons().iter().flat_map(Poly::triangles).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box from `lo` to `hi`, its faces turned out.
    fn cuboid(lo: P3, hi: P3) -> Vec<[P3; 3]> {
        let v = |x: usize, y: usize, z: usize| [[lo[0], hi[0]][x], [lo[1], hi[1]][y], [lo[2], hi[2]][z]];
        [
            [v(0, 0, 1), v(1, 0, 1), v(1, 1, 1), v(0, 1, 1)],
            [v(0, 0, 0), v(0, 1, 0), v(1, 1, 0), v(1, 0, 0)],
            [v(0, 0, 0), v(1, 0, 0), v(1, 0, 1), v(0, 0, 1)],
            [v(1, 1, 0), v(0, 1, 0), v(0, 1, 1), v(1, 1, 1)],
            [v(0, 1, 0), v(0, 0, 0), v(0, 0, 1), v(0, 1, 1)],
            [v(1, 0, 0), v(1, 1, 0), v(1, 1, 1), v(1, 0, 1)],
        ]
        .iter()
        .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]])
        .collect()
    }

    /// A closed surface's volume: the signed tetrahedra from the origin, positive while it faces out.
    fn volume(tris: &[[P3; 3]]) -> f64 {
        tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum()
    }

    /// The surface's area.
    fn area(tris: &[[P3; 3]]) -> f64 {
        tris.iter()
            .map(|[a, b, c]| {
                let n = cross(sub(*b, *a), sub(*c, *a));
                dot(n, n).sqrt() / 2.0
            })
            .sum()
    }

    fn cut(solid: &[[P3; 3]], void: &[[P3; 3]]) -> Vec<[P3; 3]> {
        subtract(solid, void, &mut Meter::default()).unwrap()
    }

    #[test]
    fn a_void_takes_its_volume_from_the_solid_wherever_it_lies() {
        let slab = cuboid([0.0, 0.0, 0.0], [1000.0, 1000.0, 100.0]);
        // The volume left, and the surface it has, of a slab less a box: through it; flush with its top, as a pocket;
        // at a corner, flush with three of its faces; across a whole side, flush with its top, bottom and front; inside
        // it, a cavity; round it all, nothing.
        for (what, lo, hi, left, surface) in [
            ("through", [400.0, 400.0, -10.0], [600.0, 600.0, 110.0], 1e8 - 4e6, 2.4e6),
            ("a pocket", [400.0, 400.0, 50.0], [600.0, 600.0, 100.0], 1e8 - 2e6, 2.44e6),
            ("a corner", [0.0, 0.0, 0.0], [500.0, 500.0, 50.0], 1e8 - 1.25e7, 2.4e6),
            ("a side", [-10.0, 0.0, 0.0], [1010.0, 300.0, 100.0], 1e8 - 3e7, 1.74e6),
            ("a cavity", [400.0, 400.0, 40.0], [600.0, 600.0, 60.0], 1e8 - 8e5, 2.496e6),
            ("all of it", [-1.0, -1.0, -1.0], [1001.0, 1001.0, 101.0], 0.0, 0.0),
        ] {
            let got = cut(&slab, &cuboid(lo, hi));
            assert!((volume(&got) - left).abs() < 1e-3, "{what}: {} left, not {left}", volume(&got));
            assert!((area(&got) - surface).abs() < 1e-3, "{what}: {} of surface, not {surface}", area(&got));
        }
    }

    #[test]
    fn a_solid_cut_by_itself_is_nothing_and_by_what_is_away_is_as_it_was() {
        let a = cuboid([0.0; 3], [100.0; 3]);
        assert!(cut(&a, &a).is_empty());
        let away = cuboid([500.0; 3], [600.0; 3]);
        assert_eq!(cut(&a, &away), a);
        // Less a box touching it only along a face, it keeps all it has.
        let beside = cuboid([100.0, 0.0, 0.0], [200.0, 100.0, 100.0]);
        assert!((volume(&cut(&a, &beside)) - 1e6).abs() < 1e-3);
    }

    #[test]
    fn a_cut_by_many_faces_runs_on_a_small_stack_and_a_hostile_one_is_refused() {
        // A tube's 512 faces make a tree 512 deep.
        let ring = |z: f64, k: usize, n: usize| {
            let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
            [200.0 * c, 200.0 * s, z]
        };
        let n = 256;
        let tube: Vec<[P3; 3]> = (0..n)
            .flat_map(|k| {
                let (a, b, c, d) = (ring(0.0, k, n), ring(0.0, k + 1, n), ring(500.0, k, n), ring(500.0, k + 1, n));
                [[a, b, d], [a, d, c], [[0.0, 0.0, 500.0], c, d], [[0.0, 0.0, 0.0], b, a]]
            })
            .collect();
        let half = cuboid([-1000.0, -1000.0, -10.0], [1000.0, 0.0, 510.0]);
        let (tube2, half2) = (tube.clone(), half.clone());
        let got = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || cut(&tube2, &half2)).unwrap().join().unwrap();
        let whole = volume(&tube);
        assert!((volume(&got) / (whole / 2.0) - 1.0).abs() < 1e-3, "{} of {whole}", volume(&got));
        // Too many triangles, or too little budget, is said, not attempted without end.
        let many = vec![tube[0]; MAX_TRIANGLES + 1];
        assert!(subtract(&many, &half, &mut Meter::default()).unwrap_err().contains("20000"));
        let mut poor = Meter { work: 1000, splits: 1000 };
        assert!(subtract(&tube, &half, &mut poor).unwrap_err().contains("too many faces"));
        // Triangles with corners that aren't numbers, or no area, take nothing and don't crash it.
        let odd = [[[f64::NAN; 3]; 3], [[f64::INFINITY; 3]; 3], [[0.0; 3]; 3]];
        assert!(subtract(&odd, &half, &mut Meter::default()).is_ok() && subtract(&half, &odd, &mut Meter::default()).is_ok());
    }
}
