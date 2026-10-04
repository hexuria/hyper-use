use std::fmt;

use hyper_use_geometry::GeometryError;
use hyper_use_hyper::HyperError;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ResonanceError {
    Hyper(HyperError),
    Geometry(GeometryError),
}

impl fmt::Display for ResonanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyper(err) => write!(f, "hypervector error: {err}"),
            Self::Geometry(err) => write!(f, "geometry error: {err}"),
        }
    }
}

impl std::error::Error for ResonanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Hyper(err) => Some(err),
            Self::Geometry(err) => Some(err),
        }
    }
}

impl From<HyperError> for ResonanceError {
    fn from(value: HyperError) -> Self {
        Self::Hyper(value)
    }
}

impl From<GeometryError> for ResonanceError {
    fn from(value: GeometryError) -> Self {
        Self::Geometry(value)
    }
}
