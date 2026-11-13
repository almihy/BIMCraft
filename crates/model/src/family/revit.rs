//! Revit's definition of a family (the add-in's package `definition`, Arqiflow's
//! `docs/superpowers/specs/2026-11-13-revit-families-design.md`, "The definition's JSON, exactly") translated into a BIMCraft
//! family's references, dimensions, parametric forms, symbols, nested parts and type catalog. The input is hostile: it is read
//! as JSON values, each thing that can't be translated is left out and said in plain words (`untranslated`), and what is
//! made is tried in every type before it is kept, so the family always loads and the check decides what is drawn.
//!
//! Lengths are mm in the family document's axes; [`Map`] takes them into the family's own (a door's or a window's: the
//! opening's): `x + x_offset`, `y × y_sign`, `z - z_minus`. A bound number (B) is a number, `{ref, offset}` (a reference
//! plane's coordinate along its axis plus an offset), `{param, scale, offset}` or `{formula}`; it becomes an [`Expr`]. A
//! reference plane's position and an extrusion's start and end are coordinates along the positive axis of their normal.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;

use super::{
    Axis, Bend, Catalog, Curve, Dimension, DimensionKind, Expr, Family, Form, Kind, Levels, Nested, Plane, Reference, Ring, Show,
    Symbol, SymbolView, Val, nameable, number,
};
use crate::geom::P2;

/// Things one definition may bring: curves in a loop, loops in a form, forms, lines.
const MOST: usize = 4096;

