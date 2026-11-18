//! The `hydrant` calculation driven through its command.

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("hydrant_size", &p).unwrap_or_else(|e| panic!("hydrant_size {p}: {e}"))
    }

    #[test]
    fn a_system_and_a_pipe() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"column": 3, "area": 6000}));
        assert_eq!(r["system"], json!("3"));
        assert_eq!(r["reserve_m3"], json!(25.0));
        let r = run(&mut s, json!({"column": 5, "area": 8000}));
        assert_eq!(r["system"], json!("5"), "type 5 above 5000 m2");
        let r = run(&mut s, json!({"type": "3", "hydrants": 3, "inside": 68, "nominal": 65, "length": 20000, "rise": 4000}));
        assert_eq!(r["flow_lpm"], json!(400.0), "two jets of 200 L/min");
        assert_eq!(r["nozzle_head_mca"], json!(40.0));
        let loss = r["loss_mca"].as_f64().unwrap_or(f64::NAN);
        assert!((r["need_mca"].as_f64().unwrap_or(f64::NAN) - (40.0 + loss + 4.0)).abs() < 0.02);
        assert_eq!(r["speed_ok"], json!(true));
        assert_eq!(r["size_ok"], json!(true));
        assert_eq!(r["ok"], json!(true));
        assert!(r["need_covers"].as_str().is_some_and(|t| t.contains("not the hose")));
        let r = run(&mut s, json!({"column": 3, "area": 6000, "type": "3", "region": "SP"}));
        assert_eq!(r["type"], json!("3"), "a type that agrees is fine");
        let r = run(&mut s, json!({"type": "5", "inside": 40, "nominal": 40, "material": "copper"}));
        assert_eq!(r["speed_ok"], json!(false), "1200 L/min in 40 mm");
        assert_eq!(r["size_ok"], json!(false), "DN 40 is under DN 65");
        assert_eq!(r["ok"], json!(false));
        assert_eq!(crate::engine::find("hydrant_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "type"),
            (json!({"column": 6, "area": 100}), "column"),
            (json!({"column": 1}), "area"),
            (json!({"area": 100}), "column"),
            (json!({"column": 4, "area": 100, "inside": 68}), "4-40"),
            (json!({"type": "9"}), "type"),
            (json!({"type": "3", "hydrants": 0}), "hydrants"),
            (json!({"type": "3", "hydrants": 1.5}), "hydrants"),
            (json!({"type": "3", "inside": 0}), "inside"),
            (json!({"type": "3", "inside": 68, "material": "lead"}), "material"),
            (json!({"type": "3", "inside": 68, "length": -1}), "length"),
            (json!({"type": "3", "nominal": 65}), "inside"),
            (json!({"column": 3, "area": 6000, "type": "5"}), "contradicts"),
            (json!({"type": "3", "region": "RJ"}), "SP"),
            (json!({"type": "3", "length": 1.0e12, "inside": 68}), "length"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("hydrant_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
