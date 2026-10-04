/// Versioned locate weights and penalties.
///
/// Positive weights are basis points of the unpenalized score and sum to 100.
/// Penalty basis points are subtracted after that sum. They are not required
/// to sum to 100; each one is an independent deduction.
///
/// `hypervector` multiplies cosine similarity in `[-1, 1]`. The other positive
/// terms multiply a score in `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResonanceModel {
    version: u32,
    hypervector_bp: u16,
    semantic_bp: u16,
    source_agreement_bp: u16,
    geometric_bp: u16,
    actionability_bp: u16,
    temporal_stability_bp: u16,
    contextual_consistency_bp: u16,
    penalty_disabled_bp: u16,
    penalty_hidden_bp: u16,
    penalty_occluded_bp: u16,
    penalty_offscreen_bp: u16,
    penalty_stale_bp: u16,
    penalty_ambiguous_bp: u16,
    penalty_detached_bp: u16,
    penalty_zero_size_bp: u16,
}

impl ResonanceModel {
    pub const V1: Self = Self {
        version: 1,
        hypervector_bp: 35,
        semantic_bp: 20,
        source_agreement_bp: 15,
        geometric_bp: 10,
        actionability_bp: 10,
        temporal_stability_bp: 5,
        contextual_consistency_bp: 5,
        penalty_disabled_bp: 25,
        penalty_hidden_bp: 45,
        penalty_occluded_bp: 20,
        penalty_offscreen_bp: 35,
        penalty_stale_bp: 15,
        penalty_ambiguous_bp: 12,
        penalty_detached_bp: 20,
        penalty_zero_size_bp: 40,
    };

    pub const fn version(self) -> u32 {
        self.version
    }

    pub const fn weight_basis_point_sum(self) -> u16 {
        self.hypervector_bp
            + self.semantic_bp
            + self.source_agreement_bp
            + self.geometric_bp
            + self.actionability_bp
            + self.temporal_stability_bp
            + self.contextual_consistency_bp
    }

    pub fn hypervector(self) -> f64 {
        bp(self.hypervector_bp)
    }
    pub fn semantic(self) -> f64 {
        bp(self.semantic_bp)
    }
    pub fn source_agreement(self) -> f64 {
        bp(self.source_agreement_bp)
    }
    pub fn geometric(self) -> f64 {
        bp(self.geometric_bp)
    }
    pub fn actionability(self) -> f64 {
        bp(self.actionability_bp)
    }
    pub fn temporal_stability(self) -> f64 {
        bp(self.temporal_stability_bp)
    }
    pub fn contextual_consistency(self) -> f64 {
        bp(self.contextual_consistency_bp)
    }
    pub fn penalty_disabled(self) -> f64 {
        bp(self.penalty_disabled_bp)
    }
    pub fn penalty_hidden(self) -> f64 {
        bp(self.penalty_hidden_bp)
    }
    pub fn penalty_occluded(self) -> f64 {
        bp(self.penalty_occluded_bp)
    }
    pub fn penalty_offscreen(self) -> f64 {
        bp(self.penalty_offscreen_bp)
    }
    pub fn penalty_stale(self) -> f64 {
        bp(self.penalty_stale_bp)
    }
    pub fn penalty_ambiguous(self) -> f64 {
        bp(self.penalty_ambiguous_bp)
    }
    pub fn penalty_detached(self) -> f64 {
        bp(self.penalty_detached_bp)
    }
    pub fn penalty_zero_size(self) -> f64 {
        bp(self.penalty_zero_size_bp)
    }
}

fn bp(value: u16) -> f64 {
    f64::from(value) / 100.0
}
