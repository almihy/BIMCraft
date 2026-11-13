//! Revit's definition of a family (the add-in's package `definition`, Arqiflow's
//! `docs/superpowers/specs/2026-11-13-revit-families-design.md`, "The definition's JSON, exactly") translated into a BIMCraft
//! family's references, dimensions, parametric forms, symbols, nested parts and type catalog. The input is hostile: it is read
//! as JSON values, each thing that can't be translated is left out and said in plain words (`untranslated`), and what is
//! made is tried in every type before it is kept, so the family always loads and the check decides what is drawn.
//!
//! Lengths are mm in the family document's axes; [`Map`] takes them into the family's own (a door's or a window's: the
//! opening's): `x_sign × x + x_offset`, `y_sign × y + y_offset`, `z - z_minus`. A bound number (B) is a number, `{ref, offset}` (a reference
//! plane's coordinate along its axis plus an offset), `{param, scale, offset}` or `{formula}`; it becomes an [`Expr`]. A
//! reference plane's position and an extrusion's start and end are coordinates along the positive axis of their normal.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;

use super::{
    Axis, Bend, Catalog, Curve, Dimension, DimensionKind, Expr, Family, Form, Kind, Levels, Nested, Plane, Reference, Ring, Show, Symbol,
    SymbolView, Val, nameable, number,
};
use crate::geom::P2;

/// Things one definition may bring: curves in a loop, loops in a form, forms, lines.
const MOST: usize = 4096;

/// The family document's axes taken into the family's own.
struct Map {
    x_offset: f64,
    x_sign: f64,
    y_sign: f64,
    y_offset: f64,
    /// What z drops by: a number, or a formula over the family's parameters (a window's sill).
    z_minus: Expr,
}

/// Reads a definition into a family: what it knows of the planes, to write bound numbers with.
struct Reader<'a> {
    map: Map,
    /// Each plane by its id: its name, its axis, its coordinate per type in the document's axes.
    planes: BTreeMap<String, (String, Axis, BTreeMap<String, f64>)>,
    params: BTreeSet<String>,
    first: String,
    notes: Vec<String>,
    family: &'a Family,
    values: BTreeMap<String, Val>,
    /// A door's or a window's: the thickness of the wall Revit's result was made in, per type.
    walls: Walls,
}

/// The thickness of the wall a door's or a window's types were exported in (Revit's result made in), per type: what its
/// formulas read as `Wall Thickness` while it is translated and checked.
pub type Walls = BTreeMap<String, f64>;

/// Type `t`'s values in the wall it was exported in.
fn vals(f: &Family, t: &str, walls: &Walls) -> Result<BTreeMap<String, Val>, String> {
    f.values_in(t, &BTreeMap::new(), walls.get(t).copied().unwrap_or(super::DEFAULT_WALL))
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite() && x.abs() <= crate::model::MAX_COORD)
}

fn text(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| !s.trim().is_empty() && s.chars().count() <= crate::model::MAX_TEXT)
}

/// A number written plainly: `12.5`, `-3`.
fn plain(x: f64) -> String {
    let s = format!("{:.6}", (x * 1e6).round() / 1e6 + 0.0);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

/// `a + b` as an expression, a number where both are.
fn plus(a: Expr, b: f64) -> Expr {
    match a {
        Expr::Number(x) => Expr::Number(x + b),
        Expr::Formula(f) if b == 0.0 => Expr::Formula(f),
        Expr::Formula(f) if b < 0.0 => Expr::Formula(format!("{f} - {}", plain(-b))),
        Expr::Formula(f) => Expr::Formula(format!("{f} + {}", plain(b))),
    }
}

fn neg(a: Expr) -> Expr {
    match a {
        Expr::Number(x) => Expr::Number(-x + 0.0),
        Expr::Formula(f) => Expr::Formula(format!("-({f})")),
    }
}

fn minus(a: Expr, b: &Expr) -> Expr {
    match (a, b) {
        (a, Expr::Number(y)) => plus(a, -y),
        (Expr::Number(0.0), Expr::Formula(g)) => Expr::Formula(format!("-({g})")),
        (Expr::Number(x), Expr::Formula(g)) => Expr::Formula(format!("{} - ({g})", plain(x))),
        (Expr::Formula(f), Expr::Formula(g)) => Expr::Formula(format!("{f} - ({g})")),
    }
}

/// The family axis a normal is square to, and whether it faces the other way; none, a sloped one.
fn axis_of(n: &Value) -> Option<(Axis, bool)> {
    let a = n.as_array()?;
    let v: Vec<f64> = a.iter().filter_map(num).collect();
    let [x, y, z] = v.as_slice() else { return None };
    let len = (x * x + y * y + z * z).sqrt();
    if len < 1e-9 {
        return None;
    }
    let (x, y, z) = (x / len, y / len, z / len);
    let near = |c: f64| c.abs() > 0.99999;
    if near(x) {
        Some((Axis::X, x < 0.0))
    } else if near(y) {
        Some((Axis::Y, y < 0.0))
    } else if near(z) {
        Some((Axis::Z, z < 0.0))
    } else {
        None
    }
}

impl Map {
    /// Whether it mirrors: one of x and y turned the other way.
    fn mirrors(&self) -> bool {
        self.x_sign * self.y_sign < 0.0
    }

    /// The sign a length along `axis` takes.
    fn sign(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.x_sign,
            Axis::Y => self.y_sign,
            Axis::Z => 1.0,
        }
    }

    /// A direction of the document's in the family's axes.
    fn turn(&self, d: [f64; 3]) -> [f64; 3] {
        [d[0] * self.x_sign, d[1] * self.y_sign, d[2]]
    }
}

impl Reader<'_> {
    /// A bound number as an expression of the document's (not taken into the family's axes): a value, a length, an angle.
    fn value(&self, b: &Value) -> Result<Expr, String> {
        if let Some(x) = num(b) {
            return Ok(Expr::Number(x));
        }
        if let Some(f) = b.get("formula").and_then(text) {
            return Ok(Expr::Formula(format!("({f})")));
        }
        if let Some(p) = b.get("param").and_then(text) {
            if !self.params.contains(p) {
                return Err(format!("it follows `{p}`, which isn't one of the family's numbers"));
            }
            let (scale, offset) = (b.get("scale").and_then(num).unwrap_or(1.0), b.get("offset").and_then(num).unwrap_or(0.0));
            let base = if scale == 1.0 { p.to_string() } else { format!("{} * {p}", plain(scale)) };
            return Ok(plus(Expr::Formula(base), offset));
        }
        if let Some(r) = b.get("ref").and_then(Value::as_str) {
            // A plane's coordinate as a value: where it stands in the first type, in the document's axes.
            let (_, _, at) = self.planes.get(r).ok_or_else(|| format!("it follows a reference `{r}` the definition doesn't have"))?;
            let x = at.get(&self.first).or_else(|| at.values().next()).copied().unwrap_or(0.0);
            return Ok(match b.get("offset").filter(|o| !o.is_null()) {
                None => Expr::Number(x),
                Some(o) => plus(self.value(o)?, x),
            });
        }
        Err(format!("it is placed by a rule BIMCraft can't read yet ({})", short(b)))
    }

    /// A coordinate along `axis`, bound as `b`, in the family's axes.
    fn coord(&self, b: &Value, axis: Axis) -> Result<Expr, String> {
        if let Some(r) = b.get("ref").and_then(Value::as_str)
            && let Some((name, a, _)) = self.planes.get(r)
            && *a == axis
        {
            let sign = self.map.sign(axis);
            let at = Expr::Formula(format!("ref({name})"));
            // The offset: a number, or itself bound (a parameter's value, a formula).
            return Ok(match b.get("offset").filter(|o| !o.is_null()) {
                None => at,
                Some(o) => match self.value(o)? {
                    Expr::Number(x) => plus(at, sign * x),
                    e if sign < 0.0 => minus(at, &e),
                    Expr::Formula(f) => Expr::Formula(format!("ref({name}) + {f}")),
                },
            });
        }
        let v = self.value(b)?;
        Ok(self.place(v, axis))
    }

    /// A document coordinate along `axis` taken into the family's axes.
    fn place(&self, v: Expr, axis: Axis) -> Expr {
        match axis {
            Axis::X if self.map.x_sign < 0.0 => plus(neg(v), self.map.x_offset),
            Axis::X => plus(v, self.map.x_offset),
            Axis::Y if self.map.y_sign < 0.0 => plus(neg(v), self.map.y_offset),
            Axis::Y => plus(v, self.map.y_offset),
            Axis::Z => minus(v, &self.map.z_minus),
        }
    }

    /// A point [B, B, B] in the family's axes.
    fn point(&self, p: &Value) -> Result<[Expr; 3], String> {
        let a = p.as_array().filter(|a| a.len() == 3).ok_or("a point is three numbers")?;
        Ok([self.coord(&a[0], Axis::X)?, self.coord(&a[1], Axis::Y)?, self.coord(&a[2], Axis::Z)?])
    }

    /// A point's numbers in the first type, in the family's axes: what arcs' turns and loops' sizes are read from.
    fn at(&self, p: &[Expr; 3]) -> Result<[f64; 3], String> {
        let mut read = HashMap::new();
        Ok([number(&p[0], &self.values, &mut read)?, number(&p[1], &self.values, &mut read)?, number(&p[2], &self.values, &mut read)?])
    }

    /// The first type's values, each plane's place among them as `ref(Name)`, in the family's axes.
    fn first_values(&mut self) {
        let mut values = vals(self.family, &self.first, &self.walls).unwrap_or_default();
        for (name, axis, at) in self.planes.values() {
            if let Some(x) = at.get(&self.first).or_else(|| at.values().next())
                && let Ok(v) = number(&self.place(Expr::Number(*x), *axis), &values, &mut HashMap::new())
            {
                values.insert(super::ref_key(name), Val::Number(v));
            }
        }
        self.values = values;
    }
}

fn short(v: &Value) -> String {
    v.to_string().chars().take(60).collect()
}

/// Whether a loop of the definition encloses something: not a lone line (a revolve's axis drawn in its sketch).
fn encloses(lp: &Value) -> bool {
    lp.as_array().is_some_and(|c| c.len() >= 2 || c.first().is_some_and(|c| c.get("arc").is_some()))
}

/// The loops of a form's `key` that enclose something ([`encloses`]), at most [`MOST`]; none, why.
fn loops_of<'v>(def: &'v Value, key: &str) -> Result<Vec<&'v Value>, &'static str> {
    let all = def[key].as_array().filter(|l| l.len() <= MOST).ok_or("its sketch has no loops")?;
    let kept: Vec<&Value> = all.iter().filter(|l| encloses(l)).collect();
    if kept.is_empty() { Err("its sketch has no loops") } else { Ok(kept) }
}

/// A loop in a plane: its corners, its arcs, and points round it in the first type (its corners and its arcs' middles).
type Loop = (Vec<[Expr; 2]>, Vec<Bend>, Vec<P2>);

/// A curve of a loop: its start (and end) point, and for an arc its centre, its normal and its middle point.
struct Bit {
    start: [Expr; 3],
    end: [Expr; 3],
    arc: Option<([Expr; 3], [f64; 3], [f64; 3])>,
    radius: Option<Expr>,
    through: Option<[Expr; 3]>,
}

