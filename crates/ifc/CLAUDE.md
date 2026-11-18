# bimcraft-ifc

IFC4 export (`ifc`) and import (`import`, with `import/shapes.rs` for the geometry it reads). Above `bimcraft-model`, beside
the drawings: it needs the model's elements and their quantities, never a view, so a change to the drawing code does not
rebuild it and a change here rebuilds only the core above. The model's modules are re-exported in `lib.rs` under their
usual paths (`crate::model`, `crate::walls`), so code moved here kept its `use` lines.

- An IFC GlobalId as text is `ids::compress` and `ids::expand`, in the model: the inquiry and the importer both read it.
- IFC4's class list is the model's (`ifc_classes`, re-exported here as `ifc::classes`): families name their class.
- Tests that need only the model and this crate stay here (`ifc_tests.rs`, `import_tests.rs`); those that drive the
  Session are in the core (`ifc_*_tests.rs`), as for every crate under it.
