//! Resonance ranking for a static interaction manifold.
//!
//! The product default is [`WeightedMatcher`]: semantic match, geometry,
//! actionability, and the versioned penalties. It does not build hypervectors.
//! [`locate`] and [`HgraMatcher`] keep the hyperdimensional ranker. Neither
//! matcher is a measured winner; there is no benchmark that says so.
//!
//! [`locate`] builds a bipolar signature for every region, probes it with the
//! structured [`LocateQuery`], and combines that cosine with semantic, source,
//! geometric, actionability, temporal, and contextual terms. Penalties are
//! subtracted afterwards. Nothing in this path reads a clock or a random
//! source. The same manifold and the same query produce the same order,
//! including the region-id tie-break.
//!
//! Model [`ResonanceModel::V1`] weights:
//! hypervector 0.35, semantic 0.20, source agreement 0.15, geometric 0.10,
//! actionability 0.10, temporal stability 0.05, contextual consistency 0.05.
//! Penalties: disabled 0.25, hidden 0.45, occluded 0.20, offscreen 0.35,
//! stale 0.15, ambiguous 0.12, detached 0.20, zero-size 0.40.
//!
//! Text-miss cap: when the query has text and a region's label shares no
//! token with it, both matchers clamp that region's total to at most
//! [`TEXT_MISS_CAP`] (0.45). Without it, a text miss still collects the
//! credit for constraints the query did not set (geometry with no position,
//! actionability with no action), which is 0.50 under the weighted V1 model
//! and more under other weights or HGRA.

#![forbid(unsafe_code)]

mod error;
mod matcher;
mod model;
mod signature;

pub use matcher::{
    default_matcher, HgraMatcher, Match, RegionMatcher, WeightedBasisPoints, WeightedMatcher,
    WeightedModel,
};

use hyper_use_core::{
    token_recall, InteractionManifold, InteractionRegion, LocateQuery, Rect, RegionId, SourceMask,
};
use hyper_use_geometry::{is_fully_offscreen, normalize, zones};
use hyper_use_hyper::{cosine, Dims, Encoder};

pub use error::ResonanceError;
pub use hyper_use_hyper::BipolarVector;
pub use model::{PenaltyBasisPoints, ResonanceModel, WeightBasisPoints};

use signature::{query_probes, region_signature as compose_signature, Memory};

/// Highest total any matcher gives a region whose label shares no token with
/// a non-empty text query.
///
/// Why 0.45: it is strictly below the executor's 0.55 act gate, so a nameless
/// or unrelated region can never be clicked from a text query, whatever the
/// weights. It is also 0.05 below the 0.50 that the weighted V1 model gives a
/// label hit with the wrong role, so under V1 a right-named control of the
/// wrong kind still ranks above every text miss instead of tying with them
/// and losing on region id.
pub const TEXT_MISS_CAP: f64 = 0.45;

/// Clamp `total` to [`TEXT_MISS_CAP`] when the query has text and `label`
/// shares none of its tokens. A query without text, or text with no tokens,
/// is unchanged.
pub(crate) fn cap_text_miss(query: &LocateQuery, label: &str, total: f64) -> f64 {
    match query.text_ref() {
        Some(text) if token_recall(text, label) == 0.0 => total.min(TEXT_MISS_CAP),
        _ => total,
    }
}

/// One ranked region. `rank` is 1-based after the deterministic sort.
#[derive(Clone, Debug, PartialEq)]
pub struct RankedCandidate {
    rank: usize,
    id: RegionId,
    score: ResonanceScore,
}

impl RankedCandidate {
    pub fn rank(&self) -> usize {
        self.rank
    }
    pub fn id(&self) -> &RegionId {
        &self.id
    }
    pub fn score(&self) -> &ResonanceScore {
        &self.score
    }
}

/// Breakdown of one region's resonance. `total = weighted positives - penalty`,
/// then clamped to [`TEXT_MISS_CAP`] on a text miss.
#[derive(Clone, Debug, PartialEq)]
pub struct ResonanceScore {
    hypervector: f64,
    semantic: f64,
    source_agreement: f64,
    geometric: f64,
    actionability: f64,
    temporal_stability: f64,
    contextual_consistency: f64,
    penalty: f64,
    total: f64,
}

