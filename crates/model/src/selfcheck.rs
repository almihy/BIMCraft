//! The model's self-checks: a fast path held to its twin, the plain, slow way of working out the same (every element asked
//! instead of the lookup's, the whole scene drawn instead of what can show), which must give the same answer.
//!
//! They are a test's, as `debug_assert!` is: compiled into every debug build, so `cargo build`, `cargo test` of any crate and
//! clippy all build this crate alike, and out of every release build (and the browser's, which is one), where [`on`] is a
//! constant `false` and the optimiser drops each check whole. A debug build of an app runs none either: it calls [`off`]
//! first, as the twins cost what the fast paths save (a plan drawn twice, a section's picture of the whole scene).
//!
//! Each check is a [`Check`], so a test can ask which ran ([`ran`]) and fail when one no longer does.

/// A self-check, by the fast path it holds to its twin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Check {
    /// `render::plan_elements`: the elements a plan finds through the lookup are every element that shows in it.
    PlanElements,
    /// Rooms' outlines joined in a plan: the outlines near one, through the grid, are every outline near it.
    NearOutlines,
    /// A run's end carried on along: the one found through the grid of ends is the first of all of them.
    RunEnd,
    /// `render::plan_shows`: what a plan shows, among what may show in it, is what it shows among every element.
    PlanShows,
    /// A section's wall layers: the walls near its line are every wall whose layers cross it.
    SectionWalls,
    /// A section's extent: the model's, from the components' farthest points.
    ModelExtent,
    /// A section's heights: those of every solid its look shows.
    ViewHeights,
    /// How far along its line a section's solids reach: as far as every solid its look shows.
    ViewReach,
    /// `render::section_picture_in`: a section's picture, from what can show in it, is the whole scene's.
    SectionPicture,
    /// A section's cut: what lies all on one side of its line is cut by none of it.
    OneSide,
    /// The same, of the solids made as the cut needs them.
    OneSideLazy,
    /// A plan drawn among the elements that may show in it is the plan drawn among every element.
    DrawPlan,
    /// A region over a section's picture keeps the same ink through its grid as among all of it.
    RegionInk,
    /// `Model::referring`: what points at an element, from the lookup, is every element that does.
    Referring,
    /// A floor's cut by shafts: those found are every shaft that cuts it.
    Shafts,
    /// `Model::on_sheet`: what is on a sheet, from the lookup, is every element that points at it.
    OnSheet,
    /// A 3D view's dimensions, from the lookup, are every one in it.
    Dims3d,
    /// The dimensions that hold an element, from the lookup, are every one that does.
    HeldBy,
    /// A component's outline in plan, from the operation's geometry, is its pieces' outlines.
    ComponentOutline,
}

impl Check {
    /// Every check.
    pub const ALL: [Check; 19] = [
        Check::PlanElements,
        Check::NearOutlines,
        Check::RunEnd,
        Check::PlanShows,
        Check::SectionWalls,
        Check::ModelExtent,
        Check::ViewHeights,
        Check::ViewReach,
        Check::SectionPicture,
        Check::OneSide,
        Check::OneSideLazy,
        Check::DrawPlan,
        Check::RegionInk,
        Check::Referring,
        Check::Shafts,
        Check::OnSheet,
        Check::Dims3d,
        Check::HeldBy,
        Check::ComponentOutline,
    ];
}

#[cfg(debug_assertions)]
mod state {
    use super::Check;
    use std::sync::atomic::{AtomicBool, Ordering};

    static OFF: AtomicBool = AtomicBool::new(false);
    static RAN: [AtomicBool; Check::ALL.len()] = [const { AtomicBool::new(false) }; Check::ALL.len()];

    pub fn on(check: Check) -> bool {
        if OFF.load(Ordering::Relaxed) {
            return false;
        }
        if let Some(ran) = RAN.get(check as usize) {
            ran.store(true, Ordering::Relaxed);
        }
        true
    }

    pub fn off() {
        OFF.store(true, Ordering::Relaxed);
    }

    pub fn ran(check: Check) -> bool {
        RAN.get(check as usize).is_some_and(|r| r.load(Ordering::Relaxed))
    }
}

/// Whether to run `check` here, noting that it ran: in a debug build, unless the app turned the checks off.
#[cfg(debug_assertions)]
#[inline]
pub fn on(check: Check) -> bool {
    state::on(check)
}

/// Whether to run `check` here: never, in a release build.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub const fn on(_check: Check) -> bool {
    false
}

/// No self-check runs in this process from now on: what an app's `main` calls first, so a debug build of it runs as fast as
/// its fast paths let it. (A release build runs none anyway.)
pub fn off() {
    #[cfg(debug_assertions)]
    state::off();
}

/// Whether `check` has run in this process.
#[cfg(debug_assertions)]
pub fn ran(check: Check) -> bool {
    state::ran(check)
}

/// Whether `check` has run in this process: never, in a release build.
#[cfg(not(debug_assertions))]
pub const fn ran(_check: Check) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_check_in_order() {
        for (i, c) in Check::ALL.iter().enumerate() {
            assert_eq!(*c as usize, i, "{c:?}");
        }
    }
}
