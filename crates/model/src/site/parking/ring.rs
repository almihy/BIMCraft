//! The ring (Arqiflow's solver.ts, "the ring"): the other way a designer fills an odd plot. A row of bays along every
//! side of the plot, square to that side whatever its angle, a loop aisle in front of them following the plot round,
//! and the middle set out in modules whose aisles run on to the loop at both ends.

use super::fields::*;
use super::geom::*;
use super::standards::{Settings, lean_of, pitch_of, row_depth, standard};

/// Each side of a counter-clockwise ring moved `by[i]` inward: its new ends, or None where the side has closed up.
fn inset_sides(ring: &[Pt], by: &[f64]) -> Vec<Option<(Pt, Pt)>> {
    let n = ring.len();
    let none = || vec![None; n];
    if n == 0 || by.len() != n {
        return none();
    }
    struct Line {
        p: Pt,
        t: Pt,
    }
    let lines: Vec<Line> = (0..n)
        .map(|i| {
            let d = by[i];
            let a = ring[i];
            let b = ring[(i + 1) % n];
            let l = or1(dist(a, b));
            let t = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            Line { p: [a[0] - t[1] * d, a[1] + t[0] * d], t }
        })
        .collect();
    let meet = |l1: &Line, l2: &Line| -> Pt {
        let den = l1.t[0] * l2.t[1] - l1.t[1] * l2.t[0];
        if den.abs() < 1e-9 {
            return l2.p;
        }
        let k = ((l2.p[0] - l1.p[0]) * l2.t[1] - (l2.p[1] - l1.p[1]) * l2.t[0]) / den;
        [l1.p[0] + l1.t[0] * k, l1.p[1] + l1.t[1] * k]
    };
    let mut active: Vec<usize> = (0..n).collect();
    for _ in 0..=n {
        let m = active.len();
        if m < 3 {
            return none();
        }
        let starts: Vec<Pt> = (0..m).map(|k| meet(&lines[active[(k + m - 1) % m]], &lines[active[k]])).collect();
        let mut worst: Option<usize> = None;
        let mut least = 0.0;
        for k in 0..m {
            let a = starts[k];
            let b = starts[(k + 1) % m];
            let t = lines[active[k]].t;
            let along = (b[0] - a[0]) * t[0] + (b[1] - a[1]) * t[1];
            let tp = lines[active[(k + m - 1) % m]].t;
            let tn = lines[active[(k + 1) % m]].t;
            if (tp[0] * tn[1] - tp[1] * tn[0]).abs() < 1e-6 {
                continue;
            }
            if along < least {
                least = along;
                worst = Some(k);
            }
        }
        match worst {
            None => {
                let mut out = none();
                for (k, &e) in active.iter().enumerate() {
                    out[e] = Some((starts[k], starts[(k + 1) % m]));
                }
                return out;
            }
            Some(k) => {
                active.remove(k);
            }
        }
    }
    none()
}

/// A piece of a loop round the middle, in the middle's frame: its one-way traffic's direction, if it has one.
#[derive(Clone, Debug)]
pub struct MiddleLoop {
    pub id: i64,
    pub poly: Ring,
    pub flow: Option<Pt>,
}

#[derive(Clone, Copy, PartialEq)]
enum Row {
    Aisle,
    N,
    S,
}

