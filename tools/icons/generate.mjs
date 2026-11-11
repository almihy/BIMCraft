#!/usr/bin/env node
// Writes crates/ui-egui/src/icons/drawn.rs: the icons BIMCraft draws from lucide (ISC) and Tabler Icons (MIT), as outlines
// on the 24 grid that go through the same pen as BIMCraft's own drawings (icons.rs). Nothing of the SVGs is kept at run
// time: each path, circle, rect, line, polyline, polygon and ellipse is flattened here into points, curves and arcs in
// steps of at most 2.8 degrees (as icons.rs's own arcs).
//
// Which icon comes from where is in sources.json ("icons": an Icon's name, then "lucide:<name>" or "tabler:<name>"), with
// the versions the drawings were taken from. To update or change the mapping:
//
//   cd tools/icons && npm install --no-save lucide-static@<version> @tabler/icons@<version>
//   node tools/icons/generate.mjs            (from the repository's root)
//
// then set the versions in sources.json and NOTICE to the ones installed. Either lucide-static or lucide-react will do
// (the same drawings); other places to read the packages from can be given: --lucide <package dir> --tabler <package dir>.
// --compare also writes the "compare" candidates (a review aid, not committed).

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const args = process.argv.slice(2);
const arg = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const config = JSON.parse(readFileSync(join(here, "sources.json"), "utf8"));
const out = resolve(root, arg("--out") ?? "crates/ui-egui/src/icons/drawn.rs");
const compare = args.includes("--compare");

function packageDir(flag, names) {
  const given = arg(flag);
  if (given) return resolve(given);
  for (const name of names) {
    const dir = join(here, "node_modules", name);
    if (existsSync(dir)) return dir;
  }
  throw new Error(`${names[0]} not found: npm install it in tools/icons, or pass ${flag} <package dir>`);
}

const lucideDir = packageDir("--lucide", [config.lucide.package, config.lucide.also]);
const tablerDir = packageDir("--tabler", [config.tabler.package]);
const version = (dir) => JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).version;
const versions = { lucide: version(lucideDir), tabler: version(tablerDir) };
for (const source of ["lucide", "tabler"]) {
  if (versions[source] !== config[source].version) {
    throw new Error(`${source} is ${versions[source]}, sources.json says ${config[source].version}: update sources.json (and NOTICE)`);
  }
}

// ---- Reading the drawings: a list of [element, attributes].

function attributes(text) {
  const out = {};
  for (const m of text.matchAll(/([a-zA-Z-]+)\s*=\s*"([^"]*)"/g)) out[m[1]] = m[2];
  return out;
}

function svgElements(svg) {
  const body = svg.replace(/<svg[^>]*>/, "").replace(/<\/svg>/, "");
  return [...body.matchAll(/<(path|circle|rect|line|polyline|polygon|ellipse)\b([^>]*)\/?>/g)].map((m) => [m[1], attributes(m[2])]);
}

function lucide(name) {
  const svg = join(lucideDir, "icons", `${name}.svg`);
  if (existsSync(svg)) return svgElements(readFileSync(svg, "utf8"));
  // lucide-react: the icon's node list, [["path", { d: "…", key: "…" }], …].
  const js = readFileSync(join(lucideDir, "dist/esm/icons", `${name}.mjs`), "utf8");
  // An old name is a re-export of the icon's present one.
  const alias = js.match(/export \{ default \} from '\.\/([\w-]+)\.mjs'/);
  if (alias) return lucide(alias[1]);
  // The icon's data is a plain object literal: { name, size, node: [["path", { d: "…", key: "…" }], …] }.
  const data = js.match(/const __iconData = (\{[\s\S]*?\n\});/);
  if (!data) throw new Error(`lucide ${name}: no icon data`);
  return new Function(`return ${data[1]}`)().node.map(([kind, attrs]) => [kind, attrs]);
}

function tabler(name) {
  const elements = svgElements(readFileSync(join(tablerDir, "icons/outline", `${name}.svg`), "utf8"));
  // Each Tabler icon opens with an invisible 24 × 24 frame (stroke="none").
  return elements.filter(([, a]) => a.stroke !== "none" && a.fill !== "currentColor");
}

// ---- Flattening into outlines: { points: [[x, y]…], closed }.

const STEP = (2.8 * Math.PI) / 180;