impl Reader<'_> {
    fn curve(&self, c: &Value) -> Result<Bit, String> {
        if let Some(l) = c.get("line").and_then(Value::as_array).filter(|l| l.len() == 2) {
            return Ok(Bit { start: self.point(&l[0])?, end: self.point(&l[1])?, arc: None, radius: None, through: None });
        }
        let a = c.get("arc").ok_or_else(|| format!("a curve of a kind it can't draw: {}", short(c)))?;
        let (start, end, centre) = (self.point(&a["start"])?, self.point(&a["end"])?, self.point(&a["center"])?);
        let n: Vec<f64> = a["normal"].as_array().map(|v| v.iter().filter_map(num).collect()).unwrap_or_default();
        let [nx, ny, nz] = n.as_slice() else { return Err("an arc of it says no direction".into()) };
        // Its middle, worked out in the document's axes (counter-clockwise about its normal), then taken over: what says
        // which way it turns once mirrored.
        let (s, e, c3) = (self.at(&start)?, self.at(&end)?, self.at(&centre)?);
        let normal = self.map.turn([*nx, *ny, *nz]);
        let doc = |p: [f64; 3]| self.map.turn(p);
        let (sd, ed, cd) = (doc(s), doc(e), doc(c3));
        let mid = middle(sd, ed, cd, [*nx, *ny, *nz]);
        let radius = a.get("radius").map(|r| self.value(r)).transpose()?;
        // A point it passes through, where the exporter gives one bound (its centre follows nothing it can say).
        // Only where it is bound: a mid of plain numbers says nothing a centre doesn't.
        let through =
            a.get("mid").filter(|m| m.as_array().is_some_and(|v| v.iter().any(|x| !x.is_number()))).map(|m| self.point(m)).transpose()?;
        Ok(Bit { start, end, arc: Some((centre, normal, doc(mid))), radius, through })
    }

    /// A loop as corners in a plane (`k`: the two axes it is drawn in) and its arcs, `ring` of a form.
    fn ring(&self, lp: &Value, k: [usize; 2], ring: Ring, hole: usize) -> Result<Loop, String> {
        let curves = lp.as_array().ok_or("a loop is a list of curves")?;
        if curves.len() > MOST {
            return Err(format!("a loop has more than {MOST} curves"));
        }
        let (mut corners, mut bends, mut numbers) = (Vec::new(), Vec::new(), Vec::new());
        for c in curves {
            let bit = self.curve(c)?;
            let s = self.at(&bit.start)?;
            if let Some((centre, _, mid)) = &bit.arc {
                let cn = self.at(centre)?;
                let (a, m) = (P2(s[k[0]] - cn[k[0]], s[k[1]] - cn[k[1]]), P2(mid[k[0]] - cn[k[0]], mid[k[1]] - cn[k[1]]));
                bends.push(Bend {
                    ring,
                    hole,
                    edge: corners.len(),
                    centre: vec![centre[k[0]].clone(), centre[k[1]].clone()],
                    clockwise: a.cross(m) < 0.0,
                    normal: None,
                    through: bit.through.as_ref().map(|t| vec![t[k[0]].clone(), t[k[1]].clone()]).unwrap_or_default(),
                });
                // Its middle too, so a ring of arcs (a circle drawn as two halves) has an inside to test.
                numbers.push(P2(s[k[0]], s[k[1]]));
                numbers.push(P2(mid[k[0]], mid[k[1]]));
                let [a, b, c3] = bit.start;
                let p = [a, b, c3];
                corners.push([p[k[0]].clone(), p[k[1]].clone()]);
                continue;
            }
            numbers.push(P2(s[k[0]], s[k[1]]));
            let [a, b, c3] = bit.start;
            let p = [a, b, c3];
            corners.push([p[k[0]].clone(), p[k[1]].clone()]);
        }
        Ok((corners, bends, numbers))
    }
}

/// The middle of the arc from `s` to `e` about `c`, counter-clockwise about `n`.
fn middle(s: [f64; 3], e: [f64; 3], c: [f64; 3], n: [f64; 3]) -> [f64; 3] {
    use crate::geom::v3::{add, cross, mul};
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let l = dot(n, n).sqrt().max(1e-12);
    let n = mul(n, 1.0 / l);
    let u = sub(s, c);
    let v = cross(n, u);
    let w = sub(e, c);
    let mut turn = dot(w, v).atan2(dot(w, u));
    if turn <= 1e-9 {
        turn += std::f64::consts::TAU;
    }
    let (sn, cs) = (turn / 2.0).sin_cos();
    add(c, add(mul(u, cs), mul(v, sn)))
}

/// Which of the family's axes a plane square to `axis` draws in, as `Plane` lays them: (x, y) up z, (x, z) along y, (y, z)
/// along x.
fn sketch(axis: Axis) -> (Plane, [usize; 2]) {
    match axis {
        Axis::Z => (Plane::Xy, [0, 1]),
        Axis::Y => (Plane::Xz, [0, 2]),
        Axis::X => (Plane::Yz, [1, 2]),
    }
}

/// A form of `base` for each outer loop of `rings` (its profile and its arcs), the loops inside it its holes (one inside a
/// hole is an outer again).
fn nest(rings: &[Loop], base: &Form) -> Vec<Form> {
    let depth: Vec<usize> = rings
        .iter()
        .enumerate()
        .map(|(i, r)| rings.iter().enumerate().filter(|(j, o)| *j != i && r.2.first().is_some_and(|p| inside(*p, &o.2))).count())
        .collect();
    let mut out = Vec::new();
    for (i, r) in rings.iter().enumerate().filter(|(i, _)| depth[*i].is_multiple_of(2)) {
        let mut form = Form { profile: r.0.clone(), arcs: r.1.clone(), ..base.clone() };
        for (j, h) in rings.iter().enumerate() {
            if depth[j] == depth[i] + 1 && h.2.first().is_some_and(|p| inside(*p, &r.2)) {
                let hole = form.holes.len();
                form.arcs.extend(h.1.iter().cloned().map(|b| Bend { ring: Ring::Hole, hole, ..b }));
                form.holes.push(h.0.clone());
            }
        }
        out.push(form);
    }
    out
}

/// A loop of a profile in 3D: its corners, its arcs (as edges of a ring, `hole` 0 for none), and points round it in the first
/// type (its corners and its arcs' middles), what says which loop is inside which.
type Loop3 = (Vec<[Expr; 3]>, Vec<Bend>, Vec<[f64; 3]>);

/// Sweeps along `points` (with the path's `arcs`) of each outer loop of `starts`, the loops inside it its holes (a tube's
/// inside), one inside a hole an outer again; a swept blend's loops at its end in `ends`, as many.
fn swept(points: &[[Expr; 3]], arcs: &[Bend], starts: &[Loop3], ends: &[Loop3], base: &Form) -> Vec<Form> {
    let flat = profile_plane(starts.iter().map(|l| l.2.as_slice()));
    let rings: Vec<Vec<P2>> = starts.iter().map(|l| l.2.iter().map(|p| flat(*p)).collect()).collect();
    let depth: Vec<usize> = (0..rings.len())
        .map(|i| (0..rings.len()).filter(|j| *j != i && rings[i].first().is_some_and(|p| inside(*p, &rings[*j]))).count())
        .collect();
    let mut out = Vec::new();
    for i in (0..rings.len()).filter(|i| depth[*i].is_multiple_of(2)) {
        let mut arcs = arcs.to_vec();
        arcs.extend(starts[i].1.iter().cloned());
        let (mut holes, mut end_holes) = (Vec::new(), Vec::new());
        for j in (0..rings.len()).filter(|j| depth[*j] == depth[i] + 1 && rings[*j].first().is_some_and(|p| inside(*p, &rings[i]))) {
            let hole = holes.len() + 1;
            arcs.extend(starts[j].1.iter().cloned().map(|b| Bend { hole, ..b }));
            holes.push(starts[j].0.clone());
            if let Some(e) = ends.get(j) {
                arcs.extend(e.1.iter().cloned().map(|b| Bend { hole, ..b }));
                end_holes.push(e.0.clone());
            }
        }
        let section_end = match ends.get(i) {
            Some(e) => {
                arcs.extend(e.1.iter().cloned());
                e.0.clone()
            }
            None => Vec::new(),
        };
        out.push(Form {
            path: points.to_vec(),
            section: starts[i].0.clone(),
            section_end,
            section_holes: holes,
            section_end_holes: end_holes,
            arcs,
            ..base.clone()
        });
    }
    out
}

/// `e` times `k`, a number where it is.
fn times(e: &Expr, k: f64) -> Expr {
    match e {
        Expr::Number(x) => Expr::Number(x * k + 0.0),
        _ if k == 1.0 => e.clone(),
        Expr::Formula(f) => Expr::Formula(format!("{} * ({f})", plain(k))),
    }
}

impl Reader<'_> {
    /// A loop of curves in 3D as a ring of `ring` ([`Loop3`]).
    fn loop3(&self, lp: &Value, ring: Ring) -> Result<Loop3, String> {
        let (mut out, mut arcs, mut round) = (Vec::new(), Vec::new(), Vec::new());
        for c in lp.as_array().into_iter().flatten().take(MOST) {
            let bit = self.curve(c)?;
            round.push(self.at(&bit.start)?);
            if let Some((centre, normal, mid)) = bit.arc {
                round.push(mid);
                arcs.push(Bend {
                    ring,
                    edge: out.len(),
                    centre: centre.to_vec(),
                    clockwise: self.map.mirrors(),
                    normal: Some(normal),
                    ..Bend::default()
                });
            }
            out.push(bit.start);
        }
        Ok((out, arcs, round))
    }

    /// An extrusion on a sloped sketch plane (a chair's back): its loops swept straight along its normal from its start to
    /// its end, a sweep of one leg. Its loops' corners follow what they are bound to, its ends what they are; the plane
    /// itself stays where it is in the family. A blend on one isn't drawn yet.
    fn sloped(&self, def: &Value, kind: &str, base: &Form) -> Result<Vec<Form>, String> {
        if kind != "extrusion" {
            return Err("it is a blend on a sloped plane, which BIMCraft can't draw yet".into());
        }
        let n: Vec<f64> = def["normal"].as_array().map(|v| v.iter().filter_map(num).collect()).unwrap_or_default();
        let [nx, ny, nz] = n.as_slice() else { return Err("it says no normal".into()) };
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        if len.is_nan() || len <= 1e-9 {
            return Err("it says no normal".into());
        }
        // Its normal in the family's axes, and the family's origin along it in the document's.
        let nf = self.map.turn([nx / len, ny / len, nz / len]);
        let z0 = self.first_number(&self.map.z_minus).unwrap_or(0.0);
        let o = [self.map.x_offset, self.map.y_offset, -z0];
        let shift = crate::geom::v3::dot(o, nf);
        let along = |key: &str| -> Result<Expr, String> { Ok(plus(self.value(&def[key])?, shift)) };
        let (start, end) = (along("start")?, along("end")?);
        let starts = loops_of(def, "loops")?.into_iter().map(|l| self.loop3(l, Ring::Section)).collect::<Result<Vec<_>, _>>()?;
        // The path: through the first corner as the first type has it, from its start to its end along the normal.
        let first = *starts.first().and_then(|l| l.2.first()).ok_or("its sketch has no loops")?;
        let level = crate::geom::v3::dot(first, nf);
        let at = |t: &Expr| -> [Expr; 3] { std::array::from_fn(|k| plus(times(t, nf[k]), first[k] - nf[k] * level)) };
        let points = [at(&start), at(&end)];
        if (self.first_number(&start)? - self.first_number(&end)?).abs() < 1e-6 {
            return Err("its start and its end are one".into());
        }
        Ok(swept(&points, &[], &starts, &[], base))
    }
}

/// Points in a plane (a sweep's profile) laid flat in it: across the plane of the loop of most points, its normal by
/// Newell's sum.
fn profile_plane<'p>(loops: impl Iterator<Item = &'p [[f64; 3]]>) -> impl Fn([f64; 3]) -> P2 {
    use crate::geom::v3::{cross, dot};
    let biggest = loops.max_by_key(|l| l.len()).unwrap_or_default();
    let mut n = [0.0; 3];
    for (i, a) in biggest.iter().enumerate() {
        let b = biggest[(i + 1) % biggest.len()];
        n = [n[0] + (a[1] - b[1]) * (a[2] + b[2]), n[1] + (a[2] - b[2]) * (a[0] + b[0]), n[2] + (a[0] - b[0]) * (a[1] + b[1])];
    }
    let len = dot(n, n).sqrt();
    let n = if len > 1e-9 { n.map(|x| x / len) } else { [0.0, 0.0, 1.0] };
    // Any direction across it, and the one square to that in it.
    let seed = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = cross(n, seed);
    let ul = dot(u, u).sqrt().max(1e-12);
    let u = u.map(|x| x / ul);
    let v = cross(n, u);
    move |p: [f64; 3]| P2(dot(p, u), dot(p, v))
}

/// Whether point `p` is inside ring `r`.
fn inside(p: P2, r: &[P2]) -> bool {
    let mut odd = false;
    for (a, b) in crate::geom::ring_edges(r) {
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            odd = !odd;
        }
    }
    odd
}

