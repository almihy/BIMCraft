//! A door's leaf in plan, as Revit's door families draw it: the leaf built in 3D, shut in its opening, is never drawn in a
//! plan (its Visibility Settings leave Plan/RCP off), and the plan draws the leaf open, square to the wall, a panel as thick
//! as the leaf, with the arc its free edge sweeps (Panel and Plan Swing, at every detail level).
//!
//! A family made before forms and symbols said which views and detail levels draw them has neither: its shut leaf was cut
//! and drawn in the wall and its open leaf was a single line. [`Family::leaves_drawn_open`] gives such a family Revit's
//! settings once, as its project is read ([`crate::model::Model::migrate`]); [`Family::shut_leaves`] finds the leaves a
//! warning names.

use std::collections::{BTreeMap, HashMap};

use super::{DEFAULT_WALL, Expr, Family, HEIGHT, Levels, Plane, Show, Val, WIDTH, number};
use crate::geom::P2;
use crate::model::Operation;

/// How thin a door's leaf is at least, and how thick at most, mm: thinner is a kick plate's or a glass's, thicker a frame's.
const LEAF: (f64, f64) = (5.0, 100.0);
/// How close two points worked out from a family's formulas are the same one, mm.
const SAME: f64 = 1.0;

impl Family {
    /// A swinging door's forms that are its leaves shut in its opening and drawn in a plan as well as its swing, by their place
    /// in [`Family::forms`]: a door family whose symbols draw an arc (its swing), whose leaves swing (not a sliding or a fixed
    /// door, whose leaves stand in the wall in plan too), and, in its first type that works out, a solid swept up from the
    /// plane of the plan, as thick as a leaf, a quarter of the opening wide or more and within it, and half its height or
    /// more; that says nothing of the views that draw it (`show`).
    pub fn shut_leaves(&self) -> Vec<usize> {
        if self.category != super::Category::Door
            || matches!(self.operation, Some(Operation::Sliding | Operation::DoubleSliding | Operation::Fixed))
            || !self.symbols.iter().any(|s| s.arc.is_some())
        {
            return Vec::new();
        }
        let Some(values) = self.types.keys().find_map(|t| self.values_in(t, &BTreeMap::new(), DEFAULT_WALL).ok()) else {
            return Vec::new();
        };
        let size = |name: &str| match values.get(name) {
            Some(Val::Number(x)) if *x > 0.0 => Some(*x),
            _ => None,
        };
        let (Some(width), Some(height)) = (size(WIDTH), size(HEIGHT)) else { return Vec::new() };
        self.shapes(&values)
            .into_iter()
            .filter(|s| {
                let form = &self.forms[s.index];
                let (along, across, up) = (s.max[0] - s.min[0], s.max[1] - s.min[1], s.max[2] - s.min[2]);
                !s.void
                    && form.show.is_none()
                    && form.plane == Plane::Xy
                    && form.axis.is_none()
                    && form.mesh.is_empty()
                    && (LEAF.0..=LEAF.1).contains(&across)
                    && along >= width / 4.0
                    && s.min[0] >= -width / 2.0 - SAME
                    && s.max[0] <= width / 2.0 + SAME
                    && up >= height / 2.0
            })
            .map(|s| s.index)
            .collect()
    }

