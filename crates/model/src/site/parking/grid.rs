//! Modules on the grid: a car park laid out as a designer lays one (docs/parking-ramps-plan.md, "Solver rework").
//!
//! In a direction, the area is cut across into a stack of modules: an aisle with a row of bays on one side or both, found
//! by dynamic programming over the area's depth. A row's bays are set out in the spans its whole depth is clear (so they
//! fall between columns), an aisle where it is clear along its length (between the column lines). The stack's aisles are
//! then joined by cross aisles at right angles, chosen greedily from what a ramp's foot reaches: the one that brings in the
//! most bays for the fewest it costs. Aisles run only as far as what they serve; what no car reaches is left out.

use super::fields::*;
use super::geom::*;
use super::standards::Settings;

/// How finely the stack's depth is searched, m.
const STEP: f64 = 0.1;
const QUICK_STEP: f64 = 0.25;
/// What an aisle in line with a ramp's foot is worth to the stack, in bays: it is how cars come in.
const FOOT_BONUS: f64 = 40.0;
/// What each piece an aisle is cut into past the first costs the stack, in bays: it must be joined by a cross aisle.
const PIECE_COST: f64 = 2.0;
/// The farthest a way off a ramp's foot runs to its aisle, m.
const FOOT_REACH: f64 = 60.0;
/// A hole no wider or deeper than this (m) is a column.
const POST: f64 = 2.0;
/// How far an aisle may fall short of a ramp's foot across it and still take its cars, m (the foot's kerbs, a wall's face).
const FOOT_SLACK: f64 = 0.5;

/// Whether an aisle from `y0` to `y1` takes the cars off a foot along it (`lo` to `hi` across it).
fn in_line(lo: f64, hi: f64, y0: f64, y1: f64) -> bool {
    y0 <= lo + FOOT_SLACK + 1e-6 && y1 >= hi - FOOT_SLACK - 1e-6
}

/// A ramp's foot in the frame: along the aisles (`along`, a car comes off it along x) or across them.
#[derive(Clone, Copy, Debug)]
struct Foot {
    along: bool,
    /// Across the foot: its y span (along) or x span (across).
    lo: f64,
    hi: f64,
    /// Where it is: its x (along) or y (across).
    at: f64,
    /// The way a car comes off it: +1 or -1, along x (along) or y (across).
    dir: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    /// A row below the aisle and one above.
    Both,
    Below,
    Above,
    /// An aisle alone (only to meet a ramp's foot).
    Alone,
}

const VARIANTS: [Variant; 4] = [Variant::Both, Variant::Below, Variant::Above, Variant::Alone];