/// What one form of the definition says beside its shape: void, material, visibility, subcategory.
fn dressed(f: &Family, def: &Value, has_material: &dyn Fn(&str) -> bool) -> Form {
    let mut form = Form { void: def["void"].as_bool().unwrap_or(false), ..Form::default() };
    if let Some(p) = def.get("material_param").and_then(text).filter(|p| f.material_parameter(p)) {
        form.material = p.to_string();
    } else if let Some(m) = def.get("material").and_then(text).filter(|m| has_material(m)) {
        form.material = m.to_string();
    }
    if let Some(v) = def.get("visible").and_then(text) {
        form.visible = Some(Expr::Formula(v.to_string()));
    }
    // A form of Revit's that shows everything says so (it is drawn at Coarse, in every view, as Revit draws it).
    if let Some(s) = def.get("show").and_then(|s| serde_json::from_value::<Show>(s.clone()).ok()) {
        form.show = Some(s);
    }
    form.subcategory = def.get("subcategory").and_then(text).filter(|s| f.subcategories.iter().any(|x| x.name == *s)).map(str::to_string);
    form
}

impl Reader<'_> {
    /// The plane a form's sketch is in, from its normal: its axis, and whether it faces the other way.
    fn forms(&self, def: &Value, has_material: &dyn Fn(&str) -> bool) -> Result<Vec<Form>, String> {
        let kind = def["kind"].as_str().unwrap_or("");
        let base = dressed(self.family, def, has_material);
        match kind {
            "extrusion" | "blend" => {
                let Some((axis, _)) = axis_of(&def["normal"]) else { return self.sloped(def, kind, &base) };
                let (plane, k) = sketch(axis);
                let (start, end) = (self.coord(&def["start"], axis)?, self.coord(&def["end"], axis)?);
                // A blend's bottom profile is at its start and its top at its end, whichever is higher (Revit extrudes one
                // against its sketch plane's normal, from a ceiling down): `from` and `to` keep that, and the blend is
                // built so in every type.
                if kind == "blend" {
                    let one =
                        |key: &str| def[key].as_array().and_then(|l| l.first()).cloned().ok_or(format!("its {key} profile is missing"));
                    let (bottom, top) = (one("bottom")?, one("top")?);
                    let (profile, mut arcs, _) = self.ring(&bottom, k, Ring::Profile, 0)?;
                    let (top, top_arcs, _) = self.ring(&top, k, Ring::Top, 0)?;
                    arcs.extend(top_arcs);
                    return Ok(vec![Form { plane, profile, top, arcs, from: start, to: end, ..base }]);
                }
                let order = |e: &Expr| self.first_number(e);
                let (from, to) = if order(&start)? <= order(&end)? { (start, end) } else { (end, start) };
                // An extrusion runs between its two ends whichever is lower in a type.
                let text = |e: &Expr| match e {
                    Expr::Number(x) => plain(*x),
                    Expr::Formula(f) => f.clone(),
                };
                let (from, to) = match (&from, &to) {
                    (Expr::Number(_), Expr::Number(_)) => (from, to),
                    _ => (
                        Expr::Formula(format!("min({}, {})", text(&from), text(&to))),
                        Expr::Formula(format!("max({}, {})", text(&from), text(&to))),
                    ),
                };
                let loops = loops_of(def, "loops")?;
                let rings = loops.iter().map(|l| self.ring(l, k, Ring::Profile, 0)).collect::<Result<Vec<_>, _>>()?;
                let base = Form { plane, from, to, ..base };
                Ok(nest(&rings, &base))
            }
            "revolution" => {
                let loops = loops_of(def, "loops")?;
                let axis = def["axis"].as_array().filter(|a| a.len() == 2).ok_or("it says no axis")?;
                let (a0, a1) = (self.point(&axis[0])?, self.point(&axis[1])?);
                // Its plane: the axis whose coordinate every corner and the axis share.
                let mut pts = vec![self.at(&a0)?, self.at(&a1)?];
                for c in loops[0].as_array().into_iter().flatten() {
                    pts.push(self.at(&self.curve(c)?.start)?);
                }
                let flat = Axis::ALL
                    .into_iter()
                    .find(|ax| pts.iter().all(|p| (p[ax.index()] - pts[0][ax.index()]).abs() < 0.05))
                    .ok_or("its sketch is on a sloped plane")?;
                let (plane, k) = sketch(flat);
                // Where its sketch plane is: the axis' first point's coordinate square to it.
                let at = Some(a0[flat.index()].clone()).filter(|e| !matches!(e, Expr::Number(x) if *x == 0.0));
                // Each outer loop a form, the loops inside it its holes.
                let rings = loops.iter().map(|l| self.ring(l, k, Ring::Profile, 0)).collect::<Result<Vec<_>, _>>()?;
                let (mut from, mut to) = (self.value(&def["start_angle"])?, self.value(&def["end_angle"])?);
                // Mirrored, it turns the other way about its axis.
                if self.map.mirrors() {
                    (from, to) = (neg(to), neg(from));
                }
                let axis = Some([[a0[k[0]].clone(), a0[k[1]].clone()], [a1[k[0]].clone(), a1[k[1]].clone()]]);
                let base = Form { plane, axis, from, to, at, ..base };
                Ok(nest(&rings, &base))
            }
            "sweep" | "swept_blend" => {
                let path = def["path"].as_array().filter(|p| !p.is_empty() && p.len() <= MOST).ok_or("it has no path")?;
                let mut points = Vec::new();
                let mut arcs = Vec::new();
                let mut last = None;
                for c in path {
                    let bit = self.curve(c)?;
                    if let Some((centre, normal, _)) = bit.arc {
                        arcs.push(Bend {
                            ring: Ring::Path,
                            edge: points.len(),
                            centre: centre.to_vec(),
                            clockwise: self.map.mirrors(),
                            normal: Some(normal),
                            ..Bend::default()
                        });
                    }
                    points.push(bit.start);
                    last = Some(bit.end);
                }
                points.extend(last);
                let count = def["profile"].as_array().map_or(1, Vec::len).clamp(1, MOST);
                let section = |key: &str, n: usize, ring: Ring| -> Result<Loop3, String> {
                    let lp = def[key].as_array().and_then(|l| l.get(n)).filter(|l| l.as_array().is_some_and(|c| !c.is_empty()));
                    self.loop3(lp.ok_or(format!("its {key} is missing (a profile family's?)"))?, ring)
                };
                let starts = (0..count).map(|n| section("profile", n, Ring::Section)).collect::<Result<Vec<_>, _>>()?;
                let ends = if kind == "swept_blend" {
                    (0..count).map(|n| section("profile_end", n, Ring::SectionEnd)).collect::<Result<Vec<_>, _>>()?
                } else {
                    Vec::new()
                };
                let out = swept(&points, &arcs, &starts, &ends, &base);
                Ok(out)
            }
            "" => Err("it says no kind".into()),
            k => Err(format!("it is a kind of form BIMCraft doesn't have yet ({k})")),
        }
    }

    /// `e`'s number in the first type.
    fn first_number(&self, e: &Expr) -> Result<f64, String> {
        number(e, &self.values, &mut HashMap::new())
    }

    /// A symbolic line's curves as symbols in its view; a model line's in each of plan, front and side its Visibility
    /// Settings draw it in (Revit draws a model line in every view that sees it, seen flat in that view).
    fn lines(&self, def: &Value) -> Result<Vec<Symbol>, String> {
        // A line drawn in Revit's invisible style draws nothing, in any view.
        if def["invisible"].as_bool() == Some(true) || def["subcategory"].as_str() == Some("<Invisible lines>") {
            return Ok(Vec::new());
        }
        // A masking region's edge is drawn as its Visibility Settings say, as a line's.
        let show = def.get("show").and_then(|s| serde_json::from_value::<Show>(s.clone()).ok());
        // One the exporter called a plan's but that stands in the family's yz or xz plane is a side's or a front's.
        let view = match def["view"].as_str().unwrap_or("plan") {
            "plan" => self.stands_in(def).unwrap_or("plan"),
            v => v,
        };
        let views = match view {
            "plan" => vec![SymbolView::Plan],
            "front" => vec![SymbolView::Front],
            "side" => vec![SymbolView::Side],
            "model" => {
                let s = show.unwrap_or_default();
                [(s.plan, SymbolView::Plan), (s.front, SymbolView::Front), (s.side, SymbolView::Side)]
                    .into_iter()
                    .filter_map(|(on, v)| on.then_some(v))
                    .collect()
            }
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for view in views {
            out.extend(self.lines_in(def, view, show)?);
        }
        Ok(out)
    }

    /// The view a line drawn as a plan's is really in, where all its points (the ends of its curves, in the first type) share
    /// their x (`side`) or their y (`front`) while their z varies: a region drawn on a plane standing in the family, which a
    /// plan would draw as a stroke across it. None where it lies flat or isn't on such a plane.
    fn stands_in(&self, def: &Value) -> Option<&'static str> {
        let curves = def["curves"].as_array().filter(|c| !c.is_empty() && c.len() <= MOST)?;
        let mut points = Vec::new();
        for c in curves {
            let bit = self.curve(c).ok()?;
            points.push(self.at(&bit.start).ok()?);
            points.push(self.at(&bit.end).ok()?);
        }
        let spread = |k: usize| {
            let (lo, hi) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[k]), hi.max(p[k])));
            hi - lo
        };
        if spread(2) <= 1.0 {
            None
        } else if spread(0) <= 0.05 {
            Some("side")
        } else if spread(1) <= 0.05 {
            Some("front")
        } else {
            None
        }
    }

    /// A line's curves as symbols in `view`.
    fn lines_in(&self, def: &Value, view: SymbolView, show: Option<Show>) -> Result<Vec<Symbol>, String> {
        let k = match view {
            SymbolView::Plan => [0, 1],
            SymbolView::Front => [0, 2],
            SymbolView::Side => [1, 2],
        };
        let levels = show.map(|s| Levels { coarse: s.coarse, medium: s.medium, fine: s.fine });
        let base = Symbol {
            view,
            show: levels,
            visible: def.get("visible").and_then(text).map(|v| Expr::Formula(v.to_string())),
            subcategory: def
                .get("subcategory")
                .and_then(text)
                .filter(|s| self.family.subcategories.iter().any(|x| x.name == *s))
                .map(str::to_string),
            ..Symbol::default()
        };
        let curves = def["curves"].as_array().filter(|c| c.len() <= MOST).ok_or("it has no curves")?;
        let mut out = Vec::new();
        for c in curves {
            let bit = self.curve(c)?;
            match &bit.arc {
                None => out.push(Symbol {
                    line: vec![[bit.start[k[0]].clone(), bit.start[k[1]].clone()], [bit.end[k[0]].clone(), bit.end[k[1]].clone()]],
                    ..base.clone()
                }),
                Some((centre, normal, _)) if normal[3 - k[0] - k[1]].abs() < 0.99999 * crate::geom::v3::dot(*normal, *normal).sqrt() => {
                    // An arc not flat in the view (a model line's seen edge-on or slanted): its points as the first type has
                    // them, seen flat.
                    let (s, e, cn) = (self.at(&bit.start)?, self.at(&bit.end)?, self.at(centre)?);
                    let mut pts = vec![s];
                    let n = if self.map.mirrors() { normal.map(|x| -x) } else { *normal };
                    pts.extend(super::shapes::arc3(s, e, cn, n, false));
                    pts.push(e);
                    let line = pts.iter().map(|p| [Expr::Number(p[k[0]]), Expr::Number(p[k[1]])]).collect();
                    out.push(Symbol { line, ..base.clone() });
                }
                Some((centre, _, mid)) => {
                    let (s, e, cn) = (self.at(&bit.start)?, self.at(&bit.end)?, self.at(centre)?);
                    let angle = |p: [f64; 3]| (p[k[1]] - cn[k[1]]).atan2(p[k[0]] - cn[k[0]]).to_degrees();
                    let (a0, a1, am) = (angle(s), angle(e), angle(*mid));
                    let ccw = P2(s[k[0]] - cn[k[0]], s[k[1]] - cn[k[1]]).cross(P2(mid[k[0]] - cn[k[0]], mid[k[1]] - cn[k[1]])) >= 0.0;
                    let _ = am;
                    let whole = (s[k[0]] - e[k[0]]).hypot(s[k[1]] - e[k[1]]) < 1e-6;
                    let to = if whole {
                        if ccw { a0 + 360.0 } else { a0 - 360.0 }
                    } else if ccw {
                        a0 + (a1 - a0).rem_euclid(360.0)
                    } else {
                        a0 - (a0 - a1).rem_euclid(360.0)
                    };
                    let radius =
                        bit.radius.clone().unwrap_or(Expr::Number(((s[k[0]] - cn[k[0]]).hypot(s[k[1]] - cn[k[1]]) * 10.0).round() / 10.0));
                    out.push(Symbol {
                        arc: Some(Curve {
                            centre: [centre[k[0]].clone(), centre[k[1]].clone()],
                            radius,
                            from: Expr::Number(a0),
                            to: Expr::Number(to),
                        }),
                        ..base.clone()
                    });
                }
            }
        }
        Ok(out)
    }
}