function arcPoints(cx, cy, rx, ry, phi, start, sweep) {
  const steps = Math.max(2, Math.ceil(Math.abs(sweep) / STEP));
  const [c, s] = [Math.cos(phi), Math.sin(phi)];
  const pts = [];
  for (let i = 1; i <= steps; i++) {
    const t = start + (sweep * i) / steps;
    const [x, y] = [rx * Math.cos(t), ry * Math.sin(t)];
    pts.push([cx + c * x - s * y, cy + s * x + c * y]);
  }
  return pts;
}

// SVG's endpoint arc to its centre (SVG 1.1, appendix F.6.5).
function svgArc([x1, y1], rx, ry, angle, large, sweepFlag, [x2, y2]) {
  if (rx === 0 || ry === 0) return [[x2, y2]];
  rx = Math.abs(rx);
  ry = Math.abs(ry);
  const phi = (angle * Math.PI) / 180;
  const [c, s] = [Math.cos(phi), Math.sin(phi)];
  const [dx, dy] = [(x1 - x2) / 2, (y1 - y2) / 2];
  const [xp, yp] = [c * dx + s * dy, -s * dx + c * dy];
  const lambda = (xp * xp) / (rx * rx) + (yp * yp) / (ry * ry);
  if (lambda > 1) {
    rx *= Math.sqrt(lambda);
    ry *= Math.sqrt(lambda);
  }
  const num = rx * rx * ry * ry - rx * rx * yp * yp - ry * ry * xp * xp;
  const den = rx * rx * yp * yp + ry * ry * xp * xp;
  let k = Math.sqrt(Math.max(0, num / den));
  if (large === sweepFlag) k = -k;
  const [cxp, cyp] = [(k * rx * yp) / ry, (-k * ry * xp) / rx];
  const [cx, cy] = [c * cxp - s * cyp + (x1 + x2) / 2, s * cxp + c * cyp + (y1 + y2) / 2];
  const ang = (ux, uy, vx, vy) => Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy);
  const start = ang(1, 0, (xp - cxp) / rx, (yp - cyp) / ry);
  let sweep = ang((xp - cxp) / rx, (yp - cyp) / ry, (-xp - cxp) / rx, (-yp - cyp) / ry);
  if (!sweepFlag && sweep > 0) sweep -= 2 * Math.PI;
  if (sweepFlag && sweep < 0) sweep += 2 * Math.PI;
  const pts = arcPoints(cx, cy, rx, ry, phi, start, sweep);
  pts[pts.length - 1] = [x2, y2];
  return pts;
}

// A Bézier curve, in steps fine enough that its turning is no more than an arc's.
function bezier(control) {
  let length = 0;
  let turn = 0;
  for (let i = 1; i < control.length; i++) {
    length += Math.hypot(control[i][0] - control[i - 1][0], control[i][1] - control[i - 1][1]);
  }
  for (let i = 2; i < control.length; i++) {
    const a = Math.atan2(control[i - 1][1] - control[i - 2][1], control[i - 1][0] - control[i - 2][0]);
    const b = Math.atan2(control[i][1] - control[i - 1][1], control[i][0] - control[i - 1][0]);
    let d = Math.abs(b - a);
    if (d > Math.PI) d = 2 * Math.PI - d;
    turn += d;
  }
  const steps = Math.min(96, Math.max(4, Math.ceil(turn / STEP), Math.ceil(length / 0.5)));
  const pts = [];
  for (let i = 1; i <= steps; i++) {
    const t = i / steps;
    let p = control.map((q) => [...q]);
    while (p.length > 1) p = p.slice(1).map((q, j) => [p[j][0] + (q[0] - p[j][0]) * t, p[j][1] + (q[1] - p[j][1]) * t]);
    pts.push(p[0]);
  }
  return pts;
}