impl ResonanceScore {
    pub fn hypervector(&self) -> f64 {
        self.hypervector
    }
    pub fn semantic(&self) -> f64 {
        self.semantic
    }
    pub fn source_agreement(&self) -> f64 {
        self.source_agreement
    }
    pub fn geometric(&self) -> f64 {
        self.geometric
    }
    pub fn actionability(&self) -> f64 {
        self.actionability
    }
    pub fn temporal_stability(&self) -> f64 {
        self.temporal_stability
    }
    pub fn contextual_consistency(&self) -> f64 {
        self.contextual_consistency
    }
    pub fn penalty(&self) -> f64 {
        self.penalty
    }
    pub fn total(&self) -> f64 {
        self.total
    }
}

/// Locate with the default 2048-dimensional encoder and [`ResonanceModel::V1`].
pub fn locate(
    manifold: &InteractionManifold,
    query: &LocateQuery,
) -> Result<Vec<RankedCandidate>, ResonanceError> {
    locate_with(
        manifold,
        query,
        &Encoder::new(Dims::DEFAULT),
        ResonanceModel::V1,
    )
}

pub fn locate_with(
    manifold: &InteractionManifold,
    query: &LocateQuery,
    encoder: &Encoder,
    model: ResonanceModel,
) -> Result<Vec<RankedCandidate>, ResonanceError> {
    let mut memory = Memory::new(encoder);
    let probes = query_probes(query, &mut memory)?;
    let mut ranked = Vec::with_capacity(manifold.len());
    for region in manifold.regions() {
        let signature = compose_signature(manifold, region, &mut memory)?;
        let hypervector = probe_similarity(&probes, &signature)?;
        let score = score_parts(manifold, region, query, hypervector, model);
        debug_assert!(score.total.is_finite());
        ranked.push(RankedCandidate {
            rank: 0,
            id: region.id().clone(),
            score,
        });
    }
    ranked.sort_by(|left, right| {
        right
            .score
            .total
            .total_cmp(&left.score.total)
            .then_with(|| left.id.cmp(&right.id))
    });
    for (index, candidate) in ranked.iter_mut().enumerate() {
        candidate.rank = index + 1;
    }
    Ok(ranked)
}

/// Public signature so other crates can compare regions without re-ranking.
pub fn region_signature(
    manifold: &InteractionManifold,
    region: &InteractionRegion,
    encoder: &Encoder,
) -> Result<BipolarVector, ResonanceError> {
    let mut memory = Memory::new(encoder);
    compose_signature(manifold, region, &mut memory)
}

fn probe_similarity(
    probes: &[BipolarVector],
    signature: &BipolarVector,
) -> Result<f64, ResonanceError> {
    if probes.is_empty() {
        return Ok(1.0);
    }
    let mut sum = 0.0;
    for probe in probes {
        sum += cosine(probe, signature)?;
    }
    Ok(sum / probes.len() as f64)
}

fn score_parts(
    manifold: &InteractionManifold,
    region: &InteractionRegion,
    query: &LocateQuery,
    hypervector: f64,
    model: ResonanceModel,
) -> ResonanceScore {
    let semantic = semantic_score(query, region);
    let source_agreement = f64::from(region.sources().count()) / f64::from(SourceMask::KNOWN_COUNT);
    let geometric = geometric_score(manifold.viewport(), region.rect(), query);
    let actionability = actionability_score(region, query);
    let temporal_stability = region.temporal_stability().get();
    let contextual_consistency = contextual_score(manifold, region);
    let penalty = penalty_total(manifold.viewport(), region, model);
    let positive = model.hypervector() * hypervector
        + model.semantic() * semantic
        + model.source_agreement() * source_agreement
        + model.geometric() * geometric
        + model.actionability() * actionability
        + model.temporal_stability() * temporal_stability
        + model.contextual_consistency() * contextual_consistency;
    ResonanceScore {
        hypervector,
        semantic,
        source_agreement,
        geometric,
        actionability,
        temporal_stability,
        contextual_consistency,
        penalty,
        total: cap_text_miss(query, region.label(), positive - penalty),
    }
}

