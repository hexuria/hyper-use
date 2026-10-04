//! Load the sidebar fixture and print the top locate hit.
//!
//! Run from the workspace: `cargo run -p hyper-use-cli --example sidebar_locate`

use hyper_use_core::{parse_fixture, LocateQuery, Role, Zone};
use hyper_use_resonance::locate;

fn main() {
    let fixture = include_str!("../../../fixtures/sidebar.manifold");
    let manifold = parse_fixture(fixture).expect("sidebar fixture");
    let query = LocateQuery::new()
        .text("Settings")
        .expect("text")
        .role(Role::Button)
        .position(Zone::Left);
    let ranked = locate(&manifold, &query).expect("locate");
    let top = ranked.first().expect("at least one region");
    println!(
        "hyper-use locate top: {} score {:.6}",
        top.id(),
        top.score().total()
    );
    assert_eq!(top.id().as_str(), "nav-settings");
}