function pathOutlines(d) {
  const outlines = [];
  let cur = null;
  let pos = [0, 0];
  let start = [0, 0];
  let lastControl = null;
  let lastCommand = "";
  // A scanner over the path's text: SVG lets numbers run together ("1-2", ".5.5") and arc flags too ("a1 1 0 00.7-1").
  let i = 0;
  const skip = () => {
    while (i < d.length && /[\s,]/.test(d[i])) i++;
  };
  const num = () => {
    skip();
    const m = /^[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?/.exec(d.slice(i));
    if (!m) throw new Error(`path ${d}: a number expected at ${i}`);
    i += m[0].length;
    return parseFloat(m[0]);
  };
  const flag = () => {
    skip();
    if (d[i] !== "0" && d[i] !== "1") throw new Error(`path ${d}: a flag expected at ${i}`);
    return d[i++] === "1";
  };
  const begin = (p) => {
    if (cur && cur.points.length > 1) outlines.push(cur);
    cur = { points: [p], closed: false };
    start = p;
  };
  let command = "";
  for (skip(); i < d.length; skip()) {
    if (/[a-zA-Z]/.test(d[i])) command = d[i++];
    else if (!command) throw new Error(`path ${d}: no command`);
    const rel = command === command.toLowerCase();
    const at = (x, y) => (rel ? [pos[0] + x, pos[1] + y] : [x, y]);
    const C = command.toUpperCase();
    let control = null;
    switch (C) {
      case "M": {
        pos = at(num(), num());
        begin(pos);
        command = rel ? "l" : "L"; // further pairs are lines
        break;
      }
      case "L":
        pos = at(num(), num());
        cur.points.push(pos);
        break;
      case "H":
        pos = [rel ? pos[0] + num() : num(), pos[1]];
        cur.points.push(pos);
        break;
      case "V":
        pos = [pos[0], rel ? pos[1] + num() : num()];
        cur.points.push(pos);
        break;
      case "C":
      case "S": {
        const c1 = C === "C" ? at(num(), num()) : /[CS]/i.test(lastCommand) && lastControl ? [2 * pos[0] - lastControl[0], 2 * pos[1] - lastControl[1]] : pos;
        const c2 = at(num(), num());
        const end = at(num(), num());
        cur.points.push(...bezier([pos, c1, c2, end]));
        control = c2;
        pos = end;
        break;
      }
      case "Q":
      case "T": {
        const c1 = C === "Q" ? at(num(), num()) : /[QT]/i.test(lastCommand) && lastControl ? [2 * pos[0] - lastControl[0], 2 * pos[1] - lastControl[1]] : pos;
        const end = at(num(), num());
        cur.points.push(...bezier([pos, c1, end]));
        control = c1;
        pos = end;
        break;
      }
      case "A": {
        const [rx, ry, angle] = [num(), num(), num()];
        const large = flag();
        const sweep = flag();
        const end = at(num(), num());
        cur.points.push(...svgArc(pos, rx, ry, angle, large, sweep, end));
        pos = end;
        break;
      }
      case "Z":
        cur.closed = true;
        pos = start;
        outlines.push(cur);
        cur = { points: [pos], closed: false };
        break;
      default:
        throw new Error(`path ${d}: ${command}`);
    }
    lastControl = control;
    lastCommand = C;
  }
  if (cur && cur.points.length > 1) outlines.push(cur);
  return outlines;
}

function ellipsePoints(cx, cy, rx, ry) {
  const n = 128;
  return Array.from({ length: n }, (_, k) => [cx + rx * Math.cos((2 * Math.PI * k) / n), cy + ry * Math.sin((2 * Math.PI * k) / n)]);
}

function elementOutlines([kind, a]) {
  const f = (k, fallback = 0) => (a[k] === undefined ? fallback : parseFloat(a[k]));
  const pairs = (s) => {
    const n = s.trim().split(/[\s,]+/).map(Number);
    return Array.from({ length: n.length / 2 }, (_, k) => [n[2 * k], n[2 * k + 1]]);
  };
  switch (kind) {
    case "path":
      return pathOutlines(a.d);
    case "circle":
      return [{ points: ellipsePoints(f("cx"), f("cy"), f("r"), f("r")), closed: true }];
    case "ellipse":
      return [{ points: ellipsePoints(f("cx"), f("cy"), f("rx"), f("ry")), closed: true }];
    case "line":
      return [{ points: [[f("x1"), f("y1")], [f("x2"), f("y2")]], closed: false }];
    case "polyline":
      return [{ points: pairs(a.points), closed: false }];
    case "polygon":
      return [{ points: pairs(a.points), closed: true }];
    case "rect": {
      const [x, y, w, h] = [f("x"), f("y"), f("width"), f("height")];
      let rx = a.rx !== undefined ? f("rx") : f("ry");
      let ry = a.ry !== undefined ? f("ry") : rx;
      rx = Math.min(rx, w / 2);
      ry = Math.min(ry, h / 2);
      if (rx <= 0 || ry <= 0) return [{ points: [[x, y], [x + w, y], [x + w, y + h], [x, y + h]], closed: true }];
      const q = Math.PI / 2;
      const corner = (cx, cy, from) => [[cx + rx * Math.cos(from), cy + ry * Math.sin(from)], ...arcPoints(cx, cy, rx, ry, 0, from, q)];
      return [
        {
          points: [
            ...corner(x + w - rx, y + ry, -q),
            ...corner(x + w - rx, y + h - ry, 0),
            ...corner(x + rx, y + h - ry, q),
            ...corner(x + rx, y + ry, 2 * q),
          ],
          closed: true,
        },
      ];
    }
    default:
      throw new Error(kind);
  }
}

// An outline's points, repeated points dropped; one that ends where it began is closed.
function tidy(o) {
  const pts = [];
  for (const p of o.points) {
    const q = pts[pts.length - 1];
    if (!q || Math.hypot(p[0] - q[0], p[1] - q[1]) > 1e-4) pts.push(p);
  }
  let closed = o.closed;
  if (pts.length > 2 && Math.hypot(pts[0][0] - pts.at(-1)[0], pts[0][1] - pts.at(-1)[1]) <= 1e-4) {
    pts.pop();
    closed = true;
  }
  // A dot ("M12 17h.01"): kept as the short stroke the SVG draws, so the pen's round ends make it a dot.
  return { points: pts.length >= 2 ? pts : [pts[0], [pts[0][0] + 0.01, pts[0][1]]], closed: closed && pts.length > 2 };
}

function drawing(ref) {
  const [source, name] = ref.split(":");
  const elements = source === "lucide" ? lucide(name) : source === "tabler" ? tabler(name) : null;
  if (!elements) throw new Error(`${ref}: the source is lucide or tabler`);
  return { source, name, outlines: elements.flatMap(elementOutlines).map(tidy) };
}

// ---- The Rust.

const n = (v) => {
  const r = Math.round(v * 1000) / 1000;
  return Number.isInteger(r) ? `${r}.0` : `${r}`;
};
const constName = (s) => s.replace(/[^a-zA-Z0-9]+/g, "_").toUpperCase();
const drawn = (constant, d) =>
  `static ${constant}: Drawn = Drawn {\n    source: Source::${d.source === "lucide" ? "Lucide" : "Tabler"},\n    name: "${d.name}",\n    outlines: &[\n` +
  d.outlines.map((o) => `        (${o.closed}, &[${o.points.map(([x, y]) => `[${n(x)}, ${n(y)}]`).join(", ")}]),\n`).join("") +
  `    ],\n};\n`;

const mapped = Object.entries(config.icons).map(([icon, ref]) => [icon, drawing(ref)]);
let rust = `// @generated by tools/icons/generate.mjs from sources.json: lucide ${versions.lucide} (ISC) and Tabler Icons ${versions.tabler}
// (MIT), their notices in NOTICE. Do not edit: change sources.json and run \`node tools/icons/generate.mjs\`.

//! The icons drawn from lucide's and Tabler's drawings, as outlines on the 24 grid.
#![allow(clippy::approx_constant)] // A point at 3.14 is a point, not π.

use super::Icon;

/// Where a drawing comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Lucide,
    Tabler,
}

/// A drawing taken from lucide or Tabler: its name there, and its outlines (closed or not, and the points on the 24 grid).
#[derive(Debug)]
pub struct Drawn {
    pub source: Source,
    pub name: &'static str,
    pub outlines: &'static [(bool, &'static [[f32; 2]])],
}

/// The versions the drawings were taken from.
pub const LUCIDE_VERSION: &str = "${versions.lucide}";
pub const TABLER_VERSION: &str = "${versions.tabler}";

/// The drawing \`icon\` is drawn from, if it is not BIMCraft's own.
pub fn drawn(icon: Icon) -> Option<&'static Drawn> {
    match icon {
${mapped.map(([icon]) => `        Icon::${icon} => Some(&${constName(icon)}),\n`).join("")}        _ => None,
    }
}
`;
for (const [icon, d] of mapped) rust += "\n" + drawn(constName(icon), d);
if (compare) {
  const candidates = Object.entries(config.compare).flatMap(([icon, refs]) => refs.map((ref) => [icon, ref, drawing(ref)]));
  rust += `\n/// Candidates for review (generate.mjs --compare): the icon, and a drawing it might be drawn from.\npub static CANDIDATES: &[(Icon, &Drawn)] = &[\n`;
  rust += candidates.map(([icon, ref]) => `    (Icon::${icon}, &C_${constName(ref)}_${constName(icon)}),\n`).join("") + "];\n";
  for (const [icon, ref, d] of candidates) rust += "\n" + drawn(`C_${constName(ref)}_${constName(icon)}`, d);
}
writeFileSync(out, rust);
console.log(`${out}: ${mapped.length} icons from lucide ${versions.lucide} and Tabler ${versions.tabler}${compare ? ", with candidates" : ""}`);
