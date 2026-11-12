// Makes the parity fixtures for site::parking from Arqiflow's own solver (web/lib/parking), which this port follows.
//
// Run from Arqiflow's web folder (it has tsx), one case per process (the solver numbers its aisles from a counter
// that runs on across solves in one process, and past a million it would misread them):
//
//   cd "G:/Software Development/arqiflow/web"
//   node --import tsx <this file> list            # how many cases
//   node --import tsx <this file> <i> <out.json>  # case i, solved
//
// A case is a solver case (`input`: the solver's input in metres, as solveParking takes it) or a car park case
// (`car_park`: a car park element and what stands round it, as carParkWrites takes them, in metres; the Rust side
// gives the same in millimetres to lay_out). Lengths in the output are kept to 0.1 mm.
import { solveParking, type ParkingInput, type ParkingLayout, type Pt } from "../../../../../../../arqiflow/web/lib/parking/index";
import { SAMPLES, randomPlot } from "../../../../../../../arqiflow/web/lib/parking/lab/check";
import { carParkChildren, carParkInput, carParkResult, type StoredLike } from "../../../../../../../arqiflow/web/lib/parking/carpark";
import { writeFileSync } from "node:fs";

const box = (x0: number, y0: number, x1: number, y1: number): Pt[] => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
const r4 = (v: number) => Math.round(v * 1e4) / 1e4;
const ring = (r: Pt[]) => r.map((p) => [r4(p[0]), r4(p[1])]);

type Case = { name: string; input?: ParkingInput; car_park?: { cp: StoredLike; elements: StoredLike[]; levels: { id: string; elevation: number }[]; streets: [Pt, Pt][] } };

function cases(): Case[] {
  const out: Case[] = [];
  SAMPLES.forEach((s, i) => out.push({ name: `sample ${i}`, input: s }));
  SAMPLES.forEach((s, i) => out.push({ name: `sample ${i} at 60°`, input: { ...s, settings: { angle: 60 } } }));
  for (let i = 0; i < 24; i++) out.push({ name: `random ${i}`, input: randomPlot() });
  out.push({ name: "below ground, no ramp", input: { site: box(0, 0, 80, 60), obstacles: [box(36, 26, 44, 34)], basement: {} } });
  out.push({ name: "below ground, a ramp drawn", input: { site: box(0, 0, 90, 64), obstacles: [box(40, 28, 48, 36)], basement: { ramps: [[[0, 0], [7, 0], [7, 27], [0, 27]]] } } });
  out.push({ name: "a building across the plot", input: { site: box(0, 0, 150, 40), obstacles: [box(60, -5, 80, 45)], streets: [0] } });
  out.push({ name: "a building in the middle", input: { site: [[0, 0], [68, 0], [71.4, 31.9], [63.5, 53.2], [11.6, 53.2], [0, 49.4]], obstacles: [box(27.7, 20.6, 43.7, 32.6)], streets: [0] } });
  out.push({ name: "short street side", input: { site: [[0, 0], [8, -3], [70, 30], [60, 90], [-10, 80]], streets: [0] } });
  out.push({ name: "removed, kept clear, ev, target", input: { site: box(0, 0, 80, 100), streets: [0], removed: [[20, 20], [41, 50]], keepClear: [box(30, 40, 50, 60)], target: [40, 100], settings: { evShare: 0.1 } } });
  out.push({ name: "quick", input: { site: [[0, 0], [70, 0], [70, 40], [110, 63], [90, 98], [40, 70], [0, 70]], streets: [0], quick: true } });
  out.push({ name: "us, 45°, island every 8", input: { site: box(0, 0, 90, 70), streets: [0], settings: { standard: "us", angle: 45, islandEvery: 8 } } });
  out.push({ name: "uk, held direction", input: { site: [[0, 0], [70, 15], [60, 80], [-10, 70]], streets: [0], direction: 30, settings: { standard: "uk" } } });
  // Car parks as the masterplan holds them.
  const level = { id: "L0", elevation: 0 };
  const below = { id: "B1", elevation: -3.5 };
  out.push({
    name: "car park on the ground, a lane along it",
    car_park: {
      cp: { id: "cp1", kind: "car_park", level: "L0", geometry: null, props: { control: [[0, 0], [90, 0], [90, 70], [0, 70]], setback: 1.5, access: [{ at: [45, 0], kind: "both" }], target: [45, 70] } },
      elements: [
        { id: "ln1", kind: "lane", level: "L0", use: "road", geometry: { type: "Polygon", coordinates: [[[-10, -8], [100, -8], [100, 0], [-10, 0], [-10, -8]]] }, props: {} },
        { id: "m1", kind: "mass", level: "L0", geometry: { type: "Polygon", coordinates: [[[60, 45], [85, 45], [85, 65], [60, 65], [60, 45]]] }, props: {} },
        { id: "fp", kind: "lane", level: "L0", use: "footpath", geometry: { type: "Polygon", coordinates: [[[90, -5], [95, -5], [95, 75], [90, 75], [90, -5]]] }, props: {} },
      ],
      levels: [level, below],
      streets: [[[-10, 70], [100, 70]]],
    },
  });
  out.push({
    name: "car park below ground, a ramp drawn",
    car_park: {
      cp: { id: "cp2", kind: "car_park", level: "B1", geometry: null, props: { control: [[0, 0], [0, 64], [90, 64], [90, 0]], column_every: 3, column_size: 0.6 } },
      elements: [{ id: "r1", kind: "ramp", level: "B1", geometry: { type: "Polygon", coordinates: [[[0, 0], [7, 0], [7, 27], [0, 27], [0, 0]]] }, props: {} }],
      levels: [level, below],
      streets: [],
    },
  });
  out.push({
    name: "car park, angled bays, a street edge",
    car_park: {
      cp: { id: "cp3", kind: "car_park", level: "L0", geometry: null, props: { control: [[0, 0], [70, 0], [70, 40], [110, 63], [90, 98], [40, 70], [0, 70]], angle: 60, stall_width: 2.5, stall_depth: 5, end_island: 2, removed: [[30, 30]] } },
      elements: [],
      levels: [level],
      streets: [[[-5, 0], [75, 0]]],
    },
  });
  // More random plots for a wider check (`node … more <i> <out>`), not kept in the repository.
  if (process.argv[2] === "more") for (let i = 24; i < 84; i++) out.push({ name: `random ${i}`, input: randomPlot() });
  return out;
}

