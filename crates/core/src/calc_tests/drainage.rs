//! The `drainage` calculation driven through its command.

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    #[test]
    fn drain_size_gives_the_dn_by_fixtures_or_by_units() {
        let mut s = Session::new();
        let r = run(&mut s, "drain_size", json!({"fixtures": {"wc": 2, "lav": 3}}));
        assert_eq!((&r["units"], &r["dn"], &r["capacity"], &r["minimum_dn"]), (&json!(15.0), &json!(100.0), &json!(160.0), &json!(100.0)));
        let r = run(&mut s, "drain_size", json!({"units": 200, "role": "main", "slope": 0.005}));
        assert_eq!((&r["dn"], &r["capacity"]), (&json!(200.0), &json!(1400.0)));
        let r = run(&mut s, "drain_size", json!({"units": 300, "role": "stack", "floors": 5}));
        assert_eq!((&r["dn"], &r["capacity"]), (&json!(100.0), &json!(500.0)));
        let r = run(&mut s, "drain_size", json!({"units": 3, "min_dn": 75}));
        assert_eq!(r["dn"], json!(75.0), "never smaller than a drain flowing into it");
        assert_eq!(crate::engine::find("drain_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn vent_size_gives_a_branch_vent_or_a_vent_stack() {
        let mut s = Session::new();
        assert_eq!(run(&mut s, "vent_size", json!({"units": 15, "wc": true}))["dn"], json!(50.0));
        assert_eq!(run(&mut s, "vent_size", json!({"units": 15}))["dn"], json!(50.0));
        assert_eq!(run(&mut s, "vent_size", json!({"units": 30}))["dn"], json!(75.0));
        let r = run(&mut s, "vent_size", json!({"stack_dn": 100, "units": 140, "length": 20000}));
        assert_eq!(r["dn"], json!(75.0));
        assert_eq!(r["longest_m"], json!(61.0));
        let r = run(&mut s, "vent_size", json!({"units": 4, "trap_dn": 50}));
        assert_eq!(r["trap_to_vent_mm"], json!(1200.0));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (id, bad, says) in [
            ("drain_size", json!({}), "units"),
            ("drain_size", json!({"units": 10, "role": "riser"}), "role"),
            ("drain_size", json!({"fixtures": {"bidet": 1}}), "bidet"),
            ("drain_size", json!({"fixtures": {"wc": -1}}), "wc"),
            ("drain_size", json!({"units": 1.0e6}), "tables"),
            ("drain_size", json!({"units": 10, "pipe": 1}), "pipe"),
            ("vent_size", json!({}), "units"),
            ("vent_size", json!({"units": 500}), "table"),
            ("vent_size", json!({"stack_dn": 125, "units": 10, "length": 1000}), "stack_dn"),
            ("vent_size", json!({"stack_dn": 100, "units": 10}), "length"),
            ("vent_size", json!({"units": 4, "trap_dn": 60}), "trap_dn"),
            ("vent_size", json!([]), "object"),
        ] {
            let e = s.execute(id, &bad).expect_err("should fail");
            assert!(e.contains(says), "{id} {bad}: {e}");
        }
    }
}
