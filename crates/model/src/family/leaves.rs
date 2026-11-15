//! A door's leaf in plan, as Revit's door families draw it: the leaf built in 3D, shut in its opening, is never drawn in a
//! plan (its Visibility Settings leave Plan/RCP off), and the plan draws the leaf open, square to the wall, a panel as thick
//! as the leaf, with the arc its free edge sweeps (Panel and Plan Swing, at every detail level).
//!
//! A family made before forms and symbols said which views and detail levels draw them has neither: its shut leaf was cut
//! and drawn in the wall and its open leaf was a single line. [`Family::leaves_drawn_open`] gives such a family Revit's
//! settings once, as its project is read ([`crate::model::Model::migrate`]); [`Family::shut_leaves`] finds the leaves a
//! warning names.
//!
//! The parts fixed to the leaf go with it: its kick plates, its levers and their roses. In Revit they are the leaf's, and
//! hidden in plan as it is. Built beside the leaf in a family that says nothing of them, seen from above they would be
//! drawn shut in the opening, two lines a leaf's thickness apart and a mark for each handle, which reads as the leaf drawn
//! shut. [`Family::leaf_parts`] finds them: a form that says nothing of its views and lies within a hidden leaf's width and
//! height and within a lever's reach of its faces. [`Family::parts_hidden_with_leaves`] gives each its leaf's settings and no
//! plan at all (a plan draws a part wholly below its cut by Top/Bottom, and a kick plate is one), as a
//! project is read, whatever its format, and as the `family` command takes a family. It touches only a form that says
//! nothing, so what a person set is kept (the Family Editor stores a setting it changed, even every box ticked, and a
//! form that says nothing is one nobody set), and what it did is not done again. The frame's jambs, head and threshold lie
//! outside the leaf's width or height and keep drawing.

use std::collections::{BTreeMap, HashMap};

use super::{DEFAULT_WALL, Expr, Family, HEIGHT, Levels, Plane, Shape, Show, Val, WIDTH, number};
use crate::geom::P2;
use crate::model::Operation;

/// How thin a door's leaf is at least, and how thick at most, mm: thinner is a kick plate's or a glass's, thicker a frame's.
const LEAF: (f64, f64) = (5.0, 100.0);
/// How close two points worked out from a family's formulas are the same one, mm.
const SAME: f64 = 1.0;
/// How far from a leaf's faces a part still rides on it, mm: a lever stands about 70 mm proud of the leaf, its rose and a
/// kick plate less, and a hook or a bolt a little more.
const REACH: f64 = 120.0;

impl Family {
    /// A swinging door family's width, its height and where each of its forms lies, in its first type that works out: none
    /// for a family that is not a door, whose leaves do not swing (a sliding or a fixed door's stand in the wall in plan too)
    /// or whose symbols draw no arc (its swing), or whose size does not work out.
    fn swinging(&self) -> Option<(f64, f64, Vec<Shape>)> {
        if self.category != super::Category::Door
            || matches!(self.operation, Some(Operation::Sliding | Operation::DoubleSliding | Operation::Fixed))
            || !self.symbols.iter().any(|s| s.arc.is_some())
        {
            return None;
        }
        let (first, values) =
            self.types.keys().find_map(|t| Some((t.as_str(), self.values_in(t, &BTreeMap::new(), DEFAULT_WALL).ok()?)))?;
        let size = |name: &str| match values.get(name) {
            Some(Val::Number(x)) if *x > 0.0 => Some(*x),
            _ => None,
        };
        Some((size(WIDTH)?, size(HEIGHT)?, self.shapes(first, &values)))
    }

    /// Whether `s` is a leaf shut in its opening, whatever it says of its views: a solid swept up from the plane of the plan,
    /// as thick as a leaf, a quarter of the opening `width` wide or more and within it, and half its `height` or more.
    fn is_leaf(&self, s: &Shape, width: f64, height: f64) -> bool {
        let form = &self.forms[s.index];
        let (along, across, up) = (s.max[0] - s.min[0], s.max[1] - s.min[1], s.max[2] - s.min[2]);
        !s.void
            && !form.snapshot
            && form.plane == Plane::Xy
            && form.axis.is_none()
            && form.mesh.is_empty()
            && (LEAF.0..=LEAF.1).contains(&across)
            && along >= width / 4.0
            && s.min[0] >= -width / 2.0 - SAME
            && s.max[0] <= width / 2.0 + SAME
            && up >= height / 2.0
    }

