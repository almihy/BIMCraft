// The Manifold trial under Node (docs/manifold-trial.md): the engine's wasm, built with `--target nodejs` into ./node, cuts a
// cylinder out of a box with Manifold and tells its volume. Expected volume 803966.4.
const { manifold_trial } = require('./node/bimcraft_web.js');
const t0 = performance.now();
const r = JSON.parse(manifold_trial());
const ms = (performance.now() - t0).toFixed(1);
const want = 1e6 - 100 * 32 * 25 * 25 * Math.sin((2 * Math.PI) / 64);
console.log(`manifold_trial() -> volume ${r.volume}, ${r.triangles} triangles (${ms} ms); expected volume ${want}`);
if (Math.abs(r.volume - want) > 1e-6 || r.triangles <= 100) {
  console.error('FAILED');
  process.exit(1);
}
console.log('OK');
