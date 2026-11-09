//! The browser's stand-in for the network (Arqiflow's build of the interface): it has no updater, so every request fails.

use std::io::Write;

use crate::Net;

#[derive(Default)]
pub struct Ureq;

impl Ureq {
    pub fn new() -> Ureq {
        Ureq
    }
}

impl Net for Ureq {
    fn get(&self, _url: &str, _limit: u64, _out: &mut dyn Write) -> Result<(), String> {
        Err("there are no updates in the browser".to_string())
    }
}
