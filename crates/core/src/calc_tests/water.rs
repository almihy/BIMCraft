//! The `water` calculation driven through its command.

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("water_size", &p).unwrap_or_else(|e| panic!("water_size {p}: {e}"))
    }

    #[test]
    fn a_bathroom_group() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"fixtures": {"lav": 1, "wc": 1, "shower": 1}, "length": 10000}));
        assert_eq!(r["flow_lps"], json!(0.3));
        assert_eq!(r["dn"], json!(20.0), "17 mm inside: 1.3 m/s");
        assert_eq!(r["ok"], json!(true));
        let r = run(
            &mut s,
            json!({"flow": 0.3, "dn": 25, "length": 10000, "fittings": {"elbow90": 2, "tee_side": 1}, "pressure": 100, "rise": 2000}),
        );
        assert_eq!(r["equivalent_length_m"], json!(14.8), "10 m + 2 x 1.2 + 2.4");
        let loss = r["loss_kpa"].as_f64().unwrap_or(f64::NAN);
        let end = r["end_kpa"].as_f64().unwrap_or(f64::NAN);
        assert!((end - (100.0 - loss - 2.0 * 9.81)).abs() < 0.1, "{end} {loss}");
        assert_eq!(r["ok"], json!(end >= 10.0));
        let r = run(&mut s, json!({"flow": 1.5, "dn": 20}));
        assert_eq!(r["ok"], json!(false), "too fast in DN 20");
        assert_eq!(crate::engine::find("water_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "flow"),
            (json!({"flow": 0.3, "fixtures": {"wc": 1}}), "fixtures"),
            (json!({"fixtures": {"bidet": 1}}), "bidet"),
            (json!({"fixtures": {"wc": 1.5}}), "whole"),
            (json!({"fixtures": 3}), "object"),
            (json!({"flow": 0.3, "method": "magic"}), "method"),
            (json!({"flow": 0.3, "dn": 30}), "dn"),
            (json!({"flow": 100}), "DN"),
            (json!({"flow": 0.3, "fittings": {"tap": 1}, "dn": 25}), "tap"),
            (json!({"flow": 0.3, "dn": 25, "fittings": {"elbow90": -1}}), "elbow90"),
            (json!({"flow": 0.3, "length": -1}), "length"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("water_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