    /// Gives a swinging door family of before Visibility Settings Revit's (see the module): its [`Family::shut_leaves`] are
    /// drawn in no plan, and each line of its symbols that is a leaf drawn open (from the centre of an arc of its as long as
    /// the arc's radius) becomes a panel as thick as the leaf, lying on the side of the line the arc shuts to, drawn at every
    /// detail level with its arc. A family any of whose forms or symbols says which views or levels draw it is the family's
    /// own already, and is left as it is. Whether it changed anything.
    pub fn leaves_drawn_open(&mut self) -> bool {
        if self.forms.iter().any(|f| f.show.is_some()) || self.symbols.iter().any(|s| s.show.is_some()) {
            return false;
        }
        let leaves = self.shut_leaves();
        if leaves.is_empty() {
            return false;
        }
        let Some(values) = self.types.keys().find_map(|t| self.values_in(t, &BTreeMap::new(), DEFAULT_WALL).ok()) else {
            return false;
        };
        // The leaf's thickness: the thinnest shut leaf's, to the tenth of a millimetre.
        let shapes = self.shapes(&values);
        let thick = shapes.iter().filter(|s| leaves.contains(&s.index)).map(|s| s.max[1] - s.min[1]).fold(f64::INFINITY, f64::min);
        let thick = (thick * 10.0).round() / 10.0;
        let opened = self.opened(&values, thick);
        for &i in &leaves {
            self.forms[i].show = Some(Show { plan: false, ..Show::default() });
        }
        for (i, k, by) in opened {
            let [a, b] = [self.symbols[i].line[0].clone(), self.symbols[i].line[1].clone()];
            let moved = |p: &[Expr; 2]| [shifted(&p[0], by.0), shifted(&p[1], by.1)];
            self.symbols[i].line = vec![a.clone(), b.clone(), moved(&b), moved(&a), a];
            self.symbols[i].show = Some(Levels::default());
            self.symbols[k].show = Some(Levels::default());
        }
        true
    }

    /// Its symbols' lines that are leaves drawn open, each with the arc it swings and how far its panel lies from it, `thick`
    /// toward the side it shuts to ([`Family::leaves_drawn_open`]): the line's place, the arc's, and that offset.
    fn opened(&self, values: &BTreeMap<String, Val>, thick: f64) -> Vec<(usize, usize, P2)> {
        // Each arc: its centre, its radius and its two ends, worked out.
        let mut read = HashMap::new();
        let mut arcs = Vec::new();
        for (i, s) in self.symbols.iter().enumerate() {
            let Some(c) = &s.arc else { continue };
            let mut n = |e| number(e, values, &mut read).ok();
            let (Some(x), Some(y), Some(r), Some(from), Some(to)) = (n(&c.centre[0]), n(&c.centre[1]), n(&c.radius), n(&c.from), n(&c.to))
            else {
                continue;
            };
            let end = |deg: f64| {
                let (s, c) = deg.to_radians().sin_cos();
                P2(x + r * c, y + r * s)
            };
            arcs.push((i, P2(x, y), r, [end(from), end(to)]));
        }
        let mut opened = Vec::new();
        for (i, s) in self.symbols.iter().enumerate() {
            let [a, b] = s.line.as_slice() else { continue };
            let mut n = |e| number(e, values, &mut read).ok();
            let (Some(ax), Some(ay), Some(bx), Some(by)) = (n(&a[0]), n(&a[1]), n(&b[0]), n(&b[1])) else { continue };
            let (p, q) = (P2(ax, ay), P2(bx, by));
            // The arc it is the open leaf of: hinged at its centre, as long as its radius; it shuts to the arc's other end.
            let found = arcs.iter().find_map(|(k, centre, r, ends)| {
                let hinged = if p.dist(*centre) < SAME {
                    Some(q)
                } else if q.dist(*centre) < SAME {
                    Some(p)
                } else {
                    None
                }?;
                if (hinged.dist(*centre) - r).abs() >= SAME {
                    return None;
                }
                let shut = ends.iter().copied().find(|e| e.dist(hinged) >= SAME)?;
                let toward = shut - *centre;
                let len = toward.dist(P2(0.0, 0.0));
                (len > 0.0).then(|| (*k, P2(toward.0 / len * thick, toward.1 / len * thick)))
            });
            if let Some((k, by)) = found {
                opened.push((i, k, by));
            }
        }
        opened
    }
}

/// `e` moved by `by` mm: a number, or its formula plus or minus the length.
fn shifted(e: &Expr, by: f64) -> Expr {
    let by = (by * 10.0).round() / 10.0 + 0.0;
    if by == 0.0 {
        return e.clone();
    }
    match e {
        Expr::Number(x) => Expr::Number(x + by),
        Expr::Formula(f) if by > 0.0 => Expr::Formula(format!("({f}) + {by}")),
        Expr::Formula(f) => Expr::Formula(format!("({f}) - {}", -by)),
    }
}