/// The middle of a ring layout, in its own frame: a stack of `n` aisles, `below` and `above` rows outside the
/// outermost aisles (2: a second row backing on to it and facing the loop). Aisles run on to the loop.
#[allow(clippy::too_many_arguments)]
fn lay_middle(
    gg: &Region,
    gr: &Region,
    lp: &[MiddleLoop],
    s: &Settings,
    n: i64,
    below: i64,
    above: i64,
    off: f64,
    shift: f64,
    tally: bool,
) -> Option<Plan> {
    let b = bounds(&gg.outer);
    let r = row_depth(s);
    let single = s.one_way && n == 1;
    let a = if single { s.aisle_two_way } else { s.aisle };
    let w = pitch_of(s);
    let lean = lean_of(s);
    let e = s.end_island;
    const LOOP: i64 = -5;
    let mut strips: Vec<(f64, f64, Row, i64)> = Vec::new();
    let mut y = b.y0 + off;
    let mut push = |kind: Row, h: f64, aisle: i64| {
        strips.push((y, y + h, kind, aisle));
        y += h;
    };
    if n == 0 {
        push(Row::S, r, LOOP);
        push(Row::N, r, LOOP);
    } else {
        if below == 2 {
            push(Row::S, r, LOOP);
        }
        if below >= 1 {
            push(Row::N, r, 0);
        }
        for i in 0..n {
            push(Row::Aisle, a, i);
            if i < n - 1 {
                push(Row::S, r, i);
                push(Row::N, r, i + 1);
            }
        }
        if above >= 1 {
            push(Row::S, r, n - 1);
        }
        if above == 2 {
            push(Row::N, r, LOOP);
        }
    }
    if y > b.y1 + 1e-6 {
        return None;
    }
    let mut plan = Plan::default();
    let mut rects: Vec<(i64, Vec<LAisle>)> = Vec::new();
    for &(y0, y1, kind, aisle) in &strips {
        if kind != Row::Aisle {
            continue;
        }
        let in_g = strip_spans(gg, y0, y1);
        let mut list: Vec<LAisle> = Vec::new();
        for [p, q] in strip_spans(gr, y0, y1) {
            if !in_g.iter().any(|&[gp, gq]| gq > p + 1.0 && gp < q - 1.0) {
                continue;
            }
            let ym = (y0 + y1) / 2.0;
            let on_loop = |x: f64| lp.iter().any(|pc| point_in_ring([x, ym], &pc.poly));
            if !on_loop(p + 0.5) || !on_loop(q - 0.5) {
                continue;
            }
            let flow = if s.one_way && !single { Some(if aisle % 2 == 0 { 1.0 } else { -1.0 }) } else { None };
            let al = LAisle { id: aisle * 100 + list.len() as i64, r: rect(p, y0, q, y1), kind: AisleKind::Aisle, flow };
            list.push(al.clone());
            plan.aisles.push(al);
        }
        match rects.iter_mut().find(|(k, _)| *k == aisle) {
            Some(x) => x.1 = list,
            None => rects.push((aisle, list)),
        }
    }
    let every = if s.island_every > 0.0 { s.island_every } else { 0.0 };
    let gx = b.x0 + shift;
    let cg = if s.column_every > 0.0 && lean == 0.0 { js_round(s.column_every) } else { 0.0 };
    let cs = if cg != 0.0 { s.column_size } else { 0.0 };
    let xk = |k: f64| gx + k * w + if cg != 0.0 { (k / cg).floor() * cs } else { 0.0 };
    let pp = w + if cg != 0.0 { cs / cg } else { 0.0 };
    for &(y0, y1, kind, aisle) in &strips {
        if kind == Row::Aisle {
            continue;
        }
        let own: Vec<LAisle> =
            if aisle >= 0 { rects.iter().find(|(k, _)| *k == aisle).map(|x| x.1.clone()).unwrap_or_default() } else { Vec::new() };
        if aisle >= 0 && own.is_empty() {
            continue;
        }
        let spans = strip_spans(gg, y0, y1);
        let (Some(first), Some(last)) = (spans.first(), spans.last()) else { continue };
        let up = kind == Row::N;
        let fy = if up { y1 + 0.3 } else { y0 - 0.3 };
        let mut sign = 1.0;
        if aisle < 0 {
            let mut m = spans[0];
            for x in &spans[1..] {
                if x[1] - x[0] > m[1] - m[0] {
                    m = *x;
                }
            }
            if let Some(piece) = lp.iter().find(|x| point_in_ring([(m[0] + m[1]) / 2.0, fy], &x.poly))
                && let Some(f) = piece.flow
            {
                sign = if f[0] < 0.0 { -1.0 } else { 1.0 };
            }
        }
        let sh = (if aisle >= 0 { own[0].flow.unwrap_or(1.0) } else { sign }) * lean;
        let k0 = ((first[0] - gx) / pp).floor() - 1.0;
        let k1 = ((last[1] - gx) / pp).ceil() + 1.0;
        if !(k1 - k0 < 1e6) {
            continue;
        }
        let side = if up { Side::N } else { Side::S };
        let tag = to_fixed(y0, 2);
        let mut run: Vec<LStall> = Vec::new();
        let close = |run: &mut Vec<LStall>, plan: &mut Plan| {
            let len = run.len() as f64;
            if tally {
                if len >= s.min_run {
                    plan.count = Some(plan.count.unwrap_or(0.0) + len);
                }
            } else if len >= s.min_run {
                plan.stalls.append(run);
            } else {
                for st in run.iter() {
                    plan.islands.push(LIsland { r: st.r, kind: IslandKind::Leftover, poly: Some(stall_poly(&st.r, st.side, st.sh)) });
                }
            }
            run.clear();
        };
        let mut k = k0;
        while k < k1 {
            let kk = k;
            k += 1.0;
            let x0 = xk(kk);
            let x1 = x0 + w;
            let inside = spans.iter().any(|&[p, q]| p + e <= x0 + 0f64.min(sh) + 1e-4 && q - e >= x1 + 0f64.max(sh) - 1e-4);
            if !inside {
                close(&mut run, &mut plan);
                continue;
            }
            let m1 = [x0 + 0.3, fy];
            let m2 = [x1 - 0.3, fy];
            let mut on: Option<i64> = None;
            if aisle >= 0 {
                if let Some(al) = own.iter().find(|x| near(&x.r, m1, 0.0) && near(&x.r, m2, 0.0)) {
                    on = Some(al.id);
                }
            } else if let Some(piece) = lp.iter().find(|x| point_in_ring(m1, &x.poly) && point_in_ring(m2, &x.poly)) {
                let along = match piece.flow {
                    None => true,
                    Some(f) => lean == 0.0 || (f[0].abs() > 0.5 && f[0].signum() == sign),
                };
                if along {
                    on = Some(piece.id + EXT_BASE);
                }
            }
            let Some(on) = on else {
                close(&mut run, &mut plan);
                continue;
            };
            let cr = rect(x0, y0, x1, y1);
            if every != 0.0 && ((kk % (every + 1.0)) + every + 1.0) % (every + 1.0) == every {
                close(&mut run, &mut plan);
                plan.islands.push(LIsland { r: cr, kind: IslandKind::Row, poly: Some(stall_poly(&cr, side, sh)) });
                continue;
            }
            run.push(LStall { r: cr, aisle: on, side, row: tag.clone(), cell: kk as i64, kind: StallKind::Standard, sh });
        }
        close(&mut run, &mut plan);
    }
    if cg != 0.0 && !tally {
        let rows: Vec<(f64, f64, bool, bool)> = strips.iter().map(|&(y0, y1, k, _)| (y0, y1, k == Row::Aisle, k == Row::N)).collect();
        plan.columns = Some(columns_of_rows(&plan, &rows, &xk, w, cg, cs));
    }
    Some(plan)
}

fn middle_height(n: i64, below: i64, above: i64, s: &Settings) -> f64 {
    let r = row_depth(s);
    if n == 0 {
        return 2.0 * r;
    }
    n as f64 * (if s.one_way && n == 1 { s.aisle_two_way } else { s.aisle }) + (n - 1) as f64 * 2.0 * r + (below + above) as f64 * r
}

#[derive(Clone, Copy)]
struct Try {
    n: i64,
    lo: i64,
    hi: i64,
    off: f64,
    step: f64,
    slack: f64,
    shift: f64,
    score: f64,
}

fn sort_tries(t: &mut [Try]) {
    t.sort_by(|p, q| q.score.partial_cmp(&p.score).unwrap_or(std::cmp::Ordering::Equal));
}

