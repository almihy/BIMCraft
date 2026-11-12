//! When NBR 5410:2004 asks surge protection (DPS) of a power line, where it stands, how it is connected and what it must
//! withstand at least (items 5.4.2.1.1, 6.3.5.2.1 to 6.3.5.2.5, 6.3.5.2.9 and Figure 13), worked by hand.

use super::*;

fn line(overhead: bool, region: Region) -> Line {
    Line { overhead, region, accepted_risk: false, risk_to_people: false }
}

#[test]
fn an_overhead_line_in_aq2_asks_for_it() {
    let v = required(&line(true, Region::Aq2));
    assert!(v.required && !v.exempt, "{v:?}");
    assert!(v.reasons[0].contains("overhead") && v.reasons[0].contains("AQ2"), "{:?}", v.reasons);
}

#[test]
fn an_underground_line_in_aq2_does_not() {
    assert!(!required(&line(false, Region::Aq2)).required);
}

#[test]
fn aq3_asks_for_it_whatever_the_line() {
    for overhead in [true, false] {
        let v = required(&line(overhead, Region::Aq3));
        assert!(v.required, "{v:?}");
        assert!(v.reasons.iter().any(|r| r.contains("AQ3")), "{:?}", v.reasons);
    }
}

#[test]
fn aq1_does_not_even_overhead() {
    assert!(!required(&line(true, Region::Aq1)).required);
}

#[test]
fn a_calculated_risk_may_be_taken_only_if_no_one_is_put_at_risk() {
    let mut l = line(true, Region::Aq2);
    l.accepted_risk = true;
    let v = required(&l);
    assert!(!v.required && v.exempt, "{v:?}");
    l.risk_to_people = true;
    let v = required(&l);
    assert!(v.required && !v.exempt, "never where the omission may hurt people: {v:?}");
    // Taking a risk is no reason when nothing asks for it.
    let mut free = line(false, Region::Aq1);
    free.accepted_risk = true;
    assert!(!required(&free).required && !required(&free).exempt);
}

#[test]
fn every_external_signal_line_asks_for_it_at_its_entry() {
    for kind in SIGNALS {
        assert!(signal_required(kind).is_some(), "{kind}");
    }
    assert!(signal_required("coax").is_none());
}

#[test]
fn where_it_stands_follows_what_it_is_for() {
    assert_eq!(place(Purpose::Line, false).len(), 2);
    assert!(place(Purpose::Line, false)[0].contains("point where the line enters") && place(Purpose::Line, false)[1].contains("main"));
    let direct = place(Purpose::Direct, false);
    assert_eq!(direct.len(), 1);
    assert!(direct[0].contains("point where the line enters"));
    // An existing individual unit may have it at the meter box, with its two conditions.
    let existing = place(Purpose::Line, true);
    assert!(existing.iter().any(|p| p.contains("meter") && p.contains("10 m") && p.contains("equipotential")), "{existing:?}");
    assert!(!place(Purpose::Direct, true).iter().any(|p| p.contains("meter")), "the meter box is for the line's surges only");
}

#[test]
fn the_connection_follows_the_earthing_and_the_neutral() {
    assert_eq!(schemes(Earthing::TnS, true), vec![2, 3]);
    assert_eq!(schemes(Earthing::Tt, true), vec![2, 3]);
    assert_eq!(schemes(Earthing::It, true), vec![2, 3]);
    assert_eq!(schemes(Earthing::TnC, true), vec![1]);
    // A circuit with no neutral, whatever the earthing; IT with no neutral distributed.
    for e in [Earthing::TnS, Earthing::Tt, Earthing::It, Earthing::TnC] {
        assert_eq!(schemes(e, false), vec![1], "{e:?}");
    }
    assert_eq!(scheme_text(1, Earthing::TnC), "each phase to the PE (the PEN)");
    assert!(scheme_text(2, Earthing::TnS).contains("neutral to the PE") && scheme_text(2, Earthing::TnS).contains("each phase"));
    assert!(scheme_text(3, Earthing::Tt).contains("each phase to the neutral"));
}