/// Each plane by its id: its name, its axis, its coordinate per type in the document's axes.
type Planes = BTreeMap<String, (String, Axis, BTreeMap<String, f64>)>;

/// Where plane `target` (square to z) stands above the family document's level, as a formula over the family's parameters
/// that holds in every type: the chain of dimensions from the level (z = 0, or a plane at 0 in every type) to it, each
/// labelled (its parameter) or the same in every type (its value). A window's sill plane placed by its head height less
/// its height, which no parameter says on its own. None where no such chain holds.
fn from_level(def: &Value, planes: &Planes, target: &str, f: &Family, params: &BTreeSet<String>, walls: &Walls) -> Option<String> {
    let types: Vec<String> = f.types.keys().cloned().collect();
    let first = types.first()?.clone();
    const LEVEL: &str = "\u{0}level";
    let pos = |id: &str, t: &str| -> Option<f64> {
        if id == LEVEL {
            return Some(0.0);
        }
        planes.get(id).filter(|p| p.1 == Axis::Z).and_then(|p| p.2.get(t).copied())
    };
    let level_like = |id: &str| types.iter().all(|t| pos(id, t).is_some_and(|x| x.abs() < 0.05));
    // Edges: (a, b, term, sign) with b = a + sign × term in every type.
    let mut edges: Vec<(String, String, String)> = Vec::new();
    for d in def["dimensions"].as_array().into_iter().flatten().take(super::MAX_DIMENSIONS) {
        let ids: Vec<String> =
            d["refs"].as_array().into_iter().flatten().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).collect();
        let [a, b] = ids.as_slice() else { continue };
        let value = |t: &str| d["value"].get(t).and_then(num);
        // An end that isn't a plane is the level where the dimension says the other end is that far above it.
        let end = |x: &str, other: &str| -> Option<String> {
            if pos(x, &first).is_some() {
                return Some(if level_like(x) { LEVEL.to_string() } else { x.to_string() });
            }
            let fits = types.iter().all(|t| matches!((pos(other, t), value(t)), (Some(o), Some(v)) if (o.abs() - v.abs()).abs() < 0.1));
            (pos(other, &first).is_some() && fits).then(|| LEVEL.to_string())
        };
        let (Some(a), Some(b)) = (end(a, b), end(b, a)) else { continue };
        if a == b {
            continue;
        }
        let label = d.get("label").and_then(text).filter(|l| params.contains(*l));
        let constant = types.iter().map(|t| value(t)).collect::<Option<Vec<f64>>>().filter(|v| v.iter().all(|x| (x - v[0]).abs() < 0.05));
        let term = match (label, constant) {
            (Some(l), _) => l.to_string(),
            (None, Some(v)) if !v.is_empty() => plain(v[0].abs()),
            _ => continue,
        };
        // Which way: the same in every type.
        let signs: Option<Vec<f64>> = types.iter().map(|t| Some((pos(&b, t)? - pos(&a, t)?).signum())).collect();
        let Some(signs) = signs.filter(|s| !s.is_empty() && s.iter().all(|x| *x == s[0] && *x != 0.0)) else { continue };
        let (a, b) = if signs[0] > 0.0 { (a, b) } else { (b, a) };
        edges.push((a, b, term));
    }
    // From the level outward, breadth first.
    let mut found: BTreeMap<String, String> = BTreeMap::from([(LEVEL.to_string(), String::new())]);
    let mut queue = std::collections::VecDeque::from([LEVEL.to_string()]);
    while let Some(at) = queue.pop_front() {
        let here = found.get(&at).cloned().unwrap_or_default();
        for (a, b, term) in &edges {
            let (next, step) = if *a == at {
                (b, format!(" + {term}"))
            } else if *b == at {
                (a, format!(" - {term}"))
            } else {
                continue;
            };
            if !found.contains_key(next) && found.len() < super::MAX_REFERENCES {
                found.insert(next.clone(), format!("{here}{step}"));
                queue.push_back(next.clone());
            }
        }
    }
    let chain = found.get(target)?;
    let formula = chain.trim_start_matches(" + ").to_string();
    let formula = if chain.starts_with(" - ") { format!("0{chain}") } else { formula };
    // It holds in every type, or it isn't it.
    let holds = types.iter().all(|t| {
        let Ok(values) = vals(f, t, walls) else { return false };
        let e = Expr::Formula(formula.clone());
        matches!((number(&e, &values, &mut HashMap::new()), pos(target, t)), (Ok(x), Some(y)) if (x - y).abs() < 0.05)
    });
    holds.then_some(formula)
}