fn semantic_score(query: &LocateQuery, region: &InteractionRegion) -> f64 {
    let mut parts = 0.0;
    let mut total = 0.0;
    if let Some(text) = query.text_ref() {
        parts += 1.0;
        total += token_recall(text, region.label());
    }
    if let Some(role) = query.role_ref() {
        parts += 1.0;
        total += if region.role() == role { 1.0 } else { 0.0 };
    }
    if parts == 0.0 {
        1.0
    } else {
        total / parts
    }
}

fn geometric_score(viewport: Rect, rect: Rect, query: &LocateQuery) -> f64 {
    let Some(wanted) = query.position_ref() else {
        return 1.0;
    };
    match normalize(rect, viewport) {
        Ok(normalized) => {
            if zones(normalized).contains(&wanted) {
                1.0
            } else {
                0.0
            }
        }
        Err(_) => 0.0,
    }
}

fn actionability_score(region: &InteractionRegion, query: &LocateQuery) -> f64 {
    match query.action_ref() {
        Some(action) => {
            if region.actions().contains(&action) {
                1.0
            } else {
                0.0
            }
        }
        None => {
            if region.actions().is_empty() {
                0.0
            } else {
                1.0
            }
        }
    }
}

fn contextual_score(manifold: &InteractionManifold, region: &InteractionRegion) -> f64 {
    if region.flags().detached() {
        return 0.0;
    }
    match region.parent() {
        None => 1.0,
        Some(parent) => {
            if manifold.get(parent).is_some() {
                1.0
            } else {
                0.5
            }
        }
    }
}

