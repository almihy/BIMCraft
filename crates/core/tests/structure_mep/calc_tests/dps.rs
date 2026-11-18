//! The `dps` calculation driven through its command.

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("dps_check", &p).unwrap_or_else(|e| panic!("dps_check {p}: {e}"))
    }

    #[test]
    fn an_overhead_line_in_aq2_gets_the_whole_answer() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ2", "overhead": true, "earthing": "TN-S", "neutral": true, "phases": 3}));
        let p = &r["power"];
        assert_eq!(p["required"], true);
        assert_eq!(p["connection"]["schemes"], json!([2, 3]));
        assert_eq!(p["discharge_ka"]["each_mode"], 5.0);
        assert_eq!(p["discharge_ka"]["neutral_to_pe_in_scheme_3"], 20.0);
        assert_eq!(p["conductor"]["section_mm2"], 4.0);
        assert_eq!(p["conductor"]["max_length_m"], 0.5);
        assert!(p["where"].as_array().is_some_and(|w| w.len() == 2), "{r}");
        assert!(
            r["not_covered"].as_str().is_some_and(|n| !n.contains("Table 31") && !n.contains("Table 49") && n.contains("coordination")),
            "{r}"
        );
        assert_eq!(crate::engine::find("dps_check").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn the_least_uc_comes_with_the_earthing_and_the_voltage() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ3", "earthing": "TN-S", "uo": 127}));
        let uc = &r["power"]["uc_min"];
        assert_eq!(uc["uo_v"], 127.0);
        assert_eq!(uc["modes"][0]["between"], "phase-neutral");
        assert!(uc["modes"][0]["volts"].as_f64().is_some_and(|x| (x - 139.7).abs() < 1e-9), "{r}");
        assert_eq!(uc["modes"][2]["between"], "neutral-PE");
        // Without uo, it is the formulas.
        let r = run(&mut s, json!({"region": "AQ3", "earthing": "IT", "neutral": false}));
        assert_eq!(r["power"]["uc_min"]["modes"][0]["formula"], "U");
        assert!(r["power"]["uc_min"]["modes"][0].get("volts").is_none(), "{r}");
        let e = s.execute("dps_check", &json!({"region": "AQ3", "uo": -1})).unwrap_err();
        assert!(e.contains("uo"), "{e}");
    }

    #[test]
    fn the_up_comes_from_the_category_of_equipment_in_tabela_31() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ3", "system": "220/380", "signal_lines": ["telephone"]}));
        let up = &r["power"]["up"];
        assert_eq!((&up["system"], &up["max_up_kv"]), (&json!("220/380"), &json!(2.5)));
        assert_eq!(up["withstand_kv"], json!({"IV": 6.0, "III": 4.0, "II": 2.5, "I": 1.5}));
        let r = run(&mut s, json!({"region": "AQ3", "system": "127/220"}));
        assert_eq!(r["power"]["up"]["max_up_kv"], 1.5);
        // A signal line at the entry: 1.5 kV at least (note 3, IEC 61663-2).
        let r = run(&mut s, json!({"region": "AQ1", "signal_lines": ["data"]}));
        assert_eq!(r["signal"][0]["min_impulse_withstand_kv"], 1.5);
        for bad in [json!({"region": "AQ3", "system": "110"}), json!({"region": "AQ3", "system": 220})] {
            assert!(s.execute("dps_check", &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_subsequent_current_the_short_circuit_and_the_dr_come_with_the_connection() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ3", "earthing": "TT"}));
        assert_eq!(r["power"]["subsequent_current"], json!({"neutral_pe_min_a": 100.0}));
        assert!(r["power"]["short_circuit"].as_str().is_some_and(|t| t.contains("presumed")), "{r}");
        assert_eq!(r["power"]["connection"]["schemes"], json!([2, 3]));
        let r = run(&mut s, json!({"region": "AQ3", "earthing": "TT", "before_dr": true}));
        assert_eq!(r["power"]["connection"]["schemes"], json!([3]), "{r}");
        let r = run(&mut s, json!({"region": "AQ3", "earthing": "IT"}));
        assert_eq!(r["power"]["subsequent_current"], json!({"neutral_pe": "the same as the DPS between phase and neutral"}));
        assert!(s.execute("dps_check", &json!({"region": "AQ3", "before_dr": 3})).is_err());
    }

    #[test]
    fn direct_lightning_asks_for_more() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ3", "purpose": "direct", "earthing": "TT", "neutral": true, "phases": 1}));
        let p = &r["power"];
        assert_eq!(
            (&p["required"], &p["impulse_ka"]["each_mode"], &p["impulse_ka"]["neutral_to_pe_in_scheme_3"]),
            (&json!(true), &json!(12.5), &json!(25.0))
        );
        assert_eq!(p["conductor"]["section_mm2"], 16.0);
        assert_eq!(p["where"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn a_line_that_does_not_need_it_has_no_numbers() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ1", "overhead": false}));
        assert_eq!(r["power"]["required"], false);
        assert!(r["power"].get("where").is_none() && r["power"].get("connection").is_none(), "{r}");
    }

    #[test]
    fn external_signal_lines_always_ask_for_it() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ1", "signal_lines": ["telephone", "antenna"]}));
        let sig = r["signal"].as_array().cloned().unwrap_or_default();
        assert_eq!(sig.len(), 2);
        assert!(sig.iter().all(|x| x["required"] == true), "{r}");
    }

    #[test]
    fn the_risk_a_project_takes() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"region": "AQ2", "overhead": true, "accepted_risk": true}));
        assert_eq!((&r["power"]["required"], &r["power"]["exempt"]), (&json!(false), &json!(true)));
        let r = run(&mut s, json!({"region": "AQ2", "overhead": true, "accepted_risk": true, "risk_to_people": true}));
        assert_eq!((&r["power"]["required"], &r["power"]["exempt"]), (&json!(true), &json!(false)));
    }

    #[test]
    fn hostile_input_is_refused() {
        let mut s = Session::new();
        for p in [
            json!([]),
            json!({}),
            json!({"region": "AQ4"}),
            json!({"region": 2}),
            json!({"region": "AQ2", "overhead": "yes"}),
            json!({"region": "AQ2", "purpose": "sun"}),
            json!({"region": "AQ2", "earthing": "TNX"}),
            json!({"region": "AQ2", "phases": 2}),
            json!({"region": "AQ2", "phases": 1e300}),
            json!({"region": "AQ2", "signal_lines": "telephone"}),
            json!({"region": "AQ2", "signal_lines": ["radio"]}),
            json!({"region": "AQ2", "signal_lines": [5]}),
            json!({"region": "AQ2", "units": 1}),
        ] {
            assert!(s.execute("dps_check", &p).is_err(), "{p}");
        }
    }
}