/// A piece of an aisle of the stack.
#[derive(Clone, Debug)]
struct Seg {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

/// A bay of the stack: its box, the aisle piece it opens onto, whether it opens up (its mouth at its top).
#[derive(Clone, Debug)]
struct Bay {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    seg: usize,
    up: bool,
    row: String,
    cell: i64,
    alive: bool,
}

/// A cross aisle joining two aisle pieces, or the way off a ramp's foot into one.
#[derive(Clone, Debug)]
struct Link {
    r: Rect,
    a: usize,
    /// The piece it joins `a` to; None for a ramp's way (`a` is reached from the foot).
    b: Option<usize>,
    /// Its bays given up.
    cost: Vec<usize>,
}

/// What a module is worth and its aisle's pieces.
struct Module {
    value: f64,
    len: f64,
}

/// The candidate directions: those given, a column grid's axes and each ramp's way off.
fn dirs_of(part: &Region, degs: &[f64], feet: &[((Pt, Pt), Pt)]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    let mut add = |d: f64| {
        let d = ((d % 180.0) + 180.0) % 180.0;
        if out.iter().all(|&o| (d - o).abs().min(180.0 - (d - o).abs()) > 1.0) {
            out.push(d);
        }
    };
    for &d in degs {
        add(d);
    }
    for h in part.holes.iter().filter(|h| h.len() >= 3) {
        let q = bounds(h);
        if q.x1 - q.x0 <= POST && q.y1 - q.y0 <= POST {
            let d = side_deg(h[0], h[1]);
            add(d);
            add(d + 90.0);
            break;
        }
    }
    for (_, o) in feet {
        let d = o[1].atan2(o[0]).to_degrees();
        add(d);
        add(d + 90.0);
    }
    out
}

/// The x starts of the bays `w` wide in span `i`: from its start, or on the lattice of `n` bays then a column `c` wide
/// from `x0` (a proposed grid).
fn cells(i: Span, w: f64, n: usize, c: f64, x0: f64) -> Vec<f64> {
    let mut out = Vec::new();
    if i[1] - i[0] < w - 1e-6 {
        return out;
    }
    if n == 0 {
        let k = ((i[1] - i[0] + 1e-6) / w).floor() as usize;
        for j in 0..k {
            out.push(i[0] + j as f64 * w);
        }
        return out;
    }
    let p = n as f64 * w + c;
    let mut g = ((i[0] - x0) / p).floor() as i64 - 1;
    loop {
        let base = x0 + g as f64 * p + c;
        if base > i[1] {
            break;
        }
        for j in 0..n {
            let x = base + j as f64 * w;
            if x >= i[0] - 1e-6 && x + w <= i[1] + 1e-6 {
                out.push(x);
            }
        }
        g += 1;
    }
    out
}

fn meet(a: Span, b: Span) -> Option<Span> {
    let lo = a[0].max(b[0]);
    let hi = a[1].min(b[1]);
    (hi > lo + 1e-9).then_some([lo, hi])
}

/// The stack in one frame.
struct Stack<'a> {
    g: &'a Region,
    s: &'a Settings,
    w: f64,
    d: f64,
    a: f64,
    n: usize,
    c: f64,
    x0: f64,
    feet: &'a [Foot],
}