/// `def` (Revit's definition of family `f`) translated into it: its references, dimensions, parametric forms and symbols,
/// nested parts and type catalog, beside the forms and symbols it has (Revit's result, which become `snapshot` ones). What
/// can't be translated is left out and said in `editable.untranslated`; materials `has_material` doesn't know are left off.
pub fn translate(f: &mut Family, def: &Value, has_material: &dyn Fn(&str) -> bool) -> Walls {
    for x in f.forms.iter_mut() {
        x.snapshot = true;
        // Revit's result says where it is drawn. A form its plan didn't draw is drawn neither cut nor below the cut: the
        // exporter writes no `plan_top` and `plan_cut` for it, which would default to drawing it below the cut.
        let s = x.show.get_or_insert(Show::default());
        if !s.plan {
            s.plan_top = false;
            s.plan_cut = false;
        }
    }
    for y in f.symbols.iter_mut() {
        y.snapshot = true;
    }
    f.references.clear();
    f.dimensions.clear();
    f.nested.clear();
    f.check.clear();
    let mut notes = Vec::new();
    if !def.is_object() {
        f.editable =
            Some(super::Editable { ok: false, reason: None, untranslated: vec!["its definition isn't one BIMCraft can read".into()] });
        return Walls::new();
    }
    let to = &def["to_bimcraft"];
    let params: BTreeSet<String> = f
        .parameters
        .iter()
        .filter(|p| matches!(p.kind, Kind::Length | Kind::Number | Kind::Angle | Kind::YesNo))
        .map(|p| p.name.clone())
        .collect();
    let first = f.types.keys().next().cloned().unwrap_or_default();
    let mut planes = BTreeMap::new();
    let mut used = BTreeSet::new();
    let mut refs = Vec::new();
    for (i, p) in def["reference_planes"].as_array().into_iter().flatten().take(super::MAX_REFERENCES).enumerate() {
        let Some(id) = p.get("id").map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)) else { continue };
        // A sloped plane stays where it is: what is sketched on it is placed by its own numbers (`Reader::sloped`), and
        // what follows it in a dimension or a binding is said where it is read.
        let Some((axis, _)) = axis_of(&p["normal"]) else { continue };
        let wanted = p.get("name").and_then(text).unwrap_or("").to_string();
        let mut name = if nameable(&wanted) { wanted } else { format!("Reference {}", i + 1) };
        let stem = name.clone();
        for n in 2.. {
            if used.insert(name.clone()) {
                break;
            }
            name = format!("{stem} {n}");
        }
        let at: BTreeMap<String, f64> = p["at"].as_object().into_iter().flatten().filter_map(|(t, v)| Some((t.clone(), num(v)?))).collect();
        refs.push((id.clone(), name.clone(), axis, p.clone()));
        planes.insert(id, (name, axis, at));
    }
    let map = Map {
        x_offset: num(&to["x_offset"]).unwrap_or(0.0),
        x_sign: if num(&to["x_sign"]).is_some_and(|s| s < 0.0) { -1.0 } else { 1.0 },
        y_sign: if num(&to["y_sign"]).is_some_and(|s| s < 0.0) { -1.0 } else { 1.0 },
        y_offset: num(&to["y_offset"]).unwrap_or(0.0),
        z_minus: Expr::Number(0.0),
    };
    // The formula that places the sill plane above the level, where no parameter says it alone.
    let mut sill_from: Option<String> = None;
    // A door's or a window's wall Revit's result was made in: the exporter places each type in a wall of its own and says how
    // thick (`to_bimcraft.wall`, mm, one for all or per type); one that says nothing made it in a 200 mm wall, centred on
    // the wall. Not the family document's template wall, whose faces only say what follows them.
    let walls: Walls = if f.category.hosted() {
        let said = &to["wall"];
        let fits = |w: f64| (1.0..=crate::model::MAX_COORD).contains(&w);
        f.types
            .keys()
            .map(|t| {
                let w = num(said).or_else(|| said.get(t).and_then(num)).filter(|w| fits(*w)).unwrap_or(super::DEFAULT_WALL);
                (t.clone(), w)
            })
            .collect()
    } else {
        Walls::new()
    };
    let mut reader =
        Reader { map, planes, params, first: first.clone(), notes: Vec::new(), family: f, values: BTreeMap::new(), walls: walls.clone() };
    if let Some(z) = to.get("z_minus").filter(|z| !z.is_null()) {
        // A plane (a window's sill): the parameter that puts it where it is in every type, else where it is in the first.
        let plane = z.get("ref").and_then(Value::as_str).and_then(|id| reader.planes.get(id)).map(|p| p.2.clone());
        let moving = plane.as_ref().is_some_and(|at| at.values().any(|x| (x - at.values().next().copied().unwrap_or(0.0)).abs() > 0.05));
        let follows = plane.as_ref().filter(|_| moving).and_then(|at| {
            let typed: Vec<(String, BTreeMap<String, Val>)> =
                f.types.keys().filter_map(|t| Some((t.clone(), vals(f, t, &reader.walls).ok()?))).collect();
            reader.params.iter().find(|p| {
                typed.iter().all(|(t, v)| match (v.get(*p), at.get(t)) {
                    (Some(Val::Number(x)), Some(y)) => (x - y).abs() < 0.05,
                    _ => false,
                })
            })
        });
        // Else the chain of dimensions that places it above the level (a head height less the height).
        let chain = match (&follows, z.get("ref").and_then(Value::as_str)) {
            (None, Some(id)) if moving => from_level(def, &reader.planes, id, f, &reader.params, &reader.walls),
            _ => None,
        };
        match (follows, chain, reader.value(z)) {
            (Some(p), _, _) => reader.map.z_minus = Expr::Formula(p.clone()),
            (None, Some(c), _) => {
                reader.map.z_minus = Expr::Formula(format!("({c})"));
                sill_from = Some(c);
            }
            (None, None, Ok(e)) => reader.map.z_minus = e,
            (None, None, Err(e)) => notes.push(format!("its height above its level is set by a rule BIMCraft can't follow yet: {e}")),
        }
    }
    reader.first_values();
    // References: where each stands in the first type, taken into the family's axes; the origin's at the origin.
    let mut references = Vec::new();
    let base_plane = to.get("z_minus").and_then(|z| z.get("ref")).and_then(Value::as_str).map(str::to_string);
    for (id, name, axis, p) in &refs {
        // The plane z is measured from (a window's sill): the family's z = 0, which stays.
        if base_plane.as_deref() == Some(id.as_str()) && *axis == Axis::Z {
            references.push(Reference {
                name: name.clone(),
                axis: *axis,
                at: Expr::Number(0.0),
                is_reference: p.get("is_reference").and_then(text).map(str::to_string),
                origin: true,
                ..Reference::default()
            });
            continue;
        }
        // A wall's faces (a door's or a window's host) follow the wall it stands in: half its thickness out each way.
        let kind = p.get("is_reference").and_then(Value::as_str).unwrap_or("");
        if f.category.hosted() && *axis == Axis::Y && (kind == "host_exterior" || kind == "host_interior") {
            // The family's +y is toward the exterior, whichever way the document had it.
            let side = if kind == "host_exterior" { "" } else { "-" };
            let half = reader.walls.get(&first).copied().unwrap_or(super::DEFAULT_WALL) / 2.0;
            reader.values.insert(super::ref_key(name), Val::Number(if side.is_empty() { half } else { -half }));
            references.push(Reference {
                name: name.clone(),
                axis: *axis,
                at: Expr::Formula(format!("{side}{} / 2", super::WALL)),
                is_reference: Some(kind.to_string()),
                ..Reference::default()
            });
            continue;
        }
        // A plane on a host face in every type (a window's Exterior) is on it.
        if f.category.hosted() && *axis == Axis::Y {
            let at = &reader.planes.get(id.as_str()).map(|x| x.2.clone()).unwrap_or_default();
            let face = |k: &str| {
                refs.iter()
                    .find(|r| r.3.get("is_reference").and_then(Value::as_str) == Some(k))
                    .and_then(|r| reader.planes.get(&r.0))
                    .map(|p| &p.2)
            };
            let on =
                |k: &str| face(k).is_some_and(|f| !at.is_empty() && at.iter().all(|(t, x)| f.get(t).is_some_and(|y| (x - y).abs() < 0.05)));
            let kind = if on("host_exterior") {
                Some("host_exterior")
            } else if on("host_interior") {
                Some("host_interior")
            } else {
                None
            };
            if let Some(kind) = kind {
                let half = reader.walls.get(&first).copied().unwrap_or(super::DEFAULT_WALL) / 2.0;
                let (side, x) = if kind == "host_exterior" { ("", half) } else { ("-", -half) };
                reader.values.insert(super::ref_key(name), Val::Number(x));
                references.push(Reference {
                    name: name.clone(),
                    axis: *axis,
                    at: Expr::Formula(format!("{side}{} / 2", super::WALL)),
                    is_reference: Some(kind.to_string()),
                    ..Reference::default()
                });
                continue;
            }
        }
        let doc = {
            let at = &reader.planes.values().find(|(n, _, _)| n == name).map(|x| x.2.clone()).unwrap_or_default();
            Expr::Number(at.get(&first).or_else(|| at.values().next()).copied().unwrap_or(0.0))
        };
        references.push(Reference {
            name: name.clone(),
            axis: *axis,
            at: reader.place(doc, *axis),
            is_reference: p.get("is_reference").and_then(text).map(str::to_string),
            origin: p["defines_origin"].as_bool().unwrap_or(false),
            ..Reference::default()
        });
    }
    // Where an axis has no plane defining its origin, the plane at 0 in every type is it (a level's plane): what Revit
    // keeps still. And the planes that never move go last, so solving keeps them where they are and moves the others.
    let moves = |p: &Value| {
        let at: Vec<f64> = p["at"].as_object().into_iter().flatten().filter_map(|(_, v)| num(v)).collect();
        at.iter().any(|x| (x - at.first().copied().unwrap_or(0.0)).abs() > 0.05)
    };
    for axis in Axis::ALL {
        if references.iter().any(|r| r.axis == axis && r.origin) {
            continue;
        }
        let at_zero = |r: &&(String, String, Axis, Value)| {
            r.2 == axis && !moves(&r.3) && r.3["at"].as_object().into_iter().flatten().all(|(_, v)| num(v).is_some_and(|x| x.abs() < 0.05))
        };
        if let Some(found) = refs.iter().find(at_zero).map(|r| r.1.clone())
            && let Some(r) =
                references.iter_mut().find(|r| r.name == found && r.is_reference.as_deref().is_none_or(|k| !k.starts_with("host_")))
        {
            r.origin = true;
        }
    }
    let still: BTreeSet<String> = refs.iter().filter(|r| !moves(&r.3)).map(|r| r.1.clone()).collect();
    // Of those, the ones standing on their axis' origin plane in every type (a desk's base on its level) go last of all: solving
    // keeps the later planes where they stand and moves the other end of a dimension (Revit flexes a desk taller upward).
    let on_origin: BTreeSet<String> = references
        .iter()
        .map(|r| r.name.clone())
        .filter(|name| {
            let Some(r) = refs.iter().find(|r| r.1 == *name) else { return false };
            let origin = references.iter().find(|q| q.axis == r.2 && q.origin).and_then(|q| refs.iter().find(|x| x.1 == q.name));
            let (Some(at), Some(origin)) = (r.3["at"].as_object(), origin.and_then(|o| o.3["at"].as_object())) else { return false };
            !at.is_empty()
                && at.iter().all(|(t, v)| matches!((num(v), origin.get(t).and_then(num)), (Some(x), Some(y)) if (x - y).abs() < 0.05))
        })
        .collect();
    references.sort_by_key(|r| (still.contains(&r.name), on_origin.contains(&r.name)));
    // What a dimension or an alignment measures to that isn't one of the planes: the host wall (its face nearer the other
    // end) and the level (z = 0, a plane of its own).
    let ids_of = |v: &Value| -> Vec<String> {
        v["refs"].as_array().into_iter().flatten().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).collect()
    };
    let lists: Vec<Vec<String>> = def["dimensions"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(def["constraints"].as_array().into_iter().flatten())
        .take(super::MAX_DIMENSIONS * 2)
        .map(ids_of)
        .collect();
    let level_wanted = lists.iter().any(|ids| {
        ids.iter().any(|i| !reader.planes.contains_key(i))
            && ids.iter().filter_map(|i| reader.planes.get(i)).all(|p| p.1 == Axis::Z)
            && ids.iter().any(|i| reader.planes.contains_key(i))
    });
    // The level a dimension measures to is the plane the family's z is measured from where that stands at the level (a desk's
    // Ground floor): not a second plane.
    let base = base_plane.as_deref().and_then(|b| refs.iter().find(|r| r.0 == b && r.2 == Axis::Z));
    let base_on_level = base.is_some_and(|r| {
        r.3["at"].as_object().is_some_and(|at| !at.is_empty() && at.values().all(|v| num(v).is_some_and(|x| x.abs() < 0.05)))
    });
    if level_wanted
        && base_on_level
        && let Some(base) = base
    {
        let at: BTreeMap<String, f64> = f.types.keys().map(|t| (t.clone(), 0.0)).collect();
        reader.planes.insert("__level".into(), (base.1.clone(), Axis::Z, at));
    } else if level_wanted && references.len() < super::MAX_REFERENCES {
        let mut name = "Ref Level".to_string();
        for n in 2.. {
            if !references.iter().any(|r| r.name == name) {
                break;
            }
            name = format!("Ref Level {n}");
        }
        let at: BTreeMap<String, f64> = f.types.keys().map(|t| (t.clone(), 0.0)).collect();
        let origin = !references.iter().any(|r| r.axis == Axis::Z && r.origin);
        references.push(Reference {
            name: name.clone(),
            axis: Axis::Z,
            at: reader.place(Expr::Number(0.0), Axis::Z),
            origin,
            ..Reference::default()
        });
        reader.planes.insert("__level".into(), (name, Axis::Z, at));
    }
    // `ids` with the one end that isn't a plane read as the level or a host wall's face, where the dimension's `value` (0 for
    // an alignment) says it is that far from the plane at the other end; else left as it is (and the dimension left out).
    let resolve = |ids: &[String], value: Option<f64>| -> Vec<String> {
        let first_at = |id: &str| reader.planes.get(id).and_then(|p| p.2.get(&first).or_else(|| p.2.values().next()).copied());
        let other = match ids {
            [a, b] if reader.planes.contains_key(a) != reader.planes.contains_key(b) => {
                let known = if reader.planes.contains_key(a) { a } else { b };
                reader.planes.get(known).map(|p| p.1).zip(first_at(known))
            }
            _ => None,
        };
        let fits = |x: f64| other.zip(value).is_some_and(|((_, at), v)| ((at - x).abs() - v.abs()).abs() < 0.1);
        ids.iter()
            .map(|i| {
                if reader.planes.contains_key(i) {
                    return i.clone();
                }
                let (ext, int) = (format!("{i}.exterior"), format!("{i}.interior"));
                if let (Some((Axis::Y, _)), Some(e), Some(n)) = (other, first_at(&ext), first_at(&int)) {
                    if fits(e) {
                        return ext;
                    }
                    if fits(n) {
                        return int;
                    }
                }
                if matches!(other, Some((Axis::Z, _))) && fits(0.0) && reader.planes.contains_key("__level") {
                    return "__level".to_string();
                }
                i.clone()
            })
            .collect()
    };
    // Dimensions between planes; alignments locked between two planes.
    let mut dimensions = Vec::new();
    for d in def["dimensions"].as_array().into_iter().flatten().take(super::MAX_DIMENSIONS) {
        let kind = match d["kind"].as_str().unwrap_or("linear") {
            "angular" => DimensionKind::Angular,
            "radial" => DimensionKind::Radial,
            "diameter" => DimensionKind::Diameter,
            _ => DimensionKind::Linear,
        };
        let measured =
            d["value"].as_object().and_then(|v| v.get(&first).or_else(|| v.values().next())).and_then(num).or_else(|| num(&d["value"]));
        let ids = resolve(&ids_of(d), measured);
        let found: Vec<&(String, Axis, BTreeMap<String, f64>)> = ids.iter().filter_map(|i| reader.planes.get(i)).collect();
        let label = d.get("label").and_then(text).map(str::to_string);
        let (mut locked, eq) = (d["locked"].as_bool().unwrap_or(false), d["eq"].as_bool().unwrap_or(false));
        // One Revit doesn't lock but that measures the same in every type while its planes move between types (a shell's
        // inner face kept its thickness in from the outer by the sketch): the planes move together, as if locked.
        if !locked && !eq && label.is_none() && f.types.len() > 1 && ids.len() == 2 {
            let values: Vec<f64> = f.types.keys().filter_map(|t| d["value"].get(t).and_then(num)).collect();
            let same = values.len() == f.types.len() && values.iter().all(|x| (x - values[0]).abs() < 0.05);
            let moving = ids.iter().filter_map(|i| reader.planes.get(i)).any(|p| {
                let at: Vec<f64> = p.2.values().copied().collect();
                at.iter().any(|x| (x - at.first().copied().unwrap_or(0.0)).abs() > 0.05)
            });
            locked = same && moving;
        }
        if found.len() != ids.len() || found.len() < 2 || kind != DimensionKind::Linear {
            // One to a form's face or a line: the form's own numbers follow what it measures already.
            continue;
        }
        if found.iter().any(|p| p.1 != found[0].1) {
            notes.push("one of its dimensions measures across two directions at once, which BIMCraft can't follow yet".into());
            continue;
        }
        // The host wall's thickness between its faces: the wall places them, as its own faces.
        if label.as_deref() == Some(super::WALL) && f.category.hosted() {
            continue;
        }
        let label = match label {
            Some(l) if reader.params.contains(&l) => Some(l),
            Some(l) => {
                notes.push(format!("one of its sizes is set by `{l}`, which isn't one of the family's numbers"));
                None
            }
            None => None,
        };
        let axis = found[0].1;
        let sign = reader.map.sign(axis);
        let sense: Vec<i8> = found
            .windows(2)
            .map(|w| {
                let gap = f.types.keys().filter_map(|t| Some(w[1].2.get(t)? - w[0].2.get(t)?)).find(|g| g.abs() > 1e-6).unwrap_or(1.0);
                if gap * sign < 0.0 { -1 } else { 1 }
            })
            .collect();
        let value =
            d["value"].as_object().and_then(|v| v.get(&first).or_else(|| v.values().next())).and_then(num).or_else(|| num(&d["value"]));
        dimensions.push(Dimension {
            kind,
            refs: found.iter().map(|p| p.0.clone()).collect(),
            label,
            locked,
            eq,
            value: value.map(f64::abs),
            sense: sense.iter().any(|s| *s < 0).then_some(sense),
            ..Dimension::default()
        });
    }
    for c in def["constraints"].as_array().into_iter().flatten() {
        if c["kind"].as_str() != Some("alignment") || !c["locked"].as_bool().unwrap_or(false) {
            continue;
        }
        let ids: Vec<&(String, Axis, BTreeMap<String, f64>)> =
            resolve(&ids_of(c), Some(0.0)).iter().filter_map(|v| reader.planes.get(v)).collect();
        if let [a, b] = ids.as_slice()
            && a.1 == b.1
        {
            dimensions.push(Dimension { refs: vec![a.0.clone(), b.0.clone()], locked: true, value: Some(0.0), ..Dimension::default() });
        }
    }
    // Planes aligned to the same edge or face of a form (an id that is no plane) are aligned to each other, where they stand
    // together in every type.
    let mut via: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in def["constraints"].as_array().into_iter().flatten().take(super::MAX_DIMENSIONS) {
        if c["kind"].as_str() != Some("alignment") || !c["locked"].as_bool().unwrap_or(false) {
            continue;
        }
        if let [a, b] = ids_of(c).as_slice() {
            match (reader.planes.contains_key(a), reader.planes.contains_key(b)) {
                (true, false) => via.entry(b.clone()).or_default().push(a.clone()),
                (false, true) => via.entry(a.clone()).or_default().push(b.clone()),
                _ => {}
            }
        }
    }
    for planes in via.values() {
        for w in planes.windows(2) {
            let (Some(a), Some(b)) = (reader.planes.get(&w[0]), reader.planes.get(&w[1])) else { continue };
            let together = a.1 == b.1 && !a.2.is_empty() && a.2.iter().all(|(t, x)| b.2.get(t).is_some_and(|y| (x - y).abs() < 0.05));
            if together && a.0 != b.0 && dimensions.len() < super::MAX_DIMENSIONS {
                dimensions.push(Dimension { refs: vec![a.0.clone(), b.0.clone()], locked: true, value: Some(0.0), ..Dimension::default() });
            }
        }
    }
    // A plane nothing else places, measured from another by a dimension Revit doesn't lock, keeps that distance: what holds
    // it there in Revit (its sketch locked to it, a form's face) the definition doesn't carry, and it moves with the other
    // when the family flexes.
    let placed = |d: &Dimension| d.locked || d.eq || d.label.is_some();
    for i in 0..dimensions.len() {
        if placed(&dimensions[i]) || dimensions[i].refs.len() != 2 || dimensions[i].value.is_none() {
            continue;
        }
        // And only where the gap is the same in every type.
        let at = |name: &str| reader.planes.values().find(|p| p.0 == name).map(|p| p.2.clone()).unwrap_or_default();
        let (a, b) = (at(&dimensions[i].refs[0]), at(&dimensions[i].refs[1]));
        let gaps: Vec<f64> = a.iter().filter_map(|(t, x)| Some(b.get(t)? - x)).collect();
        let same = !gaps.is_empty() && gaps.iter().all(|g| (g - gaps[0]).abs() < 0.05);
        if same && dimensions[i].refs.iter().any(|n| free(&dimensions, &references, n)) {
            dimensions[i].locked = true;
        }
    }
    // A plane that moves with a host's face (`follows`, as the exporter found by thickening the host) and that nothing else
    // ties to what stays: kept its offset from the face.
    for (_, name, axis, p) in &refs {
        let Some(fw) = p.get("follows").filter(|v| v.is_object()) else { continue };
        let (Some(target), Some(off)) = (fw.get("ref").and_then(Value::as_str).and_then(|r| reader.planes.get(r)), num(&fw["offset"]))
        else {
            continue;
        };
        if target.1 != *axis || target.0 == *name || dimensions.len() >= super::MAX_DIMENSIONS || !free(&dimensions, &references, name) {
            continue;
        }
        let sense = if off * reader.map.sign(*axis) < 0.0 { Some(vec![-1]) } else { None };
        dimensions.push(Dimension {
            refs: vec![target.0.clone(), name.clone()],
            locked: true,
            value: Some(off.abs()),
            sense,
            ..Dimension::default()
        });
    }
    // Forms and lines.
    let mut forms = Vec::new();
    for d in def["forms"].as_array().into_iter().flatten().take(MOST) {
        match reader.forms(d, has_material) {
            Ok(made) => forms.extend(made),
            Err(e) => notes.push(format!("its {}: {e}", part_of(d))),
        }
    }
    let mut symbols = Vec::new();
    // Where in `symbols` each masking region's edges are: they are dressed once the planes are solved.
    let mut regions: Vec<std::ops::Range<usize>> = Vec::new();
    for d in def["lines"].as_array().into_iter().flatten().take(MOST) {
        match reader.lines(d) {
            Ok(made) => {
                if d["masking"].as_bool() == Some(true) {
                    regions.push(symbols.len()..symbols.len() + made.len());
                }
                symbols.extend(made)
            }
            Err(e) => notes.push(format!("one of its lines: {e}")),
        }
    }
    // Nested families, placed: mirrored by the family's own axes where they are.
    let mut nested = Vec::new();
    for n in def["nested"].as_array().into_iter().flatten().take(super::MAX_NESTED) {
        let made = (|| -> Result<Nested, String> {
            let family = n.get("family").and_then(text).ok_or("it names no family")?.to_string();
            let type_name = n.get("type").and_then(text).ok_or("it names no type")?.to_string();
            // Its insertion point (`at`): where its own family's origin lands. Its document's origin (`origin`) is that
            // only where its family's axes are its document's; a door's or a window's are its opening's, the insertion
            // point's (a handle's family is a door's).
            let at = reader.point(&n["at"])?;
            let rotation = n.get("rotation").map(|r| reader.value(r)).transpose()?.unwrap_or_default();
            let mirrored = n["mirrored"].as_bool().unwrap_or(false);
            let mut params = BTreeMap::new();
            for (k, b) in n["params"].as_object().into_iter().flatten() {
                // A material or a text handed down sizes nothing: the nested family keeps its own.
                if b.get("param").and_then(Value::as_str).is_some_and(|p| !reader.params.contains(p)) {
                    continue;
                }
                match reader.value(b) {
                    Ok(e) => {
                        params.insert(k.clone(), e);
                    }
                    Err(e) => return Err(format!("its `{k}`: {e}")),
                }
            }
            let visible = n.get("visible").and_then(text).map(|v| Expr::Formula(v.to_string()));
            let show = n.get("show").and_then(|s| serde_json::from_value::<Show>(s.clone()).ok()).filter(|s| *s != Show::default());
            // Its axes where the definition gives them (a part on a face), taken into the family's: y the other way where
            // the family's is.
            let axis = |k: &str| -> Option<[f64; 3]> {
                let v: Vec<f64> = n[k].as_array()?.iter().filter_map(num).collect();
                let [x, y, z] = v.as_slice() else { return None };
                Some([*x, *y, *z])
            };
            // Its own axes in the document's: given, or turned about z and mirrored across its x; taken into the family's
            // where those turn a way round.
            let given = match (axis("x_axis"), axis("y_axis")) {
                (Some(x), Some(y)) => Some([x, y, axis("z_axis").unwrap_or_else(|| crate::geom::v3::cross(x, y))]),
                _ => None,
            };
            let turned = reader.map.x_sign < 0.0 || reader.map.y_sign < 0.0;
            let axes = match (given, &rotation) {
                (Some(a), _) => Some(a.map(|d| reader.map.turn(d))),
                (None, Expr::Number(r)) if turned => {
                    let (sn, cs) = r.to_radians().sin_cos();
                    let flip = if mirrored { -1.0 } else { 1.0 };
                    Some([[cs, sn, 0.0], [-flip * sn, flip * cs, 0.0], [0.0, 0.0, 1.0]].map(|d| reader.map.turn(d)))
                }
                _ => None,
            };
            // The type the parent's types each choose of it, where they choose others.
            let type_in: BTreeMap<String, String> = n["types"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(t, _)| f.types.contains_key(*t))
                .filter_map(|(t, v)| Some((t.clone(), text(v)?.to_string())))
                .collect();
            let scale = planting_scale(f, &family, &type_name);
            Ok(Nested { family, type_name, at, rotation, mirrored, params, visible, show, axes, type_in, scale, ..Nested::default() })
        })();
        match made {
            Ok(x) => nested.push(x),
            Err(e) => notes
                .push(format!("its {} is placed by a rule BIMCraft can't follow yet: {e}", n["family"].as_str().unwrap_or("nested part"))),
        }
    }
    let catalog = def.get("type_catalog").filter(|c| !c.is_null()).and_then(|c| serde_json::from_value::<Catalog>(c.clone()).ok());
    notes.append(&mut reader.notes);
    f.references = references;
    f.dimensions = dimensions;
    f.catalog = catalog;
    // A masking region's edges, each at the levels Revit's plan draws it at, where the first type's planes stand solved.
    if !regions.is_empty()
        && let Ok(solved) = vals(f, &first, &walls)
    {
        for region in regions {
            masked_as_revit(f, &first, &solved, &mut symbols[region]);
        }
    }
    // A window whose sill plane stands above its level by a chain no parameter says (its Sill Height none, or 0 in every
    // type): its Sill Height is that chain, so it is placed in its wall where Revit places it.
    if let Some(c) = sill_from
        && f.category == super::Category::Window
    {
        let zero = f
            .types
            .keys()
            .all(|t| matches!(vals(f, t, &walls).ok().and_then(|v| v.get(super::SILL).cloned()), None | Some(Val::Number(0.0))));
        let room = f.parameters.len() < super::MAX_PARAMETERS;
        match f.parameters.iter_mut().find(|p| p.name == super::SILL) {
            Some(p) if p.formula.is_none() && zero && matches!(p.kind, Kind::Length) && !p.instance => {
                p.formula = Some(c);
                // A type sets no value a formula works out.
                for set in f.types.values_mut() {
                    set.remove(super::SILL);
                }
            }
            Some(_) => {}
            None if room => f.parameters.push(super::Parameter {
                name: super::SILL.to_string(),
                kind: Kind::Length,
                value: serde_json::json!(0.0),
                formula: Some(c),
                ..super::Parameter::default()
            }),
            None => {}
        }
    }
    // What is made is tried in every type: a form or a line that doesn't work out in one is left out, and said.
    let forms = turned_as_revit(f, forms, &walls);
    let (forms, symbols, nested) = tried(f, forms, symbols, nested, &mut notes, &walls);
    f.forms.extend(forms);
    f.symbols.extend(symbols);
    f.nested = nested;
    notes.truncate(64);
    f.editable = Some(super::Editable { ok: false, reason: None, untranslated: notes });
    walls
}

