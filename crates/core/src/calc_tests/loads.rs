//! The `loads` calculation driven through its command.

use crate::loads::*;

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    #[test]
    fn rooms_given_are_added_up() {
        let mut s = Session::new();
        let r = run(
            &mut s,
            "load_points",
            json!({"rooms": [
                {"name": "Cozinha", "area": 9, "perimeter": 12000},
                {"name": "Banheiro", "area": 4, "perimeter": 8000, "points": 2},
                {"name": "Closet", "area": 3, "perimeter": 7000, "kind": "dry"},
                {"name": "Depósito", "area": 3, "perimeter": 7000},
            ]}),
        );
        let k = &r["rooms"][0];
        assert_eq!(
            (&k["kind"], &k["tug_points"], &k["tug_va"], &k["counter_outlets"]),
            (&json!("kitchen"), &json!(4), &json!(1900.0), &json!(2))
        );
        assert_eq!((&r["rooms"][1]["tug_points"], &r["rooms"][1]["tug_va"]), (&json!(2), &json!(1200.0)));
        assert_eq!(r["rooms"][2]["kind"], "dry");
        assert!(r["rooms"][3]["note"].as_str().is_some_and(|n| n.contains("other")), "{r}");
        assert_eq!(r["lighting_va"], 400.0);
        assert_eq!(r["tug_va"], 1900.0 + 1200.0 + 200.0 + 100.0);
        assert_eq!(r["ok"], true);
        assert_eq!(crate::engine::find("load_points").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn fewer_points_than_the_least_are_not_ok() {
        let mut s = Session::new();
        let r = run(&mut s, "load_points", json!({"rooms": [{"name": "Sala", "area": 20, "perimeter": 18000, "points": 2}]}));
        assert_eq!((&r["rooms"][0]["tug_minimum"], &r["rooms"][0]["ok"], &r["ok"]), (&json!(4), &json!(false), &json!(false)));
    }

    #[test]
    fn the_alternative_is_taken_only_when_asked_and_possible() {
        let mut s = Session::new();
        let rooms = json!([
            {"name": "Cozinha", "area": 12, "perimeter": 14000},
            {"name": "Banheiro", "area": 4, "perimeter": 8000, "points": 2},
            {"name": "Lavanderia", "area": 5, "perimeter": 9000},
        ]);
        let a = run(&mut s, "load_points", json!({"rooms": rooms}));
        let b = run(&mut s, "load_points", json!({"rooms": rooms, "alternative": true}));
        assert_eq!((&a["alternative_possible"], &a["alternative_applied"]), (&json!(true), &json!(false)));
        assert_eq!(b["alternative_applied"], true);
        assert!(b["tug_va"].as_f64() < a["tug_va"].as_f64());
    }

    #[test]
    fn the_rooms_of_the_model_are_measured_inside_the_walls() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0], [5000, 4000], [0, 4000]], "closed": true}));
        run(&mut s, "room", json!({"point": [2500, 2000], "name": "Dormitório"}));
        let r = run(&mut s, "load_points", json!({}));
        let room = &r["rooms"][0];
        let area = room["area_m2"].as_f64().unwrap_or(0.0);
        assert!(area > 15.0 && area < 20.0, "inside the walls, less than 5 m by 4 m: {area}");
        assert_eq!(room["kind"], "dry");
        assert_eq!(room["lighting_va"], super::lighting_va(area));
        assert_eq!(r["skipped"], json!([]));
    }

    #[test]
    fn hostile_input_is_refused_and_changes_nothing() {
        let mut s = Session::new();
        for p in [
            json!([]),
            json!({"rooms": 5}),
            json!({"rooms": [5]}),
            json!({"rooms": [{"name": "A"}]}),
            json!({"rooms": [{"name": "A", "area": -1, "perimeter": 1}]}),
            json!({"rooms": [{"name": "A", "area": 1e300, "perimeter": 1}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "points": -1}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "points": 1e300}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "kind": "attic"}]}),
            json!({"rooms": [{"name": "A", "area": 1, "perimeter": 1, "colour": "red"}]}),
            json!({"alternative": "yes"}),
            json!({"units": 1}),
        ] {
            assert!(s.execute("load_points", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn the_answer_names_the_standard_and_each_room_s_source() {
        let mut s = Session::new();
        let r = run(&mut s, "load_points", json!({"rooms": [{"name": "Cozinha", "area": 9, "perimeter": 12000}]}));
        assert_eq!(r["standard"], "NBR 5410:2004");
        let src = r["rooms"][0]["source"].as_str().unwrap_or_default();
        assert!(src.contains("9.5.2.2.1") && src.contains("9.5.2.2.2"), "{r}");
        assert!(r["lighting_source"].as_str().is_some_and(|t| t.contains("9.5.2.1")), "{r}");
        let same =
            run(&mut s, "load_points", json!({"standard": "NBR 5410:2004", "rooms": [{"name": "Cozinha", "area": 9, "perimeter": 12000}]}));
        assert_eq!(same, r);
        assert!(s.execute("load_points", &json!({"standard": "NEC 2023", "rooms": []})).is_err());
    }
}
