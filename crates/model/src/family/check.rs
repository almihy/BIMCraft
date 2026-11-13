//! The check of a family from Revit: each type's parametric result (its references solved, its parametric forms and nested
//! parts built) against Revit's own for it (its `snapshot` forms and symbols), at each detail level: the volume within 1 %, the
//! box within 2 mm, the shape surface to surface both ways, and the plan's and the elevations' lines point by point, within
//! [`NEAR`] mm ([`compare`]). It runs once, when the family comes in. A type that matched draws its parametric forms; one
//! that didn't draws Revit's, and says why ([`TypeCheck`]); the family is editable when every type matched and nothing was
//! left untranslated ([`Editable`]).

use std::collections::{BTreeMap, HashMap};

use super::shapes::volume;
use super::{Build, Editable, Family, Others, Piece, SymbolView, TypeCheck, compare, csg};
use crate::geom::P2;
use crate::geom::v3::P3;
use crate::graphics::Detail;

/// How far the volumes may be apart, as a share of Revit's, and the boxes and the plan symbols' extents, mm.
pub const VOLUME: f64 = 0.01;
/// How far the volumes may be apart where its definition has curves (revolves, arcs): Revit's result is its curves in a few
/// facets, as its views tessellate them, where BIMCraft's are in many, so a curved form holds a little more in BIMCraft.
pub const VOLUME_CURVED: f64 = 0.03;
pub const BOX: f64 = 2.0;
/// How far a point of BIMCraft's drawing may be from Revit's, and one of Revit's from BIMCraft's, mm: of the shape's
/// surfaces, and of the plan's and the elevations' lines.
pub const NEAR: f64 = 2.0;
/// What is let pass beyond [`NEAR`]: a share of a drawing this small ([`NOISE_SHARE`]) no farther off than this, mm: a seam
/// of facets or a rounding in the export, never a part out of place.
pub const NOISE: f64 = 5.0;
pub const NOISE_SHARE: f64 = 0.001;
/// How far a point is looked for, mm: one farther off is said to be more than this.
pub const REACH: f64 = 500.0;
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

fn level(d: Detail) -> &'static str {
    match d {
        Detail::Coarse => "coarse",
        Detail::Medium => "medium",
        Detail::Fine => "fine",
    }
}

/// What a matched type of an RPC family says of its 3D.
pub const RPC_3D: &str = "drawn as Revit made it in 3D (an RPC render placeholder)";

const AXES: [&str; 3] = ["width", "depth", "height"];

impl Family {
    /// Every type checked as Revit exported it ([`Family::check_type`]), a door or a window in the wall Revit's result was
    /// made in (`walls`, per type; else one of [`super::DEFAULT_WALL`]): run once, when the family comes in from Revit (loaded
    /// or reloaded with its definition), never after an edit. Then whether it is editable ([`Family::summarize`]). A family
    /// carrying no Revit result has nothing to check.
    pub fn run_check(&mut self, others: Others, walls: &super::revit::Walls) {
        if !self.has_snapshot() && self.editable.is_none() {
            return;
        }
        let check = self
            .types
            .keys()
            .filter_map(|t| {
                let wall = walls.get(t).copied();
                let c = self.check_type(t, others, wall.unwrap_or(super::DEFAULT_WALL))?;
                Some((t.clone(), super::TypeCheck { wall, ..c }))
            })
            .collect();
        self.check = check;
        self.summarize();
    }

