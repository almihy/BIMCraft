//! Revit's Array (AR): `array`, copies of elements spaced along a line (linear) or turned about a point (radial), made as
//! `copy` and `rotate` make theirs, so the doors and windows of copied walls come along. Revit's "Group and Associate" is not
//! here: a model group is a later piece (docs/areas/desktop.md, "Array"), until then the copies stand on their own.

use serde_json::{Value, json};

use super::{Motion, R, absent, bad, count_opt, duplicate, ids_param, micron, num, only, point_opt, string};
use crate::engine::Session;
use crate::geom::P2;

/// The most members an array makes, the original among them: Revit stops at the same order of size.
pub(super) const MAX_MEMBERS: usize = 1000;
/// The most elements one array makes in all.
const MAX_MADE: usize = 10_000;

/// Which member the typed distance or angle reaches.
#[derive(Clone, Copy, PartialEq)]
enum MoveTo {
    /// The second member: the distance or angle is the step between members.
    Second,
    /// The last member: the distance or angle is the span from the first to the last, shared out evenly.
    Last,
}

pub(super) fn array(s: &mut Session, p: &Value) -> R {
    let c = "array";
    only(c, p, &["ids", "id", "mode", "count", "move_to", "by", "from", "to", "centre", "angle"])?;
    let ids = ids_param(c, &s.model, p)?;
    let count = count_opt(c, p, "count", 2)?;
    if !(2..=MAX_MEMBERS).contains(&count) {
        return Err(bad(c, format!("`count` is the members in all, the original among them: 2 to {MAX_MEMBERS}, not {count}")));
    }
    if ids.len().saturating_mul(count - 1) > MAX_MADE {
        return Err(bad(
            c,
            format!("{} elements in {count} members make more than {MAX_MADE} elements; array fewer, or fewer times", ids.len()),
        ));
    }
    let move_to = match string(c, p, "move_to")?.as_deref() {
        None | Some("second") => MoveTo::Second,
        Some("last") => MoveTo::Last,
        Some(other) => return Err(bad(c, format!("`move_to` is `second` or `last`, not `{other}`"))),
    };
    let mode = string(c, p, "mode")?;
    let radial = match mode.as_deref() {
        None | Some("linear") => false,
        Some("radial") => true,
        Some(other) => return Err(bad(c, format!("`mode` is `linear` or `radial`, not `{other}`"))),
    };
    let steps = count - 1;
    // Member `i` (1 to `steps`) is the original taken by `motion(i)`.
    let mut made = Vec::new();
    let mut members = Vec::new();
    if radial {
        for key in ["by", "from", "to"] {
            if !absent(p.get(key)) {
                return Err(bad(c, format!("`{key}` is for a linear array; a radial one takes `centre` and `angle`")));
            }
        }
        let centre = point_opt(c, p, "centre")?.ok_or_else(|| bad(c, "`centre` [x, y], the point the copies turn about, is required"))?;
        let angle = num(c, p, "angle")?.ok_or_else(|| bad(c, "`angle` in degrees, counter-clockwise, is required"))?;
        if angle == 0.0 || angle.abs() > 360.0 {
            return Err(bad(c, format!("`angle` is more than 0 and at most 360 degrees either way, not {angle}")));
        }
        // 360 degrees to the last member comes back to the first: the members share the circle, none on the original.
        let step = match move_to {
            MoveTo::Second => angle,
            MoveTo::Last if angle.abs() == 360.0 => angle / count as f64,
            MoveTo::Last => angle / steps as f64,
        };
        for i in 1..=steps {
            let (sin, cos) = (step * i as f64).to_radians().sin_cos();
            let turn = |q: P2| {
                let d = q - centre;
                micron(centre + P2(d.0 * cos - d.1 * sin, d.0 * sin + d.1 * cos))
            };
            let new = duplicate(c, s, &ids, &Motion { map: &turn, slide: None, mirror: false })?;
            members.push(new["ids"].clone());
            made.extend(new["ids"].as_array().cloned().unwrap_or_default());
        }
    } else {
        if !absent(p.get("centre")) || !absent(p.get("angle")) {
            return Err(bad(c, "`centre` and `angle` are for a radial array; a linear one takes `by`, or `from` and `to`"));
        }
        let by = match (point_opt(c, p, "by")?, point_opt(c, p, "from")?, point_opt(c, p, "to")?) {
            (Some(by), None, None) => by,
            (None, Some(from), Some(to)) => to - from,
            (None, None, None) => {
                return Err(bad(c, "`by` [dx, dy], or `from` and `to`, the two points the distance runs between, is required"));
            }
            _ => return Err(bad(c, "give `by` [dx, dy], or `from` and `to`, not a mix")),
        };
        if by.length() <= crate::model::TOL {
            return Err(bad(c, "the distance is more than 0 mm: the copies would stand on the original"));
        }
        let step = match move_to {
            MoveTo::Second => by,
            MoveTo::Last => by * (1.0 / steps as f64),
        };
        for i in 1..=steps {
            let at = micron(step * i as f64);
            let new = duplicate(c, s, &ids, &Motion { map: &|q| micron(q + at), slide: Some(at), mirror: false })?;
            members.push(new["ids"].clone());
            made.extend(new["ids"].as_array().cloned().unwrap_or_default());
        }
    }
    Ok(json!({"ids": made, "members": members}))
}
