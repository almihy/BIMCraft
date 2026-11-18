//! The `reservoir` calculation driven through its command.

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("reservoir_size", &p).unwrap_or_else(|e| panic!("reservoir_size {p}: {e}"))
    }

    #[test]
    fn a_building_by_its_people() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"people": 120, "per_person_l": 200, "fire_l": 15000, "pumps": 2, "compartments": 2}));
        assert_eq!(r["daily_consumption_l"], json!(24000.0));
        assert_eq!(r["potable_volume_l"], json!(24000.0));
        assert_eq!(r["total_volume_l"], json!(39000.0));
        assert_eq!(r["consumption"]["kind"], json!("configurable assumption"));
        assert_eq!(r["ok"], json!(true));
        assert!(r["pending"].as_array().is_some_and(|p| p.iter().any(|x| x == "refill")), "{r}");
        assert_eq!(crate::engine::find("reservoir_size").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_failed_check_makes_it_not_ok_and_the_others_still_run() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"daily_l": 10000, "days": 4, "pumps": 1}));
        assert_eq!(r["ok"], json!(false));
        let failed: Vec<&str> =
            r["checks"].as_array().into_iter().flatten().filter(|c| c["status"] == "fail").filter_map(|c| c["id"].as_str()).collect();
        assert_eq!(failed, vec!["potability", "pumps"]);
        assert_eq!(r["total_volume_l"], json!(40000.0));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "daily_l"),
            (json!({"daily_l": 1000, "people": 10}), "people"),
            (json!({"people": 10}), "per_person_l"),
            (json!({"per_person_l": 100}), "people"),
            (json!({"daily_l": -1}), "daily_l"),
            (json!({"daily_l": 1000, "days": 0}), "days"),
            (json!({"daily_l": 1000, "lower_percent": 120}), "lower_percent"),
            (json!({"daily_l": 1000, "pumps": 1.5}), "pumps"),
            (json!({"daily_l": 1000, "towers": 0}), "towers"),
            (json!({"daily_l": 1000, "house": "yes"}), "house"),
            (json!({"daily_l": 1000, "magic": 1}), "magic"),
            (json!("x"), "object"),
        ] {
            let e = s.execute("reservoir_size", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