impl Stack<'_> {
    /// The aisle's pieces at `ya`: clear spans twice as long as an aisle is wide, or longer (a shorter piece, between two
    /// columns, is no aisle a designer would lay).
    fn segs(&self, ya: f64) -> Vec<Span> {
        strip_spans(self.g, ya, ya + self.a).into_iter().filter(|s| s[1] - s[0] >= 2.0 * self.a - 1e-6).collect()
    }

    /// The bays a row from `y0` to `y1` gets onto `segs`: (piece, x start) each.
    fn row(&self, y0: f64, y1: f64, segs: &[Span]) -> Vec<(usize, f64)> {
        let mut out = Vec::new();
        let spans = strip_spans(self.g, y0, y1);
        for (k, sg) in segs.iter().enumerate() {
            for sp in &spans {
                if let Some(i) = meet(*sp, *sg) {
                    out.extend(cells(i, self.w, self.n, self.c, self.x0).into_iter().map(|x| (k, x)));
                }
            }
        }
        out
    }

    fn module(&self, y: f64, v: Variant, bonus: bool) -> Option<Module> {
        let (below, above) = match v {
            Variant::Both => (true, true),
            Variant::Below => (true, false),
            Variant::Above => (false, true),
            Variant::Alone => (false, false),
        };
        let ya = if below { y + self.d } else { y };
        let segs = self.segs(ya);
        if segs.is_empty() {
            return None;
        }
        let mut used = vec![false; segs.len()];
        let mut bays = 0.0;
        if below {
            for (k, _) in self.row(ya - self.d, ya, &segs) {
                used[k] = true;
                bays += 1.0;
            }
        }
        if above {
            for (k, _) in self.row(ya + self.a, ya + self.a + self.d, &segs) {
                used[k] = true;
                bays += 1.0;
            }
        }
        let mut gain = 0.0;
        if bonus {
            for f in self.feet.iter().filter(|f| f.along) {
                if in_line(f.lo, f.hi, ya, ya + self.a) && segs.iter().any(|sg| sg[0] <= f.at + 1.0 && sg[1] >= f.at - 1.0) {
                    gain += FOOT_BONUS;
                }
            }
        }
        if (v == Variant::Alone && gain == 0.0) || (bays == 0.0 && gain == 0.0) {
            return None;
        }
        let pieces = used.iter().filter(|u| **u).count();
        let paved: f64 = segs.iter().zip(&used).filter(|(_, u)| **u || gain > 0.0).map(|(sg, _)| (sg[1] - sg[0]) * self.a).sum();
        let len = self.a + if below { self.d } else { 0.0 } + if above { self.d } else { 0.0 };
        Some(Module { value: bays - paved / PAVED_PER_BAY - pieces.saturating_sub(1) as f64 * PIECE_COST + gain, len })
    }

    /// The best stack from `y0` up, on the lattice `anchor + k·step`: its modules (start, variant).
    fn best(&self, y0: f64, y1: f64, anchor: f64, step: f64, bonus: bool) -> Vec<(f64, Variant)> {
        let k0 = ((y0 - anchor) / step - 1e-6).ceil() as i64;
        let k1 = ((y1 - anchor) / step + 1e-6).floor() as i64;
        if k1 <= k0 {
            return Vec::new();
        }
        let n = (k1 - k0 + 1) as usize;
        let at = |i: usize| anchor + (k0 + i as i64) as f64 * step;
        // f[i]: the best from position i up; how[i]: the module taken there (or none: a step of gap).
        let mut f = vec![0.0f64; n + 1];
        let mut how: Vec<Option<(Variant, usize)>> = vec![None; n + 1];
        for i in (0..n).rev() {
            f[i] = f[i + 1];
            let y = at(i);
            for v in VARIANTS {
                let Some(m) = self.module(y, v, bonus) else { continue };
                if y + m.len > y1 + 1e-6 {
                    continue;
                }
                let next = (((y + m.len - anchor) / step - 1e-6).ceil() as i64 - k0).max(i as i64 + 1) as usize;
                let next = next.min(n);
                let total = m.value + f[next];
                if total > f[i] + 1e-9 {
                    f[i] = total;
                    how[i] = Some((v, next));
                }
            }
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < n {
            match how[i] {
                Some((v, next)) => {
                    out.push((at(i), v));
                    i = next;
                }
                None => i += 1,
            }
        }
        out
    }
}

/// Union-find.
fn find(p: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while p[r] != r {
        r = p[r];
    }
    let mut j = i;
    while p[j] != r {
        let k = p[j];
        p[j] = r;
        j = k;
    }
    r
}

fn overlaps(r: &Rect, b: &Bay) -> bool {
    r.x0.max(b.x0) < r.x1.min(b.x1) - 1e-6 && r.y0.max(b.y0) < r.y1.min(b.y1) - 1e-6
}