    /// Whether it is editable, from its types' checks as they stand: every type that came in matched Revit's result or has
    /// been changed since, at least one was compared and matched, and nothing was left untranslated; else why not, in plain
    /// words. Nothing is built or compared: the check ran when it came in.
    pub fn summarize(&mut self) {
        if !self.has_snapshot() && self.editable.is_none() {
            return;
        }
        let untranslated = self.editable.as_ref().map(|e| e.untranslated.clone()).unwrap_or_default();
        let failed: Vec<String> = self
            .check
            .iter()
            .filter(|(_, c)| c.as_revit())
            .map(|(t, c)| format!("its type {t}: {}", c.reason.clone().unwrap_or_default()))
            .collect();
        let compared = self.check.values().any(|c| c.matched);
        let ok = untranslated.is_empty() && failed.is_empty() && compared && self.has_parametric();
        let reason = if ok {
            None
        } else if let Some(u) = untranslated.first() {
            Some(if untranslated.len() > 1 { format!("{u} (and {} more)", untranslated.len() - 1) } else { u.clone() })
        } else if let Some(f) = failed.first() {
            Some(if failed.len() > 1 { format!("{f} (and {} more types)", failed.len() - 1) } else { f.clone() })
        } else if !self.has_parametric() {
            Some("it came without the rules that size it, so it is drawn as Revit made it".into())
        } else if self.check.is_empty() {
            Some("it came without Revit's result for its types, so nothing was checked against Revit".into())
        } else {
            Some("none of its types matched Revit's result when it came in; each is drawn from its rules since it was changed, which may differ from Revit's".into())
        };
        self.editable = Some(Editable { ok, reason, untranslated });
    }

    /// Whether it has a parametric definition: forms, symbols or nested parts that aren't Revit's result.
    pub fn has_parametric(&self) -> bool {
        self.forms.iter().any(|f| !f.snapshot) || self.symbols.iter().any(|y| !y.snapshot) || !self.nested.is_empty()
    }

    /// Type `t`'s parametric result against Revit's own, at Coarse, Medium and Fine: the volume within [`VOLUME`] of Revit's,
    /// the box within [`BOX`] mm, the shape surface to surface both ways within [`NEAR`] mm ([`compare::surface_gap`]: a
    /// part mirrored, upside down or out of place fails though the totals agree), and the plan's and the elevations' lines
    /// point by point within [`NEAR`] mm. Measured in a wall `wall` mm thick for a door or a window (the one Revit's result
    /// was made in). None for a type Revit's result says nothing of: it isn't checked.
    pub fn check_type(&self, t: &str, others: Others, wall: f64) -> Option<TypeCheck> {
        let fail = |reason: String| Some(TypeCheck { matched: false, reason: Some(reason), ..TypeCheck::default() });
        let snapshot_here =
            self.forms.iter().any(|f| f.snapshot && f.of_type(t)) || self.symbols.iter().any(|y| y.snapshot && y.of_type(t));
        if !snapshot_here {
            return None;
        }
        if !self.has_parametric() {
            return fail("it came without the rules that size it, so it is drawn as Revit made it".into());
        }
        match self.compare_type(t, others, wall, true) {
            Ok(found) => match found.iter().find_map(Compared::failed) {
                Some(why) => fail(why),
                // A planting's 3D is Revit's render placeholder, drawn as Revit made it whatever its plan says.
                None => Some(TypeCheck { matched: true, reason: self.rpc.then(|| RPC_3D.to_string()), ..TypeCheck::default() }),
            },
            Err(why) => fail(why),
        }
    }

