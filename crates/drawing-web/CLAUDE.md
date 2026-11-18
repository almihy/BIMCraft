# bimcraft-drawing-web

The agent's drawings in the browser: a DWG or DXF read once into CADCraft's index and asked of from it, as a WebAssembly
of its own, small beside the engine's (`crates/web`). Arqiflow runs it in a Web Worker (its `web/lib/compute/worker.ts`)
for `drawing_read` and `file_view`; the design is Arqiflow's `docs/superpowers/specs/2026-11-16-dwg-drawings-design.md`.

- It takes one thing of the engine, the outlines a drawing's lines close (`bimcraft_core::cad::outlines`, Pick Lines), so
  a drawing read alone gives the agent the outlines a drawing imported into a model does; the linker keeps only that.
- Blocks stay blocks: nothing is exploded but what a question asks for.
