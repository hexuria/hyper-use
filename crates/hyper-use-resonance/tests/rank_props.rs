//! Property tests for deterministic ranking. They call `locate_with` and
//! `WeightedMatcher::rank`. They are not a second ranker.

use hyper_use_core::{
    Action, InteractionManifold, InteractionRegion, LocateQuery, Rect, RegionFlags, RegionId,
    RegionParts, Role, SourceMask, UnitInterval, Zone,
};
use hyper_use_hyper::{Dims, Encoder};
use hyper_use_resonance::{locate_with, RegionMatcher, ResonanceModel, WeightedMatcher};
use proptest::prelude::*;

fn region(id: &str, label: &str, flags: RegionFlags) -> InteractionRegion {
    InteractionRegion::try_new(RegionParts {
        id: RegionId::try_new(id).unwrap(),
        role: Role::Button,
        label: label.into(),
        rect: Rect::try_new(16.0, 40.0, 80.0, 20.0).unwrap(),
        actions: vec![Action::Click],
        parent: None,
        sources: SourceMask::DOM,
        flags,
        temporal_stability: UnitInterval::ONE,
    })
    .unwrap()
}

fn flag(index: usize) -> RegionFlags {
    let mut flags = RegionFlags::none();
    match index {
        0 => flags.set_disabled(true),
        1 => flags.set_hidden(true),
        2 => flags.set_occluded(true),
        3 => flags.set_stale(true),
        4 => flags.set_ambiguous(true),
        5 => flags.set_offscreen(true),
        _ => flags.set_detached(true),
    }
    flags
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn equal_scores_follow_id_not_insertion_order(order in Just((0..6).collect::<Vec<_>>()).prop_shuffle()) {
        let labels = ["r0", "r1", "r2", "r3", "r4", "r5"];
        let regions: Vec<_> = order
            .iter()
            .map(|index| region(labels[*index], "Same", RegionFlags::none()))
            .collect();
        let manifold = InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 1440.0, 900.0).unwrap(),
            regions,
            0,
        )
        .unwrap();
        let query = LocateQuery::new().text("Same").unwrap().role(Role::Button);
        let ranked = locate_with(
            &manifold,
            &query,
            &Encoder::new(Dims::D512),
            ResonanceModel::V1,
        )
        .unwrap();
        let ids: Vec<_> = ranked.iter().map(|candidate| candidate.id().as_str()).collect();
        prop_assert_eq!(ids, vec!["r0", "r1", "r2", "r3", "r4", "r5"]);
        let mut previous = f64::INFINITY;
        for candidate in &ranked {
            prop_assert!(previous >= candidate.score().total());
            previous = candidate.score().total();
        }
    }

    #[test]
    fn one_penalty_strictly_lowers_the_same_region(index in 0usize..7) {
        let query = LocateQuery::new()
            .text("Settings")
            .unwrap()
            .role(Role::Button)
            .position(Zone::Left);
        let encoder = Encoder::new(Dims::D512);
        let clean = InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 1440.0, 900.0).unwrap(),
            vec![region("only", "Settings", RegionFlags::none())],
            0,
        )
        .unwrap();
        let penalized = InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 1440.0, 900.0).unwrap(),
            vec![region("only", "Settings", flag(index))],
            0,
        )
        .unwrap();
        let clean_score = locate_with(&clean, &query, &encoder, ResonanceModel::V1).unwrap()[0]
            .score()
            .total();
        let penalized_row = &locate_with(&penalized, &query, &encoder, ResonanceModel::V1).unwrap()[0];
        prop_assert!(penalized_row.score().penalty() > 0.0);
        prop_assert!(penalized_row.score().total() < clean_score);
    }

    #[test]
    fn extra_label_tokens_strictly_lower_the_weighted_total(extra in 1usize..5) {
        let words = ["alpha", "bravo", "charlie", "delta"];
        let superset = format!("Send {}", words[..extra].join(" "));
        let viewport = Rect::try_viewport(0.0, 0.0, 1440.0, 900.0).unwrap();
        let manifold = InteractionManifold::try_new(
            viewport,
            vec![
                region("a-superset", &superset, RegionFlags::none()),
                region("z-exact", "Send", RegionFlags::none()),
            ],
            0,
        )
        .unwrap();
        let query = LocateQuery::new().text("Send").unwrap();
        let ranked = WeightedMatcher::default().rank(&query, &manifold).unwrap();
        prop_assert_eq!(ranked[0].id().as_str(), "z-exact");
        prop_assert!(ranked[0].confidence() > ranked[1].confidence());
    }
}