    /// Type `t`'s parametric result and Revit's own, measured against each other at each detail level ([`Compared`]), in a
    /// wall `wall` mm thick for a door or a window; `first_failure`, up to the first that doesn't match. Why they can't be,
    /// where one doesn't come out.
    pub fn compare_type(&self, t: &str, others: Others, wall: f64, first_failure: bool) -> Result<Vec<Compared>, String> {
        let values = self.values_in(t, &BTreeMap::new(), wall)?;
        if let Err(e) = self.solve(&values) {
            return Err(format!("its sizes can't all hold at once: {e}"));
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
            (Err(e), _) => return Err(format!("it doesn't come out in this type: {e}")),
            (_, Err(e)) => return Err(format!("what Revit made of it can't be drawn: {e}")),
        };
        let curved =
            self.forms.iter().any(|f| !f.snapshot && f.of_type(t) && (f.axis.is_some() || f.circle.is_some() || !f.arcs.is_empty()));
        // Its plan as Revit's floor plan sees it (a door or a window at its sill), cutting 1200 mm over the level
        // ([`Piece::in_plan_cut`]).
        let cut = PLAN_CUT
            - match values.get(super::SILL) {
                Some(super::Val::Number(x)) => *x,
                _ => 0.0,
            };
        let hosted = self.category.hosted();
        let mut out = Vec::new();
        // A drawing level's pieces measured once: Medium and Fine often draw the same as each other.
        let mut shapes: Vec<(Vec<usize>, Vec<usize>, Gaps)> = Vec::new();
        for d in [Detail::Coarse, Detail::Medium, Detail::Fine] {
            // An RPC family's 3D is a render placeholder: only what its plan and its elevations draw is compared.
            if !self.rpc {
                let ((va, ba), (vb, bb)) = (measure(&made, d), measure(&revit, d));
                let picked = |pieces: &[Piece]| -> Vec<usize> { (0..pieces.len()).filter(|i| pieces[*i].at(d)).collect() };
                let (pa, pb) = (picked(&made), picked(&revit));
                let shape = match shapes.iter().find(|(a, b, _)| *a == pa && *b == pb) {
                    Some((_, _, g)) => *g,
                    None => {
                        let tris = |pieces: &[Piece], which: &[usize]| -> Vec<[P3; 3]> {
                            which.iter().flat_map(|i| pieces[*i].tris.iter().copied()).collect()
                        };
                        let g = surfaces(&tris(&made, &pa), &tris(&revit, &pb));
                        shapes.push((pa, pb, g));
                        g
                    }
                };
                out.push(Compared { detail: d, what: View::Shape, volume: Some((va, vb, curved)), boxes: Some((ba, bb)), gaps: shape });
                if first_failure && out.last().and_then(Compared::failed).is_some() {
                    return Ok(out);
                }
            }
            // Its nested parts' plan symbols with its own (Revit's result has them among its own).
            let marks = |snapshot: bool, view: SymbolView| -> Result<Vec<super::Mark>, String> {
                let mut out = self
                    .marks_where(&values, &|y: &super::Symbol| y.view == view && y.of_type(t) && y.snapshot == snapshot && y.drawn_at(d))?;
                if !snapshot && view == SymbolView::Plan {
                    self.nested_marks(t, &values, Some(d), others, (0, &std::cell::Cell::new(0)), &mut out)?;
                }
                Ok(out)
            };
            for view in [SymbolView::Plan, SymbolView::Front, SymbolView::Side] {
                let (ma, mb) = match (marks(false, view), marks(true, view)) {
                    (Ok(a), Ok(b)) => (a, b),
                    (Err(e), _) => return Err(format!("its {} lines don't come out: {e}", view_name(view))),
                    (_, Err(e)) => return Err(format!("Revit's {} lines can't be drawn: {e}", view_name(view))),
                };
                // What the plan draws: its symbols and the forms it shows from above (Revit's plan of a family it brings
                // may be its forms' lines, which BIMCraft draws from the forms).
                let lines = |marks: Vec<super::Mark>, pieces: &[Piece]| -> Vec<Vec<P2>> {
                    let mut runs: Vec<Vec<P2>> = marks.iter().map(runs_of).collect();
                    if view == SymbolView::Plan {
                        runs.extend(pieces.iter().filter(|p| p.in_plan_cut(d, cut, hosted)).flat_map(|p| plan_runs(p, cut)));
                    }
                    runs.retain(|r| !r.is_empty());
                    runs
                };
                let (la, lb) = (lines(ma, &made), lines(mb, &revit));
                let what = match view {
                    SymbolView::Plan => View::Plan,
                    SymbolView::Front => View::Front,
                    SymbolView::Side => View::Side,
                };
                out.push(Compared { detail: d, what, volume: None, boxes: None, gaps: drawings(&la, &lb) });
                if first_failure && out.last().and_then(Compared::failed).is_some() {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
}

/// What a view draws, compared: the shape (3D, sections and elevations), the plan's lines, or an elevation's own lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Shape,
    Plan,
    Front,
    Side,
}

fn view_name(v: SymbolView) -> &'static str {
    match v {
        SymbolView::Plan => "plan",
        SymbolView::Front => "front elevation",
        SymbolView::Side => "side elevation",
    }
}

/// How far BIMCraft's drawing and Revit's are from each other: `None` where one draws nothing, the other something.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gaps {
    /// Neither draws anything.
    Empty,
    /// Only BIMCraft's draws something, or only Revit's (`false`).
    Alone(bool),
    /// BIMCraft's measured to Revit's, and Revit's to BIMCraft's.
    Both(compare::Gap, compare::Gap),
}

/// Two shapes' surfaces measured each to the other: the outer surface of each as a solid, for Revit's joined forms are one
/// solid (the plan and the sections draw no line at a joint) and a mesh may hold faces back to back between its lumps.
fn surfaces(made: &[[P3; 3]], revit: &[[P3; 3]]) -> Gaps {
    match (compare::Surface::new(made, NEAR), compare::Surface::new(revit, NEAR)) {
        (None, None) => Gaps::Empty,
        (Some(_), None) => Gaps::Alone(true),
        (None, Some(_)) => Gaps::Alone(false),
        // Each measured as its own solid: its joined forms are one surface, so a face inside it is no part of it.
        (Some(a), Some(b)) => Gaps::Both(
            compare::surface_gap(made, &b, NEAR, REACH, Some(&compare::Solid::new(made))),
            compare::surface_gap(revit, &a, NEAR, REACH, Some(&compare::Solid::new(revit))),
        ),
    }
}

/// Two drawings' lines measured each to the other.
fn drawings(made: &[Vec<P2>], revit: &[Vec<P2>]) -> Gaps {
    match (compare::Lines::new(made, NEAR), compare::Lines::new(revit, NEAR)) {
        (None, None) => Gaps::Empty,
        (Some(_), None) => Gaps::Alone(true),
        (None, Some(_)) => Gaps::Alone(false),
        (Some(a), Some(b)) => Gaps::Both(compare::line_gap(made, &b, NEAR, REACH), compare::line_gap(revit, &a, NEAR, REACH)),
    }
}

/// "its plan" as Revit's: "plan".
fn strip(thing: &str) -> &str {
    thing.trim_start_matches("its ")
}

/// A mark's points as one run: a line's, a dot's centre, a label's place.
fn runs_of(m: &super::Mark) -> Vec<P2> {
    match m {
        super::Mark::Line(pts) => pts.clone(),
        super::Mark::Dot { centre, .. } => vec![*centre],
        super::Mark::Label { at, .. } => vec![*at],
    }
}

/// What a plan cutting at `cut` draws of a piece it draws: its section there if the plane cuts it (Revit's plan draws the
/// section of what its cut plane crosses, a frame's jambs and their rebates, not the silhouette round the whole frame), else
/// its outline seen from above.
fn plan_runs(p: &Piece, cut: f64) -> Vec<Vec<P2>> {
    if p.bottom < cut - CUT_EPS && cut < p.top {
        compare::section(&p.tris, cut).into_iter().map(|s| s.to_vec()).collect()
    } else {
        vec![closed(&p.outline)]
    }
}

/// An outline as a run back to its first point.
fn closed(outline: &[P2]) -> Vec<P2> {
    let mut run = outline.to_vec();
    if let (Some(a), Some(b)) = (outline.first(), outline.last())
        && a != b
        && outline.len() > 2
    {
        run.push(*a);
    }
    run
}

/// A box, its least corner and its most.
pub type Box3 = [[f64; 3]; 2];

/// One comparison of a type's rebuild with Revit's result: at a detail level, of what one view draws; for the shape, the
/// volumes (BIMCraft's, Revit's, and whether its definition has curves) and the boxes too.
#[derive(Clone, Debug, PartialEq)]
pub struct Compared {
    pub detail: Detail,
    pub what: View,
    pub volume: Option<(f64, f64, bool)>,
    pub boxes: Option<(Option<Box3>, Option<Box3>)>,
    pub gaps: Gaps,
}

impl Compared {
    /// Why it doesn't match, in plain words, or none.
    pub fn failed(&self) -> Option<String> {
        let l = level(self.detail);
        if let Some((va, vb, curved)) = self.volume {
            let off = (va - vb).abs();
            let share = if curved && va > vb { VOLUME_CURVED } else { VOLUME };
            if off > share * vb.max(1.0) && off > 1000.0 {
                let pct = if vb > 0.0 { off / vb * 100.0 } else { 100.0 };
                let way = if va > vb { "more" } else { "less" };
                return Some(format!("drawn {l} it holds {pct:.1} % {way} than Revit's ({:.4} m³ against {:.4} m³)", va / 1e9, vb / 1e9));
            }
        }
        if let Some((Some(a), Some(b))) = self.boxes {
            for k in 0..3 {
                let gap = (a[0][k] - b[0][k]).abs().max((a[1][k] - b[1][k]).abs());
                if gap > BOX {
                    return Some(format!("drawn {l} its {} is {gap:.0} mm off Revit's", AXES[k]));
                }
            }
        }
        let (thing, lines) = match self.what {
            View::Shape => ("its shape", false),
            View::Plan => ("its plan", true),
            View::Front => ("its front elevation", true),
            View::Side => ("its side elevation", true),
        };
        match self.gaps {
            Gaps::Empty => None,
            Gaps::Alone(true) if lines => Some(format!("{thing} drawn {l} has lines where Revit's has none")),
            Gaps::Alone(false) if lines => Some(format!("{thing} drawn {l} has no lines where Revit's has")),
            Gaps::Alone(true) => Some(format!("drawn {l} it shows something where Revit's shows nothing")),
            Gaps::Alone(false) => Some(format!("drawn {l} it shows nothing where Revit's shows something")),
            Gaps::Both(ours, theirs) => {
                let (worst, at, side) =
                    if ours.worst >= theirs.worst { (ours.worst, ours.at, "") } else { (theirs.worst, theirs.at, " Revit's has") };
                let share = ours.share.max(theirs.share);
                if worst <= NEAR || (worst <= NOISE && share <= NOISE_SHARE) {
                    return None;
                }
                let far = if worst >= REACH { format!("more than {REACH:.0}") } else { format!("{worst:.0}") };
                let share = share * 100.0;
                let at = if lines { format!("{:.0}, {:.0}", at[0], at[1]) } else { format!("{:.0}, {:.0}, {:.0}", at[0], at[1], at[2]) };
                let what = if side.is_empty() {
                    format!("{thing} drawn {l} is {far} mm off Revit's at ({at})")
                } else {
                    format!("drawn {l}, part of Revit's {} is missing from {thing}: {far} mm off at ({at})", strip(thing))
                };
                Some(format!("{what}, over {share:.1} % of it"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// A door frame as one sweep: a U round its opening (jambs 2100 high, 900 between their middles), its section a jamb with
    /// a rebate (100 wide and 100 deep, a 20 mm step on its inner side).
    fn frame() -> Value {
        json!({
            "path": [[-450, 0, 0], [-450, 0, 2100], [450, 0, 2100], [450, 0, 0]],
            "section": [[-500, -50, 0], [-420, -50, 0], [-420, 0, 0], [-400, 0, 0], [-400, 50, 0], [-500, 50, 0]]
        })
    }

    /// The family with its frame, and with Revit's result for it as a mesh that its plan doesn't draw (as Revit's plan doesn't
    /// draw what it draws by its symbols) and the plan symbols `plan` (Revit's jamb sections, as lines).
    fn door(plan: Vec<Value>) -> Family {
        let mut form = frame();
        form["show"] = json!({"coarse": false});
        let own: Family = serde_json::from_value(json!({"category": "door", "types": {"A": {}}, "forms": [form.clone()]})).unwrap();
        let made = own.pieces("A", &own.values("A", &BTreeMap::new()).unwrap()).unwrap();
        let mesh: Vec<Vec<Vec<f64>>> = made.iter().flat_map(|p| p.tris.iter().map(|t| t.iter().map(|q| q.to_vec()).collect())).collect();
        let revit = json!({"mesh": mesh, "snapshot": true, "show": {"coarse": false, "plan": false, "plan_top": false, "plan_cut": false}});
        let symbols: Vec<Value> = plan.into_iter().map(|line| json!({"line": line, "snapshot": true})).collect();
        serde_json::from_value(json!({"category": "door", "types": {"A": {}}, "forms": [form, revit], "symbols": symbols})).unwrap()
    }

    /// A jamb's section, as the plan's line: the rebated section's corners round to its first, shifted `dx` across.
    fn jamb(dx: f64, mirror: bool) -> Value {
        let pts = [[-50.0, -50.0], [30.0, -50.0], [30.0, 0.0], [50.0, 0.0], [50.0, 50.0], [-50.0, 50.0], [-50.0, -50.0]];
        json!(pts.iter().map(|[u, y]| [dx + if mirror { -u } else { *u }, *y]).collect::<Vec<_>>())
    }

    #[test]
    fn a_frame_swept_round_the_opening_is_drawn_in_plan_by_its_cut_jambs() {
        let f = door(vec![jamb(-450.0, false), jamb(450.0, true)]);
        let others: Others = &|_| None;
        let check = f.check_type("A", others, super::super::DEFAULT_WALL).expect("a type Revit's result is for");
        assert!(check.matched, "{:?}", check.reason);
        // Its plan: the two jambs' sections, the rebate among them, and no line across the opening.
        let values = f.values_in("A", &BTreeMap::new(), super::super::DEFAULT_WALL).unwrap();
        let pieces = f.pieces("A", &values).unwrap();
        let runs = plan_runs(&pieces[0], PLAN_CUT);
        // A face the triangles split is several segments: what matters is that the rebate's line is drawn along its length.
        let on = |p: P2| {
            runs.iter().any(|r| {
                let q = compare::near_on_segment(p, [r[0], r[1]]);
                r.len() == 2 && (p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6
            })
        };
        assert!([-45.0, -25.0, -5.0].iter().all(|y| on(P2(-420.0, *y)) && on(P2(420.0, *y))), "the rebate: {runs:?}");
        assert!(runs.iter().all(|r| r.iter().all(|p| p.0.abs() >= 400.0 - 1e-6)), "a line across the opening: {runs:?}");
        // The silhouette it replaces runs across the opening.
        assert!(closed(&pieces[0].outline).windows(2).any(|w| w[0].0 < 0.0 && w[1].0 > 0.0), "{:?}", pieces[0].outline);
    }

    /// A planting as Revit exports it: its 3D a render placeholder (a box as a mesh, drawn in no plan), its plan symbols a circle
    /// of the radius Revit scales for each type (`small` and `tall`), and its base family (a circle of radius 1000 when it is
    /// 6000 tall) nested in it scaled by the tree's Height over that.
    fn tree(rpc: bool, tall: f64) -> Family {
        let circle = |r: f64, types: &str| -> Vec<Value> {
            [(0, 180), (180, 360)]
                .iter()
                .map(|(a, b)| json!({"snapshot": true, "arc": {"centre": [0, 0], "radius": r, "from": a, "to": b}, "types": [types]}))
                .collect()
        };
        let base = json!({"category": "generic", "types": {"Base": {}},
            "symbols": [{"arc": {"centre": [0, 0], "radius": 1000, "from": 0, "to": 180}}, {"arc": {"centre": [0, 0], "radius": 1000, "from": 180, "to": 360}}]});
        let mesh: Vec<Value> = [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]]
            .iter()
            .map(|p| json!([[p[0], p[1], p[2]], [p[0] + 100.0, p[1], p[2]], [p[0], p[1] + 100.0, p[2]]]))
            .collect();
        let mut symbols = circle(500.0, "Small");
        symbols.extend(circle(tall, "Tall"));
        serde_json::from_value(json!({
            "category": "generic", "rpc": rpc,
            "parameters": [{"name": "Height", "kind": "length", "value": 3000}],
            "types": {"Small": {"Height": 3000}, "Tall": {"Height": 6000}},
            "forms": [{"snapshot": true, "mesh": mesh, "show": {"plan": false, "plan_top": false, "plan_cut": false}}],
            "symbols": symbols,
            "nested": [{"family": "Base", "type": "Base", "at": [0, 0, 0], "scale": "Height / 6000"}],
            "embedded": {"Base": base}
        }))
        .unwrap()
    }

    #[test]
    fn an_rpc_planting_is_checked_in_plan_only_and_its_3d_is_revits() {
        let others: Others = &|_| None;
        // Not an RPC family: its rules draw no solid where Revit's placeholder is, which fails it.
        let plain = tree(false, 1000.0);
        let check = plain.check_type("Small", others, super::super::DEFAULT_WALL).expect("a type Revit's result is for");
        assert!(!check.matched, "{:?}", check.reason);
        // An RPC one: only its plan is compared, the nested base scaled by Height / 6000 (a half, a whole).
        let mut f = tree(true, 1000.0);
        for t in ["Small", "Tall"] {
            let check = f.check_type(t, others, super::super::DEFAULT_WALL).expect("checked");
            assert!(check.matched, "{t}: {:?}", check.reason);
            assert_eq!(check.reason.as_deref(), Some(RPC_3D));
        }
        f.run_check(others, &Default::default());
        assert!(f.check.values().all(|c| c.matched) && f.editable.as_ref().is_some_and(|e| e.ok), "{:?}", f.editable);
        // Its plan is drawn from its rules (the base scaled), its 3D is always Revit's.
        assert!(!f.draws_snapshot("Tall") && f.draws_snapshot_forms("Tall"));
        let values = f.values("Tall", &BTreeMap::new()).unwrap();
        let pieces = f.pieces("Tall", &values).unwrap();
        assert_eq!(pieces.len(), 1);
        assert!(pieces[0].top > 2999.0, "Revit's box, not what the rules build");
        let marks = f.marks_with("Tall", &values, SymbolView::Plan, None, others).unwrap();
        let far = marks.iter().flat_map(runs_of).map(|p| p.0.abs().max(p.1.abs())).fold(0.0, f64::max);
        assert!((far - 1000.0).abs() < 1e-6, "the base at a whole: {far}");
        let small = f.marks_with("Small", &f.values("Small", &BTreeMap::new()).unwrap(), SymbolView::Plan, None, others).unwrap();
        let far = small.iter().flat_map(runs_of).map(|p| p.0.abs().max(p.1.abs())).fold(0.0, f64::max);
        assert!((far - 500.0).abs() < 1e-6, "the base at a half: {far}");
    }

    #[test]
    fn an_rpc_planting_whose_plan_is_off_still_fails_and_is_drawn_as_revit_made_it() {
        // Revit's plan circle for Tall is 900 across where the base scaled makes 1000.
        let mut f = tree(true, 900.0);
        f.run_check(&|_| None, &Default::default());
        assert!(f.check["Small"].matched);
        let c = &f.check["Tall"];
        assert!(!c.matched && c.reason.as_deref().is_some_and(|r| r.contains("plan")), "{c:?}");
        assert!(f.draws_snapshot("Tall") && f.draws_snapshot_forms("Tall"));
        assert!(f.editable.as_ref().is_some_and(|e| !e.ok));
    }

    #[test]
    fn a_nested_part_with_no_scale_is_drawn_as_it_is_and_one_that_scales_to_nothing_is_refused() {
        let mut f = tree(true, 1000.0);
        f.nested[0].scale = None;
        let values = f.values("Small", &BTreeMap::new()).unwrap();
        let marks = f.marks_with("Small", &values, SymbolView::Plan, None, &|_| None).unwrap();
        assert!(marks.iter().flat_map(runs_of).any(|p| p.0.abs() > 900.0));
        f.nested[0].scale = Some(super::super::Expr::Formula("Height - 3000".into()));
        let err = f.marks_with("Small", &values, SymbolView::Plan, None, &|_| None).unwrap_err();
        assert!(err.contains("scale"), "{err}");
    }

    #[test]
    fn a_frame_whose_plan_lacks_a_rebate_does_not_match() {
        // The jambs plain rectangles: Revit's plan has no rebate, ours does.
        let plain = |dx: f64| json!([[dx - 50.0, -50.0], [dx + 50.0, -50.0], [dx + 50.0, 50.0], [dx - 50.0, 50.0], [dx - 50.0, -50.0]]);
        let f = door(vec![plain(-450.0), plain(450.0)]);
        let others: Others = &|_| None;
        let check = f.check_type("A", others, super::super::DEFAULT_WALL).expect("checked");
        assert!(!check.matched);
    }
}
