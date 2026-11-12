//! BIMCraft's MEP calculations, each by its standard and knowing nothing of the model: circuits by NBR 5410 (`nbr5410`,
//! `dr`, `dps`, `conduit`, `loads`), an installation's demand by its utility (`demand`), cold water and its reservation by NBR
//! 5626 (`water`, `reservoir`), drains and vents by NBR 8160 (`drainage`, with the fixtures of `plumbing`), storm drains by
//! NBR 10844 (`storm`) and hydrants by IT 22/2025 (`hydrant`). Their commands are the core's.

pub mod conduit;
pub mod demand;
pub mod dps;
pub mod dr;
pub mod drainage;
pub mod hydrant;
pub mod loads;
pub mod nbr5410;
pub mod plumbing;
pub mod reservoir;
pub mod storm;
pub mod water;