/// The car park laid out as modules on the grid, in each direction tried: plans for the solver to pick from. 90° bays
/// only; `feet` are the ramps' ends on the level (the line across each, the way a car comes off it).
pub fn lay_grid(ctx: &mut Ctx, part: &Region, s: &Settings, origin: Pt, degs: &[f64], feet: &[((Pt, Pt), Pt)], quick: bool) -> Vec<WPlan> {
    if s.angle < 89.9 || part.outer.len() < 3 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut dirs = dirs_of(part, degs, feet);
    if quick {
        dirs.truncate(2);
    }
    let step = if quick { QUICK_STEP } else { STEP };
    for deg in dirs {
        let frame = frame_of(origin, deg);
        let g = region_to_local(&frame, part);
        let b = bounds(&g.outer);
        if !(b.x1 - b.x0).is_finite() || b.y1 - b.y0 < s.aisle {
            continue;
        }
        let local_feet: Vec<Foot> = feet
            .iter()
            .filter_map(|((p, q), o)| {
                let (p, q) = (to_local(&frame, *p), to_local(&frame, *q));
                let o = [o[0] * frame.c + o[1] * frame.s, -o[0] * frame.s + o[1] * frame.c];
                if o[0].abs() > 0.9 {
                    Some(Foot { along: true, lo: p[1].min(q[1]), hi: p[1].max(q[1]), at: (p[0] + q[0]) / 2.0, dir: o[0].signum() })
                } else if o[1].abs() > 0.9 {
                    Some(Foot { along: false, lo: p[0].min(q[0]), hi: p[0].max(q[0]), at: (p[1] + q[1]) / 2.0, dir: o[1].signum() })
                } else {
                    None
                }
            })
            .collect();
        let n = if s.column_every > 0.0 { s.column_every.round().max(1.0) as usize } else { 0 };
        let st = Stack { g: &g, s, w: s.stall_width, d: s.stall_depth, a: s.aisle, n, c: s.column_size, x0: b.x0, feet: &local_feet };
        // The stack's phase: the area's edge, and each foot's aisle (flush with it either side, or on its middle).
        let mut runs: Vec<(f64, bool)> = vec![(b.y0, false)];
        for f in local_feet.iter().filter(|f| f.along) {
            for ya in [f.lo, f.hi - st.a, (f.lo + f.hi - st.a) / 2.0] {
                let ya = ya.clamp(b.y0, b.y1 - st.a);
                if runs.iter().all(|r| (r.0 - ya).abs() > 1e-3 || !r.1) {
                    runs.push((ya, true));
                }
            }
        }
        if quick {
            runs.truncate(2);
        }
        let mut seen: Vec<Vec<(i64, Variant)>> = Vec::new();
        for (anchor, bonus) in runs {
            let stack = st.best(b.y0, b.y1, anchor, step, bonus);
            if stack.is_empty() {
                continue;
            }
            let key: Vec<(i64, Variant)> = stack.iter().map(|(y, v)| ((y * 1000.0).round() as i64, *v)).collect();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            if std::env::var_os("PARKING_DEBUG").is_some() {
                eprintln!("grid {deg} anchor {anchor:.2} bonus {bonus}: {:?}", stack.iter().map(|(y, v)| format!("{y:.2}{v:?}")).collect::<Vec<_>>());
            }
            if let Some((plan, ups)) = build(&st, &stack, &local_feet) {
                if std::env::var_os("PARKING_DEBUG").is_some() {
                    eprintln!("   -> {} bays, {} aisles", plan.stalls.len(), plan.aisles.len());
                }
                let mut w = place(ctx, &plan, &frame, "G", deg);
                for (a, up) in w.aisles.iter_mut().zip(ups) {
                    if let Some(f) = up {
                        a.flow = Some([-f * frame.s, f * frame.c]);
                    }
                }
                out.push(w);
            }
        }
    }
    out
}

