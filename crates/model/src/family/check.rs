//! The check of a family from Revit: each type's parametric result (its references solved, its parametric forms and nested
//! parts built) against Revit's own for it (its `snapshot` forms and symbols), at each detail level: the volume within 1 %, the
//! box within 2 mm, and the plan's extent (its symbols and the forms it shows) within 2 mm. A type that matched draws its parametric forms; one that didn't
//! draws Revit's, and says why ([`TypeCheck`]); the family is editable when every type matched and nothing was left
//! untranslated ([`Editable`]).

use std::collections::{BTreeMap, HashMap};

use super::shapes::volume;
use super::{Build, Editable, Family, Others, Piece, SymbolView, TypeCheck, csg};
use crate::graphics::Detail;

/// How far the volumes may be apart, as a share of Revit's, and the boxes and the plan symbols' extents, mm.
pub const VOLUME: f64 = 0.01;
/// How far the volumes may be apart where its definition has curves (revolves, arcs): Revit's result is its curves in a few
/// facets, as its views tessellate them, where BIMCraft's are in many, so a curved form holds a little more in BIMCraft.
pub const VOLUME_CURVED: f64 = 0.03;
pub const BOX: f64 = 2.0;
/// The height over its level a floor plan cuts at, mm: Revit's default, which its plan of a family is drawn at.
pub const PLAN_CUT: f64 = 1200.0;
/// How near the cut plane a form's bottom may be and still be seen below it, not cut, mm.
pub const CUT_EPS: f64 = 0.5;

/// The volume and box of what a view at a detail level draws of `pieces`.
fn measure(pieces: &[Piece], d: Detail) -> (f64, Option<[[f64; 3]; 2]>) {
    let mut v = 0.0;
    let mut b: Option<[[f64; 3]; 2]> = None;
    for p in pieces.iter().filter(|p| p.at(d)) {
        v += volume(&p.tris).abs();
        for q in p.tris.iter().flatten() {
            b = Some(match b {
                None => [*q, *q],
                Some([lo, hi]) => [std::array::from_fn(|k| lo[k].min(q[k])), std::array::from_fn(|k| hi[k].max(q[k]))],
            });
        }
    }
    (v, b)
}

/// The extent of `marks`, (x, y) from above.
fn extent(marks: &[super::Mark]) -> Option<[[f64; 2]; 2]> {
    let mut b: Option<[[f64; 2]; 2]> = None;
    let mut take = |p: crate::geom::P2| {
        b = Some(match b {
            None => [[p.0, p.1], [p.0, p.1]],
            Some([lo, hi]) => [[lo[0].min(p.0), lo[1].min(p.1)], [hi[0].max(p.0), hi[1].max(p.1)]],
        });
    };
    for m in marks {
        match m {
            super::Mark::Line(pts) => pts.iter().for_each(|p| take(*p)),
            super::Mark::Dot { centre, .. } => take(*centre),
            super::Mark::Label { at, .. } => take(*at),
        }
    }
    b
}

fn level(d: Detail) -> &'static str {
    match d {
        Detail::Coarse => "coarse",
        Detail::Medium => "medium",
        Detail::Fine => "fine",
    }
}

const AXES: [&str; 3] = ["width", "depth", "height"];

impl Family {
    /// Every type checked again ([`Family::check_type`]), but those flexed since they came in, and whether it is editable:
    /// every type matched and nothing left untranslated. A family carrying no Revit result has nothing to check.
    pub fn run_check(&mut self, others: Others) {
        let untranslated = self.editable.as_ref().map(|e| e.untranslated.clone()).unwrap_or_default();
        if !self.has_snapshot() && self.editable.is_none() {
            return;
        }
        let mut check = BTreeMap::new();
        for t in self.types.keys() {
            let c = match self.check.get(t).filter(|c| c.flexed) {
                Some(c) => c.clone(),
                None => self.check_type(t, others),
            };
            check.insert(t.clone(), c);
        }
        self.check = check;
        let failed: Vec<String> = self
            .check
            .iter()
            .filter(|(_, c)| !c.matched)
            .map(|(t, c)| format!("its type {t}: {}", c.reason.clone().unwrap_or_default()))
            .collect();
        let ok = untranslated.is_empty() && failed.is_empty() && self.has_parametric();
        let reason = if ok {
            None
        } else if let Some(u) = untranslated.first() {
            Some(if untranslated.len() > 1 { format!("{u} (and {} more)", untranslated.len() - 1) } else { u.clone() })
        } else if let Some(f) = failed.first() {
            Some(if failed.len() > 1 { format!("{f} (and {} more types)", failed.len() - 1) } else { f.clone() })
        } else {
            Some("it came without the rules that size it, so it is drawn as Revit made it".into())
        };
        self.editable = Some(Editable { ok, reason, untranslated });
    }

    /// Whether it has a parametric definition: forms, symbols or nested parts that aren't Revit's result.
    pub fn has_parametric(&self) -> bool {
        self.forms.iter().any(|f| !f.snapshot) || self.symbols.iter().any(|y| !y.snapshot) || !self.nested.is_empty()
    }

