//! Structure-of-arrays instance storage, without per-instance heap allocations.
//!
//! After a read-only validation pass, each frame updates instances in insertion
//! order in one pass, without sorting: add speed to angle, then rebuild the
//! matrix from that new angle. The frame number is a label, not a time delta;
//! every call advances once, including repeated or nonconsecutive frame numbers.

/// Invalid numeric input or an unrepresentable frame update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceError {
    NonFiniteAngle,
    NonFiniteSpeed,
    AngleOverflow,
}

/// Parallel arrays whose lengths are kept equal by the store.
#[derive(Debug, Default)]
pub struct Instances {
    matrices: Vec<[f32; 12]>,
    angles: Vec<f32>,
    speeds: Vec<f32>,
    layers: Vec<u32>,
    nodes: Vec<u32>,
}

impl Instances {
    pub fn with_capacity(n: usize) -> Self {
        Self {
            matrices: Vec::with_capacity(n),
            angles: Vec::with_capacity(n),
            speeds: Vec::with_capacity(n),
            layers: Vec::with_capacity(n),
            nodes: Vec::with_capacity(n),
        }
    }

    /// All finite f32 angles and speeds are accepted. No wrapping is applied.
    pub fn push(
        &mut self,
        layer: u32,
        node: u32,
        angle: f32,
        speed: f32,
    ) -> Result<(), InstanceError> {
        let matrix = matrix_from_angle(angle)?;
        if !speed.is_finite() {
            return Err(InstanceError::NonFiniteSpeed);
        }
        self.matrices.push(matrix);
        self.angles.push(angle);
        self.speeds.push(speed);
        self.layers.push(layer);
        self.nodes.push(node);
        Ok(())
    }

    /// Reject overflow before changing any instance; no frame allocations.
    pub fn update_frame(&mut self, _frame: u32) -> Result<(), InstanceError> {
        if self
            .angles
            .iter()
            .zip(&self.speeds)
            .any(|(&angle, &speed)| !(angle + speed).is_finite())
        {
            return Err(InstanceError::AngleOverflow);
        }
        for ((angle, &speed), matrix) in self
            .angles
            .iter_mut()
            .zip(&self.speeds)
            .zip(&mut self.matrices)
        {
            *angle += speed;
            *matrix = finite_angle_matrix(*angle);
        }
        Ok(())
    }

    pub fn matrices(&self) -> &[[f32; 12]] {
        &self.matrices
    }

    pub fn angles(&self) -> &[f32] {
        &self.angles
    }

    pub fn speeds(&self) -> &[f32] {
        &self.speeds
    }

    pub fn layers(&self) -> &[u32] {
        &self.layers
    }

    pub fn nodes(&self) -> &[u32] {
        &self.nodes
    }
}

/// Row-major 3x4 right-handed Y rotation multiplying column vectors.
/// Angles are radians; every finite f32 is supported.
pub fn matrix_from_angle(angle: f32) -> Result<[f32; 12], InstanceError> {
    if !angle.is_finite() {
        return Err(InstanceError::NonFiniteAngle);
    }
    Ok(finite_angle_matrix(angle))
}

fn finite_angle_matrix(angle: f32) -> [f32; 12] {
    let sine = angle.sin();
    let cosine = angle.cos();
    [
        cosine, 0.0, sine, 0.0, 0.0, 1.0, 0.0, 0.0, -sine, 0.0, cosine, 0.0,
    ]
}