/// The best middles: every stack, its place across the middle and the bays' grid along it tried; the few best.
fn best_middles(gg: &Region, gr: &Region, lp: &[MiddleLoop], s: &Settings, keep: usize, grid: &[f64]) -> Vec<Plan> {
    let b = bounds(&gg.outer);
    let h = b.y1 - b.y0;
    let r = row_depth(s);
    let w = pitch_of(s);
    let score_p = |p: &Plan| {
        let mut paved = 0.0;
        for x in &p.aisles {
            paved += area(&x.r);
        }
        p.count.unwrap_or(0.0) - paved / PAVED_PER_BAY
    };
    let mut tried: Vec<Try> = Vec::new();
    let on_grid = |shift: f64| {
        grid.iter().any(|&g| {
            let d = (((b.x0 + shift - g) % w) + w) % w;
            d < 0.01 || w - d < 0.01
        })
    };
    let lay = |tried: &mut Vec<Try>, t: Try| {
        if let Some(plan) = lay_middle(gg, gr, lp, s, t.n, t.lo, t.hi, t.off, t.shift, true)
            && plan.count.unwrap_or(0.0) != 0.0
        {
            tried.push(Try { score: score_p(&plan) + if on_grid(t.shift) { 0.001 } else { 0.0 }, ..t });
        }
    };
    let n_max = ((h + 2.0 * r) / (s.aisle + 2.0 * r)).floor() + 1.0;
    if !(n_max < 10_000.0) {
        return Vec::new();
    }
    for n in 0..=(n_max as i64) {
        for lo in 0..=2 {
            for hi in 0..=2 {
                if n == 0 && (lo != 0 || hi != 0) {
                    continue;
                }
                let slack = h - middle_height(n, lo, hi, s);
                if slack < -1e-6 {
                    continue;
                }
                let steps = 5f64.min(1f64.max((slack / 3.0).ceil()));
                let mut i = 0.0;
                while i <= steps {
                    lay(&mut tried, Try { n, lo, hi, off: (slack * i) / steps, step: slack / steps, slack, shift: 0.0, score: 0.0 });
                    i += 1.0;
                }
            }
        }
    }
    sort_tries(&mut tried);
    let top: Vec<Try> = tried.iter().take(keep + 2).copied().collect();
    for t in top {
        for d in [-0.5, -0.25, 0.25, 0.5] {
            let off = t.off + d * t.step;
            if off > -1e-9 && off < t.slack + 1e-9 && t.step > 0.2 {
                lay(&mut tried, Try { off, step: t.step / 4.0, ..t });
            }
        }
    }
    sort_tries(&mut tried);
    let mut shifts: Vec<f64> = Vec::new();
    let mut all: Vec<f64> = grid.iter().map(|&g| (((g - b.x0) % w) + w) % w).collect();
    all.extend([w * 0.25, w * 0.5, w * 0.75]);
    for x in all {
        let v = js_round(x * 1000.0) / 1000.0;
        if !shifts.contains(&v) {
            shifts.push(v);
        }
    }
    let top: Vec<Try> = tried.iter().take(keep + 2).copied().collect();
    for t in top {
        for &shift in &shifts {
            lay(&mut tried, Try { shift, ..t });
        }
    }
    sort_tries(&mut tried);
    tried
        .iter()
        .take(keep)
        .filter_map(|t| lay_middle(gg, gr, lp, s, t.n, t.lo, t.hi, t.off, t.shift, false))
        .filter(|p| !p.stalls.is_empty())
        .collect()
}

/// A side a row of bays can face a loop from: the row stands to the left of `v`, opening onto `piece`.
#[derive(Clone)]
struct LoopSide {
    v: (Pt, Pt),
    piece: WAisle,
    tag: String,
}

/// A row of square bays along a side: from `v` leftwards `dep` deep, as many as fit between `lo` and `hi` along it
/// (the grid moved on by `shift`), each kept where `ok` says; runs too short are planted.
#[allow(clippy::too_many_arguments)]
fn row_along(
    v: (Pt, Pt),
    mut lo: f64,
    mut hi: f64,
    dep: f64,
    ew: f64,
    shift: f64,
    min_run: f64,
    ok: &dyn Fn(&[Pt]) -> bool,
    mouth_at: f64,
    aisle: i64,
    tag: &str,
    sh: f64,
) -> (Vec<WStall>, Vec<WStall>) {
    let l = dist(v.0, v.1);
    let mut kept = Vec::new();
    let mut planted = Vec::new();
    if l == 0.0 || l.is_nan() {
        return (kept, planted);
    }
    let t = [(v.1[0] - v.0[0]) / l, (v.1[1] - v.0[1]) / l];
    let nr = [-t[1], t[0]];
    let pt = |x: f64, yy: f64| -> Pt { [v.0[0] + t[0] * x + nr[0] * yy, v.0[1] + t[1] * x + nr[1] * yy] };
    lo += 0f64.max(-sh);
    hi -= 0f64.max(sh);
    let k = ((hi - lo - shift) / ew + 1e-9).floor();
    let start = lo + shift + (hi - lo - shift - k * ew) / 2.0;
    let mut run: Vec<WStall> = Vec::new();
    let close = |run: &mut Vec<WStall>, kept: &mut Vec<WStall>, planted: &mut Vec<WStall>| {
        if run.len() as f64 >= min_run {
            kept.append(run);
        } else {
            planted.append(run);
        }
    };
    if !(k < 1e6) {
        return (kept, planted);
    }
    let mut c = 0.0;
    while c < k {
        let x = start + c * ew;
        let cell = c as i64;
        c += 1.0;
        let poly = if mouth_at == 0.0 {
            vec![pt(x, 0.0), pt(x + ew, 0.0), pt(x + ew + sh, dep), pt(x + sh, dep)]
        } else {
            vec![pt(x + sh, 0.0), pt(x + ew + sh, 0.0), pt(x + ew, dep), pt(x, dep)]
        };
        if !ok(&poly) || run.iter().any(|o| hits(&o.poly, &poly)) {
            close(&mut run, &mut kept, &mut planted);
            continue;
        }
        run.push(WStall {
            poly,
            mouth: pt(x + ew / 2.0, mouth_at),
            aisle,
            row: tag.to_string(),
            cell,
            kind: StallKind::Standard,
            access: false,
        });
    }
    close(&mut run, &mut kept, &mut planted);
    (kept, planted)
}

/// The ground with the buildings on it: one standing on its edge, or nearer the edge than `within`, is cut out of
/// the edge; the rest are holes in it. A building across the ground cuts it in parts, the largest first.
pub fn attach(outer: &[Pt], holes: &[Ring], within: f64) -> (Region, Vec<Region>) {
    let mut grounds: Vec<Ring> = vec![outer.to_vec()];
    let mut inner: Vec<Ring> = Vec::new();
    for given in holes {
        let mut h = given.clone();
        let at = grounds
            .iter()
            .position(|g| h.iter().any(|&p| point_in_ring(p, g)) || g.iter().any(|&p| point_in_ring(p, &h)) || cut_out(g, &h).is_some());
        let Some(at) = at else { continue };
        let ground = grounds[at].clone();
        if h.iter().all(|&p| point_in_ring(p, &ground) && dist_to_ring(p, &ground) > 1e-4) {
            let mut gap = f64::INFINITY;
            for &p in &h {
                gap = gap.min(dist_to_ring(p, &ground));
            }
            for &p in &ground {
                gap = gap.min(dist_to_ring(p, &h));
            }
            if gap >= within {
                inner.push(h);
                continue;
            }
            let reach: Vec<Pt> = h
                .iter()
                .filter(|&&p| dist_to_ring(p, &ground) < gap + 2.0)
                .map(|&p| {
                    let q = nearest_on_ring(p, &ground);
                    let d = or1(dist(p, q));
                    [q[0] + ((q[0] - p[0]) / d) * 0.05, q[1] + ((q[1] - p[1]) / d) * 0.05]
                })
                .collect();
            let mut all = h.clone();
            all.extend(reach);
            h = hull(&all);
        }
        if cut_out(&ground, &h).is_none() && h.iter().any(|&p| dist_to_ring(p, &ground) < 1e-3) {
            h = offset_ring(&ccw(&h), -0.05);
        }
        match cut_out(&ground, &h) {
            Some(left) if !left.is_empty() => {
                let parts: Vec<Ring> = left.iter().map(|l| offset_ring(&simplify(l, 0.75), 0.0)).filter(|l| l.len() >= 3).collect();
                grounds.splice(at..at + 1, parts);
            }
            _ => inner.push(given.clone()),
        }
    }
    grounds.retain(|g| ring_area(&ccw(g)) > 20.0);
    grounds.sort_by(|a, b| ring_area(&ccw(b)).partial_cmp(&ring_area(&ccw(a))).unwrap_or(std::cmp::Ordering::Equal));
    if grounds.is_empty() {
        grounds = vec![outer.to_vec()];
    }
    let parts: Vec<Region> = grounds
        .into_iter()
        .map(|g| {
            let hs = inner
                .iter()
                .filter(|h| h.iter().any(|&p| point_in_ring(p, &g)) || shared_area(&hull(h), &hull(&g)) > 0.01)
                .cloned()
                .collect();
            Region::new(g, hs)
        })
        .collect();
    let first = parts.first().cloned().unwrap_or_default();
    (first, parts)
}