/// The family document's axes taken into the family's own.
struct Map {
    x_offset: f64,
    y_sign: f64,
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
        (Expr::Number(x), Expr::Formula(g)) if x == 0.0 => Expr::Formula(format!("-({g})")),
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
                return Err(format!("it follows `{p}`, which the family has no number parameter of"));
            }
            let (scale, offset) = (b.get("scale").and_then(num).unwrap_or(1.0), b.get("offset").and_then(num).unwrap_or(0.0));
            let base = if scale == 1.0 { p.to_string() } else { format!("{} * {p}", plain(scale)) };
            return Ok(plus(Expr::Formula(base), offset));
        }
        if let Some(r) = b.get("ref").and_then(Value::as_str) {
            // A plane's coordinate as a value: where it stands in the first type, in the document's axes.
            let (_, _, at) = self.planes.get(r).ok_or_else(|| format!("it follows a reference `{r}` the definition doesn't have"))?;
            let x = at.get(&self.first).or_else(|| at.values().next()).copied().unwrap_or(0.0);
            return Ok(Expr::Number(x + b.get("offset").and_then(num).unwrap_or(0.0)));
        }
        Err(format!("`{}` isn't a number the definition says", short(b)))
    }

    /// A coordinate along `axis`, bound as `b`, in the family's axes.
    fn coord(&self, b: &Value, axis: Axis) -> Result<Expr, String> {
        if let Some(r) = b.get("ref").and_then(Value::as_str)
            && let Some((name, a, _)) = self.planes.get(r)
            && *a == axis
        {
            let offset = b.get("offset").and_then(num).unwrap_or(0.0);
            let sign = if axis == Axis::Y { self.map.y_sign } else { 1.0 };
            return Ok(plus(Expr::Formula(format!("ref({name})")), sign * offset));
        }
        let v = self.value(b)?;
        Ok(self.place(v, axis))
    }

    /// A document coordinate along `axis` taken into the family's axes.
    fn place(&self, v: Expr, axis: Axis) -> Expr {
        match axis {
            Axis::X => plus(v, self.map.x_offset),
            Axis::Y if self.map.y_sign < 0.0 => neg(v),
            Axis::Y => v,
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
        let mut values = self.family.values(&self.first, &BTreeMap::new()).unwrap_or_default();
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

/// A curve of a loop: its start (and end) point, and for an arc its centre, its normal and its middle point.
struct Bit {
    start: [Expr; 3],
    end: [Expr; 3],
    arc: Option<([Expr; 3], [f64; 3], [f64; 3])>,
    radius: Option<Expr>,
}

impl Reader<'_> {
    fn curve(&self, c: &Value) -> Result<Bit, String> {
        if let Some(l) = c.get("line").and_then(Value::as_array).filter(|l| l.len() == 2) {
            return Ok(Bit { start: self.point(&l[0])?, end: self.point(&l[1])?, arc: None, radius: None });
        }
        let a = c.get("arc").ok_or_else(|| format!("a curve of a kind it can't draw: {}", short(c)))?;
        let (start, end, centre) = (self.point(&a["start"])?, self.point(&a["end"])?, self.point(&a["center"])?);
        let n: Vec<f64> = a["normal"].as_array().map(|v| v.iter().filter_map(num).collect()).unwrap_or_default();
        let [nx, ny, nz] = n.as_slice() else { return Err("an arc says no normal".into()) };
        // Its middle, worked out in the document's axes (counter-clockwise about its normal), then taken over: what says
        // which way it turns once mirrored.
        let (s, e, c3) = (self.at(&start)?, self.at(&end)?, self.at(&centre)?);
        let normal = [*nx, ny * self.map.y_sign, *nz];
        let doc = |p: [f64; 3]| [p[0], p[1] * self.map.y_sign, p[2]];
        let (sd, ed, cd) = (doc(s), doc(e), doc(c3));
        let mid = middle(sd, ed, cd, [*nx, *ny, *nz]);
        let radius = a.get("radius").map(|r| self.value(r)).transpose()?;
        Ok(Bit { start, end, arc: Some((centre, normal, doc(mid))), radius })
    }

    /// A loop as corners in a plane (`k`: the two axes it is drawn in) and its arcs, `ring` of a form.
    fn ring(&self, lp: &Value, k: [usize; 2], ring: Ring, hole: usize) -> Result<(Vec<[Expr; 2]>, Vec<Bend>, Vec<P2>), String> {
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
                });
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
    if let Some(s) = def.get("show").and_then(|s| serde_json::from_value::<Show>(s.clone()).ok())
        && s != Show::default()
    {
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
                let (axis, _) = axis_of(&def["normal"]).ok_or("its sketch is on a sloped plane")?;
                let (plane, k) = sketch(axis);
                let (start, end) = (self.coord(&def["start"], axis)?, self.coord(&def["end"], axis)?);
                let order = |e: &Expr| self.first_number(e);
                let (from, to) = if order(&start)? <= order(&end)? { (start, end) } else { (end, start) };
                if kind == "blend" {
                    let one = |key: &str| def[key].as_array().and_then(|l| l.first()).cloned().ok_or(format!("its {key} profile is missing"));
                    let (bottom, top) = (one("bottom")?, one("top")?);
                    let (profile, mut arcs, _) = self.ring(&bottom, k, Ring::Profile, 0)?;
                    let (top, top_arcs, _) = self.ring(&top, k, Ring::Top, 0)?;
                    arcs.extend(top_arcs);
                    return Ok(vec![Form { plane, profile, top, arcs, from, to, ..base }]);
                }
                let loops = def["loops"].as_array().filter(|l| !l.is_empty() && l.len() <= MOST).ok_or("its sketch has no loops")?;
                let rings = loops.iter().map(|l| self.ring(l, k, Ring::Profile, 0)).collect::<Result<Vec<_>, _>>()?;
                // Outer loops each a form, the loops inside them its holes (one inside a hole is an outer again).
                let depth: Vec<usize> = rings
                    .iter()
                    .enumerate()
                    .map(|(i, r)| rings.iter().enumerate().filter(|(j, o)| *j != i && r.2.first().is_some_and(|p| inside(*p, &o.2))).count())
                    .collect();
                let mut out = Vec::new();
                for (i, r) in rings.iter().enumerate().filter(|(i, _)| depth[*i] % 2 == 0) {
                    let mut form = Form { plane, profile: r.0.clone(), arcs: r.1.clone(), from: from.clone(), to: to.clone(), ..base.clone() };
                    for (j, h) in rings.iter().enumerate() {
                        if depth[j] == depth[i] + 1 && h.2.first().is_some_and(|p| inside(*p, &r.2)) {
                            let hole = form.holes.len();
                            form.arcs.extend(h.1.iter().cloned().map(|b| Bend { ring: Ring::Hole, hole, ..b }));
                            form.holes.push(h.0.clone());
                        }
                    }
                    out.push(form);
                }
                Ok(out)
            }
            "revolution" => {
                let loops = def["loops"].as_array().filter(|l| !l.is_empty()).ok_or("its sketch has no loops")?;
                if loops.len() > 1 {
                    return Err("a revolve of more than one loop".into());
                }
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
                let (profile, arcs, _) = self.ring(&loops[0], k, Ring::Profile, 0)?;
                let (mut from, mut to) = (self.value(&def["start_angle"])?, self.value(&def["end_angle"])?);
                // Mirrored, it turns the other way about its axis.
                if self.map.y_sign < 0.0 {
                    (from, to) = (neg(to), neg(from));
                }
                let axis = Some([[a0[k[0]].clone(), a0[k[1]].clone()], [a1[k[0]].clone(), a1[k[1]].clone()]]);
                Ok(vec![Form { plane, profile, arcs, axis, from, to, ..base }])
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
                            clockwise: self.map.y_sign < 0.0,
                            normal: Some(normal),
                            ..Bend::default()
                        });
                    }
                    points.push(bit.start);
                    last = Some(bit.end);
                }
                points.extend(last);
                let section = |key: &str, ring: Ring, arcs: &mut Vec<Bend>| -> Result<Vec<[Expr; 3]>, String> {
                    let lp = def[key].as_array().and_then(|l| l.first()).ok_or(format!("its {key} is missing"))?;
                    let mut out = Vec::new();
                    for c in lp.as_array().into_iter().flatten() {
                        let bit = self.curve(c)?;
                        if let Some((centre, normal, _)) = bit.arc {
                            arcs.push(Bend {
                                ring,
                                edge: out.len(),
                                centre: centre.to_vec(),
                                clockwise: self.map.y_sign < 0.0,
                                normal: Some(normal),
                                ..Bend::default()
                            });
                        }
                        out.push(bit.start);
                    }
                    Ok(out)
                };
                let section_start = section("profile", Ring::Section, &mut arcs)?;
                let section_end = if kind == "swept_blend" { section("profile_end", Ring::SectionEnd, &mut arcs)? } else { Vec::new() };
                if def["profile"].as_array().is_some_and(|l| l.len() > 1) {
                    return Err("a sweep of a profile of more than one loop".into());
                }
                Ok(vec![Form { path: points, section: section_start, section_end, arcs, ..base }])
            }
            "" => Err("it says no kind".into()),
            k => Err(format!("a form of a kind BIMCraft doesn't have yet ({k})")),
        }
    }

    /// `e`'s number in the first type.
    fn first_number(&self, e: &Expr) -> Result<f64, String> {
        number(e, &self.values, &mut HashMap::new())
    }

    /// A symbolic line's curves as symbols in its view.
    fn lines(&self, def: &Value) -> Result<Vec<Symbol>, String> {
        let view = match def["view"].as_str().unwrap_or("plan") {
            "plan" => SymbolView::Plan,
            "front" => SymbolView::Front,
            "side" => SymbolView::Side,
            // A model line is drawn as the forms are, which BIMCraft's families don't hold.
            _ => return Ok(Vec::new()),
        };
        let k = match view {
            SymbolView::Plan => [0, 1],
            SymbolView::Front => [0, 2],
            SymbolView::Side => [1, 2],
        };
        let show = def.get("show").and_then(|s| serde_json::from_value::<Show>(s.clone()).ok());
        let levels = show.map(|s| Levels { coarse: s.coarse, medium: s.medium, fine: s.fine });
        let base = Symbol {
            view,
            show: levels,
            visible: def.get("visible").and_then(text).map(|v| Expr::Formula(v.to_string())),
            subcategory: def.get("subcategory").and_then(text).filter(|s| self.family.subcategories.iter().any(|x| x.name == *s)).map(str::to_string),
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
                    let radius = bit.radius.clone().unwrap_or(Expr::Number(((s[k[0]] - cn[k[0]]).hypot(s[k[1]] - cn[k[1]]) * 10.0).round() / 10.0));
                    out.push(Symbol {
                        arc: Some(Curve { centre: [centre[k[0]].clone(), centre[k[1]].clone()], radius, from: Expr::Number(a0), to: Expr::Number(to) }),
                        ..base.clone()
                    });
                }
            }
        }
        Ok(out)
    }
}