function layout(L: ParkingLayout) {
  return {
    stalls: L.stalls.map((s) => ({ poly: ring(s.poly), kind: s.kind, mouth: [r4(s.mouth[0]), r4(s.mouth[1])] })),
    accessAisles: L.accessAisles.map(ring),
    aisles: L.aisles.map((a) => ({ poly: ring(a.poly), kind: a.kind, ...(a.flow ? { flow: [r4(a.flow[0]), r4(a.flow[1])] } : {}) })),
    columns: L.columns.map(ring),
    entries: L.entries.map((e) => ({ at: [r4(e.at[0]), r4(e.at[1])], kind: e.kind })),
    ramps: L.ramps.map((r) => ({ poly: ring(r.poly), found: r.found })),
    islands: L.islands.length,
    metrics: L.metrics,
    notes: L.notes,
    direction: L.direction,
    directions: L.directions,
    arrangement: L.arrangement,
  };
}

const all = cases();
if (process.argv[2] === "list") console.log(all.length);
else {
  const more = process.argv[2] === "more";
  if (more) process.argv.splice(2, 1);
  const c = more ? all[56 + Number(process.argv[2])] : all[Number(process.argv[2])];
  const t = performance.now();
  let out: Record<string, unknown>;
  if (c.input) {
    const L = solveParking(c.input);
    out = { name: c.name, input: c.input, out: layout(L) };
  } else {
    const k = c.car_park!;
    const { input } = carParkInput(k.cp, k.elements, k.levels, k.streets);
    const L = solveParking(input);
    const kids = carParkChildren(k.cp, L).map((e) => ({ kind: e.kind, props: e.props, ring: ring((e.geometry!.coordinates as Pt[][])[0].slice(0, -1)) }));
    out = { name: c.name, car_park: k, solver_input: input, pieces: kids, result: carParkResult(L, !!input.basement) };
  }
  out.ms = Math.round(performance.now() - t);
  writeFileSync(process.argv[3], JSON.stringify(out));
}
