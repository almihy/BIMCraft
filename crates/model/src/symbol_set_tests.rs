//! The AREI symbol set: every family loads through the real command, in each language, and draws where its data says.

use crate::family::{Category, Family};
use crate::family::{MAX_PROFILE, MAX_SYMBOLS};
use crate::symbol_set::*;

fn names(n: Names) -> Vec<String> {
    families("arei", n).into_iter().map(|(name, _)| name).collect()
}

#[test]
fn the_set_has_seventy_six_families_each_a_flat_symbol_of_one_type() {
    let all = families("arei", Names::English);
    assert_eq!(all.len(), 76);
    for (name, f) in &all {
        assert!(f.forms.is_empty(), "{name}: a flat plan symbol");
        assert_eq!(f.types.keys().collect::<Vec<_>>(), ["Standard"], "{name}");
        assert!(!f.symbols.is_empty() && f.symbols.len() <= MAX_SYMBOLS, "{name}");
        assert!(f.symbols.iter().all(|s| s.line.len() <= MAX_PROFILE), "{name}");
    }
    assert!(families("nothing", Names::English).is_empty());
}

#[test]
fn names_exist_in_each_language_and_are_unique_in_it_and_against_the_starter_families() {
    use crate::language::Language;
    for n in [Names::English, Names::Dutch, Names::French] {
        let list = names(n);
        let unique: std::collections::BTreeSet<&String> = list.iter().collect();
        assert_eq!(unique.len(), 76, "{n:?}");
        assert!(list.iter().all(|x| !x.trim().is_empty()), "{n:?}");
    }
    assert_ne!(names(Names::English), names(Names::Dutch));
    assert_ne!(names(Names::English), names(Names::French));
    // None collides with a family a new project has, so loading never skips one.
    for language in Language::ALL {
        let m = crate::model::Model::new_in("x", language);
        for n in [Names::English, Names::of(language)] {
            for name in names(n) {
                assert!(!m.families.contains_key(&name), "{language:?}: {name}");
            }
        }
    }
}

#[test]
fn the_categories_follow_the_kind_of_device() {
    let all: std::collections::BTreeMap<String, Family> = families("arei", Names::English).into_iter().collect();
    let cat = |n: &str| all[n].category;
    assert_eq!(cat("Switch, single-pole"), Category::Switch);
    assert_eq!(cat("Push button"), Category::Switch);
    assert_eq!(cat("Socket, protective contact"), Category::Outlet);
    assert_eq!(cat("Data socket"), Category::DataOutlet);
    assert_eq!(cat("Light point"), Category::Lighting);
    assert_eq!(cat("Cooker"), Category::Generic);
    assert_eq!(cat("Circuit breaker, 20 A curve C"), Category::Generic);
}