/// The ring without the outward corners that stand less than `tol` off the line between their neighbours.
fn straighten(r: &[Pt], tol: f64) -> Ring {
    let mut out = r.to_vec();
    let mut i: isize = 0;
    while (i as usize) < out.len() && out.len() > 3 {
        let n = out.len();
        let iu = i as usize;
        let a = out[(iu + n - 1) % n];
        let b = out[(iu + 1) % n];
        let p = out[iu];
        let turn = (p[0] - a[0]) * (b[1] - p[1]) - (p[1] - a[1]) * (b[0] - p[0]);
        let l = or1(dist(a, b));
        if turn > 0.0 && ((p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])).abs() / l < tol {
            out.remove(iu);
            i = (-1isize).max(i - 2);
        }
        i += 1;
    }
    out
}

struct Sizes {
    a: f64,
    dep: f64,
    pitch: f64,
    lean: f64,
    one_way: bool,
    cut: f64,
}

/// The ring's sizes: square bays on a two-way loop; or, with angled bays, bays at that angle leaning with the loop's
/// one-way traffic.
fn ring_sizes(s: &Settings, square: bool) -> Sizes {
    let angled = s.angle < 89.9;
    if square || !angled {
        return Sizes {
            a: if angled { standard(&s.standard).aisle_for(90.0, false) } else { s.aisle },
            dep: s.stall_depth,
            pitch: s.stall_width,
            lean: 0.0,
            one_way: false,
            cut: if angled { s.aisle_two_way } else { s.aisle },
        };
    }
    Sizes {
        a: s.aisle,
        dep: row_depth(s),
        pitch: pitch_of(s),
        lean: lean_of(s),
        one_way: angled && s.one_way,
        cut: if angled { s.aisle_two_way } else { s.aisle },
    }
}

/// The plot laid out as a ring: a row along every side, the loop aisle, and the middle set out along each of `degs`.
pub fn lay_ring(ctx: &mut Ctx, given: &Region, s: &Settings, origin: Pt, degs: &[f64], quick: bool, square_only: bool) -> Vec<WPlan> {
    if quick {
        return lay_ring_as(ctx, given, s, origin, degs, true, true);
    }
    let squares: &[bool] = if s.angle < 89.9 && !square_only { &[true, false] } else { &[true] };
    let mut out = Vec::new();
    for &square in squares {
        out.extend(lay_ring_as(ctx, given, s, origin, degs, square, false));
    }
    out
}

fn worth(m: &WPlan) -> f64 {
    m.stalls.len() as f64 - paved_w(m) / PAVED_PER_BAY
}