/// `forms` with each part-turn revolve turned the way Revit turns it. The definition says its axis and its angles but not
/// which way round they run from its sketch, so each is turned the other way where that brings the first type's volume
/// nearer Revit's result (a void's, by what it cuts).
fn turned_as_revit(f: &Family, mut forms: Vec<Form>, walls: &Walls) -> Vec<Form> {
    let Some(first) = f.types.keys().next().cloned() else { return forms };
    let Ok(values) = vals(f, &first, walls) else { return forms };
    let snapshot: Vec<&Form> = f.forms.iter().filter(|x| x.snapshot && x.of_type(&first) && x.show.is_none_or(|s| s.fine)).collect();
    let revit: f64 = snapshot.iter().map(|x| super::shapes::volume(&x.mesh).abs()).sum();
    if revit <= 0.0 {
        return forms;
    }
    type Bounds = ([f64; 3], [f64; 3]);
    let bounds = |tris: &[[crate::geom::v3::P3; 3]]| -> Option<Bounds> {
        let mut b: Option<Bounds> = None;
        for q in tris.iter().flatten() {
            b = Some(match b {
                None => (*q, *q),
                Some((lo, hi)) => (std::array::from_fn(|k| lo[k].min(q[k])), std::array::from_fn(|k| hi[k].max(q[k]))),
            });
        }
        b
    };
    let boxes: Vec<Bounds> = snapshot.iter().filter_map(|x| bounds(&x.mesh)).collect();
    let mut probe = Family { forms: Vec::new(), symbols: Vec::new(), nested: Vec::new(), check: BTreeMap::new(), ..f.clone() };
    // How far it is from Revit's: each piece's box from the nearest of Revit's pieces' (mm), then the volume.
    let mut off = |forms: &[Form]| -> (f64, f64) {
        probe.forms = forms.to_vec();
        let how = super::Build { snapshot: false, others: &|_| None, depth: 0, spent: &std::cell::Cell::new(0) };
        match probe.build(&first, &values, &mut HashMap::new(), &mut crate::csg::Meter::default(), how) {
            Ok(pieces) => {
                let apart = pieces
                    .iter()
                    .filter_map(|p| bounds(&p.tris))
                    .map(|(lo, hi)| {
                        boxes
                            .iter()
                            .map(|(a, b)| (0..3).map(|k| (lo[k] - a[k]).abs() + (hi[k] - b[k]).abs()).sum::<f64>())
                            .fold(f64::INFINITY, f64::min)
                    })
                    .sum::<f64>();
                (apart, (pieces.iter().map(|p| super::shapes::volume(&p.tris).abs()).sum::<f64>() - revit).abs())
            }
            Err(_) => (f64::INFINITY, f64::INFINITY),
        }
    };
    let better = |now: (f64, f64), best: (f64, f64)| now.0 < best.0 - 1.0 || (now.0 <= best.0 + 1.0 && now.1 < best.1 * 0.999);
    let mut best = off(&forms);
    for i in 0..forms.len() {
        let part = forms[i].axis.is_some()
            && match (&forms[i].from, &forms[i].to) {
                (Expr::Number(a), Expr::Number(b)) => (b - a).abs() < 359.9,
                _ => true,
            };
        if !part {
            continue;
        }
        let was = forms[i].clone();
        (forms[i].from, forms[i].to) = (neg(was.to.clone()), neg(was.from.clone()));
        let now = off(&forms);
        if better(now, best) {
            best = now;
        } else {
            forms[i] = was;
        }
    }
    forms
}

