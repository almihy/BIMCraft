//! Fields: a car park set out as a stack of modules across a field (Arqiflow's solver.ts, "local plan" and "on the
//! plot"). A field is set out along one direction as aisles with a row of bays each side, perimeter rows, cross aisles
//! at both ends and end rows; the plot is one field, or two either side of a drive aisle cut at an inside corner.

use std::rc::Rc;

use super::geom::*;
use super::standards::{lean_of, pitch_of, row_depth, Settings};

// ====================================================================================================== shared
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AisleKind {
    Aisle,
    Cross,
    Drive,
    Throat,
}

impl AisleKind {
    pub fn name(self) -> &'static str {
        match self {
            AisleKind::Aisle => "aisle",
            AisleKind::Cross => "cross",
            AisleKind::Drive => "drive",
            AisleKind::Throat => "throat",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StallKind {
    Standard,
    Accessible,
    Ev,
}

impl StallKind {
    pub fn name(self) -> &'static str {
        match self {
            StallKind::Standard => "standard",
            StallKind::Accessible => "accessible",
            StallKind::Ev => "ev",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IslandKind {
    End,
    Row,
    Leftover,
}

/// What one solve keeps as it goes: the aisles' numbers, and each field laid out (the same part of the plot turns up
/// from more than one cut).
pub struct Ctx {
    pub next_id: i64,
    pub field_memo: FxMap<FieldKey, Option<Rc<Plan>>>,
}

impl Ctx {
    pub fn new() -> Ctx {
        Ctx { next_id: 0, field_memo: FxMap::default() }
    }
    pub fn id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// What a bay is worth in paving: a layout with more aisle must win this many m² of it back a bay.
pub const PAVED_PER_BAY: f64 = 100.0;
/// A stall facing a drive aisle keeps the drive's id above this, so `place` leaves it as it is.
pub const EXT_BASE: i64 = 1_000_000;

// ====================================================================================================== local plan
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
    Rect { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
}
pub fn corners(r: &Rect) -> Ring {
    vec![[r.x0, r.y0], [r.x1, r.y0], [r.x1, r.y1], [r.x0, r.y1]]
}
pub fn area(r: &Rect) -> f64 {
    (r.x1 - r.x0) * (r.y1 - r.y0)
}

#[derive(Clone, Debug)]
pub struct LAisle {
    pub id: i64,
    pub r: Rect,
    pub kind: AisleKind,
    /// A one-way aisle's direction along x: 1 or -1.
    pub flow: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    N,
    S,
    E,
    W,
}

#[derive(Clone, Debug)]
pub struct LStall {
    pub r: Rect,
    pub aisle: i64,
    pub side: Side,
    pub row: String,
    pub cell: i64,
    pub kind: StallKind,
    pub sh: f64,
}

#[derive(Clone, Debug)]
pub struct LIsland {
    pub r: Rect,
    pub poly: Option<Ring>,
    pub kind: IslandKind,
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub aisles: Vec<LAisle>,
    pub stalls: Vec<LStall>,
    pub islands: Vec<LIsland>,
    pub columns: Option<Vec<Rect>>,
    /// Laid out only to count its bays: how many.
    pub count: Option<f64>,
}

/// A field's long side: a row on its own aisle, nothing, a row facing a drive aisle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FSide {
    Row,
    None,
    Out,
}
/// A field's end: end bays on a cross aisle, a cross aisle alone, open to a drive aisle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Row,
    None,
    Open,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ext {
    pub s: bool,
    pub n: bool,
    pub w: bool,
    pub e: bool,
}

pub struct FieldSpec<'a> {
    pub region: &'a Region,
    pub ext: Ext,
    /// The side a street runs along: what the rows leave over is planted there.
    pub street: Option<Side>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldKey(Vec<i64>, Ext, u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StripKind {
    Aisle,
    RowN,
    RowS,
}

#[derive(Clone, Copy, Debug)]
struct Strip {
    y0: f64,
    y1: f64,
    kind: StripKind,
    aisle: i64,
}

fn push_strip(strips: &mut Vec<Strip>, y: &mut f64, kind: StripKind, h: f64, aisle: i64) {
    strips.push(Strip { y0: *y, y1: *y + h, kind, aisle });
    *y += h;
}

/// `((k % m) + m) % m === v` as JavaScript works it, in numbers.
fn modeq(k: f64, m: f64, v: f64) -> bool {
    ((k % m) + m) % m == v
}

/// A bay's outline: its front on the aisle, its back shifted along the row when the bays are angled.
pub fn stall_poly(r: &Rect, side: Side, sh: f64) -> Ring {
    let Rect { x0, y0, x1, y1 } = *r;
    match side {
        Side::N => vec![[x0 + sh, y0], [x1 + sh, y0], [x1, y1], [x0, y1]],
        Side::S => vec![[x0, y0], [x1, y0], [x1 + sh, y1], [x0 + sh, y1]],
        _ => corners(r),
    }
}

pub fn mouth_of(r: &Rect, side: Side) -> Pt {
    let Rect { x0, y0, x1, y1 } = *r;
    match side {
        Side::N => [(x0 + x1) / 2.0, y1],
        Side::S => [(x0 + x1) / 2.0, y0],
        Side::E => [x1, (y0 + y1) / 2.0],
        Side::W => [x0, (y0 + y1) / 2.0],
    }
}

pub fn near(r: &Rect, p: Pt, tol: f64) -> bool {
    p[0] >= r.x0 - tol && p[0] <= r.x1 + tol && p[1] >= r.y0 - tol && p[1] <= r.y1 + tol
}

/// Two pieces that clash: they share more than a sliver.
pub fn hits(a: &[Pt], b: &[Pt]) -> bool {
    shared_area(a, b) > 0.005
}

/// The widest span (the first of the widest).
fn widest(spans: &[Span]) -> Option<Span> {
    let mut it = spans.iter();
    let mut best = *it.next()?;
    for q in it {
        if q[1] - q[0] > best[1] - best[0] {
            best = *q;
        }
    }
    Some(best)
}

/// The field laid out one way: a stack of `n` aisles across it, its sides and ends as said. None when the aisles do
/// not fit inside.
#[allow(clippy::too_many_arguments)]
fn lay_field(
    f: &FieldSpec,
    s: &Settings,
    n: i64,
    south: FSide,
    north: FSide,
    west: End,
    east: End,
    offset: f64,
    tally: bool,
) -> Option<Plan> {
    let g = f.region;
    let b = bounds(&g.outer);
    let r = row_depth(s);
    let single = s.one_way && n == 1;
    let a = if single { s.aisle_two_way } else { s.aisle };
    let w = pitch_of(s);
    let lean = lean_of(s);
    let er = s.stall_depth;
    let ew = s.stall_width;
    let ca = if s.angle < 89.9 { s.drive_aisle } else { a };
    let e = s.end_island;
    let mut strips: Vec<Strip> = Vec::new();
    let mut y = b.y0 + offset;
    const EXT_S: i64 = -1;
    const EXT_N: i64 = -2;
    if n == 0 {
        if south == FSide::Out {
            push_strip(&mut strips, &mut y, StripKind::RowS, r, EXT_S);
        }
        if north == FSide::Out {
            y = b.y1 - r;
            push_strip(&mut strips, &mut y, StripKind::RowN, r, EXT_N);
        }
    } else {
        if south == FSide::Out {
            push_strip(&mut strips, &mut y, StripKind::RowS, r, EXT_S);
            push_strip(&mut strips, &mut y, StripKind::RowN, r, 0);
        } else if south == FSide::Row {
            push_strip(&mut strips, &mut y, StripKind::RowN, r, 0);
        }
        for i in 0..n {
            push_strip(&mut strips, &mut y, StripKind::Aisle, a, i);
            if i < n - 1 {
                push_strip(&mut strips, &mut y, StripKind::RowS, r, i);
                push_strip(&mut strips, &mut y, StripKind::RowN, r, i + 1);
            }
        }
        if north == FSide::Row {
            push_strip(&mut strips, &mut y, StripKind::RowS, r, n - 1);
        } else if north == FSide::Out {
            push_strip(&mut strips, &mut y, StripKind::RowS, r, n - 1);
            push_strip(&mut strips, &mut y, StripKind::RowN, r, EXT_N);
        }
    }
    if strips.is_empty() || y > b.y1 + 1e-6 {
        return None;
    }

    let mut plan = Plan::default();
    let own: Vec<Strip> = strips.iter().filter(|t| t.kind == StripKind::Aisle).copied().collect();
    let mut xl;
    let mut xr;
    if let (Some(first), Some(last)) = (own.first(), own.last()) {
        let y_a = first.y0;
        let y_b = last.y1;
        let spans = strip_spans(g, y_a, y_b);
        let [x0s, x1s] = widest(&spans)?;
        let west_face = if west == End::Open { x0s } else { x0s + (if west == End::Row { er } else { 0.0 }) + ca };
        let east_face = if east == End::Open { x1s } else { x1s - (if east == End::Row { er } else { 0.0 }) - ca };
        if east_face - west_face < (w * s.min_run).max(6.0) {
            return None;
        }
        xl = west_face;
        xr = east_face;
        let aisle_x0 = if west == End::Open { x0s } else { west_face - ca };
        let aisle_x1 = if east == End::Open { x1s } else { east_face + ca };
        for t in &own {
            let ar = rect(aisle_x0, t.y0, aisle_x1, t.y1);
            if !box_inside(g, ar.x0, ar.y0, ar.x1, ar.y1) {
                return None;
            }
            let flow = if s.one_way && !single { Some(if t.aisle % 2 == 0 { 1.0 } else { -1.0 }) } else { None };
            plan.aisles.push(LAisle { id: t.aisle, r: ar, kind: AisleKind::Aisle, flow });
        }
        if west != End::Open {
            plan.aisles.push(LAisle { id: 100, r: rect(west_face - ca, y_a, west_face, y_b), kind: AisleKind::Cross, flow: None });
        }
        if east != End::Open {
            plan.aisles.push(LAisle { id: 101, r: rect(east_face, y_a, east_face + ca, y_b), kind: AisleKind::Cross, flow: None });
        }
        // End rows: bays facing the cross aisles from the field's ends.
        let end_row = |plan: &mut Plan, x0: f64, x1: f64, aisle: i64, side: Side, tag: &str| {
            let len = y_b - y_a;
            let k = (len / ew + 1e-9).floor();
            let start = y_a + (len - k * ew) / 2.0;
            let cols = column_spans(g, x0, x1);
            let mut i = 0.0;
            while i < k {
                let sr = rect(x0, start + i * ew, x1, start + (i + 1.0) * ew);
                i += 1.0;
                if !cols.iter().any(|&[p, q]| p <= sr.y0 + 1e-6 && q >= sr.y1 - 1e-6) {
                    continue;
                }
                if tally {
                    plan.count = Some(plan.count.unwrap_or(0.0) + 1.0);
                } else {
                    plan.stalls.push(LStall {
                        r: sr,
                        aisle,
                        side,
                        row: tag.to_string(),
                        cell: (i - 1.0) as i64,
                        kind: StallKind::Standard,
                        sh: 0.0,
                    });
                }
            }
        };
        if west == End::Row {
            end_row(&mut plan, west_face - ca - er, west_face - ca, 100, Side::E, "W");
        }
        if east == End::Row {
            end_row(&mut plan, east_face + ca, east_face + ca + er, 101, Side::W, "E");
        }
    } else {
        let spans = strip_spans(g, strips[0].y0, strips[strips.len() - 1].y1);
        let [p, q] = widest(&spans)?;
        xl = p;
        xr = q;
    }
    let _ = (&mut xl, &mut xr);

    // One grid for every row: the bays between the cross aisles, the slack shared at both ends.
    let cg = if s.column_every > 0.0 && lean == 0.0 { js_round(s.column_every) } else { 0.0 };
    let cs = if cg != 0.0 { s.column_size } else { 0.0 };
    let inner = xr - xl - 2.0 * e;
    let pp = w + if cg != 0.0 { cs / cg } else { 0.0 };
    let cells = ((inner + if cg != 0.0 { cs } else { 0.0 }) / pp + 1e-9).floor();
    let used = cells * w + if cg != 0.0 { 0f64.max(((cells - 1.0) / cg).floor()) * cs } else { 0.0 };
    let gx = xl + e + (inner - used) / 2.0;
    let xk = |k: f64| gx + k * w + if cg != 0.0 { (k / cg).floor() * cs } else { 0.0 };
    let every = if s.island_every > 0.0 { s.island_every } else { 0.0 };
    let ns = strips.len();
    for (ti, t) in strips.iter().enumerate() {
        if t.kind == StripKind::Aisle {
            continue;
        }
        let faces_own = t.aisle >= 0;
        let outer = faces_own && (ti == 0 || ti == ns - 1);
        let mut lo = xl + e;
        let mut hi = xr - e;
        if outer && !own.is_empty() {
            if let Some(al) = plan.aisles.iter().find(|x| x.id == t.aisle) {
                lo = al.r.x0 + e;
                hi = al.r.x1 - e;
            }
        }
        if !faces_own {
            lo = b.x0 + e;
            hi = b.x1 - e;
        }
        let flow = if faces_own { plan.aisles.iter().find(|x| x.id == t.aisle).and_then(|x| x.flow).unwrap_or(1.0) } else { 1.0 };
        let sh = flow * lean;
        lo += 0f64.max(-sh);
        hi -= 0f64.max(sh);
        let spans = strip_spans(g, t.y0, t.y1);
        let cgf = if cg != 0.0 { 1.0 } else { 0.0 };
        let k0 = ((lo - gx) / pp - 1e-9).ceil() - cgf;
        let k1 = ((hi - gx) / pp + 1e-9).floor() + cgf;
        let tag = to_fixed(t.y0, 2);
        let side = if t.kind == StripKind::RowN { Side::N } else { Side::S };
        let cell = |k: f64| rect(xk(k), t.y0, xk(k) + w, t.y1);
        let mut run: Vec<f64> = Vec::new();
        let close = |run: &mut Vec<f64>, plan: &mut Plan| {
            let len = run.len() as f64;
            if tally {
                if len >= s.min_run {
                    plan.count = Some(plan.count.unwrap_or(0.0) + len);
                }
            } else if len >= s.min_run {
                for &k in run.iter() {
                    plan.stalls.push(LStall {
                        r: cell(k),
                        aisle: t.aisle,
                        side,
                        row: tag.clone(),
                        cell: k as i64,
                        kind: StallKind::Standard,
                        sh,
                    });
                }
            } else {
                for &k in run.iter() {
                    let r = cell(k);
                    plan.islands.push(LIsland { r, kind: IslandKind::Leftover, poly: Some(stall_poly(&r, side, sh)) });
                }
            }
            run.clear();
        };
        let mut k_first = f64::INFINITY;
        let mut k_last = f64::NEG_INFINITY;
        for &[p, q] in &spans {
            let a0 = k0.max(((p - 0f64.min(sh) - 1e-4 - gx) / pp - 1e-9).ceil() - cgf);
            let b0 = (k1 - 1.0).min(((q - 0f64.max(sh) + 1e-4 - gx) / pp - 1.0 + 1e-9).floor() + cgf);
            let mut k = a0;
            // A span is bounded by the plot; a broken one is not walked.
            if !(b0 - a0 < 1e6) {
                continue;
            }
            while k <= b0 {
                let kk = k;
                k += 1.0;
                if cg != 0.0 && (xk(kk) + 0f64.min(sh) < p.max(lo) - 1e-4 || xk(kk) + w + 0f64.max(sh) > q.min(hi) + 1e-4) {
                    continue;
                }
                k_first = k_first.min(kk);
                k_last = k_last.max(kk);
                if let Some(&last) = run.last() {
                    if kk != last + 1.0 {
                        close(&mut run, &mut plan);
                    }
                }
                if every != 0.0 && modeq(kk, every + 1.0, every) {
                    close(&mut run, &mut plan);
                    let r = cell(kk);
                    plan.islands.push(LIsland { r, kind: IslandKind::Row, poly: Some(stall_poly(&r, side, sh)) });
                    continue;
                }
                run.push(kk);
            }
        }
        close(&mut run, &mut plan);
        // The end islands, where the row stops short of a cross aisle.
        if e > 0.0 && faces_own && !own.is_empty() && !outer {
            let xf = if cg != 0.0 {
                if k_first <= k_last {
                    xk(k_first)
                } else {
                    xl
                }
            } else {
                gx + k0 * w
            };
            let xe = if cg != 0.0 {
                if k_first <= k_last {
                    xk(k_last) + w
                } else {
                    xr
                }
            } else {
                gx + k1 * w
            };
            let nn = t.kind == StripKind::RowN;
            if west != End::Open && xf > xl + 0.05 {
                let poly = if nn {
                    vec![[xl, t.y0], [xf + sh, t.y0], [xf, t.y1], [xl, t.y1]]
                } else {
                    vec![[xl, t.y0], [xf, t.y0], [xf + sh, t.y1], [xl, t.y1]]
                };
                plan.islands.push(LIsland { r: rect(xl, t.y0, xf, t.y1), kind: IslandKind::End, poly: Some(poly) });
            }
            if east != End::Open && xr > xe + 0.05 {
                let poly = if nn {
                    vec![[xe + sh, t.y0], [xr, t.y0], [xr, t.y1], [xe, t.y1]]
                } else {
                    vec![[xe, t.y0], [xr, t.y0], [xr, t.y1], [xe + sh, t.y1]]
                };
                plan.islands.push(LIsland { r: rect(xe, t.y0, xr, t.y1), kind: IslandKind::End, poly: Some(poly) });
            }
        }
    }
    if cg != 0.0 && !tally {
        let rows: Vec<(f64, f64, StripKind)> = strips.iter().map(|t| (t.y0, t.y1, t.kind)).collect();
        plan.columns = Some(columns_of(&plan, &rows, &xk, w, cg, cs));
    }
    Some(plan)
}

/// Below ground, the columns of a field or a middle: where two rows stand back to back, in each gap the grid leaves
/// after a group of `cg` bays with a bay beyond it in either row.
fn columns_of(plan: &Plan, strips: &[(f64, f64, StripKind)], xk: &dyn Fn(f64) -> f64, w: f64, cg: f64, cs: f64) -> Vec<Rect> {
    // Each row's cells, in the order the bays came.
    let mut cells_of: Vec<(&str, Vec<i64>)> = Vec::new();
    for st in &plan.stalls {
        match cells_of.iter_mut().find(|(r, _)| *r == st.row) {
            Some((_, v)) => {
                if !v.contains(&st.cell) {
                    v.push(st.cell);
                }
            }
            None => cells_of.push((&st.row, vec![st.cell])),
        }
    }
    let get = |tag: &str| cells_of.iter().find(|(r, _)| *r == tag).map(|(_, v)| v.clone()).unwrap_or_default();
    let mut out = Vec::new();
    for i in 0..strips.len().saturating_sub(1) {
        let (t, u) = (strips[i], strips[i + 1]);
        if t.2 != StripKind::RowS || u.2 != StripKind::RowN {
            continue;
        }
        let mut ks = get(&to_fixed(t.0, 2));
        for k in get(&to_fixed(u.0, 2)) {
            if !ks.contains(&k) {
                ks.push(k);
            }
        }
        for &k in &ks {
            if !modeq((k + 1) as f64, cg, 0.0) || !ks.contains(&(k + 1)) {
                continue;
            }
            let x = xk(k as f64) + w;
            out.push(rect(x, t.1 - cs / 2.0, x + cs, t.1 + cs / 2.0));
        }
    }
    out
}

pub fn columns_of_rows(plan: &Plan, rows: &[(f64, f64, bool, bool)], xk: &dyn Fn(f64) -> f64, w: f64, cg: f64, cs: f64) -> Vec<Rect> {
    let strips: Vec<(f64, f64, StripKind)> = rows
        .iter()
        .map(|&(y0, y1, aisle, north)| {
            (
                y0,
                y1,
                if aisle {
                    StripKind::Aisle
                } else if north {
                    StripKind::RowN
                } else {
                    StripKind::RowS
                },
            )
        })
        .collect();
    columns_of(plan, &strips, xk, w, cg, cs)
}

/// The best layout of one field: every stack, side and end treatment tried; the most bays wins.
pub fn best_field(ctx: &mut Ctx, f: &FieldSpec, s: &Settings) -> Option<Rc<Plan>> {
    let mut key: Vec<i64> = Vec::new();
    let mm = |v: f64| js_round(v * 1000.0) as i64;
    key.push(f.region.outer.len() as i64);
    for p in &f.region.outer {
        key.push(mm(p[0]));
        key.push(mm(p[1]));
    }
    for h in &f.region.holes {
        key.push(h.len() as i64);
        for p in h {
            key.push(mm(p[0]));
            key.push(mm(p[1]));
        }
    }
    let k = FieldKey(
        key,
        f.ext,
        match f.street {
            None => 0,
            Some(Side::S) => 1,
            Some(_) => 2,
        },
    );
    if let Some(p) = ctx.field_memo.get(&k) {
        return p.clone();
    }
    let plan = best_field_of(f, s).map(Rc::new);
    ctx.field_memo.insert(k, plan.clone());
    plan
}

fn stack_height(n: i64, south: FSide, north: FSide, s: &Settings) -> f64 {
    let r = row_depth(s);
    if n == 0 {
        return (if south == FSide::Out { r } else { 0.0 }) + (if north == FSide::Out { r } else { 0.0 });
    }
    let side = |x: FSide| match x {
        FSide::Row => r,
        FSide::Out => 2.0 * r,
        FSide::None => 0.0,
    };
    n as f64 * (if s.one_way && n == 1 { s.aisle_two_way } else { s.aisle }) + (n - 1) as f64 * 2.0 * r + side(south) + side(north)
}

fn paved_of(plan: &Plan) -> f64 {
    let mut t = 0.0;
    for x in &plan.aisles {
        t += area(&x.r);
    }
    t
}

fn best_field_of(f: &FieldSpec, s: &Settings) -> Option<Plan> {
    let b = bounds(&f.region.outer);
    let h = b.y1 - b.y0;
    let r = row_depth(s);
    let a = s.aisle;
    let square = region_area_js(f.region) / 1f64.max((b.x1 - b.x0) * h) > 0.97;
    let sides = [FSide::Row, FSide::None];
    let mut best: Option<(Plan, f64)> = None;
    let souths: Vec<FSide> = if f.ext.s { vec![FSide::Out, FSide::Row, FSide::None] } else { sides.to_vec() };
    let norths: Vec<FSide> = if f.ext.n { vec![FSide::Out, FSide::Row, FSide::None] } else { sides.to_vec() };
    let wests: Vec<End> = if f.ext.w { vec![End::Open, End::Row, End::None] } else { vec![End::Row, End::None] };
    let easts: Vec<End> = if f.ext.e { vec![End::Open, End::Row, End::None] } else { vec![End::Row, End::None] };
    let n_max = ((h + 2.0 * r) / (a + 2.0 * r)).floor() + 1.0;
    if !(n_max < 10_000.0) {
        return None;
    }
    let n_max = n_max as i64;
    struct Stack {
        n: i64,
        so: FSide,
        no: FSide,
        off: f64,
        slack: f64,
        count: f64,
    }
    let mut stacks: Vec<Stack> = Vec::new();
    let n_from = if f.ext.s || f.ext.n { 0 } else { 1 };
    for n in n_from..=n_max {
        let sos: Vec<FSide> = if n == 0 { vec![FSide::Out, FSide::None] } else { souths.clone() };
        let nos: Vec<FSide> = if n == 0 { vec![FSide::Out, FSide::None] } else { norths.clone() };
        for &so in &sos {
            for &no in &nos {
                if n == 0 && so != FSide::Out && no != FSide::Out {
                    continue;
                }
                let height = stack_height(n, so, no, s);
                let slack = h - height;
                if slack < -1e-6 {
                    continue;
                }
                let offsets: Vec<f64> = if so == FSide::Out && no == FSide::Out {
                    if n == 0 || slack < 0.05 {
                        vec![0.0]
                    } else {
                        vec![]
                    }
                } else if so == FSide::Out {
                    vec![0.0]
                } else if no == FSide::Out {
                    vec![slack]
                } else if square {
                    vec![slack / 2.0, 0.0, slack]
                } else {
                    let steps = 6f64.min(1f64.max((slack / 1.0).ceil()));
                    let mut o = vec![slack / 2.0];
                    let mut i = 0.0;
                    while i <= steps {
                        o.push((slack * i) / steps);
                        i += 1.0;
                    }
                    o
                };
                for off in offsets {
                    if let Some(p) = lay_field(f, s, n, so, no, wests[0], easts[0], off, true) {
                        stacks.push(Stack { n, so, no, off, slack, count: p.count.unwrap_or(0.0) - paved_of(&p) / PAVED_PER_BAY });
                    }
                }
            }
        }
    }
    stacks.sort_by(|p, q| q.count.partial_cmp(&p.count).unwrap_or(std::cmp::Ordering::Equal));
    for t in stacks.iter().take(8) {
        for &we in &wests {
            for &ea in &easts {
                let Some(plan) = lay_field(f, s, t.n, t.so, t.no, we, ea, t.off, false) else { continue };
                let paved = paved_of(&plan);
                let to_street = if t.slack > 0.05 {
                    match f.street {
                        Some(Side::S) => 1.0 - t.off / t.slack,
                        Some(_) => t.off / t.slack,
                        None => 0.0,
                    }
                } else {
                    0.0
                };
                let score = plan.stalls.len() as f64 - paved / PAVED_PER_BAY - 0.01 * to_street;
                if best.as_ref().is_none_or(|(_, b)| score > *b) {
                    best = Some((plan, score));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

// ====================================================================================================== on the plot
#[derive(Clone, Debug)]
pub struct WAisle {
    pub id: i64,
    pub poly: Ring,
    pub kind: AisleKind,
    pub flow: Option<Pt>,
}

#[derive(Clone, Debug)]
pub struct WStall {
    pub poly: Ring,
    pub mouth: Pt,
    pub aisle: i64,
    pub row: String,
    pub cell: i64,
    pub kind: StallKind,
    pub access: bool,
}

#[derive(Clone, Debug)]
pub struct WIsland {
    pub poly: Ring,
    pub kind: IslandKind,
}

#[derive(Clone, Debug, Default)]
pub struct WPlan {
    pub aisles: Vec<WAisle>,
    pub stalls: Vec<WStall>,
    pub islands: Vec<WIsland>,
    pub columns: Vec<Ring>,
    pub fields: i64,
    pub dirs: Vec<f64>,
    /// Laid out as a ring round the plot, not as fields.
    pub ring: bool,
}

/// A field's plan, laid out in `frame`, on the plot.
pub fn place(ctx: &mut Ctx, plan: &Plan, frame: &Frame, tag: &str, deg: f64) -> WPlan {
    let mut ids: FxMap<i64, i64> = FxMap::default();
    let w = |p: Pt| to_world(frame, p);
    let aisles: Vec<WAisle> = plan
        .aisles
        .iter()
        .map(|a| {
            let id = ctx.id();
            ids.insert(a.id, id);
            WAisle { id, poly: corners(&a.r).into_iter().map(w).collect(), kind: a.kind, flow: a.flow.map(|f| [f * frame.c, f * frame.s]) }
        })
        .collect();
    let stalls = plan
        .stalls
        .iter()
        .map(|st| WStall {
            poly: stall_poly(&st.r, st.side, st.sh).into_iter().map(w).collect(),
            mouth: w(mouth_of(&st.r, st.side)),
            aisle: if st.aisle >= EXT_BASE { st.aisle } else { ids.get(&st.aisle).copied().unwrap_or(-1) },
            row: format!("{tag}|{}|{}", st.row, st.aisle),
            cell: st.cell,
            kind: st.kind,
            access: false,
        })
        .collect();
    let islands = plan
        .islands
        .iter()
        .map(|i| WIsland { poly: i.poly.clone().unwrap_or_else(|| corners(&i.r)).into_iter().map(w).collect(), kind: i.kind })
        .collect();
    let columns = plan.columns.as_ref().map(|c| c.iter().map(|r| corners(r).into_iter().map(w).collect()).collect()).unwrap_or_default();
    WPlan { aisles, stalls, islands, columns, fields: 1, dirs: vec![deg], ring: false }
}

pub fn join(parts: Vec<WPlan>, drives: Vec<WAisle>) -> WPlan {
    let mut out = WPlan { aisles: drives, ..WPlan::default() };
    for p in parts {
        out.aisles.extend(p.aisles);
        out.stalls.extend(p.stalls);
        out.islands.extend(p.islands);
        out.columns.extend(p.columns);
        out.fields += p.fields;
        out.dirs.extend(p.dirs);
        out.ring = out.ring || p.ring;
    }
    out
}

/// The angle of a side, 0 to 180.
pub fn side_deg(p: Pt, q: Pt) -> f64 {
    ((q[1] - p[1]).atan2(q[0] - p[0]) * 180.0 / std::f64::consts::PI + 360.0) % 180.0
}

/// The directions the rows could run: the outline's longest sides, and square to them.
pub fn directions(site: &[Pt], most: usize, least: f64) -> Vec<f64> {
    let n = site.len();
    let mut sides: Vec<(f64, f64)> = (0..n).map(|i| (side_deg(site[i], site[(i + 1) % n]), dist(site[i], site[(i + 1) % n]))).collect();
    sides.sort_by(|p, q| q.1.partial_cmp(&p.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f64> = Vec::new();
    for (deg, len) in sides {
        if len < least {
            break;
        }
        for d in [deg, (deg + 90.0) % 180.0] {
            if out.iter().all(|&o| (d - o).abs().min(180.0 - (d - o).abs()) > 1.0) {
                out.push(d);
            }
        }
        if out.len() >= most {
            break;
        }
    }
    out
}

pub fn paved_w(p: &WPlan) -> f64 {
    let mut t = 0.0;
    for a in &p.aisles {
        t += ring_area(&a.poly).abs();
    }
    t
}

pub fn score_of(p: &WPlan) -> f64 {
    p.stalls.len() as f64 - paved_w(p) / PAVED_PER_BAY
}

fn angle_off(a: f64, b: f64) -> f64 {
    (a - b).abs().min(180.0 - (a - b).abs())
}

/// The plot laid out with the rows along `deg`: as one field, or as two either side of a drive aisle cut at an inside
/// corner, each of the two then also tried in its own directions.
pub fn lay_plot(
    ctx: &mut Ctx,
    usable: &Region,
    deg: f64,
    s: &Settings,
    origin: Pt,
    streets_w: &[(Pt, Pt)],
    own_directions: bool,
) -> Vec<WPlan> {
    let frame = frame_of(origin, deg);
    let g = region_to_local(&frame, usable);
    let street_side = |f: &Frame, r: &Region| -> Option<Side> {
        let bb = bounds(&r.outer);
        let mut best: Option<(f64, Side)> = None;
        for &(wa, wb) in streets_w {
            let a = to_local(f, wa);
            let b = to_local(f, wb);
            if (b[0] - a[0]).abs() < (b[1] - a[1]).abs() {
                continue;
            }
            let y = (a[1] + b[1]) / 2.0;
            for (d, side) in [((y - bb.y0).abs(), Side::S), ((y - bb.y1).abs(), Side::N)] {
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, side));
                }
            }
        }
        best.map(|(_, s)| s)
    };
    let mut out: Vec<WPlan> = Vec::new();
    let whole = best_field(ctx, &FieldSpec { region: &g, ext: Ext::default(), street: street_side(&frame, &g) }, s);
    if let Some(whole) = whole {
        out.push(place(ctx, &whole, &frame, "A", deg));
    }
    let d = s.drive_aisle;
    let mut tried: Vec<String> = Vec::new();
    let outer_corners: Vec<String> =
        reflex_corners(&g.outer, &[]).iter().map(|p| format!("{},{}", to_fixed(p[0], 3), to_fixed(p[1], 3))).collect();
    for c in reflex_corners(&g.outer, &g.holes) {
        let wing = outer_corners.contains(&format!("{},{}", to_fixed(c[0], 3), to_fixed(c[1], 3)));
        for horizontal in [true, false] {
            for shift in [-d, 0.0] {
                let at = (if horizontal { c[1] } else { c[0] }) + shift;
                let key = format!("{}{}", if horizontal { "h" } else { "v" }, to_fixed(at, 1));
                if tried.contains(&key) {
                    continue;
                }
                tried.push(key);
                let lo = move |p: Pt| if horizontal { p[1] } else { p[0] };
                let below = clip_half(&g, &|p| at - lo(p));
                let above = clip_half(&g, &|p| lo(p) - (at + d));
                let (Some(below), Some(above)) = (below, above) else { continue };
                let spans = if horizontal { strip_spans(&g, at, at + d) } else { column_spans(&g, at, at + d) };
                let drive_rects: Vec<Rect> = spans
                    .iter()
                    .filter(|[p, q]| q - p > d)
                    .map(|&[p, q]| if horizontal { rect(p, at, q, at + d) } else { rect(at, p, at + d, q) })
                    .collect();
                if drive_rects.is_empty() {
                    continue;
                }
                let drives_l: Vec<LAisle> =
                    drive_rects.iter().map(|r| LAisle { id: ctx.id(), r: *r, kind: AisleKind::Drive, flow: None }).collect();
                let drives: Vec<WAisle> = drives_l
                    .iter()
                    .map(|a| WAisle {
                        id: a.id,
                        poly: corners(&a.r).into_iter().map(|p| to_world(&frame, p)).collect(),
                        kind: AisleKind::Drive,
                        flow: None,
                    })
                    .collect();
                let part_of = |ctx: &mut Ctx, region: &Region, ext: Ext, tag: &str| -> Option<WPlan> {
                    let aligned = best_field(ctx, &FieldSpec { region, ext, street: street_side(&frame, region) }, s);
                    let mut best = aligned.map(|a| {
                        let resolved = resolve_ext(&a, &drives_l, &|_| true);
                        place(ctx, &resolved, &frame, tag, deg)
                    });
                    if own_directions && wing {
                        let world = region_to_world(&frame, region);
                        for dd in directions(&world.outer, 4, 15.0) {
                            if angle_off(dd, deg) < 1.0 || (angle_off(dd, deg) - 90.0).abs() < 1.0 {
                                continue;
                            }
                            let f2 = frame_of(origin, dd);
                            let r2 = region_to_local(&f2, &world);
                            let Some(p2) =
                                best_field(ctx, &FieldSpec { region: &r2, ext: Ext::default(), street: street_side(&f2, &r2) }, s)
                            else {
                                continue;
                            };
                            let w2 = place(ctx, &p2, &f2, tag, dd);
                            if !w2.aisles.iter().any(|a| drives.iter().any(|dr| touch_along(&a.poly, &dr.poly, 2.0))) {
                                continue;
                            }
                            if best.as_ref().is_none_or(|b| score_of(&w2) > score_of(b)) {
                                best = Some(w2);
                            }
                        }
                    }
                    best
                };
                let pa = part_of(
                    ctx,
                    &below,
                    if horizontal { Ext { n: true, ..Ext::default() } } else { Ext { e: true, ..Ext::default() } },
                    "B",
                );
                let pb = part_of(
                    ctx,
                    &above,
                    if horizontal { Ext { s: true, ..Ext::default() } } else { Ext { w: true, ..Ext::default() } },
                    "C",
                );
                if pa.is_none() && pb.is_none() {
                    continue;
                }
                out.push(join(pa.into_iter().chain(pb).collect(), drives));
            }
        }
    }
    out
}

/// A plot with a wing at an angle: cut along the side of the wing at each inside corner, a drive aisle on the cut,
/// and each part set out in its own direction.
pub fn lay_oblique(ctx: &mut Ctx, usable: &Region, s: &Settings, origin: Pt) -> Vec<WPlan> {
    let mut out = Vec::new();
    let d = s.drive_aisle;
    let outer = ccw(&usable.outer);
    let n = outer.len();
    if n < 3 {
        return out;
    }
    let reach = outer.iter().map(|&p| dist(p, outer[0])).fold(f64::NEG_INFINITY, f64::max) * 2.0 + 10.0;
    let mut tried: Vec<String> = Vec::new();
    let inside: Vec<usize> = (0..n)
        .filter(|&i| {
            let (a, c, b) = (outer[(i + n - 1) % n], outer[i], outer[(i + 1) % n]);
            (c[0] - a[0]) * (b[1] - c[1]) - (c[1] - a[1]) * (b[0] - c[0]) < -1e-6
        })
        .collect();
    let mut cuts: Vec<(Pt, Pt)> = Vec::new();
    for &i in &inside {
        cuts.push((outer[(i + n - 1) % n], outer[i]));
        cuts.push((outer[i], outer[(i + 1) % n]));
    }
    for i in 0..inside.len() {
        for j in i + 1..inside.len() {
            cuts.push((outer[inside[i]], outer[inside[j]]));
        }
    }
    for (p, q) in cuts {
        let c = p;
        let l = dist(p, q);
        if l < 10.0 {
            continue;
        }
        let u = [(q[0] - p[0]) / l, (q[1] - p[1]) / l];
        let deg = (u[1].atan2(u[0]) * 180.0 / std::f64::consts::PI + 360.0) % 180.0;
        if [0.0, 90.0].iter().any(|k| {
            let m = ((deg - k) % 90.0 + 90.0) % 90.0;
            m.abs() < 2.0 || (m - 90.0).abs() < 2.0
        }) {
            continue;
        }
        let side = move |x: Pt| u[0] * (x[1] - c[1]) - u[1] * (x[0] - c[0]);
        for (lo, hi) in [(0.0, d), (-d, 0.0)] {
            let key = format!("{}|{}", to_fixed(deg, 1), to_fixed(side([0.0, 0.0]) - lo, 1));
            if tried.contains(&key) {
                continue;
            }
            tried.push(key);
            let nn = [-u[1], u[0]];
            let p0 = [c[0] - u[0] * reach + nn[0] * lo, c[1] - u[1] * reach + nn[1] * lo];
            let p1 = [c[0] + u[0] * reach + nn[0] * lo, c[1] + u[1] * reach + nn[1] * lo];
            let strip = vec![
                p0,
                p1,
                [p1[0] + nn[0] * (hi - lo), p1[1] + nn[1] * (hi - lo)],
                [p0[0] + nn[0] * (hi - lo), p0[1] + nn[1] * (hi - lo)],
            ];
            let drive_poly = clip_convex(&outer, &strip);
            if drive_poly.len() < 3 || ring_area(&drive_poly).abs() < d * 10.0 {
                continue;
            }
            if usable.holes.iter().any(|h| shared_area(&drive_poly, h) > 1.0) {
                continue;
            }
            let drive = WAisle { id: ctx.id(), poly: drive_poly, kind: AisleKind::Drive, flow: None };
            let parts = [clip_half(usable, &|x| side(x) - hi), clip_half(usable, &|x| lo - side(x))];
            let mut laid: Vec<WPlan> = Vec::new();
            for (k, part) in parts.iter().enumerate() {
                let Some(part) = part else { continue };
                let mut best: Option<WPlan> = None;
                let mut dirs = vec![deg, (deg + 90.0) % 180.0];
                dirs.extend(directions(&part.outer, 4, 15.0));
                for dd in dirs {
                    let f = frame_of(origin, dd);
                    let r = region_to_local(&f, part);
                    let dl: Ring = drive.poly.iter().map(|&x| to_local(&f, x)).collect();
                    let db = bounds(&dl);
                    let pb = bounds(&r.outer);
                    let along = ((dd - deg + 180.0) % 180.0).abs() < 0.5;
                    let across = ((dd - deg + 270.0) % 180.0).abs() < 0.5;
                    let ext = Ext {
                        s: along && db.y1 <= pb.y0 + 0.5,
                        n: along && db.y0 >= pb.y1 - 0.5,
                        w: across && db.x1 <= pb.x0 + 0.5,
                        e: across && db.x0 >= pb.x1 - 0.5,
                    };
                    let Some(raw) = best_field(ctx, &FieldSpec { region: &r, ext, street: None }, s) else { continue };
                    let plan_w = if ext.s || ext.n || ext.w || ext.e {
                        let dr = [LAisle { id: drive.id, r: rect(db.x0, db.y0, db.x1, db.y1), kind: AisleKind::Drive, flow: None }];
                        let resolved = resolve_ext(&raw, &dr, &|m| point_in_ring(m, &dl));
                        place(ctx, &resolved, &f, if k > 0 { "E" } else { "D" }, dd)
                    } else {
                        place(ctx, &raw, &f, if k > 0 { "E" } else { "D" }, dd)
                    };
                    let Some(w) = connect(ctx, plan_w, &drive.poly) else { continue };
                    if best.as_ref().is_none_or(|b| score_of(&w) > score_of(b)) {
                        best = Some(w);
                    }
                }
                if let Some(b) = best {
                    laid.push(b);
                }
            }
            if !laid.is_empty() {
                out.push(join(laid, vec![drive]));
            }
        }
    }
    out
}

/// The two short sides of a piece (shortest first) as indices into its ring.
pub fn short_sides(c: &[Pt]) -> Vec<(Pt, Pt)> {
    let n = c.len();
    let mut sides: Vec<(Pt, Pt)> = (0..n).map(|i| (c[i], c[(i + 1) % n])).collect();
    sides.sort_by(|x, y| dist(x.0, x.1).partial_cmp(&dist(y.0, y.1)).unwrap_or(std::cmp::Ordering::Equal));
    sides.truncate(2);
    sides
}

/// The field joined to a drive aisle: as it is when an aisle meets it, else by the shortest straight run on from the
/// end of an aisle (12 m at most), through the bays in the way.
fn connect(ctx: &mut Ctx, mut w: WPlan, drive: &[Pt]) -> Option<WPlan> {
    if w.aisles.iter().any(|x| touch_along(&x.poly, drive, 2.0)) {
        return Some(w);
    }
    let mut best: Option<(Ring, f64)> = None;
    for al in &w.aisles {
        let c = ccw(&al.poly);
        let sides = short_sides(&c);
        if sides.len() < 2 {
            continue;
        }
        for (j, &(p, q)) in sides.iter().enumerate() {
            let end = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
            let other = sides[1 - j];
            let back = [(other.0[0] + other.1[0]) / 2.0, (other.0[1] + other.1[1]) / 2.0];
            let l = or1(dist(end, back));
            let dir = [(end[0] - back[0]) / l, (end[1] - back[1]) / l];
            let Some(t) = ray_into(end, dir, drive) else { continue };
            if t > 12.0 {
                continue;
            }
            if best.as_ref().is_none_or(|(_, bt)| t < *bt) {
                best = Some((band(end, [end[0] + dir[0] * (t + 1.0), end[1] + dir[1] * (t + 1.0)], dist(p, q)), t));
            }
        }
    }
    let (link, _) = best?;
    w.stalls.retain(|st| !hits(&link, &st.poly));
    w.islands.retain(|i| !hits(&link, &i.poly));
    w.aisles.push(WAisle { id: ctx.id(), poly: link, kind: AisleKind::Drive, flow: None });
    Some(w)
}

/// A field's rows that face a drive aisle open onto the one they touch.
fn resolve_ext(plan: &Plan, drives: &[LAisle], on: &dyn Fn(Pt) -> bool) -> Plan {
    let mut out = plan.clone();
    for st in &mut out.stalls {
        if st.aisle >= 0 {
            continue;
        }
        let m = mouth_of(&st.r, st.side);
        let o = match st.side {
            Side::N => [m[0], m[1] + 0.3],
            Side::S => [m[0], m[1] - 0.3],
            Side::E => [m[0] + 0.3, m[1]],
            Side::W => [m[0] - 0.3, m[1]],
        };
        st.aisle = match drives.iter().find(|x| near(&x.r, m, 0.05) && on(o)) {
            Some(d) => d.id + EXT_BASE,
            None => -1,
        };
    }
    out
}
