//! The `nbr5410` calculation driven through its command.

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("circuit", &p).unwrap_or_else(|e| panic!("circuit {p}: {e}"))
    }

    #[test]
    fn the_circuit_command_sizes_a_circuit_and_changes_nothing() {
        let mut s = Session::new();
        let before = s.execute("inspect", &json!({})).map(|v| v["history"].clone()).unwrap_or_default();
        let r = run(&mut s, json!({"load": 1800, "grouped": 4, "ambient": 40, "length": 25000}));
        assert_eq!((&r["section_mm2"], &r["breaker_a"]), (&json!(4.0), &json!(16.0)));
        assert_eq!(r["conductors"], json!("1 × 4 + N 4 + PE 4 mm²"));
        assert_eq!(r["carries_a"], json!(18.1));
        assert_eq!(r["current_a"], json!(14.17));
        assert_eq!(r["factors"]["temperature"], json!(0.87));
        assert_eq!(r["factors"]["grouping"], json!(0.65));
        assert_eq!(r["drop_percent"], json!(3.3), "25 m of flexible 4 mm2 at 14.2 A in 127 V");
        assert_eq!(r["grew"], json!(true));
        assert_eq!(s.execute("inspect", &json!({})).map(|v| v["history"].clone()).unwrap_or_default(), before, "no undo step");
        assert_eq!(crate::engine::find("circuit").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_three_phase_circuit_at_another_voltage_and_a_chosen_section() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"load": 20000, "phases": "3FN", "system": "220/380", "section": 70, "reduced_neutral": true}));
        assert_eq!(r["volts"], json!(380.0));
        assert_eq!((&r["section_mm2"], &r["breaker_a"]), (&json!(70.0), &json!(32.0)), "never smaller than chosen");
        assert_eq!(r["conductors"], json!("3 × 70 + N 35 + PE 35 mm²"));
        assert_eq!(r["chosen"]["section_mm2"], json!(70.0));
        assert_eq!(r["grew"], json!(true), "a breaker was still to choose");
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "load"),
            (json!({"load": -5}), "load"),
            (json!({"load": 100, "voltage": 220}), "voltage"),
            (json!({"load": 100, "phases": "2F"}), "phases"),
            (json!({"load": 100, "method": "99"}), "method"),
            (json!({"load": 100, "section": 3}), "section"),
            (json!({"load": 100, "breaker": 7}), "breaker"),
            (json!({"load": 100, "power_factor": 0}), "power_factor"),
            (json!({"load": 100, "ambient": 5}), "10 degrees"),
            (json!({"load": 100, "ambient": 85, "insulation": "epr"}), "10 degrees"),
            (json!({"load": 1e8}), "split the circuit"),
            (json!({"load": f64::MAX}), "load"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("circuit", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