/// A masking region's edges `made`, each drawn at the detail levels Revit's plan draws it at in the first type: its family says
/// none (a masking region has no Visibility Settings), yet Revit leaves a door's out at Coarse. An edge is there at a level
/// where its two ends and its middle each lie on a line of Revit's plan at that level (within 1 mm); an edge says only the
/// levels it is found at, even none. Where no edge is found at any level (the region is hidden in the first type), what the
/// definition says stays.
fn masked_as_revit(f: &Family, first: &str, values: &BTreeMap<String, Val>, made: &mut [Symbol]) {
    use crate::graphics::Detail;
    let probes: Vec<Option<[P2; 3]>> = made.iter().map(|y| edge_points(y, values)).collect();
    let mut found = vec![[false; 3]; made.len()];
    for (level, d) in [Detail::Coarse, Detail::Medium, Detail::Fine].into_iter().enumerate() {
        let Ok(marks) = f.marks_where(values, &|y: &Symbol| y.snapshot && y.view == SymbolView::Plan && y.of_type(first) && y.drawn_at(d))
        else {
            return;
        };
        let segments: Vec<(P2, P2)> = marks
            .iter()
            .filter_map(|m| match m {
                super::Mark::Line(pts) => Some(pts),
                _ => None,
            })
            .flat_map(|pts| pts.windows(2).map(|w| (w[0], w[1])))
            .collect();
        for (edge, probe) in probes.iter().enumerate() {
            if let Some(points) = probe
                && points.iter().all(|p| segments.iter().any(|(a, b)| near_segment(*p, *a, *b, 1.0)))
            {
                found[edge][level] = true;
            }
        }
    }
    if !found.iter().flatten().any(|x| *x) {
        return;
    }
    for ((y, probe), at) in made.iter_mut().zip(&probes).zip(found) {
        if probe.is_some() {
            y.show = Some(Levels { coarse: at[0], medium: at[1], fine: at[2] });
        }
    }
}

/// A symbol's two ends and its middle along its length, in the first type; none where it doesn't work out.
fn edge_points(y: &Symbol, values: &BTreeMap<String, Val>) -> Option<[P2; 3]> {
    let n = |e: &Expr| number(e, values, &mut HashMap::new()).ok();
    if let Some(c) = &y.arc {
        let (cx, cy, r, from, to) = (n(&c.centre[0])?, n(&c.centre[1])?, n(&c.radius)?, n(&c.from)?, n(&c.to)?);
        let at = |a: f64| P2(cx + r * a.to_radians().cos(), cy + r * a.to_radians().sin());
        return Some([at(from), at((from + to) / 2.0), at(to)]);
    }
    let pts: Vec<P2> = y.line.iter().map(|[a, b]| Some(P2(n(a)?, n(b)?))).collect::<Option<_>>()?;
    let (start, end) = (*pts.first()?, *pts.last()?);
    let lengths: Vec<f64> = pts.windows(2).map(|w| w[0].dist(w[1])).collect();
    let mut left = lengths.iter().sum::<f64>() / 2.0;
    let mut middle = start;
    for (w, len) in pts.windows(2).zip(&lengths) {
        if left <= *len || *len <= 0.0 {
            let t = if *len > 0.0 { left / len } else { 0.0 };
            middle = P2(w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
            break;
        }
        left -= len;
        middle = w[1];
    }
    Some([start, middle, end])
}

/// Whether point `p` is within `tol` of the segment from `a` to `b`.
fn near_segment(p: P2, a: P2, b: P2, tol: f64) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(P2(a.0 + dx * t, a.1 + dy * t)) <= tol
}

/// Whether reference `name` is free: nothing ties it, through the dimensions that place planes (locked, labelled, EQ), to a
/// plane that stays (the origin, a host's face).
fn free(dimensions: &[Dimension], references: &[Reference], name: &str) -> bool {
    let placed = |d: &Dimension| d.locked || d.eq || d.label.is_some();
    let stays =
        |n: &str| references.iter().any(|r| r.name == n && (r.origin || r.is_reference.as_deref().is_some_and(|k| k.starts_with("host_"))));
    let mut group: BTreeSet<&str> = BTreeSet::from([name]);
    loop {
        let before = group.len();
        let more: Vec<&str> = dimensions
            .iter()
            .filter(|d| placed(d) && d.refs.iter().any(|x| group.contains(x.as_str())))
            .flat_map(|d| d.refs.iter().map(String::as_str))
            .collect();
        group.extend(more);
        if group.len() == before || group.len() > super::MAX_REFERENCES {
            break;
        }
    }
    !group.iter().any(|n| stays(n))
}

/// How a planting's base family is scaled in the family it is placed in. Revit draws a planting's plan symbol (its base family
/// nested in it) at the parent's `Height` over the height the base is drawn at: the top plane of the base, the highest of its
/// planes standing upright. A formula for it, `Height / 6096`; none where the family isn't a planting, holds its base itself
/// (a shared base is the project's, not read here), or has no `Height` length to follow or no top plane to scale from.
fn planting_scale(f: &Family, family: &str, type_name: &str) -> Option<Expr> {
    if f.source.as_ref()?.category.as_deref() != Some("Planting") {
        return None;
    }
    if !f.parameters.iter().any(|p| p.name == "Height" && p.kind == Kind::Length) {
        return None;
    }
    let base = f.embedded.get(family)?;
    let values = base.values_in(type_name, &BTreeMap::new(), super::DEFAULT_WALL).ok()?;
    let solved = base.solve(&values).ok()?;
    let top = base.references.iter().filter(|r| r.axis == Axis::Z).filter_map(|r| solved.get(&r.name)).copied().fold(0.0_f64, f64::max);
    (top.is_finite() && top > 0.0).then(|| Expr::Formula(format!("Height / {top}")))
}

/// What an architect calls a form of the definition: its subcategory, else its material, and what kind of form it is.
fn part_of(def: &Value) -> String {
    let kind = match def["kind"].as_str().unwrap_or("") {
        "extrusion" => "extrusion",
        "blend" => "blend",
        "revolution" => "revolve",
        "sweep" => "sweep",
        "swept_blend" => "swept blend",
        _ => "form",
    };
    let name = ["subcategory", "material_param", "material"].iter().find_map(|k| def.get(*k).and_then(text));
    match name {
        Some(n) => format!("{} ({kind})", n.to_lowercase()),
        None => kind.to_string(),
    }
}

/// [`part_of`] of a form as translated, the `i`th: its subcategory, else its material.
fn part(form: &Form, i: usize) -> String {
    match form.subcategory.as_deref().or(Some(form.material.as_str()).filter(|m| !m.is_empty())) {
        Some(n) => n.to_lowercase(),
        None => format!("form {}", i + 1),
    }
}

