#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "development scenarios fail immediately on invalid fixtures or violated contracts"
)]

//! Opt-in contract and user acceptance scenarios. Every scenario is ignored by default;
//! `scripts/test-scenarios.sh` runs them explicitly with the release binary.

#[path = "support/command.rs"]
mod test_command;

#[path = "development_scenarios/acceptance/mod.rs"]
mod acceptance;
#[path = "development_scenarios/boundaries.rs"]
mod boundaries;
#[path = "development_scenarios/efficiency/mod.rs"]
mod efficiency;
#[path = "development_scenarios/inventory.rs"]
mod inventory;
#[path = "development_scenarios/journeys/mod.rs"]
mod journeys;
#[path = "development_scenarios/navigation.rs"]
mod navigation;
#[path = "development_scenarios/revisions.rs"]
mod revisions;
#[path = "development_scenarios/support.rs"]
mod support;
