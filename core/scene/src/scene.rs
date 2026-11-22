//! Scene-owned geometry, hierarchy, instances, and layers.

use crate::hierarchy::Hierarchy;
use crate::instance::Instances;
use crate::layer::Layer;

/// Indexed geometry referenced by a node's `mesh` index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct Scene {
    pub hierarchy: Hierarchy,
    pub instances: Instances,
    pub layers: Vec<Layer>,
    pub meshes: Vec<Mesh>,
}

impl Scene {
    /// Returns instance indices in insertion order.
    ///
    /// An instance is visible when any visible layer has its layer ID.
    /// Unmatched IDs are hidden; duplicate layer IDs never duplicate instances.
    pub fn visible_instances(&self) -> Vec<usize> {
        self.instances
            .layers()
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                self.layers
                    .iter()
                    .any(|layer| layer.id == *id && layer.visible)
                    .then_some(index)
            })
            .collect()
    }
}
