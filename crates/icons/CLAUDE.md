# bimcraft-icons

The desktop's icons as data, apart from the interface so their 0.6 MB of generated outlines are not rebuilt with it: the
`Icon` list (`icon_names!` in `lib.rs`; a new icon is a name here) and `drawn.rs`, the drawings as outlines on the 24 grid,
lucide's and Arqiflow's own. `drawn.rs` is generated: change `tools/icons/sources.json` or the SVGs in `tools/icons/own` and
run `node tools/icons/generate.mjs`, never edit it. How they are drawn (the one pen, the small drawings, BIMCraft's own
drawings for icons with none here) is `crates/ui-egui/src/icons.rs`, which re-exports all of this as `icons::*`.