#[test]
fn the_least_discharge_and_impulse_currents_by_purpose() {
    assert_eq!(discharge(Purpose::Line, false, 3), 5.0);
    assert_eq!(discharge(Purpose::Line, false, 1), 5.0);
    // Between the neutral and the PE of connection 3: 20 kA on a three-phase network, 10 kA on a single-phase one.
    assert_eq!(discharge(Purpose::Line, true, 3), 20.0);
    assert_eq!(discharge(Purpose::Line, true, 1), 10.0);
    assert_eq!(impulse(false, 3), 12.5);
    assert_eq!(impulse(true, 3), 50.0);
    assert_eq!(impulse(true, 1), 25.0);
}

#[test]
fn the_connecting_conductors() {
    assert_eq!(conductor_section(Purpose::Line), 4.0);
    assert_eq!(conductor_section(Purpose::Direct), 16.0);
    assert_eq!(CONDUCTOR_LENGTH, 0.5);
}

#[test]
fn tabela_49_the_least_uc_by_earthing_and_connection() {
    let row = |e: Earthing, neutral: bool| -> Vec<(&'static str, f64)> {
        uc_min(e, neutral, 127.0).into_iter().map(|u| (u.between, u.volts)).collect()
    };
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let tt = row(Earthing::Tt, true);
    assert_eq!(tt.iter().map(|x| x.0).collect::<Vec<_>>(), ["phase-neutral", "phase-PE", "neutral-PE"]);
    assert!(near(tt[0].1, 139.7) && near(tt[1].1, 139.7) && near(tt[2].1, 127.0), "{tt:?}");
    assert_eq!(row(Earthing::TnS, true), tt.clone());
    let c = row(Earthing::TnC, true);
    assert_eq!(c.iter().map(|x| x.0).collect::<Vec<_>>(), ["phase-PEN"]);
    assert!(near(c[0].1, 139.7));
    let it = row(Earthing::It, true);
    assert_eq!(it.iter().map(|x| x.0).collect::<Vec<_>>(), ["phase-neutral", "phase-PE", "neutral-PE"]);
    assert!(near(it[0].1, 139.7) && near(it[1].1, 127.0 * 3f64.sqrt()) && near(it[2].1, 127.0), "{it:?}");
    // IT with no neutral distributed: phase to PE at U, the voltage between phases.
    let n = row(Earthing::It, false);
    assert_eq!(n.iter().map(|x| x.0).collect::<Vec<_>>(), ["phase-PE"]);
    assert!(near(n[0].1, 127.0 * 3f64.sqrt()));
    // A line with no neutral on TT or TN-S has no neutral modes: phase to PE only.
    let t = row(Earthing::Tt, false);
    assert_eq!(t.iter().map(|x| x.0).collect::<Vec<_>>(), ["phase-PE"]);
    assert!(near(t[0].1, 139.7));
}

#[test]
fn tabela_31_is_as_printed() {
    // [category IV, III, II, I] in kV
    assert_eq!(withstand("120/208"), Some([4.0, 2.5, 1.5, 0.8]));
    assert_eq!(withstand("127/220"), Some([4.0, 2.5, 1.5, 0.8]));
    for system in ["220/380", "230/400", "277/480"] {
        assert_eq!(withstand(system), Some([6.0, 4.0, 2.5, 1.5]), "{system}");
    }
    assert_eq!(withstand("400/690"), Some([8.0, 6.0, 4.0, 2.5]));
    assert_eq!(withstand("110/190"), None);
    assert_eq!(SIGNAL_ENTRY_WITHSTAND_KV, 1.5);
}

#[test]
fn the_subsequent_current_between_neutral_and_pe() {
    assert_eq!(subsequent_current(Earthing::Tt), Some(Subsequent::AtLeastA(100.0)));
    assert_eq!(subsequent_current(Earthing::TnS), Some(Subsequent::AtLeastA(100.0)));
    assert_eq!(subsequent_current(Earthing::It), Some(Subsequent::LikePhaseNeutral));
    assert_eq!(subsequent_current(Earthing::TnC), None, "TN-C has no neutral-PE device");
}

#[test]
fn a_tt_installation_with_the_dps_before_the_dr_takes_scheme_3_only() {
    assert_eq!(schemes_before_dr(Earthing::Tt, true), vec![3]);
    assert_eq!(schemes_before_dr(Earthing::TnS, true), vec![2, 3]);
    assert_eq!(schemes_before_dr(Earthing::Tt, false), vec![1]);
}
