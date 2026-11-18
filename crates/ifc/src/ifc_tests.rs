//! `ifc`'s tests that need only the model.

use crate::ifc::*;
use std::collections::BTreeSet;

#[test]
fn step_strings_escape_quotes_backslashes_and_other_scripts() {
    assert_eq!(s("it's"), "'it''s'");
    assert_eq!(s(r"a\b"), r"'a\\b'");
    assert_eq!(s("Área"), r"'\X2\00C1\X0\rea'");
    // Past the Basic Multilingual Plane, UCS-4: as IFC readers expect, not UTF-16's surrogates.
    assert_eq!(s("Tile 😀"), r"'Tile \X4\0001F600\X0\'");
    assert_eq!(s("x');#9=IFCWALL("), "'x'');#9=IFCWALL('", "a name can't end the string early");
}

#[test]
fn reals_always_carry_a_decimal_point() {
    assert_eq!([real(1500.0), real(0.00001), real(-0.0), real(-200.5), real(f64::NAN)], ["1500.", "0.00001", "0.", "-200.5", "0."]);
}

#[test]
fn global_ids_are_22_base64_characters_and_stable() {
    let a = guid(7, 5, 12);
    assert_eq!(a.len(), 22);
    assert!(matches!(a.as_bytes()[0], b'0'..=b'3'), "{a}");
    assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'$'), "{a}");
    assert_eq!(a, guid(7, 5, 12));
    let all: BTreeSet<String> = (0..2000).flat_map(|id| [guid(7, 5, id), guid(7, 6, id), guid(8, 5, id)]).collect();
    assert_eq!(all.len(), 6000);
}

#[test]
fn timestamps_are_utc_iso_dates() {
    assert_eq!(utc(0), "1970-01-01T00:00:00");
    assert_eq!(utc(951_868_800), "2000-03-01T00:00:00");
    assert_eq!(utc(1_000_000_000), "2001-09-09T01:46:40");
}