/// The stack's plan: its aisles' pieces and bays, joined by cross aisles, trimmed to what they serve.
fn build(st: &Stack, stack: &[(f64, Variant)], feet: &[Foot]) -> Option<(Plan, Vec<Option<f64>>)> {
    let s = st.s;
    let mut segs: Vec<Seg> = Vec::new();
    let mut bays: Vec<Bay> = Vec::new();
    for (mi, &(y, v)) in stack.iter().enumerate() {
        let below = matches!(v, Variant::Both | Variant::Below);
        let above = matches!(v, Variant::Both | Variant::Above);
        let ya = if below { y + st.d } else { y };
        let pieces = st.segs(ya);
        let first = segs.len();
        segs.extend(pieces.iter().map(|p| Seg { x0: p[0], x1: p[1], y0: ya, y1: ya + st.a }));
        for (up, on, y0, y1) in [(true, below, ya - st.d, ya), (false, above, ya + st.a, ya + st.a + st.d)] {
            if !on {
                continue;
            }
            let row = format!("{mi}{}", if up { "b" } else { "t" });
            let mut cell = 0i64;
            let mut last = f64::NEG_INFINITY;
            let mut laid = st.row(y0, y1, &pieces);
            laid.sort_by(|p, q| p.1.total_cmp(&q.1));
            for (k, x) in laid {
                // Bays side by side are numbered on; a column or a gap between them breaks the run.
                cell += if (x - last - st.w).abs() < 1e-6 { 1 } else { 10 };
                last = x;
                bays.push(Bay { x0: x, x1: x + st.w, y0, y1, seg: first + k, up, row: row.clone(), cell, alive: true });
            }
        }
    }
    if bays.is_empty() || segs.is_empty() {
        return None;
    }
    let ns = segs.len();
    let mut parent: Vec<usize> = (0..ns).collect();
    let mut reached = vec![false; ns];
    let mut links: Vec<Link> = Vec::new();
    let cost_of = |r: &Rect, bays: &[Bay]| -> Vec<usize> { (0..bays.len()).filter(|&i| bays[i].alive && overlaps(r, &bays[i])).collect() };
    // A ramp's foot: an aisle in line with it, or a way straight off it into the first aisle it meets.
    for f in feet {
        if f.along {
            let mut best: Option<(f64, usize)> = None;
            for (k, sg) in segs.iter().enumerate() {
                if !in_line(f.lo, f.hi, sg.y0, sg.y1) {
                    continue;
                }
                let gap = if f.dir > 0.0 { sg.x0 - f.at } else { f.at - sg.x1 };
                if gap < -1.0 || gap > FOOT_REACH {
                    continue;
                }
                let (lo, hi) = (f.lo.max(sg.y0), f.hi.min(sg.y1));
                // Between the foot and the aisle's end the way is clear (a sliver the ramp's cut-out leaves is no matter).
                if gap > FOOT_SLACK && !box_inside(st.g, f.at.min(f.at + f.dir * gap), lo + 0.1, f.at.max(f.at + f.dir * gap), hi - 0.1) {
                    continue;
                }
                if best.is_none_or(|(g, _)| gap < g) {
                    best = Some((gap, k));
                }
            }
            if let Some((_, k)) = best {
                reached[k] = true;
                let r = if f.dir > 0.0 { rect(f.at, segs[k].y0, segs[k].x0.max(f.at), segs[k].y1) } else { rect(segs[k].x1.min(f.at), segs[k].y0, f.at, segs[k].y1) };
                links.push(Link { r, a: k, b: None, cost: Vec::new() });
            }
        } else {
            let mid = (f.lo + f.hi) / 2.0;
            let half = (f.hi - f.lo).max(s.one_way_entry) / 2.0;
            let mut best: Option<(f64, usize)> = None;
            for (k, sg) in segs.iter().enumerate() {
                if sg.x0 > mid - half + FOOT_SLACK || sg.x1 < mid + half - FOOT_SLACK {
                    continue;
                }
                let gap = if f.dir > 0.0 { sg.y0 - f.at } else { f.at - sg.y1 };
                if gap < -1.0 || gap > FOOT_REACH {
                    continue;
                }
                if best.is_none_or(|(g, _)| gap < g) {
                    best = Some((gap, k));
                }
            }
            if let Some((gap, k)) = best {
                let r = if f.dir > 0.0 { rect(mid - half, f.at, mid + half, f.at + gap.max(0.0)) } else { rect(mid - half, f.at - gap.max(0.0), mid + half, f.at) };
                // Clear from just off the foot (the ramp's cut-out may leave a sliver there) to the aisle.
                let (y0, y1) = if f.dir > 0.0 { (r.y0 + FOOT_SLACK, r.y1) } else { (r.y0, r.y1 - FOOT_SLACK) };
                if gap <= FOOT_SLACK || box_inside(st.g, r.x0 + 0.1, y0, r.x1 - 0.1, y1) {
                    let cost = cost_of(&r, &bays);
                    for &i in &cost {
                        bays[i].alive = false;
                    }
                    reached[k] = true;
                    links.push(Link { r, a: k, b: None, cost });
                }
            }
        }
    }
    let ramps = !feet.is_empty();
    if std::env::var_os("PARKING_DEBUG").is_some() {
        eprintln!("   feet {feet:?} segs {:?} reached {reached:?}", segs.iter().map(|s| format!("{:.1}-{:.1}x{:.1}-{:.1}", s.x0, s.x1, s.y0, s.y1)).collect::<Vec<_>>());
    }
    if !reached.iter().any(|r| *r) {
        if ramps {
            // No foot meets this stack: another candidate will.
            return None;
        }
        // Above ground the ways in are found later: the aisle piece with the most bays is where it starts.
        let mut count = vec![0usize; ns];
        for b in &bays {
            count[b.seg] += 1;
        }
        let top = (0..ns).max_by_key(|&k| count[k]).unwrap_or(0);
        reached[top] = true;
    }
    // Cross aisles: between two pieces of the stack one above the other, at a bay's side, a piece's end or a column's side.
    let cw = s.aisle;
    let mut xs: Vec<f64> = Vec::new();
    for sg in &segs {
        xs.push(sg.x0);
        xs.push(sg.x1 - cw);
    }
    for b in &bays {
        xs.push(b.x0);
        xs.push(b.x1 - cw);
        xs.push(b.x1);
        xs.push(b.x0 - cw);
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|p, q| (*p - *q).abs() < 0.01);
    let mut options: Vec<Link> = Vec::new();
    for &x in &xs {
        let mut over: Vec<usize> = (0..ns).filter(|&k| segs[k].x0 <= x + 1e-6 && segs[k].x1 >= x + cw - 1e-6).collect();
        over.sort_by(|p, q| segs[*p].y0.total_cmp(&segs[*q].y0));
        for w in over.windows(2) {
            let (lo, hi) = (w[0], w[1]);
            let r = rect(x, segs[lo].y1, x + cw, segs[hi].y0);
            if r.y1 - r.y0 > 1e-6 && !box_inside(st.g, r.x0, r.y0, r.x1, r.y1) {
                continue;
            }
            let cost = cost_of(&r, &bays);
            options.push(Link { r, a: lo, b: Some(hi), cost });
        }
    }
    // Greedily: the cross aisle bringing in the most bays for the fewest it costs, until none gains.
    for k in 0..ns {
        if reached[k] {
            let r = find(&mut parent, k);
            reached[r] = true;
        }
    }
    let mut used = vec![false; options.len()];
    loop {
        let mut alive_in = vec![0usize; ns];
        for b in bays.iter().filter(|b| b.alive) {
            let r = find(&mut parent, b.seg);
            alive_in[r] += 1;
        }
        let mut best: Option<(f64, usize)> = None;
        for (i, o) in options.iter().enumerate() {
            if used[i] {
                continue;
            }
            let Some(ob) = o.b else { continue };
            let (ra, rb) = (find(&mut parent, o.a), find(&mut parent, ob));
            if ra == rb || reached[ra] == reached[rb] {
                continue;
            }
            let new = if reached[ra] { rb } else { ra };
            let lost = o.cost.iter().filter(|&&j| bays[j].alive).count() as f64;
            let gain = alive_in[new] as f64 - lost - (o.r.x1 - o.r.x0) * (o.r.y1 - o.r.y0) / PAVED_PER_BAY;
            if best.is_none_or(|(g, _)| gain > g) {
                best = Some((gain, i));
            }
        }
        let Some((gain, i)) = best else { break };
        if gain <= 0.0 {
            break;
        }
        used[i] = true;
        for &j in &options[i].cost {
            bays[j].alive = false;
        }
        let (ra, rb) = (find(&mut parent, options[i].a), find(&mut parent, options[i].b.unwrap_or(options[i].a)));
        parent[ra] = rb;
        reached[rb] = true;
    }
    let mut on = vec![false; ns];
    for k in 0..ns {
        let r = find(&mut parent, k);
        on[k] = reached[r];
    }
    // Loops: a cross aisle at an aisle's end that costs no bay and joins two aisles already reached; on a one-way car
    // park, one at each end of every aisle whatever it costs (the traffic must come round).
    let ends = |sg: &Seg, r: &Rect| (r.x0 - sg.x0).abs() < st.w + 0.5 || (sg.x1 - r.x1).abs() < st.w + 0.5;
    if s.one_way {
        for k in (0..ns).filter(|&k| on[k]) {
            for left in [true, false] {
                let near_end = |r: &Rect| if left { (r.x0 - segs[k].x0).abs() < st.w * 3.0 } else { (segs[k].x1 - r.x1).abs() < st.w * 3.0 };
                let has = options.iter().enumerate().any(|(i, o)| used[i] && (o.a == k || o.b == Some(k)) && near_end(&o.r));
                if has {
                    continue;
                }
                let pick = options
                    .iter()
                    .enumerate()
                    .filter(|(i, o)| !used[*i] && (o.a == k || o.b == Some(k)) && o.b.is_some_and(|b| on[b] && on[o.a]) && near_end(&o.r))
                    .min_by_key(|(_, o)| o.cost.iter().filter(|&&j| bays[j].alive).count());
                if let Some((i, _)) = pick {
                    used[i] = true;
                    for &j in &options[i].cost {
                        bays[j].alive = false;
                    }
                }
            }
        }
    } else {
        for i in 0..options.len() {
            let o = &options[i];
            let Some(ob) = o.b else { continue };
            if used[i] || !on[o.a] || !on[ob] || o.cost.iter().any(|&j| bays[j].alive) {
                continue;
            }
            if !(ends(&segs[o.a], &o.r) || ends(&segs[ob], &o.r)) {
                continue;
            }
            // One a place: not beside one already there.
            if options.iter().enumerate().any(|(j, p)| used[j] && p.a == o.a && p.b == o.b && (p.r.x0 - o.r.x0).abs() < cw * 2.0) {
                continue;
            }
            used[i] = true;
        }
    }
    let crosses: Vec<Link> = options.into_iter().zip(used).filter(|(_, u)| *u).map(|(o, _)| o).collect();
    // Each aisle piece runs only as far as what it serves: its bays, its cross aisles, a ramp's way onto it.
    let mut keep = vec![false; ns];
    let mut span: Vec<(f64, f64)> = vec![(f64::INFINITY, f64::NEG_INFINITY); ns];
    let take = |k: usize, lo: f64, hi: f64, span: &mut Vec<(f64, f64)>| {
        span[k].0 = span[k].0.min(lo);
        span[k].1 = span[k].1.max(hi);
    };
    for b in bays.iter().filter(|b| b.alive && on[b.seg]) {
        take(b.seg, b.x0, b.x1, &mut span);
    }
    for l in crosses.iter().chain(links.iter()) {
        take(l.a, l.r.x0, l.r.x1, &mut span);
        if let Some(b) = l.b {
            take(b, l.r.x0, l.r.x1, &mut span);
        }
    }
    for k in 0..ns {
        if on[k] && span[k].1 > span[k].0 {
            keep[k] = true;
            segs[k].x0 = segs[k].x0.max(span[k].0);
            segs[k].x1 = segs[k].x1.min(span[k].1);
        }
    }
    // A one-way car park's traffic: the aisles each the other way to the one beside it, the cross aisles carrying it round.
    let mut flows: Vec<Option<f64>> = vec![None; ns];
    if s.one_way {
        let mut ys: Vec<f64> = (0..ns).filter(|&k| keep[k]).map(|k| segs[k].y0).collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup_by(|p, q| (*p - *q).abs() < 1e-6);
        for k in (0..ns).filter(|&k| keep[k]) {
            let i = ys.iter().position(|y| (y - segs[k].y0).abs() < 1e-6).unwrap_or(0);
            flows[k] = Some(if i % 2 == 0 { 1.0 } else { -1.0 });
        }
    }
    let mut plan = Plan::default();
    for k in (0..ns).filter(|&k| keep[k]) {
        let sg = &segs[k];
        plan.aisles.push(LAisle { id: k as i64, r: rect(sg.x0, sg.y0, sg.x1, sg.y1), kind: AisleKind::Aisle, flow: flows[k] });
    }
    // A cross aisle's traffic, one way: up (+y) from the aisle below when that aisle runs towards it, else down.
    let mut ups: Vec<Option<f64>> = vec![None; plan.aisles.len()];
    for (i, l) in crosses.iter().enumerate() {
        let flow = l.b.and_then(|_| {
            let fa = flows[l.a]?;
            let mid = (l.r.x0 + l.r.x1) / 2.0;
            Some(if fa * (mid - (segs[l.a].x0 + segs[l.a].x1) / 2.0) > 0.0 { 1.0 } else { -1.0 })
        });
        plan.aisles.push(LAisle { id: (ns + i) as i64, r: l.r, kind: AisleKind::Cross, flow: None });
        ups.push(flow);
    }
    for b in bays.iter().filter(|b| b.alive && keep[b.seg]) {
        plan.stalls.push(LStall {
            r: rect(b.x0, b.y0, b.x1, b.y1),
            aisle: b.seg as i64,
            side: if b.up { Side::N } else { Side::S },
            row: b.row.clone(),
            cell: b.cell,
            kind: StallKind::Standard,
            sh: 0.0,
        });
    }
    if st.n > 0 {
        plan.columns = Some(columns(st, &bays, &plan));
    }
    (!plan.stalls.is_empty()).then_some((plan, ups))
}

