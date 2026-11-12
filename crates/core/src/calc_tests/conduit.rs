//! The `conduit` calculation driven through its command.

mod command {
    use serde_json::{Value, json};

    use crate::engine::Session;

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("conduit_fill", &p).unwrap_or_else(|e| panic!("conduit_fill {p}: {e}"))
    }

    #[test]
    fn the_command_gives_the_fill_the_smallest_size_and_its_sources() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"section": 2.5, "circuits": 4}));
        assert_eq!((&r["conductors"], &r["smallest"]), (&json!(12), &json!("3/4\"")));
        assert_eq!(r["fill_percent"], json!(null), "no size asked: only the smallest");
        let r = run(&mut s, json!({"section": 4, "circuits": 4, "size": "3/4\""}));
        assert_eq!((&r["limit_percent"], &r["ok"], &r["smallest"]), (&json!(40.0), &json!(false), &json!("1\"")));
        assert!(r["fill_percent"].as_f64().is_some_and(|p| p > 40.0 && p < 44.0));
        assert!(r["source"]["url"].as_str().is_some_and(|u| u.starts_with("https://")));
        let r = run(&mut s, json!({"section": 35, "phases": "3FN", "family": "steel-rir"}));
        assert_eq!((&r["conductors"], &r["smallest"]), (&json!(5), &json!("1.1/4\"")));
        let r = run(&mut s, json!({"section": 300}));
        assert!(r["pending"].as_str().is_some_and(|p| p.contains("300")), "no diameter: pending, never guessed");
        assert_eq!(crate::engine::find("conduit_fill").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_bad_parameter_says_which() {
        let mut s = Session::new();
        for (bad, says) in [
            (json!({}), "section"),
            (json!({"section": 2.5, "family": "wood"}), "family"),
            (json!({"section": 2.5, "size": "9\""}), "size"),
            (json!({"section": 2.5, "phases": "2F"}), "phases"),
            (json!({"section": 2.5, "circuits": 0}), "circuits"),
            (json!({"section": 2.5, "pipe": 1}), "pipe"),
            (json!([]), "object"),
        ] {
            let e = s.execute("conduit_fill", &bad).expect_err("should fail");
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