fn penalty_total(viewport: Rect, region: &InteractionRegion, model: ResonanceModel) -> f64 {
    let flags = region.flags();
    let mut penalty = 0.0;
    if flags.disabled() {
        penalty += model.penalty_disabled();
    }
    if flags.hidden() {
        penalty += model.penalty_hidden();
    }
    if flags.occluded() {
        penalty += model.penalty_occluded();
    }
    if flags.offscreen() || is_fully_offscreen(region.rect(), viewport) {
        penalty += model.penalty_offscreen();
    }
    if flags.stale() {
        penalty += model.penalty_stale();
    }
    if flags.ambiguous() {
        penalty += model.penalty_ambiguous();
    }
    if flags.detached() {
        penalty += model.penalty_detached();
    }
    if region.rect().is_zero_area() {
        penalty += model.penalty_zero_size();
    }
    penalty
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::{
        parse_fixture, Action, InteractionRegion, Rect, RegionFlags, RegionId, RegionParts, Role,
        SourceMask, UnitInterval, Zone,
    };
    use hyper_use_hyper::HyperError;

    fn sample(id: &str, label: &str, x: f64, flags: RegionFlags, width: f64) -> InteractionRegion {
        InteractionRegion::try_new(RegionParts {
            id: RegionId::try_new(id).unwrap(),
            role: Role::Button,
            label: label.into(),
            rect: Rect::try_new(x, 40.0, width, if width == 0.0 { 0.0 } else { 20.0 }).unwrap(),
            actions: vec![Action::Click],
            parent: None,
            sources: SourceMask::DOM.union(SourceMask::ACCESSIBILITY),
            flags,
            temporal_stability: UnitInterval::ONE,
        })
        .unwrap()
    }

    fn manifold(regions: Vec<InteractionRegion>) -> InteractionManifold {
        InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 1440.0, 900.0).unwrap(),
            regions,
            0,
        )
        .unwrap()
    }

    fn settings_query() -> LocateQuery {
        LocateQuery::new()
            .text("Settings")
            .unwrap()
            .role(Role::Button)
            .position(Zone::Left)
    }

    #[test]
    fn weights_are_versioned_and_sum_to_one() {
        let model = ResonanceModel::V1;
        assert_eq!(model.version(), 1);
        assert_eq!(model.weight_basis_point_sum(), 100);
        let rebuilt = model
            .weight_basis_points()
            .try_model(model.version(), model.penalty_basis_points())
            .unwrap();
        assert_eq!(rebuilt, model);
        let mut bad = model.weight_basis_points();
        bad.hypervector = 34;
        let err = bad.try_model(1, model.penalty_basis_points()).unwrap_err();
        assert_eq!(err, ResonanceError::WeightsDoNotSum { sum: 99 });
        assert_eq!(
            err.to_string(),
            "positive weights sum to 99 basis points, expected 100"
        );
        let overflow = WeightBasisPoints {
            hypervector: u16::MAX,
            semantic: u16::MAX,
            source_agreement: u16::MAX,
            geometric: u16::MAX,
            actionability: u16::MAX,
            temporal_stability: u16::MAX,
            contextual_consistency: u16::MAX,
        };
        let err = overflow
            .try_model(1, model.penalty_basis_points())
            .unwrap_err();
        assert_eq!(
            err,
            ResonanceError::WeightsDoNotSum {
                sum: 7 * u32::from(u16::MAX)
            }
        );
        assert_eq!(model.hypervector(), 0.35);
        assert_eq!(model.semantic(), 0.20);
        assert_eq!(model.source_agreement(), 0.15);
        assert_eq!(model.geometric(), 0.10);
        assert_eq!(model.actionability(), 0.10);
        assert_eq!(model.temporal_stability(), 0.05);
        assert_eq!(model.contextual_consistency(), 0.05);
        assert_eq!(model.penalty_disabled(), 0.25);
        assert_eq!(model.penalty_hidden(), 0.45);
        assert_eq!(model.penalty_zero_size(), 0.40);
    }

    #[test]
    fn same_manifold_and_query_rank_identically() {
        let manifold = manifold(vec![
            sample("b", "Settings", 16.0, RegionFlags::none(), 80.0),
            sample("a", "Settings", 16.0, RegionFlags::none(), 80.0),
            sample("c", "Help", 900.0, RegionFlags::none(), 80.0),
        ]);
        let query = settings_query();
        let encoder = Encoder::new(Dims::D512);
        let once = locate_with(&manifold, &query, &encoder, ResonanceModel::V1).unwrap();
        let twice = locate_with(&manifold, &query, &encoder, ResonanceModel::V1).unwrap();
        assert_eq!(once.len(), twice.len());
        for (left, right) in once.iter().zip(twice.iter()) {
            assert_eq!(left.id(), right.id());
            assert_eq!(left.rank(), right.rank());
            assert_eq!(
                left.score().total().to_bits(),
                right.score().total().to_bits()
            );
        }
        let ids: Vec<_> = once
            .iter()
            .map(|candidate| candidate.id().as_str())
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn penalty_flags_lower_the_score_by_the_versioned_amount() {
        let query = settings_query();
        let encoder = Encoder::new(Dims::D512);
        let model = ResonanceModel::V1;
        let cases: [(&str, RegionFlags, f64); 7] = [
            (
                "disabled",
                with_flag(|flags| flags.set_disabled(true)),
                model.penalty_disabled(),
            ),
            (
                "hidden",
                with_flag(|flags| flags.set_hidden(true)),
                model.penalty_hidden(),
            ),
            (
                "occluded",
                with_flag(|flags| flags.set_occluded(true)),
                model.penalty_occluded(),
            ),
            (
                "stale",
                with_flag(|flags| flags.set_stale(true)),
                model.penalty_stale(),
            ),
            (
                "ambiguous",
                with_flag(|flags| flags.set_ambiguous(true)),
                model.penalty_ambiguous(),
            ),
            (
                "offscreen",
                with_flag(|flags| flags.set_offscreen(true)),
                model.penalty_offscreen(),
            ),
            (
                "detached",
                with_flag(|flags| flags.set_detached(true)),
                model.penalty_detached() + model.contextual_consistency(),
            ),
        ];
        for (name, flags, expected_drop) in cases {
            let clean = manifold(vec![sample(
                "only",
                "Settings",
                16.0,
                RegionFlags::none(),
                80.0,
            )]);
            let penalized = manifold(vec![sample("only", "Settings", 16.0, flags, 80.0)]);
            let clean_score = locate_with(&clean, &query, &encoder, model).unwrap()[0]
                .score()
                .total();
            let penalized_candidate = &locate_with(&penalized, &query, &encoder, model).unwrap()[0];
            let drop = clean_score - penalized_candidate.score().total();
            assert!(
                (drop - expected_drop).abs() < 1e-9,
                "{name} drop {drop} expected {expected_drop}"
            );
            assert!(penalized_candidate.score().penalty() > 0.0, "{name}");
        }
    }

    fn with_flag(set: impl FnOnce(&mut RegionFlags)) -> RegionFlags {
        let mut flags = RegionFlags::none();
        set(&mut flags);
        flags
    }

    #[test]
    fn zero_size_and_geometric_offscreen_are_penalized() {
        let query = LocateQuery::new().text("Ghost").unwrap().role(Role::Button);
        let encoder = Encoder::new(Dims::D512);
        let visible = manifold(vec![sample("g", "Ghost", 16.0, RegionFlags::none(), 40.0)]);
        let zero = manifold(vec![sample("g", "Ghost", 16.0, RegionFlags::none(), 0.0)]);
        let visible_score = locate_with(&visible, &query, &encoder, ResonanceModel::V1).unwrap();
        let zero_score = locate_with(&zero, &query, &encoder, ResonanceModel::V1).unwrap();
        assert!(
            (zero_score[0].score().penalty() - ResonanceModel::V1.penalty_zero_size()).abs()
                < 1e-12
        );
        assert!(zero_score[0].score().total() < visible_score[0].score().total());

        let off = InteractionRegion::try_new(RegionParts {
            id: RegionId::try_new("off").unwrap(),
            role: Role::Button,
            label: "Undo".into(),
            rect: Rect::try_new(16.0, 2000.0, 80.0, 20.0).unwrap(),
            actions: vec![Action::Click],
            parent: None,
            sources: SourceMask::SCREENSHOT,
            flags: RegionFlags::none(),
            temporal_stability: UnitInterval::ONE,
        })
        .unwrap();
        let on = sample("off", "Undo", 16.0, RegionFlags::none(), 80.0);
        let off_ranked = locate_with(
            &manifold(vec![off]),
            &LocateQuery::new().text("Undo").unwrap(),
            &encoder,
            ResonanceModel::V1,
        )
        .unwrap();
        let on_ranked = locate_with(
            &manifold(vec![on]),
            &LocateQuery::new().text("Undo").unwrap(),
            &encoder,
            ResonanceModel::V1,
        )
        .unwrap();
        assert!(off_ranked[0].score().penalty() >= ResonanceModel::V1.penalty_offscreen() - 1e-12);
        assert!(off_ranked[0].score().total() < on_ranked[0].score().total());
    }

    #[test]
    fn sidebar_settings_ranks_first_and_is_deterministic() {
        let manifold = parse_fixture(include_str!("../../../fixtures/sidebar.manifold")).unwrap();
        let query = settings_query();
        let once = locate(&manifold, &query).unwrap();
        let twice = locate(&manifold, &query).unwrap();
        assert_eq!(once[0].id().as_str(), "nav-settings");
        assert_eq!(once[0].rank(), 1);
        assert!(once[0].score().geometric() == 1.0);
        assert!(once[0].score().semantic() == 1.0);
        let main = once
            .iter()
            .find(|c| c.id().as_str() == "main-settings")
            .unwrap();
        assert!(once[0].score().total() > main.score().total());
        assert!(main.rank() > 1);
        let ids_once: Vec<_> = once.iter().map(|c| c.id().as_str()).collect();
        let ids_twice: Vec<_> = twice.iter().map(|c| c.id().as_str()).collect();
        assert_eq!(ids_once, ids_twice);
        for (left, right) in once.iter().zip(twice.iter()) {
            assert_eq!(
                left.score().total().to_bits(),
                right.score().total().to_bits()
            );
        }
        assert_eq!(once.len(), manifold.len());
        let help = relations_help_above(&manifold);
        assert!(help);
    }

    fn relations_help_above(manifold: &InteractionManifold) -> bool {
        let help = manifold.get_str("nav-help").unwrap().rect();
        let settings = manifold.get_str("nav-settings").unwrap().rect();
        help.bottom() <= settings.y()
            && manifold.get_str("nav-profile").unwrap().rect().y() >= settings.bottom()
    }

    #[test]
    fn disabled_hidden_and_zero_size_rank_below_a_clean_match() {
        let manifold = parse_fixture(include_str!("../../../fixtures/sidebar.manifold")).unwrap();
        let query = LocateQuery::new().role(Role::Button).position(Zone::Left);
        let ranked = locate_with(
            &manifold,
            &query,
            &Encoder::new(Dims::D512),
            ResonanceModel::V1,
        )
        .unwrap();
        let score = |id: &str| {
            ranked
                .iter()
                .find(|candidate| candidate.id().as_str() == id)
                .unwrap()
                .score()
                .total()
        };
        assert!(score("nav-help") > score("nav-disabled"));
        assert!(score("nav-help") > score("nav-hidden"));
        assert!(score("nav-help") > score("zero-ghost"));
        assert!(ranked.iter().any(|c| c.id().as_str() == "nav-hidden"));
    }

    #[test]
    fn signature_is_deterministic_and_dim_mismatch_surfaces() {
        let manifold = manifold(vec![sample(
            "a",
            "Settings",
            16.0,
            RegionFlags::none(),
            80.0,
        )]);
        let region = manifold.get_str("a").unwrap();
        let encoder = Encoder::new(Dims::D512);
        let once = region_signature(&manifold, region, &encoder).unwrap();
        let twice = region_signature(&manifold, region, &encoder).unwrap();
        assert_eq!(once, twice);
        let other = Encoder::new(Dims::D1024);
        let wide = region_signature(&manifold, region, &other).unwrap();
        let err = cosine(&once, &wide).unwrap_err();
        assert!(matches!(err, HyperError::DimMismatch { .. }));
    }

    #[test]
    fn hgra_send_order_is_unchanged() {
        // Regression pin for the HGRA semantic term, which the weighted text
        // precision change does not touch. Not a claim that HGRA wins.
        let manifold =
            parse_fixture(include_str!("../../../fixtures/send-buttons.manifold")).unwrap();
        let query = LocateQuery::new().text("Send").unwrap().role(Role::Button);
        let ranked = HgraMatcher::default().rank(&query, &manifold).unwrap();
        let ids: Vec<_> = ranked.iter().map(|m| m.id().as_str()).collect();
        assert_eq!(ids, ["z-send", "a-feedback", "b-device"]);
    }

    #[test]
    fn weighted_and_hgra_both_rank_sidebar_settings_first() {
        let manifold = parse_fixture(include_str!("../../../fixtures/sidebar.manifold")).unwrap();
        let query = settings_query();
        let weighted = WeightedMatcher::default().rank(&query, &manifold).unwrap();
        let hgra = HgraMatcher::default().rank(&query, &manifold).unwrap();
        assert_eq!(weighted[0].id().as_str(), "nav-settings");
        assert_eq!(weighted[0].rank(), 1);
        assert_eq!(hgra[0].id().as_str(), "nav-settings");
        assert_eq!(hgra[0].rank(), 1);
        assert_eq!(
            default_matcher().rank(&query, &manifold).unwrap()[0]
                .id()
                .as_str(),
            "nav-settings"
        );
        // No benchmark compares these totals. Do not treat a higher number as a win.
        assert!(weighted[0].confidence().is_finite());
        assert!(hgra[0].confidence().is_finite());
    }
}