fn lay_ring_as(ctx: &mut Ctx, given: &Region, s: &Settings, origin: Pt, degs: &[f64], square: bool, quick: bool) -> Vec<WPlan> {
    let mut out = Vec::new();
    let z = ring_sizes(s, square);
    let (usable, _) = attach(&given.outer, &given.holes, z.dep + z.a);
    out.extend(lay_ring_with(ctx, &usable, s, origin, degs, -1.0, !quick, square, quick, None));
    if !usable.holes.is_empty() && !quick {
        let mut best: Vec<(f64, f64)> = Vec::new();
        for m in &out {
            if let Some(&d) = m.dirs.first() {
                let wv = worth(m);
                match best.iter_mut().find(|(k, _)| *k == d) {
                    Some(x) => x.1 = x.1.max(wv),
                    None => best.push((d, wv)),
                }
            }
        }
        let get = |d: f64| best.iter().find(|(k, _)| *k == d).map(|x| x.1);
        let mut top: Vec<f64> = degs.iter().copied().filter(|&d| get(d).is_some()).collect();
        top.sort_by(|a, b| get(*b).unwrap_or(0.0).partial_cmp(&get(*a).unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
        top.truncate(2);
        let use_degs: Vec<f64> = if top.is_empty() { degs.iter().take(2).copied().collect() } else { top };
        for against in [0.0, z.dep] {
            out.extend(lay_ring_with(ctx, &usable, s, origin, &use_degs, against, false, square, false, None));
        }
    }
    if !quick {
        out.extend(lay_ring_wings(ctx, &usable, s, origin, square));
    }
    out
}

/// Dedupe directions within a degree, the first kept.
fn distinct_dirs(all: Vec<f64>) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for (j, &d) in all.iter().enumerate() {
        if all.iter().position(|&e| (e - d).abs().min(180.0 - (e - d).abs()) < 1.0) == Some(j) {
            out.push(d);
        }
    }
    out
}

/// A plot with a wing: cut across at an inside corner by an aisle from side to side; each part a ring of its own
/// round its own middle, the two sharing the aisle on the cut.
fn lay_ring_wings(ctx: &mut Ctx, usable: &Region, s: &Settings, origin: Pt, square: bool) -> Vec<WPlan> {
    let mut out = Vec::new();
    let cc = ring_sizes(s, square).cut;
    let outer = ccw(&usable.outer);
    let n = outer.len();
    if n < 3 {
        return out;
    }
    let mut tried: Vec<String> = Vec::new();
    let inside: Vec<usize> = (0..n)
        .filter(|&i| {
            let (a, c, b) = (outer[(i + n - 1) % n], outer[i], outer[(i + 1) % n]);
            (c[0] - a[0]) * (b[1] - c[1]) - (c[1] - a[1]) * (b[0] - c[0]) < -1e-6
        })
        .collect();
    let mut cuts: Vec<(Pt, Pt, Pt)> = Vec::new();
    for &i in &inside {
        let c = outer[i];
        cuts.push((c, outer[(i + n - 1) % n], c));
        cuts.push((c, c, outer[(i + 1) % n]));
    }
    for i in 0..inside.len() {
        for j in i + 1..inside.len() {
            cuts.push((outer[inside[i]], outer[inside[i]], outer[inside[j]]));
        }
    }
    for (c, p, q) in cuts {
        if dist(p, q) < 10.0 {
            continue;
        }
        let deg = side_deg(p, q);
        let f = frame_of(c, deg);
        let local = region_to_local(&f, usable);
        for (y0, y1) in [(0.0, cc), (-cc, 0.0)] {
            let key = format!("{}|{}|{}", to_fixed(deg, 1), to_fixed(to_local(&frame_of(origin, deg), c)[1], 1), y0);
            if tried.contains(&key) {
                continue;
            }
            tried.push(key);
            let chord = |yy: f64| scanline(&local, yy).into_iter().find(|&[lo, hi]| lo <= 1.0 && hi >= -1.0);
            let (Some(lo0), Some(hi0)) = (chord(y0 + 1e-6), chord(y1 - 1e-6)) else { continue };
            if (lo0[1] - lo0[0]).min(hi0[1] - hi0[0]) < 15.0 {
                continue;
            }
            let span = [lo0[0].min(hi0[0]), lo0[1].max(hi0[1])];
            let aisle = WAisle {
                id: ctx.id(),
                poly: [[lo0[0], y0], [lo0[1], y0], [hi0[1], y1], [hi0[0], y1]].iter().map(|&x| to_world(&f, x)).collect(),
                kind: AisleKind::Cross,
                flow: None,
            };
            if !usable.holes.iter().all(|h| !hits(h, &aisle.poly)) {
                continue;
            }
            let lv = |x: Pt| to_local(&f, x)[1];
            let along = |x: Pt| to_local(&f, x)[0];
            let halves = [clip_half(usable, &|x| lv(x) - y1), clip_half(usable, &|x| y0 - lv(x))];
            if halves.iter().any(|h| h.as_ref().is_none_or(|h| region_area_js(h) < 300.0)) {
                continue;
            }
            let mut laid: Vec<WPlan> = Vec::new();
            for (k, half) in halves.iter().enumerate() {
                let Some(half) = half else { continue };
                let yy = if k == 0 { y1 } else { y0 };
                let on_cut = move |u: Pt, v: Pt| {
                    (lv(u) - yy).abs() < 0.01
                        && (lv(v) - yy).abs() < 0.01
                        && along(u).min(along(v)) >= span[0] - 0.5
                        && along(u).max(along(v)) <= span[1] + 0.5
                };
                let mut all = vec![deg, (deg + 90.0) % 180.0];
                all.extend(directions(&half.outer, 3, 10.0));
                let dirs = distinct_dirs(all);
                let mut plans = lay_ring_with(ctx, half, s, origin, &dirs, -1.0, false, square, false, Some((&on_cut, &aisle)));
                plans.sort_by(|m, n| worth(n).partial_cmp(&worth(m)).unwrap_or(std::cmp::Ordering::Equal));
                if let Some(best) = plans.into_iter().next() {
                    laid.push(best);
                }
            }
            if laid.len() < 2 {
                continue;
            }
            let mut plan = join(laid, vec![]);
            let mut seen: Vec<i64> = Vec::new();
            plan.aisles.retain(|x| {
                if seen.contains(&x.id) {
                    false
                } else {
                    seen.push(x.id);
                    true
                }
            });
            out.push(plan);
        }
    }
    out
}

fn seg_distance(p: Pt, a: Pt, b: Pt) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let u = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    // A side of no length: JavaScript's NaN makes every comparison false.
    if u.is_nan() {
        return f64::NAN;
    }
    dist(p, [a[0] + dx * u, a[1] + dy * u])
}

