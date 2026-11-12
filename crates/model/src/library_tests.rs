//! `library`'s tests that need only the model.

use crate::ids::Guid;
use crate::library::*;

#[test]
fn parts_are_kept_by_id_and_by_guid_and_a_copy_shares_what_it_didnt_change() {
    let part = |i: usize, guid: Guid| Part { what: "sketch line", of: "floor x".into(), owner: "x".into(), index: i, guid };
    let (a, b, c) = (Guid::new(), Guid::new(), Guid::new());
    let mut parts: Parts = [(999_000, part(0, a)), (999_001, part(1, b)), (5_000, part(2, c))].into_iter().collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts.iter().map(|(id, _)| id).collect::<Vec<_>>(), [5_000, 999_000, 999_001]);
    assert_eq!((parts.first_id(), parts.last_id()), (Some(5_000), Some(999_001)));
    assert_eq!((parts.by_guid(a), parts.by_guid(c)), (Some(999_000), Some(5_000)));
    let copy = parts.clone();
    assert_eq!(copy, parts);
    // Two parts of one GUID (out of step): the other keeps it when one goes.
    parts.insert(999_002, part(3, a));
    assert_ne!(copy, parts);
    assert_eq!(parts.remove(999_000).map(|p| p.index), Some(0));
    assert_eq!(parts.by_guid(a), Some(999_002));
    assert_eq!(parts.remove(999_002).map(|p| p.guid), Some(a));
    assert_eq!(parts.by_guid(a), None);
    assert_eq!(parts.remove(999_002), None);
    // A part given again with another GUID.
    parts.insert(5_000, part(2, b));
    assert_eq!((parts.by_guid(c), parts.len()), (None, 2));
    parts.remove(999_001);
    assert_eq!(parts.by_guid(b), Some(5_000));
    // What the copy didn't change, it shares; and it holds what it held.
    assert_eq!(copy.len(), 3);
    assert_eq!(copy.by_guid(a), Some(999_000));
}
