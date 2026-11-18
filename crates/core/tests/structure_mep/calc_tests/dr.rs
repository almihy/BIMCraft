//! The `dr` calculation driven through its command.

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("dr_check", &p).unwrap_or_else(|e| panic!("dr_check {p}: {e}"))
    }

    #[test]
    fn circuits_come_back_with_what_they_need() {
        let mut s = Session::new();
        let r = run(
            &mut s,
            json!({"circuits": [
                {"name": "C1 bath", "points": [{"place": "bath", "kind": "outlet"}], "residual": 30},
                {"name": "C2 bedrooms", "points": [{"place": "dry", "kind": "outlet"}]},
                {"name": "C3 kitchen", "points": [{"place": "kitchen", "kind": "outlet", "rating": 20}]},
                {"name": "C4 pump", "points": [{"place": "external", "kind": "outlet"}], "it_continuity": true},
            ]}),
        );
        let c = &r["circuits"];
        assert_eq!((&c[0]["required"], &c[0]["ok"]), (&json!(true), &json!(true)));
        assert_eq!((&c[1]["required"], &c[1]["ok"]), (&json!(false), &json!(null)));
        assert_eq!((&c[2]["required"], &c[2]["ok"]), (&json!(true), &json!(false)));
        assert_eq!(c[3]["exempt"], true);
        assert_eq!(r["missing"], json!(["C3 kitchen"]));
        assert_eq!(r["max_residual_ma"], 30.0);
        assert!(r["not_covered"].as_str().is_some_and(|n| n.contains("volume")), "{r}");
        assert_eq!(crate::engine::find("dr_check").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_building_that_is_not_a_dwelling_is_said() {
        let mut s = Session::new();
        let pts = json!([{"place": "kitchen", "kind": "lighting"}]);
        let r = run(&mut s, json!({"circuits": [{"name": "A", "points": pts}]}));
        assert_eq!(r["circuits"][0]["required"], true);
        let r = run(&mut s, json!({"building": "other", "circuits": [{"name": "A", "points": pts}]}));
        assert_eq!(r["circuits"][0]["required"], false);
    }

    #[test]
    fn hostile_input_is_refused() {
        let mut s = Session::new();
        let ok = json!({"place": "bath", "kind": "outlet"});
        for p in [
            json!([]),
            json!({}),
            json!({"circuits": 5}),
            json!({"circuits": [5]}),
            json!({"circuits": [{"name": "A"}]}),
            json!({"circuits": [{"name": "A", "points": []}]}),
            json!({"circuits": [{"name": "A", "points": [5]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "attic", "kind": "outlet"}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "lamp"}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "rating": -1}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "rating": 1e300}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "height": "x"}]}]}),
            json!({"circuits": [{"name": "A", "points": [{"place": "bath", "kind": "outlet", "colour": 1}]}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()], "residual": -5}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()], "it_continuity": "yes"}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()], "units": 1}]}),
            json!({"building": "castle", "circuits": [{"name": "A", "points": [ok.clone()]}]}),
            json!({"circuits": [{"name": "A", "points": [ok.clone()]}], "units": 1}),
        ] {
            assert!(s.execute("dr_check", &p).is_err(), "{p}");
        }
    }
}