    /// A swinging door's forms that are its leaves shut in its opening and drawn in a plan as well as its swing, by their place
    /// in [`Family::forms`]: a door family whose symbols draw an arc (its swing), whose leaves swing (not a sliding or a fixed
    /// door, whose leaves stand in the wall in plan too), and, in its first type that works out, a solid swept up from the
    /// plane of the plan, as thick as a leaf, a quarter of the opening wide or more and within it, and half its height or
    /// more; that says nothing of the views that draw it (`show`).
    pub fn shut_leaves(&self) -> Vec<usize> {
        let Some((width, height, shapes)) = self.swinging() else { return Vec::new() };
        shapes.iter().filter(|s| self.forms[s.index].show.is_none() && self.is_leaf(s, width, height)).map(|s| s.index).collect()
    }

    /// A swinging door's parts that are fixed to a leaf hidden in plan and say nothing of their own views, each with the
    /// settings of the leaf it rides on and no plan at all (`plan`, `plan_top` and `plan_cut` off, as a plan draws a part
    /// below its cut by Top/Bottom), by their place in [`Family::forms`]: a solid that is no void or snapshot, of any
    /// plane (a rose is a circle in the plane of the leaf), within the leaf's width and height and, across the wall, within
    /// the leaf's faces and [`REACH`] beyond them (a lever, a kick plate on each face, a rose, a hook). A leaf hidden in
    /// plan is one [`Family::shut_leaves`]' test accepts that says `plan: false`. Jambs, a head and a threshold lie outside
    /// the leaf's width or height, and are none of them.
    pub fn leaf_parts(&self) -> Vec<(usize, Show)> {
        let Some((width, height, shapes)) = self.swinging() else { return Vec::new() };
        let hidden: Vec<(&Shape, Show)> = shapes
            .iter()
            .filter_map(|s| Some((s, self.forms[s.index].show.filter(|v| !v.plan)?)))
            .filter(|(s, _)| self.is_leaf(s, width, height))
            .collect();
        shapes
            .iter()
            .filter(|p| !p.void && !self.forms[p.index].snapshot && self.forms[p.index].show.is_none())
            .filter_map(|p| {
                let (_, show) = hidden.iter().find(|(l, _)| {
                    (0..3).all(|k| {
                        let reach = if k == 1 { REACH } else { SAME };
                        p.min[k] >= l.min[k] - reach && p.max[k] <= l.max[k] + reach
                    })
                })?;
                // A plan draws a door's part wholly below its cut by Top/Bottom alone (`Piece::in_plan_cut`): a kick plate
                // is hidden only with that off as well.
                Some((p.index, Show { plan: false, plan_top: false, plan_cut: false, ..*show }))
            })
            .collect()
    }

    /// Gives each of a swinging door family's [`Family::leaf_parts`] the settings of the leaf it is fixed to (see the module),
    /// so a plan draws neither. Whether it changed anything.
    pub fn parts_hidden_with_leaves(&mut self) -> bool {
        let parts = self.leaf_parts();
        for &(i, show) in &parts {
            self.forms[i].show = Some(show);
        }
        !parts.is_empty()
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
        let Some((first, values)) =
            self.types.keys().find_map(|t| Some((t.clone(), self.values_in(t, &BTreeMap::new(), DEFAULT_WALL).ok()?)))
        else {
            return false;
        };
        // The leaf's thickness: the thinnest shut leaf's, to the tenth of a millimetre.
        let shapes = self.shapes(&first, &values);
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
            let (Some(x), Some(y), Some(r), Some((from, to))) = (n(&c.centre[0]), n(&c.centre[1]), n(&c.radius), c.ends(values)) else {
                continue;
            };
            arcs.push((i, P2(x, y), r, [from, to]));
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