/// `forms`, `symbols` and `nested` parts kept where each works out in every type of `f` (its references where they're
/// placed); one that doesn't is left out, and said in `notes`.
fn tried(
    f: &Family,
    forms: Vec<Form>,
    symbols: Vec<Symbol>,
    nested: Vec<Nested>,
    notes: &mut Vec<String>,
    walls: &Walls,
) -> (Vec<Form>, Vec<Symbol>, Vec<Nested>) {
    let values: Vec<BTreeMap<String, Val>> = f.types.keys().filter_map(|t| vals(f, t, walls).ok()).collect();
    let mut probe = Family { forms: Vec::new(), symbols: Vec::new(), nested: Vec::new(), check: BTreeMap::new(), ..f.clone() };
    let mut kept_forms = Vec::new();
    for (i, form) in forms.into_iter().enumerate() {
        probe.forms = vec![form];
        let fails = values.iter().find_map(|v| probe.solid(0, v, &mut HashMap::new()).err());
        match fails {
            None => kept_forms.append(&mut probe.forms),
            Some(e) => notes.push(format!("its {} doesn't come out in every type: {e}", part(&probe.forms[0], i))),
        }
    }
    let mut kept_symbols = Vec::new();
    for y in symbols {
        probe.symbols = vec![y];
        let fails = values.iter().find_map(|v| probe.marks_where(v, &|_| true).err());
        match fails {
            None => kept_symbols.append(&mut probe.symbols),
            Some(e) => notes.push(format!("one of its lines doesn't come out in every type: {e}")),
        }
    }
    let mut kept_nested = Vec::new();
    for n in nested {
        let mut read = HashMap::new();
        let ok = values.iter().all(|v| {
            n.at.iter()
                .chain([&n.rotation])
                .chain(&n.visible)
                .chain(&n.scale)
                .chain(n.params.values())
                .all(|e| number(e, v, &mut read).is_ok())
        });
        if ok {
            kept_nested.push(n);
        } else {
            notes.push(format!("its {} is placed by a rule BIMCraft can't follow yet", n.family));
        }
    }
    (kept_forms, kept_symbols, kept_nested)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::graphics::Detail;

    /// The twelve triangles of a box, outward facing.
    fn boxed(a: [f64; 3], b: [f64; 3]) -> Value {
        let p = |x: usize, y: usize, z: usize| [[a[0], b[0]][x], [a[1], b[1]][y], [a[2], b[2]][z]];
        let quad = |q: [[f64; 3]; 4]| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]];
        let faces = [
            quad([p(0, 0, 0), p(0, 1, 0), p(1, 1, 0), p(1, 0, 0)]),
            quad([p(0, 0, 1), p(1, 0, 1), p(1, 1, 1), p(0, 1, 1)]),
            quad([p(0, 0, 0), p(1, 0, 0), p(1, 0, 1), p(0, 0, 1)]),
            quad([p(0, 1, 0), p(0, 1, 1), p(1, 1, 1), p(1, 1, 0)]),
            quad([p(0, 0, 0), p(0, 0, 1), p(0, 1, 1), p(0, 1, 0)]),
            quad([p(1, 0, 0), p(1, 1, 0), p(1, 1, 1), p(1, 0, 1)]),
        ];
        json!(faces.iter().flatten().collect::<Vec<_>>())
    }

    /// A door family of two types, Revit's own result `extra` (its forms and symbols) in it.
    fn door(extra: Value) -> Family {
        let mut v = json!({
            "category": "door",
            "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 2100}],
            "types": {"A": {}, "B": {}}
        });
        for (k, x) in extra.as_object().into_iter().flatten() {
            v[k] = x.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    /// A closed rectangle of lines in the plane `z`, from (x0, y0) to (x1, y1).
    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Value {
        json!([
            {"line": [[x0, y0, z], [x1, y0, z]]},
            {"line": [[x1, y0, z], [x1, y1, z]]},
            {"line": [[x1, y1, z], [x0, y1, z]]},
            {"line": [[x0, y1, z], [x0, y0, z]]}
        ])
    }

    /// Revit's plan line from (x0, y0) to (x1, y1) at the levels `show` says.
    fn drawn(x0: f64, y0: f64, x1: f64, y1: f64, show: Value) -> Value {
        json!({"line": [[x0, y0], [x1, y1]], "show": show, "snapshot": true})
    }

    fn translated(f: &mut Family, def: Value) {
        translate(f, &def, &|_| true);
    }

    /// What the definition's own lines became, not Revit's.
    fn made(f: &Family) -> Vec<&Symbol> {
        f.symbols.iter().filter(|y| !y.snapshot).collect()
    }

    fn at_levels(coarse: bool, medium: bool, fine: bool) -> Option<Levels> {
        Some(Levels { coarse, medium, fine })
    }

    /// A planting's base family: its ground at 0 and its top plane at 6096 mm, in its only type.
    fn base() -> Family {
        serde_json::from_value(json!({
            "category": "generic", "types": {"Base": {}},
            "references": [{"name": "Ground", "axis": "z", "at": 0, "origin": true}, {"name": "Top", "axis": "z", "at": 6096}]
        }))
        .unwrap()
    }

    /// A family of the Revit category `category`, with a `Height` parameter if `height`, holding the base nested in it.
    fn planted(category: &str, height: bool) -> Family {
        let parameters = if height { json!([{"name": "Height", "kind": "length", "value": 6000}]) } else { json!([]) };
        serde_json::from_value(json!({
            "category": "generic", "source": {"category": category}, "parameters": parameters,
            "types": {"Tree": {"Height": 3000}}, "embedded": {"Base": base()}
        }))
        .unwrap()
    }

    fn base_in() -> Value {
        json!({"to_bimcraft": {}, "nested": [{"id": "1", "family": "Base", "type": "Base", "at": [0, 0, 0], "rotation": 0,
                                              "mirrored": false, "params": {}}]})
    }

    #[test]
    fn a_plantings_base_is_scaled_by_the_trees_height_over_the_bases_top_plane() {
        let mut f = planted("Planting", true);
        translated(&mut f, base_in());
        assert_eq!(f.nested.len(), 1, "{:?}", f.editable);
        assert_eq!(f.nested[0].scale, Some(Expr::Formula("Height / 6096".into())));
        // Any other category's nested part is as it is, and so is a planting with no Height to follow.
        let mut other = planted("Furniture", true);
        translated(&mut other, base_in());
        assert_eq!(other.nested[0].scale, None);
        let mut flat = planted("Planting", false);
        translated(&mut flat, base_in());
        assert_eq!(flat.nested[0].scale, None);
    }

    #[test]
    fn a_revit_form_hidden_in_plan_is_not_drawn_below_the_cut() {
        // A threshold Revit's plan doesn't draw, and a leaf it does: the exporter writes no `plan_top` or `plan_cut`.
        let mut f = door(json!({"forms": [
            {"snapshot": true, "mesh": boxed([-450.0, -50.0, 0.0], [450.0, 50.0, 50.0]), "show": {"plan": false}},
            {"snapshot": true, "mesh": boxed([-450.0, -20.0, 50.0], [450.0, 20.0, 2100.0])}
        ]}));
        translated(&mut f, json!({"to_bimcraft": {}}));
        let threshold = f.forms[0].show.unwrap();
        assert!(!threshold.plan && !threshold.plan_top && !threshold.plan_cut, "{threshold:?}");
        let values = f.values("A", &BTreeMap::new()).unwrap();
        let pieces = f.pieces("A", &values).unwrap();
        assert_eq!(pieces.len(), 2);
        assert!(!pieces[0].in_plan_cut(Detail::Medium, 1200.0, true), "the threshold is not drawn below the cut");
        assert!(pieces[1].in_plan_cut(Detail::Medium, 1200.0, true), "the leaf the cut passes through still is");
        // A form that said nothing says everything now.
        assert_eq!(f.forms[1].show, Some(Show::default()));
    }

    #[test]
    fn a_definition_form_showing_everything_keeps_saying_so() {
        let mut f = door(json!({}));
        let form = |show: Value| {
            let mut x = json!({"id": "1", "kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 50,
                "loops": [rectangle(0.0, 0.0, 100.0, 40.0, 0.0)]});
            if !show.is_null() {
                x["show"] = show;
            }
            x
        };
        let all = json!({"coarse": true, "medium": true, "fine": true, "plan": true, "front": true, "side": true,
            "plan_top": true, "plan_cut": true});
        translated(&mut f, json!({"to_bimcraft": {}, "forms": [form(all), form(Value::Null)]}));
        let ours: Vec<&Form> = f.forms.iter().filter(|x| !x.snapshot).collect();
        assert_eq!(ours.len(), 2, "{:?}", f.editable);
        assert_eq!(ours[0].show, Some(Show::default()), "it says so, though it says what the default does");
        assert_eq!(ours[1].show, None, "one that says nothing still says nothing");
    }

    /// A door whose Revit plan draws a rectangle's four edges at the levels `shows` say, one for each, and whose definition has
    /// that rectangle as a masking region.
    fn masked(shows: [Value; 4]) -> Family {
        let edges = [(0.0, 0.0, 100.0, 0.0), (100.0, 0.0, 100.0, 50.0), (100.0, 50.0, 0.0, 50.0), (0.0, 50.0, 0.0, 0.0)];
        let symbols: Vec<Value> = edges.iter().zip(shows).map(|((a, b, c, d), s)| drawn(*a, *b, *c, *d, s)).collect();
        let mut f = door(json!({ "symbols": symbols }));
        translated(
            &mut f,
            json!({"to_bimcraft": {}, "lines": [
            {"view": "plan", "masking": true, "curves": rectangle(0.0, 0.0, 100.0, 50.0, 0.0)}]}),
        );
        f
    }

    #[test]
    fn a_masking_region_edge_is_drawn_only_at_the_levels_revit_draws_that_edge() {
        // Revit's plan: the first edge at Medium and Fine, the others at Medium only.
        let both = json!({"coarse": false});
        let medium = json!({"coarse": false, "fine": false});
        let f = masked([both, medium.clone(), medium.clone(), medium]);
        let edges = made(&f);
        assert_eq!(edges.len(), 4, "{:?}", f.editable);
        assert_eq!(edges[0].show, at_levels(false, true, true));
        for e in &edges[1..] {
            assert_eq!(e.show, at_levels(false, true, false), "not drawn at Fine, where Revit's plan has no such edge");
        }
    }

    #[test]
    fn a_masking_region_sharing_one_edge_with_a_coarse_line_is_still_hidden_at_coarse() {
        // A line of Revit's at Coarse lies along the first edge only.
        let always = json!({});
        let medium = json!({"coarse": false, "fine": false});
        let f = masked([always, medium.clone(), medium.clone(), medium]);
        let edges = made(&f);
        assert_eq!(edges.len(), 4, "{:?}", f.editable);
        assert!(edges[0].drawn_at(Detail::Coarse), "the edge Revit draws at Coarse is");
        for e in &edges[1..] {
            assert!(!e.drawn_at(Detail::Coarse), "the others are hidden at Coarse");
            assert!(e.drawn_at(Detail::Medium) && !e.drawn_at(Detail::Fine));
        }
    }

    #[test]
    fn a_masking_region_nobody_draws_says_what_its_definition_says() {
        // None of its edges is among Revit's lines (hidden in the first type): its own settings stay.
        let mut f = door(json!({"symbols": [drawn(500.0, 500.0, 600.0, 500.0, json!({}))]}));
        translated(
            &mut f,
            json!({"to_bimcraft": {}, "lines": [{"view": "plan", "masking": true, "show": {"coarse": false, "fine": false},
            "curves": rectangle(0.0, 0.0, 100.0, 50.0, 0.0)}]}),
        );
        let edges = made(&f);
        assert_eq!(edges.len(), 4);
        assert!(edges.iter().all(|e| e.show == at_levels(false, true, false)));
    }

    #[test]
    fn a_line_in_invisible_style_draws_nothing() {
        let mut f = door(json!({}));
        translated(
            &mut f,
            json!({"to_bimcraft": {}, "lines": [
                {"view": "plan", "subcategory": "<Invisible lines>", "curves": [{"line": [[0, 0, 0], [100, 0, 0]]}]},
                {"view": "plan", "curves": [{"line": [[0, 10, 0], [100, 10, 0]]}]}]}),
        );
        let lines = made(&f);
        assert_eq!(lines.len(), 1, "only the one in a style that draws");
        assert_eq!(lines[0].line[0][1], Expr::Number(10.0));
    }

    #[test]
    fn a_region_whose_points_share_x_is_a_side_symbol() {
        let mut f = door(json!({}));
        // Exported as plan: one standing in the family's yz plane, one in its xz plane, one lying flat.
        let region = |curves: Value| json!({"view": "plan", "masking": true, "curves": curves});
        let upright = |x: [f64; 2], y: [f64; 2]| {
            let (a, b) = ([x[0], y[0]], [x[1], y[1]]);
            json!([
                {"line": [[a[0], a[1], 0], [b[0], b[1], 0]]},
                {"line": [[b[0], b[1], 0], [b[0], b[1], 30]]},
                {"line": [[b[0], b[1], 30], [a[0], a[1], 30]]},
                {"line": [[a[0], a[1], 30], [a[0], a[1], 0]]}
            ])
        };
        translated(
            &mut f,
            json!({"to_bimcraft": {}, "lines": [
                region(upright([100.0, 100.0], [0.0, 50.0])),
                region(upright([0.0, 100.0], [40.0, 40.0])),
                region(rectangle(0.0, 0.0, 100.0, 50.0, 0.0))]}),
        );
        let views: Vec<SymbolView> = made(&f).iter().map(|y| y.view).collect();
        assert_eq!(views.len(), 12, "{:?}", f.editable);
        assert!(views[..4].iter().all(|v| *v == SymbolView::Side), "{views:?}");
        assert!(views[4..8].iter().all(|v| *v == SymbolView::Front), "{views:?}");
        assert!(views[8..].iter().all(|v| *v == SymbolView::Plan), "{views:?}");
    }

    /// A furniture family on a level at 0 whose base plane is at 0 in each type, its top at 762, `Height` labelled from one
    /// to the other, and the one form between them; `more` is added to the definition.
    fn desk(more: Value) -> Family {
        let mut f: Family = serde_json::from_value(json!({
            "category": "furniture",
            "parameters": [{"name": "Height", "kind": "length", "value": 762}],
            "types": {"A": {"Height": 762}, "B": {"Height": 762}}
        }))
        .unwrap();
        let plane = |id: &str, name: &str, at: f64, extra: Value| {
            let mut p = json!({"id": id, "name": name, "normal": [0, 0, 1], "at": {"A": at, "B": at}});
            for (k, x) in extra.as_object().into_iter().flatten() {
                p[k] = x.clone();
            }
            p
        };
        let mut def = json!({
            "to_bimcraft": {"z_minus": {"ref": "L"}},
            "reference_planes": [
                plane("L", "Ground floor", 0.0, json!({"is_reference": "level"})),
                plane("B", "Base Plane", 0.0, json!({})),
                plane("T", "Top Plane", 762.0, json!({}))],
            "dimensions": [{"id": "d", "kind": "linear", "refs": ["B", "T"], "label": "Height", "locked": true,
                "value": {"A": 762, "B": 762}}],
            "forms": [{"id": "f", "kind": "extrusion", "normal": [0, 0, 1], "start": {"ref": "B"}, "end": {"ref": "T"},
                "loops": [rectangle(0.0, 0.0, 100.0, 40.0, 0.0)]}]
        });
        for (k, x) in more.as_object().into_iter().flatten() {
            def[k] = x.clone();
        }
        translated(&mut f, def);
        f
    }

    #[test]
    fn a_desk_flexed_taller_keeps_its_base_on_the_level() {
        let f = desk(json!({}));
        for height in [900.0, 600.0] {
            let given = BTreeMap::from([("Height".to_string(), height)]);
            let values = f.values_given("A", &given, super::super::DEFAULT_WALL).unwrap();
            let at = f.solve(&values).unwrap();
            assert!(at["Base Plane"].abs() < 1e-6 && (at["Top Plane"] - height).abs() < 1e-6, "{at:?}");
            let pieces = f.pieces("A", &values).unwrap();
            assert_eq!(pieces.len(), 1, "{:?}", f.editable);
            assert!(pieces[0].bottom.abs() < 1e-6 && (pieces[0].top - height).abs() < 1e-6, "{} .. {}", pieces[0].bottom, pieces[0].top);
        }
    }

    #[test]
    fn a_dimension_to_the_level_measures_to_the_base_plane_where_that_is_the_level() {
        // A dimension from the top plane to a form's face (no plane): the level it may stand for is the family's base plane
        // where that is at 0, not a second plane.
        let to_a_face = json!({"dimensions": [
            {"id": "d", "kind": "linear", "refs": ["B", "T"], "label": "Height", "locked": true, "value": {"A": 762, "B": 762}},
            {"id": "e", "kind": "linear", "refs": ["T", "face"], "locked": false, "value": {"A": 762, "B": 762}}]});
        let f = desk(to_a_face);
        assert!(f.references.iter().all(|r| r.name != "Ref Level"), "{:?}", f.references);
        assert_eq!(f.references.len(), 3);
    }
}
