# bimcraft-mep-calc

The MEP calculations, each by its standard, knowing nothing of the model: a calculation takes plain numbers and tables and
answers with the sizes and checks its standard asks for. Their commands (`circuit`, `demand`, `water_size`, ...) are in the
core's `commands.rs`, which reads the model and calls these; their tests through the commands are the core's
`calc_tests/`, the tests of the sums themselves are each module's `tests.rs` here. `bimcraft-core` re-exports each module under
its old path. The rule sets and fixture libraries are in `data/`.

## Modules

- `hydrant.rs`: the `hydrant_size` command, hydrants and hose reels by IT 22/2025.
- `loads.rs`: the `load_points` command, a dwelling's lighting and outlets by room, by rule sets in `data/load-points.json` (NBR 5410 item 9.5.2 first).
- `dr.rs`: the `dr_check` command, which circuits NBR 5410 asks a 30 mA residual-current device for.
- `demand.rs`: the `demand` command, an installation's demand and service category by a utility's own standard.
- `dps.rs`: the `dps_check` command, surge protection of a power line by NBR 5410 (when, where, connection, currents).
- `water.rs`, `reservoir.rs`: `water_size`, cold water by NBR 5626, and `reservoir_size`, its reservation. `plumbing.rs`: the fixtures library (`data/plumbing.json`) they and `drain_size` read.
- `storm.rs`: the `storm_size` command, storm drains by NBR 10844 (Manning, the downpipe abacuses).
- `drainage.rs`: the `drain_size` and `vent_size` commands, sanitary drains and vents by NBR 8160's tables.
- `conduit.rs`: the `conduit_fill` command, a conduit's fill by NBR 5410 (makers' catalogues in `conduit/tables.rs`).
- `nbr5410.rs`: the `circuit` command's sizing of a circuit by NBR 5410 (tables in `nbr5410/tables.rs`; [structure and MEP](../../docs/areas/structure-and-mep.md)).