    /// Type `t`'s parametric result against Revit's own, at Coarse, Medium and Fine: the volume within [`VOLUME`] of Revit's,
    /// the box within [`BOX`] mm, and the plan symbols' extent within [`BOX`] mm. A type Revit's result says nothing of
    /// matches.
    pub fn check_type(&self, t: &str, others: Others) -> TypeCheck {
        let fail = |reason: String| TypeCheck { matched: false, reason: Some(reason), flexed: false };
        let snapshot_here =
            self.forms.iter().any(|f| f.snapshot && f.of_type(t)) || self.symbols.iter().any(|y| y.snapshot && y.of_type(t));
        if !snapshot_here {
            return TypeCheck { matched: true, reason: None, flexed: false };
        }
        if !self.has_parametric() {
            return fail("it came without the rules that size it, so it is drawn as Revit made it".into());
        }
        let values = match self.values(t, &BTreeMap::new()) {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        if let Err(e) = self.solve(&values) {
            return fail(format!("its sizes can't all hold at once: {e}"));
        }
        let build = |snapshot: bool| {
            self.build(
                t,
                &values,
                &mut HashMap::new(),
                &mut csg::Meter::default(),
                Build { snapshot, others, depth: 0, spent: &std::cell::Cell::new(0) },
            )
        };
        let (made, revit) = match (build(false), build(true)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) => return fail(format!("it doesn't come out in this type: {e}")),
            (_, Err(e)) => return fail(format!("what Revit made of it can't be drawn: {e}")),
        };
        let curved =
            self.forms.iter().any(|f| !f.snapshot && f.of_type(t) && (f.axis.is_some() || f.circle.is_some() || !f.arcs.is_empty()));
        for d in [Detail::Coarse, Detail::Medium, Detail::Fine] {
            let ((va, ba), (vb, bb)) = (measure(&made, d), measure(&revit, d));
            let off = (va - vb).abs();
            let share = if curved && va > vb { VOLUME_CURVED } else { VOLUME };
            if off > share * vb.max(1.0) && off > 1000.0 {
                let pct = if vb > 0.0 { off / vb * 100.0 } else { 100.0 };
                let way = if va > vb { "more" } else { "less" };
                return fail(format!(
                    "drawn {} it holds {pct:.1} % {way} than Revit's ({:.4} m³ against {:.4} m³)",
                    level(d),
                    va / 1e9,
                    vb / 1e9
                ));
            }
            match (ba, bb) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    for k in 0..3 {
                        let gap = (a[0][k] - b[0][k]).abs().max((a[1][k] - b[1][k]).abs());
                        if gap > BOX {
                            return fail(format!("drawn {} its {} is {gap:.0} mm off Revit's", level(d), AXES[k]));
                        }
                    }
                }
                (None, _) => return fail(format!("drawn {} it shows nothing where Revit's shows something", level(d))),
                (_, None) => return fail(format!("drawn {} it shows something where Revit's shows nothing", level(d))),
            }
            // Its nested parts' plan symbols with its own (Revit's result has them among its own).
            let marks = |snapshot: bool| -> Result<Vec<super::Mark>, String> {
                let mut out = self.marks_where(&values, &|y: &super::Symbol| {
                    y.view == SymbolView::Plan && y.of_type(t) && y.snapshot == snapshot && y.drawn_at(d)
                })?;
                if !snapshot {
                    self.nested_marks(t, &values, Some(d), others, (0, &std::cell::Cell::new(0)), &mut out)?;
                }
                Ok(out)
            };
            // What the plan draws: its symbols and the forms it shows from above (Revit's plan of a family it brings may
            // be its forms' lines, which BIMCraft draws from the forms).
            // Its plan as Revit's floor plan sees it (a door or a window at its sill), cutting 1200 mm over the level
            // ([`Piece::in_plan_cut`]).
            let cut = PLAN_CUT
                - match values.get(super::SILL) {
                    Some(super::Val::Number(x)) => *x,
                    _ => 0.0,
                };
            let hosted = self.category.hosted();
            let shown = |p: &Piece| p.in_plan_cut(d, cut, hosted);
            let plan = |marks: Vec<super::Mark>, pieces: &[Piece]| {
                let mut all = marks;
                all.extend(pieces.iter().filter(|p| shown(p)).map(|p| super::Mark::Line(p.outline.clone())));
                extent(&all)
            };
            let (ma, mb) = match (marks(false), marks(true)) {
                (Ok(a), Ok(b)) => (plan(a, &made), plan(b, &revit)),
                (Err(e), _) => return fail(format!("its plan lines don't come out: {e}")),
                (_, Err(e)) => return fail(format!("Revit's plan lines can't be drawn: {e}")),
            };
            match (ma, mb) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    let gap = (0..2).map(|k| (a[0][k] - b[0][k]).abs().max((a[1][k] - b[1][k]).abs())).fold(0.0, f64::max);
                    if gap > BOX {
                        return fail(format!("its plan drawn {} reaches {gap:.0} mm off Revit's", level(d)));
                    }
                }
                (None, Some(_)) => return fail(format!("its plan drawn {} has no lines where Revit's has", level(d))),
                (Some(_), None) => return fail(format!("its plan drawn {} has lines where Revit's has none", level(d))),
            }
        }
        TypeCheck { matched: true, reason: None, flexed: false }
    }
}
