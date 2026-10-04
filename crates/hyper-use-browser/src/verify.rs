//! Postconditions on a manifold. This does not click and does not diff.
//! The host diffs with `hyper-use-observe` and then calls [`verify`].

use std::fmt;

use hyper_use_core::{token_recall, tokenize, InteractionManifold, RegionId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expectation {
    TextPresent(String),
    RegionAbsent(RegionId),
}

impl Expectation {
    pub fn text_present(text: impl Into<String>) -> Result<Self, VerifyError> {
        let text = text.into();
        if tokenize(&text).is_empty() {
            return Err(VerifyError::EmptyExpectation);
        }
        Ok(Self::TextPresent(text))
    }

    pub fn region_absent(id: RegionId) -> Self {
        Self::RegionAbsent(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VerifyError {
    ExpectedTextMissing { expected: String },
    RegionStillPresent { id: String },
    EmptyExpectation,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedTextMissing { expected } => {
                write!(f, "expected text `{expected}` did not appear")
            }
            Self::RegionStillPresent { id } => write!(f, "region `{id}` is still present"),
            Self::EmptyExpectation => {
                f.write_str("expected text must contain at least one alphanumeric token")
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// Succeeds when every token of the expected text is present in some region
/// label, or when the named region id is gone.
pub fn verify(
    manifold: &InteractionManifold,
    expectation: &Expectation,
) -> Result<(), VerifyError> {
    match expectation {
        Expectation::TextPresent(expected) => {
            let found = manifold
                .regions()
                .any(|region| token_recall(expected, region.label()) == 1.0);
            if found {
                Ok(())
            } else {
                Err(VerifyError::ExpectedTextMissing {
                    expected: expected.clone(),
                })
            }
        }
        Expectation::RegionAbsent(id) => {
            if manifold.get(id).is_none() {
                Ok(())
            } else {
                Err(VerifyError::RegionStillPresent { id: id.to_string() })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::{
        Action, InteractionRegion, Rect, RegionFlags, RegionParts, Role, SourceMask, UnitInterval,
    };

    fn manifold(label: &str, id: &str) -> InteractionManifold {
        let region = InteractionRegion::try_new(RegionParts {
            id: RegionId::try_new(id).unwrap(),
            role: Role::Heading,
            label: label.into(),
            rect: Rect::try_new(10.0, 10.0, 40.0, 20.0).unwrap(),
            actions: vec![Action::Focus],
            parent: None,
            sources: SourceMask::DOM,
            flags: RegionFlags::none(),
            temporal_stability: UnitInterval::ONE,
        })
        .unwrap();
        InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 100.0, 100.0).unwrap(),
            vec![region],
            0,
        )
        .unwrap()
    }

    #[test]
    fn missing_text_is_the_exact_error_and_present_text_succeeds() {
        let sign_in = manifold("Sign in", "n100");
        let err = verify(&sign_in, &Expectation::text_present("Welcome").unwrap()).unwrap_err();
        assert_eq!(
            err,
            VerifyError::ExpectedTextMissing {
                expected: "Welcome".into(),
            }
        );
        assert_eq!(err.to_string(), "expected text `Welcome` did not appear");

        let welcome = manifold("Welcome back", "n2");
        assert!(verify(&welcome, &Expectation::text_present("Welcome").unwrap()).is_ok());

        let err = Expectation::text_present("...").unwrap_err();
        assert_eq!(err, VerifyError::EmptyExpectation);
    }

    #[test]
    fn region_that_remains_is_the_exact_error() {
        let sign_in = manifold("Sign in", "n100");
        let err = verify(
            &sign_in,
            &Expectation::region_absent(RegionId::try_new("n100").unwrap()),
        )
        .unwrap_err();
        assert_eq!(err, VerifyError::RegionStillPresent { id: "n100".into() });
        assert_eq!(err.to_string(), "region `n100` is still present");
        assert!(verify(
            &sign_in,
            &Expectation::region_absent(RegionId::try_new("gone").unwrap()),
        )
        .is_ok());
    }
}