/// A proposed grid: a column after every `n` bays, on the lattice, at the back of each row (on the line between two rows
/// back to back), clear of every aisle.
fn columns(st: &Stack, bays: &[Bay], plan: &Plan) -> Vec<Rect> {
    let c = st.c;
    let p = st.n as f64 * st.w + c;
    let mut out: Vec<Rect> = Vec::new();
    for b in bays.iter().filter(|b| b.alive) {
        // The lattice's column before this bay, when it is the first of its group, and after it when the last.
        let g = ((b.x0 - st.x0 - c + 1e-6) / p).floor();
        let start = st.x0 + g * p + c;
        let j = ((b.x0 - start) / st.w).round() as i64;
        let mut xs = Vec::new();
        if j == 0 {
            xs.push(b.x0 - c / 2.0);
        }
        if j == st.n as i64 - 1 {
            xs.push(b.x1 + c / 2.0);
        }
        let back = if b.up { b.y0 } else { b.y1 };
        for xc in xs {
            let on_line = rect(xc - c / 2.0, back - c / 2.0, xc + c / 2.0, back + c / 2.0);
            let inside = if b.up { rect(xc - c / 2.0, back, xc + c / 2.0, back + c) } else { rect(xc - c / 2.0, back - c, xc + c / 2.0, back) };
            let r = if box_inside(st.g, on_line.x0, on_line.y0, on_line.x1, on_line.y1) { on_line } else { inside };
            if !box_inside(st.g, r.x0, r.y0, r.x1, r.y1) {
                continue;
            }
            let hits_aisle = plan.aisles.iter().any(|a| r.x0.max(a.r.x0) < r.x1.min(a.r.x1) && r.y0.max(a.r.y0) < r.y1.min(a.r.y1));
            if hits_aisle || out.iter().any(|q| (q.x0 - r.x0).abs() < c && (q.y0 - r.y0).abs() < c) {
                continue;
            }
            out.push(r);
        }
    }
    out
}
