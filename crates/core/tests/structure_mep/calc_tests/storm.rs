//! The `storm` calculation driven through its command.

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("storm_size", &p).unwrap_or_else(|e| panic!("storm_size {p}: {e}"))
    }

    #[test]
    fn a_downpipe_and_a_horizontal_drain() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"kind": "downpipe", "area": 100, "gutter": 50, "length": 6300}));
        assert_eq!(r["flow_lpm"], json!(250.0), "150 mm/h on 100 m2");
        assert_eq!(r["inside_mm"], json!(70.0), "under 70 mm on the chart: 70");
        let r = run(&mut s, json!({"kind": "downpipe", "area": 300, "gutter": 70, "length": 6350, "outlet": "funnel", "rainfall": 150}));
        assert_eq!(r["flow_lpm"], json!(750.0));
        assert!(r["inside_mm"].as_f64().is_some_and(|d| (70.0..100.0).contains(&d)));
        let r = run(&mut s, json!({"kind": "horizontal", "flow": 1000, "slope": 0.01}));
        assert!(r["inside_mm"].as_f64().is_some_and(|d| d > 130.0 && d < 165.0));
        let r = run(&mut s, json!({"kind": "horizontal", "flow": 1000, "slope": 0.01, "inside": 144.8}));
        assert_eq!(r["ok"], json!(false), "DN 150 (144.8 mm inside) carries about 770 L/min at 1 %: short of 1000");
        assert_eq!(crate::engine::find("storm_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "kind"),
            (json!({"kind": "gutter"}), "kind"),
            (json!({"kind": "downpipe", "area": 100}), "gutter"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 70}), "length"),
            (json!({"kind": "downpipe", "gutter": 70, "length": 3000}), "area"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 40, "length": 3000}), "50"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 70, "length": 100}), "0.3"),
            (json!({"kind": "downpipe", "area": 9000, "rainfall": 150, "gutter": 50, "length": 3000}), "150 mm inside"),
            (json!({"kind": "downpipe", "area": 300, "gutter": 70, "length": 3000}), "rainfall"),
            (json!({"kind": "downpipe", "area": 100, "flow": 500, "gutter": 70, "length": 3000}), "two ways"),
            (json!({"kind": "downpipe", "area": 100, "gutter": 70, "length": 3000, "slope": 0.01}), "does not apply"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.01, "gutter": 70}), "does not apply"),
            (json!({"kind": "horizontal", "area": 100}), "slope"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.001}), "0.5"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.01, "outlet": "tap"}), "does not apply"),
            (json!({"kind": "horizontal", "area": 100, "slope": 0.01, "pipe": 1}), "pipe"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("storm_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