/// `def` (Revit's definition of family `f`) translated into it: its references, dimensions, parametric forms and symbols,
/// nested parts and type catalog, beside the forms and symbols it has (Revit's result, which become `snapshot` ones). What
/// can't be translated is left out and said in `editable.untranslated`; materials `has_material` doesn't know are left off.
pub fn translate(f: &mut Family, def: &Value, has_material: &dyn Fn(&str) -> bool) {
    for x in f.forms.iter_mut() {
        x.snapshot = true;
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
        f.editable = Some(super::Editable { ok: false, reason: None, untranslated: vec!["its definition isn't one BIMCraft can read".into()] });
        return;
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
        let Some((axis, _)) = axis_of(&p["normal"]) else {
            notes.push(format!("reference plane `{}` is sloped: BIMCraft's references are square to the family's axes", p["name"].as_str().unwrap_or(&id)));
            continue;
        };
        let wanted = p.get("name").and_then(text).unwrap_or("").to_string();
        let mut name = if nameable(&wanted) { wanted } else { format!("Reference {}", i + 1) };
        let stem = name.clone();
        for n in 2.. {
            if used.insert(name.clone()) {
                break;
            }
            name = format!("{stem} {n}");
        }
        let at: BTreeMap<String, f64> =
            p["at"].as_object().into_iter().flatten().filter_map(|(t, v)| Some((t.clone(), num(v)?))).collect();
        refs.push((id.clone(), name.clone(), axis, p.clone()));
        planes.insert(id, (name, axis, at));
    }
    let map = Map {
        x_offset: num(&to["x_offset"]).unwrap_or(0.0),
        y_sign: if num(&to["y_sign"]).is_some_and(|s| s < 0.0) { -1.0 } else { 1.0 },
        z_minus: Expr::Number(0.0),
    };
    let mut reader = Reader { map, planes, params, first: first.clone(), notes: Vec::new(), family: f, values: BTreeMap::new() };
    if let Some(z) = to.get("z_minus").filter(|z| !z.is_null()) {
        match reader.value(z) {
            Ok(e) => reader.map.z_minus = e,
            Err(e) => notes.push(format!("where its z starts: {e}")),
        }
    }
    reader.first_values();
    // References: where each stands in the first type, taken into the family's axes; the origin's at the origin.
    let mut references = Vec::new();
    for (_, name, axis, p) in &refs {
        let doc = if p["defines_origin"].as_bool().unwrap_or(false) {
            Expr::Number(0.0)
        } else {
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
    // Dimensions between planes; alignments locked between two planes.
    let mut dimensions = Vec::new();
    for d in def["dimensions"].as_array().into_iter().flatten().take(super::MAX_DIMENSIONS) {
        let kind = match d["kind"].as_str().unwrap_or("linear") {
            "angular" => DimensionKind::Angular,
            "radial" => DimensionKind::Radial,
            "diameter" => DimensionKind::Diameter,
            _ => DimensionKind::Linear,
        };
        let ids: Vec<String> = d["refs"].as_array().into_iter().flatten().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).collect();
        let found: Vec<&(String, Axis, BTreeMap<String, f64>)> = ids.iter().filter_map(|i| reader.planes.get(i)).collect();
        let label = d.get("label").and_then(text).map(str::to_string);
        let (locked, eq) = (d["locked"].as_bool().unwrap_or(false), d["eq"].as_bool().unwrap_or(false));
        if found.len() != ids.len() || found.len() < 2 || kind != DimensionKind::Linear {
            if (label.is_some() || locked || eq) && kind == DimensionKind::Linear {
                notes.push(format!(
                    "a {} dimension measures to something other than reference planes: it doesn't place them",
                    if eq { "EQ" } else { "labelled or locked" }
                ));
            }
            continue;
        }
        if found.iter().any(|p| p.1 != found[0].1) {
            notes.push("a dimension measures between reference planes square to different axes".into());
            continue;
        }
        let label = match label {
            Some(l) if reader.params.contains(&l) => Some(l),
            Some(l) => {
                notes.push(format!("a dimension is labelled `{l}`, which isn't a number parameter of the family"));
                None
            }
            None => None,
        };
        let axis = found[0].1;
        let sign = if axis == Axis::Y { reader.map.y_sign } else { 1.0 };
        let sense: Vec<i8> = found
            .windows(2)
            .map(|w| {
                let gap = f.types.keys().filter_map(|t| Some(w[1].2.get(t)? - w[0].2.get(t)?)).find(|g| g.abs() > 1e-6).unwrap_or(1.0);
                if gap * sign < 0.0 { -1 } else { 1 }
            })
            .collect();
        let value = d["value"].as_object().and_then(|v| v.get(&first).or_else(|| v.values().next())).and_then(num).or_else(|| num(&d["value"]));
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
        let ids: Vec<&(String, Axis, BTreeMap<String, f64>)> = c["refs"].as_array().into_iter().flatten().filter_map(|v| reader.planes.get(v.as_str()?)).collect();
        if let [a, b] = ids.as_slice()
            && a.1 == b.1
        {
            dimensions.push(Dimension { refs: vec![a.0.clone(), b.0.clone()], locked: true, value: Some(0.0), ..Dimension::default() });
        }
    }
    // Forms and lines.
    let mut forms = Vec::new();
    for (i, d) in def["forms"].as_array().into_iter().flatten().take(MOST).enumerate() {
        match reader.forms(d, has_material) {
            Ok(made) => forms.extend(made),
            Err(e) => notes.push(format!("form {} ({}): {e}", i + 1, d["kind"].as_str().unwrap_or("?"))),
        }
    }
    let mut symbols = Vec::new();
    for (i, d) in def["lines"].as_array().into_iter().flatten().take(MOST).enumerate() {
        match reader.lines(d) {
            Ok(made) => symbols.extend(made),
            Err(e) => notes.push(format!("line {}: {e}", i + 1)),
        }
    }
    // Nested families, placed: mirrored by the family's own axes where they are.
    let mut nested = Vec::new();
    for (i, n) in def["nested"].as_array().into_iter().flatten().take(super::MAX_NESTED).enumerate() {
        let made = (|| -> Result<Nested, String> {
            let family = n.get("family").and_then(text).ok_or("it names no family")?.to_string();
            let type_name = n.get("type").and_then(text).ok_or("it names no type")?.to_string();
            let at = reader.point(&n["at"])?;
            let mut rotation = n.get("rotation").map(|r| reader.value(r)).transpose()?.unwrap_or_default();
            let mut mirrored = n["mirrored"].as_bool().unwrap_or(false);
            if reader.map.y_sign < 0.0 {
                rotation = neg(rotation);
                mirrored = !mirrored;
            }
            let mut params = BTreeMap::new();
            for (k, b) in n["params"].as_object().into_iter().flatten() {
                match reader.value(b) {
                    Ok(e) => {
                        params.insert(k.clone(), e);
                    }
                    Err(e) => return Err(format!("its `{k}`: {e}")),
                }
            }
            let visible = n.get("visible").and_then(text).map(|v| Expr::Formula(v.to_string()));
            Ok(Nested { family, type_name, at, rotation, mirrored, params, visible, ..Nested::default() })
        })();
        match made {
            Ok(x) => nested.push(x),
            Err(e) => notes.push(format!("nested family {} ({}): {e}", i + 1, n["family"].as_str().unwrap_or("?"))),
        }
    }
    let catalog = def.get("type_catalog").filter(|c| !c.is_null()).and_then(|c| serde_json::from_value::<Catalog>(c.clone()).ok());
    notes.append(&mut reader.notes);
    f.references = references;
    f.dimensions = dimensions;
    f.catalog = catalog;
    // What is made is tried in every type: a form or a line that doesn't work out in one is left out, and said.
    let (forms, symbols, nested) = tried(f, forms, symbols, nested, &mut notes);
    f.forms.extend(forms);
    f.symbols.extend(symbols);
    f.nested = nested;
    notes.truncate(64);
    f.editable = Some(super::Editable { ok: false, reason: None, untranslated: notes });
}

/// `forms`, `symbols` and `nested` parts kept where each works out in every type of `f` (its references where they're
/// placed); one that doesn't is left out, and said in `notes`.
fn tried(f: &Family, forms: Vec<Form>, symbols: Vec<Symbol>, nested: Vec<Nested>, notes: &mut Vec<String>) -> (Vec<Form>, Vec<Symbol>, Vec<Nested>) {
    let values: Vec<BTreeMap<String, Val>> = f.types.keys().filter_map(|t| f.values(t, &BTreeMap::new()).ok()).collect();
    let mut probe = Family { forms: Vec::new(), symbols: Vec::new(), nested: Vec::new(), check: BTreeMap::new(), ..f.clone() };
    let mut kept_forms = Vec::new();
    for (i, form) in forms.into_iter().enumerate() {
        probe.forms = vec![form];
        let fails = values.iter().find_map(|v| probe.solid(0, v, &mut HashMap::new()).err());
        match fails {
            None => kept_forms.extend(probe.forms.drain(..)),
            Some(e) => notes.push(format!("form {} as translated doesn't build: {e}", i + 1)),
        }
    }
    let mut kept_symbols = Vec::new();
    for (i, y) in symbols.into_iter().enumerate() {
        probe.symbols = vec![y];
        let fails = values.iter().find_map(|v| probe.marks_where(v, &|_| true).err());
        match fails {
            None => kept_symbols.extend(probe.symbols.drain(..)),
            Some(e) => notes.push(format!("line {} as translated doesn't work out: {e}", i + 1)),
        }
    }
    let mut kept_nested = Vec::new();
    for n in nested {
        let mut read = HashMap::new();
        let ok = values.iter().all(|v| {
            n.at.iter().chain([&n.rotation]).chain(&n.visible).chain(n.params.values()).all(|e| number(e, v, &mut read).is_ok())
        });
        if ok {
            kept_nested.push(n);
        } else {
            notes.push(format!("nested family `{}`: its placement doesn't work out", n.family));
        }
    }
    (kept_forms, kept_symbols, kept_nested)
}