fn unit(a: Pt, b: Pt) -> Pt {
    let l = or1(dist(a, b));
    [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
}

struct Part {
    g: Region,
    gr: Region,
    dirs: Vec<f64>,
}

type Open<'a> = Option<(&'a dyn Fn(Pt, Pt) -> bool, &'a WAisle)>;

/// One ring. `open` says which sides of the ground lie on its shared aisle, which the ring shares with the ring
/// beyond it (a wing's): no row along such a side and no loop of its own there.
#[allow(clippy::too_many_arguments)]
fn lay_ring_with(
    ctx: &mut Ctx,
    usable: &Region,
    s: &Settings,
    origin: Pt,
    degs: &[f64],
    against: f64,
    cuts: bool,
    square: bool,
    quick: bool,
    open: Open,
) -> Vec<WPlan> {
    let ring = offset_ring(&straighten(&offset_ring(&usable.outer, 0.0), 0.3), 0.0);
    if ring.len() < 3 {
        return Vec::new();
    }
    let nn = ring.len();
    let Sizes { a: aa, dep, pitch: ew, lean, one_way, cut: cc } = ring_sizes(s, square);
    let shared = open.map(|o| o.1);
    let open_side: Vec<bool> = (0..nn).map(|i| open.is_some_and(|(f, _)| f(ring[i], ring[(i + 1) % nn]))).collect();
    let deep = |d: f64| open_side.iter().map(|&o| if o { 0.0 } else { d }).collect::<Vec<f64>>();
    let at_r = inset_sides(&ring, &deep(dep));
    let at_a = inset_sides(&ring, &deep(dep + aa));
    if !at_r.iter().any(Option::is_some) {
        return Vec::new();
    }
    let holes: Vec<Ring> = usable.holes.iter().map(|h| ccw(h)).collect();
    let hole_hit = |poly: &[Pt]| {
        holes.iter().any(|h| {
            hits(poly, h)
                || h.iter().any(|&p| point_in_ring(p, poly))
                || poly.iter().any(|&p| point_in_ring(p, h) && dist_to_ring(p, h) > 0.001)
        })
    };
    let on_ground = |poly: &[Pt]| poly.iter().all(|&p| point_in_ring(p, &ring) || dist_to_ring(p, &ring) < 0.01);
    let deep_enough = |d: f64| {
        let ring = &ring;
        let open_side = &open_side;
        move |p: Pt| (0..nn).all(|i| open_side[i] || seg_distance(p, ring[i], ring[(i + 1) % nn]) >= d - 0.05)
    };
    let loops_of = |sides: &[Option<(Pt, Pt)>], d: f64| -> Vec<Ring> {
        let pts: Ring = sides.iter().filter_map(|x| x.map(|x| x.0)).collect();
        let keep = deep_enough(d);
        untangle(&pts, &keep)
    };

    // The loop: one piece in front of each side's row.
    let mut lp: Vec<WAisle> = Vec::new();
    let mut sides: Vec<LoopSide> = Vec::new();
    let mut piece_of: Vec<Option<WAisle>> = vec![None; nn];
    for i in 0..nn {
        if open_side[i] {
            if let Some(sh) = shared {
                sides.push(LoopSide { v: (ring[i], ring[(i + 1) % nn]), piece: sh.clone(), tag: format!("s{i}") });
            }
            continue;
        }
        let Some(u) = at_r[i] else { continue };
        let v = at_a[i];
        let poly: Ring = match v {
            Some(v) => vec![u.0, u.1, v.1, v.0],
            None => {
                let mut j = (i + 1) % nn;
                while j != i && at_a[j].is_none() {
                    j = (j + 1) % nn;
                }
                let Some(aj) = at_a[j].filter(|_| j != i) else { continue };
                vec![u.0, u.1, aj.0]
            }
        };
        if ring_area(&poly) < 1.0 || hole_hit(&poly) || !on_ground(&poly) || poly.iter().any(|&x| dist_to_ring(x, &ring) > (dep + aa) * 2.0)
        {
            continue;
        }
        let piece = WAisle {
            id: ctx.id(),
            poly,
            kind: AisleKind::Cross,
            flow: if one_way { Some(unit(ring[i], ring[(i + 1) % nn])) } else { None },
        };
        lp.push(piece.clone());
        piece_of[i] = Some(piece.clone());
        if let Some(v) = v {
            sides.push(LoopSide { v, piece, tag: format!("o{i}") });
        }
    }
    if let Some(sh) = shared {
        lp.push(sh.clone());
    }
    if lp.is_empty() {
        return Vec::new();
    }
    let mut busy_base = PieceIndex::new();
    let mut bay_index = PieceIndex::new();
    for p in &lp {
        busy_base.add(&p.poly);
    }

    // The row along each side of the plot, square to it.
    let mut stalls: Vec<WStall> = Vec::new();
    let mut islands: Vec<WIsland> = Vec::new();
    for i in 0..nn {
        let (Some(piece), Some(u)) = (&piece_of[i], at_r[i]) else { continue };
        let p = ring[i];
        let q = ring[(i + 1) % nn];
        let l = dist(p, q);
        let t = [(q[0] - p[0]) / l, (q[1] - p[1]) / l];
        let lo = 0f64.max((u.0[0] - p[0]) * t[0] + (u.0[1] - p[1]) * t[1]);
        let hi = l.min((u.1[0] - p[0]) * t[0] + (u.1[1] - p[1]) * t[1]);
        if hi - lo < ew {
            continue;
        }
        let strip = ccw(&[p, q, u.1, u.0]);
        let in_strip = |poly: &[Pt]| poly.iter().all(|&x| point_in_ring(x, &strip) || dist_to_ring(x, &strip) < 0.01);
        let (kept, planted) = {
            let bb = &busy_base;
            row_along(
                (p, q),
                lo,
                hi,
                dep,
                ew,
                0.0,
                s.min_run,
                &|poly| in_strip(poly) && !hole_hit(poly) && !bb.clash(poly),
                dep,
                piece.id,
                &format!("ring|{i}"),
                lean,
            )
        };
        for st in kept {
            busy_base.add(&st.poly);
            bay_index.add(&st.poly);
            stalls.push(st);
        }
        for st in planted {
            islands.push(WIsland { poly: st.poly, kind: IslandKind::Leftover });
        }
    }

    // Round each building: its own loop, `against` m out from it.
    let mut hole_out: Vec<Ring> = Vec::new();
    let mut hole_front: Vec<Ring> = Vec::new();
    for (hk, h) in holes.iter().enumerate() {
        if against < 0.0 {
            hole_out.push(h.clone());
            hole_front.push(h.clone());
            continue;
        }
        let m = h.len();
        let side_of = |d: f64| inset_sides(h, &vec![-d; m]);
        let at_b: Vec<Option<(Pt, Pt)>> =
            if against > 0.0 { side_of(against) } else { (0..m).map(|i| Some((h[i], h[(i + 1) % m]))).collect() };
        let at_l = side_of(against + aa);
        let mut pieces: Vec<Option<WAisle>> = Vec::new();
        for i in 0..m {
            let (Some(b), Some(l)) = (at_b[i], at_l[i]) else {
                pieces.push(None);
                continue;
            };
            let poly = vec![b.1, b.0, l.0, l.1];
            let piece =
                WAisle { id: ctx.id(), poly: ccw(&poly), kind: AisleKind::Cross, flow: if one_way { Some(unit(l.1, l.0)) } else { None } };
            let ok = ring_area(&piece.poly) > 1.0
                && on_ground(&piece.poly)
                && !bay_index.clash(&piece.poly)
                && !holes.iter().enumerate().any(|(ok2, o)| ok2 != hk && hits(o, &piece.poly));
            pieces.push(if ok { Some(piece) } else { None });
        }
        for (i, piece) in pieces.iter().enumerate() {
            let Some(piece) = piece else { continue };
            lp.push(piece.clone());
            busy_base.add(&piece.poly);
            if let Some(l) = at_l[i] {
                sides.push(LoopSide { v: (l.1, l.0), piece: piece.clone(), tag: format!("h{hk}|{i}") });
            }
        }
        if against > 0.0 {
            for (i, piece) in pieces.iter().enumerate() {
                let Some(piece) = piece else { continue };
                let p = h[(i + 1) % m];
                let q = h[i];
                let l = dist(p, q);
                let (kept, planted) = {
                    let bb = &busy_base;
                    row_along(
                        (p, q),
                        0.0,
                        l,
                        against,
                        ew,
                        0.0,
                        s.min_run,
                        &|poly| !hole_hit(poly) && !bb.clash(poly) && on_ground(poly),
                        against,
                        piece.id,
                        &format!("bld|{hk}|{i}"),
                        lean,
                    )
                };
                for st in kept {
                    busy_base.add(&st.poly);
                    bay_index.add(&st.poly);
                    stalls.push(st);
                }
                for st in planted {
                    islands.push(WIsland { poly: st.poly, kind: IslandKind::Leftover });
                }
            }
        }
        hole_out.push(offset_ring(h, -(against + aa)));
        hole_front.push(if against > 0.0 { offset_ring(h, -against) } else { h.clone() });
    }

    // The middle, inside the loop: each part of it deep enough, on its own.
    let inners = loops_of(&at_a, dep + aa);
    let fronts = loops_of(&at_r, dep);
    let in_middle =
        |p: Pt| inners.iter().any(|r| point_in_ring(p, r)) && !hole_out.iter().any(|h| point_in_ring(p, h) && dist_to_ring(p, h) > 0.01);
    let front_of = |c: &[Pt]| -> Option<Ring> {
        let c0 = c.first().copied();
        fronts.iter().find(|r| c0.is_some_and(|c0| point_in_ring(c0, r))).or(fronts.first()).cloned()
    };

    let inner_rows = |dir_of: &dyn Fn(Pt) -> Option<f64>, taken: &[Ring], extra: &[WAisle]| -> Vec<WStall> {
        let mut inside: Vec<WStall> = Vec::new();
        let mut busy = PieceIndex::new();
        for p in taken {
            busy.add(p);
        }
        for x in &lp {
            busy.add(&x.poly);
        }
        for x in extra {
            busy.add(&x.poly);
        }
        for e in &stalls {
            busy.add(&e.poly);
        }
        for side in &sides {
            let (v0, v1) = side.v;
            let l = dist(v0, v1);
            if l < ew * s.min_run {
                continue;
            }
            let t = [(v1[0] - v0[0]) / l, (v1[1] - v0[1]) / l];
            let deg = dir_of([(v0[0] + v1[0]) / 2.0 - t[1] * 1.5, (v0[1] + v1[1]) / 2.0 + t[0] * 1.5]);
            if let Some(deg) = deg {
                let turn = (t[1].atan2(t[0]) * 180.0 / std::f64::consts::PI - deg + 720.0) % 90.0;
                if turn.min(90.0 - turn) <= 3.0 {
                    continue;
                }
            }
            let mut best_row: Vec<WStall> = Vec::new();
            for f in [0.0, 0.25, 0.5, 0.75] {
                let bz = &busy;
                let ok = |poly: &[Pt]| {
                    poly.iter().all(|&p| in_middle(p) || seg_distance(p, v0, v1) < 0.01) && !hole_hit(poly) && !bz.clash(poly)
                };
                let (kept, _) =
                    row_along(side.v, 0.0, l, dep, ew, f * ew, s.min_run, &ok, 0.0, side.piece.id, &format!("inner|{}", side.tag), lean);
                if kept.len() > best_row.len() {
                    best_row = kept;
                }
            }
            for x in &best_row {
                busy.add(&x.poly);
            }
            inside.extend(best_row);
        }
        inside
    };

    // The ring with its middle in `parts`, each set out in the best of its directions, and `extra` aisles.
    let finish = |ctx: &mut Ctx, parts: &[Part], extra: &[WAisle]| -> WPlan {
        let pieces: Vec<&WAisle> = lp.iter().chain(extra.iter()).collect();
        let mut local_loops: Vec<(f64, Vec<MiddleLoop>)> = Vec::new();
        let mut options: Vec<Vec<WPlan>> = Vec::new();
        let mut regions: Vec<&Region> = Vec::new();
        for part in parts {
            let mut found: Vec<WPlan> = Vec::new();
            for &deg in &part.dirs {
                let f = frame_of(origin, deg);
                let along = |v: Pt| -> Pt {
                    let a = to_local(&f, f.o);
                    let b = to_local(&f, [f.o[0] + v[0], f.o[1] + v[1]]);
                    [b[0] - a[0], b[1] - a[1]]
                };
                if !local_loops.iter().any(|(d, _)| *d == deg) {
                    let made = pieces
                        .iter()
                        .map(|x| MiddleLoop {
                            id: x.id,
                            poly: ccw(&x.poly.iter().map(|&p| to_local(&f, p)).collect::<Ring>()),
                            flow: x.flow.map(along),
                        })
                        .collect();
                    local_loops.push((deg, made));
                }
                let loop_l: &[MiddleLoop] = local_loops.iter().find(|(d, _)| *d == deg).map(|x| x.1.as_slice()).unwrap_or(&[]);
                let mut grid: Vec<f64> = Vec::new();
                for st in &stalls {
                    let (Some(&a), Some(&b)) = (st.poly.first(), st.poly.get(1)) else { continue };
                    let d = along([b[0] - a[0], b[1] - a[1]]);
                    if d[1].abs() < 1e-3 * hypot(d[0], d[1]) {
                        let g = js_round(to_local(&f, a)[0].min(to_local(&f, b)[0]) * 100.0) / 100.0;
                        if !grid.contains(&g) {
                            grid.push(g);
                        }
                    }
                }
                let keep = if part.dirs.len() > 1 { 2 } else { 4 };
                for p in best_middles(&region_to_local(&f, &part.g), &region_to_local(&f, &part.gr), loop_l, s, keep, &grid) {
                    found.push(place(ctx, &p, &f, "R", deg));
                }
            }
            if found.is_empty() {
                continue;
            }
            found.sort_by(|p, q| worth(q).partial_cmp(&worth(p)).unwrap_or(std::cmp::Ordering::Equal));
            found.truncate(4);
            options.push(found);
            regions.push(&part.g);
        }
        let total = |choice: &[usize]| -> (Vec<usize>, Vec<WStall>, f64) {
            let middles: Vec<&WPlan> = choice.iter().enumerate().map(|(j, &k)| &options[j][k]).collect();
            let mut taken: Vec<Ring> = Vec::new();
            for m in &middles {
                taken.extend(m.stalls.iter().map(|x| x.poly.clone()));
                taken.extend(m.aisles.iter().map(|x| x.poly.clone()));
                taken.extend(m.columns.iter().cloned());
            }
            let dir_of = |p: Pt| -> Option<f64> {
                let j = regions.iter().position(|g| point_in_ring(p, &g.outer))?;
                middles.get(j).and_then(|m| m.dirs.first().copied())
            };
            let inside = inner_rows(&dir_of, &taken, extra);
            let mut score = 0.0;
            for m in &middles {
                score += worth(m);
            }
            score += inside.len() as f64;
            (choice.to_vec(), inside, score)
        };
        let mut pick: Vec<usize> = vec![0; options.len()];
        let mut best = total(&pick);
        for j in 0..options.len() {
            for k in 1..options[j].len() {
                let mut trial = pick.clone();
                trial[j] = k;
                let r = total(&trial);
                if r.2 > best.2 {
                    best = r;
                    pick[j] = k;
                }
            }
        }
        let (choice, inside, _) = best;
        let under = |il: &WIsland| inside.iter().any(|st| hits(&st.poly, &il.poly));
        let middles: Vec<&WPlan> = choice.iter().enumerate().map(|(j, &k)| &options[j][k]).collect();
        let mut ring_stalls = stalls.clone();
        ring_stalls.extend(inside.iter().cloned());
        let ring_plan = WPlan {
            aisles: lp.iter().chain(extra.iter()).cloned().collect(),
            stalls: ring_stalls,
            islands: islands.iter().filter(|il| !under(il)).cloned().collect(),
            columns: Vec::new(),
            fields: 1,
            dirs: if middles.is_empty() { vec![parts.first().and_then(|p| p.dirs.first().copied()).unwrap_or(0.0)] } else { vec![] },
            ring: true,
        };
        let mut laid = vec![ring_plan];
        for m in middles {
            let mut c = m.clone();
            c.islands.retain(|il| !under(il));
            laid.push(c);
        }
        join(laid, vec![])
    };

    let mut plans: Vec<WPlan> = Vec::new();
    let whole = |c: &Ring, dirs: Vec<f64>| -> Option<Part> {
        let fr = front_of(c)?;
        Some(Part { g: Region::new(c.clone(), hole_out.clone()), gr: Region::new(fr, hole_front.clone()), dirs })
    };
    // The middle as it is in each direction; then, in the two that did best, with a row along each skewed side kept
    // free first.
    let mut plain: Vec<(f64, WPlan)> = Vec::new();
    for &deg in degs {
        let parts: Vec<Part> = inners.iter().filter(|c| ring_area(c) >= 50.0).filter_map(|c| whole(c, vec![deg])).collect();
        let plan = finish(ctx, &parts, &[]);
        plain.push((deg, plan));
    }
    plans.extend(plain.iter().map(|x| x.1.clone()));
    plain.sort_by(|a, b| worth(&b.1).partial_cmp(&worth(&a.1)).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<f64> = plain.iter().take(if quick { 0 } else { 2 }).map(|x| x.0).collect();
    for deg in top {
        let skewed: Vec<bool> = (0..nn)
            .map(|i| {
                let p = ring[i];
                let q = ring[(i + 1) % nn];
                let a = ((q[1] - p[1]).atan2(q[0] - p[0]) * 180.0 / std::f64::consts::PI - deg + 360.0) % 90.0;
                a.min(90.0 - a) > 3.0
            })
            .collect();
        if !skewed.iter().any(|&k| k) {
            continue;
        }
        let by: Vec<f64> = (0..nn).map(|i| if open_side[i] { 0.0 } else { dep + aa + if skewed[i] { dep } else { 0.0 } }).collect();
        let parts = loops_of(&inset_sides(&ring, &by), dep + aa);
        let parts: Vec<Part> = parts.iter().filter(|c| ring_area(c) >= 50.0).filter_map(|c| whole(c, vec![deg])).collect();
        plans.push(finish(ctx, &parts, &[]));
    }

    // A middle with an inside corner (a wing): cut across at the corner by an aisle from loop to loop; each part set
    // out in its own direction.
    if cuts {
        for (ci, comp) in inners.iter().enumerate() {
            let c0 = ccw(comp);
            if ring_area(&c0) < 300.0 {
                continue;
            }
            let Some(front) = front_of(&c0) else { continue };
            let others: Vec<Part> = inners
                .iter()
                .enumerate()
                .filter(|(xi, x)| *xi != ci && ring_area(x) >= 50.0)
                .filter_map(|(_, x)| whole(x, directions(x, 2, 10.0)))
                .collect();
            let mut tried: Vec<String> = Vec::new();
            let m = c0.len();
            for i in 0..m {
                let c = c0[i];
                let a = c0[(i + m - 1) % m];
                let b = c0[(i + 1) % m];
                if (c[0] - a[0]) * (b[1] - c[1]) - (c[1] - a[1]) * (b[0] - c[0]) > -1e-6 {
                    continue;
                }
                for (p, q) in [(a, c), (c, b)] {
                    if dist(p, q) < 8.0 {
                        continue;
                    }
                    let deg = side_deg(p, q);
                    for (y0, y1) in [(0.0, cc), (-cc, 0.0)] {
                        let key = format!("{}|{}|{}|{}", to_fixed(deg, 1), to_fixed(c[0], 1), to_fixed(c[1], 1), y0);
                        if tried.contains(&key) {
                            continue;
                        }
                        tried.push(key);
                        let f = frame_of(c, deg);
                        let front_l = region_to_local(&f, &Region::new(front.clone(), hole_front.clone()));
                        let Some(span) = strip_spans(&front_l, y0, y1).into_iter().find(|&[lo, hi]| lo <= 0.5 && hi >= -0.5) else {
                            continue;
                        };
                        if span[1] - span[0] < 10.0 {
                            continue;
                        }
                        let cut = WAisle {
                            id: ctx.id(),
                            poly: corners(&rect(span[0], y0, span[1], y1)).into_iter().map(|x| to_world(&f, x)).collect(),
                            kind: AisleKind::Cross,
                            flow: None,
                        };
                        if hole_hit(&cut.poly) || bay_index.clash(&cut.poly) {
                            continue;
                        }
                        let lv = |x: Pt| to_local(&f, x)[1];
                        let g_reg = Region::new(c0.clone(), hole_out.clone());
                        let r_reg = Region::new(front.clone(), hole_front.clone());
                        let sides_of = [
                            (clip_half(&g_reg, &|x| lv(x) - y1), clip_half(&r_reg, &|x| lv(x) - y0)),
                            (clip_half(&g_reg, &|x| y0 - lv(x)), clip_half(&r_reg, &|x| y1 - lv(x))),
                        ];
                        let mut parts: Vec<Part> = Vec::new();
                        for (g, gr) in sides_of {
                            let (Some(g), Some(gr)) = (g, gr) else { continue };
                            if region_area_js(&g) <= 50.0 {
                                continue;
                            }
                            let mut all = vec![deg, (deg + 90.0) % 180.0];
                            all.extend(directions(&g.outer, 2, 15.0));
                            parts.push(Part { dirs: distinct_dirs(all), g, gr });
                        }
                        if parts.len() < 2 {
                            continue;
                        }
                        for o in &others {
                            parts.push(Part { g: o.g.clone(), gr: o.gr.clone(), dirs: o.dirs.clone() });
                        }
                        plans.push(finish(ctx, &parts, std::slice::from_ref(&cut)));
                    }
                }
            }
        }
    }
    plans
}
