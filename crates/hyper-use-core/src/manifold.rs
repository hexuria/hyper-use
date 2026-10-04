use std::collections::BTreeMap;

use crate::error::CoreError;
use crate::id::RegionId;
use crate::rect::Rect;
use crate::region::InteractionRegion;

/// A static snapshot of interactive regions in one viewport.
///
/// Regions are stored in identifier order. Iteration order is part of the
/// locate contract: the same regions always bind neighborhood vectors in the
/// same sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionManifold {
    viewport: Rect,
    regions: BTreeMap<RegionId, InteractionRegion>,
    captured_at_ms: u64,
}

impl InteractionManifold {
    pub fn try_new(
        viewport: Rect,
        regions: Vec<InteractionRegion>,
        captured_at_ms: u64,
    ) -> Result<Self, CoreError> {
        if viewport.width() <= 0.0 || viewport.height() <= 0.0 {
            return Err(CoreError::NonPositiveViewport);
        }
        let mut map = BTreeMap::new();
        for region in regions {
            let id = region.id().clone();
            if map.insert(id.clone(), region).is_some() {
                return Err(CoreError::DuplicateRegion(id.to_string()));
            }
        }
        Ok(Self {
            viewport,
            regions: map,
            captured_at_ms,
        })
    }

    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    pub fn captured_at_ms(&self) -> u64 {
        self.captured_at_ms
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn get(&self, id: &RegionId) -> Option<&InteractionRegion> {
        self.regions.get(id)
    }

    pub fn get_str(&self, id: &str) -> Option<&InteractionRegion> {
        let id = RegionId::try_new(id).ok()?;
        self.regions.get(&id)
    }

    pub fn regions(&self) -> impl Iterator<Item = &InteractionRegion> {
        self.regions.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = &RegionId> {
        self.regions.keys()
    }

    pub fn insert(&mut self, region: InteractionRegion) -> Result<(), CoreError> {
        let id = region.id().clone();
        if self.regions.contains_key(&id) {
            return Err(CoreError::DuplicateRegion(id.to_string()));
        }
        self.regions.insert(id, region);
        Ok(())
    }

    pub fn remove(&mut self, id: &RegionId) -> Option<InteractionRegion> {
        self.regions.remove(id)
    }

    /// Replace an existing id, or insert it if absent.
    pub fn replace(&mut self, region: InteractionRegion) {
        self.regions.insert(region.id().clone(), region);
    }
}
