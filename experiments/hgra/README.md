# HGRA experiment

Hyperdimensional ranking algebra (`hyper-use-hyper`) and the `hgra` feature on
`hyper-use-resonance`. Not the product default. Promote only after it beats
`WeightedMatcher` on a large adversarial target-resolution suite.

```bash
cargo test -p hyper-use-hyper
cargo test -p hyper-use-resonance --features hgra
```

## Scoring notes

- The HGRA semantic term is the weighted matcher's (`recall * (0.5 + 0.5 *
  precision)`, min with the role hit), so exact labels beat supersets in both.
- The hypervector term is one cosine of the bundled query (role, label tokens,
  position, action; equal weight) against the region signature, not the mean of
  per-probe cosines.

Before/after numbers and the ablation: `crates/hyper-use-resonance/HGRA_REMEASURE.md`.
Decision: `docs/DECISIONS.md`, "HGRA semantic parity and bundled query".

```bash
cargo test -p hyper-use-resonance --features hgra --test hgra_remeasure -- --nocapture
```
