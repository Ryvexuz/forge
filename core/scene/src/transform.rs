//! Local transforms, applied in scale, rotation, translation order.

use crate::math::{Mat4, Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::default(),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }
}

impl Transform {
    /// None means a non-finite input or an unrepresentable result.
    pub fn to_matrix(&self) -> Option<Mat4> {
        Mat4::from_trs(self.translation, self.rotation, self.scale)
    }
}
