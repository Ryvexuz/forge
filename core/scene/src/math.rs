//! Row-major matrices multiplying column vectors, in a right-handed coordinate system.
//!
//! Vec3 and Mat4 operations copied from `projects/render/src/math.rs`, commit
//! `41bf7caf67906d28e04f3bdcfcc3930772ffa446`, lines 5–172.
//! Additions: unit quaternion `Quat`, `Mat4::from_trs`, and `Mat4::inverse_trs`.
//! Copied operations retain their original f32 evaluation order and input contract.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    /// Unit vector, or the zero vector if the length is zero.
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l > 0.0 {
            self * (1.0 / l)
        } else {
            Vec3::default()
        }
    }
    pub fn lerp(self, o: Vec3, t: f32) -> Vec3 {
        self + (o - self) * t
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    /// `m[row][col]`.
    pub m: [[f32; 4]; 4],
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// `self * o`: applying the result applies `o` first, then `self`.
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut m = [[0.0f32; 4]; 4];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.m[r][k] * o.m[k][c]).sum();
            }
        }
        Mat4 { m }
    }

    pub fn translation(t: Vec3) -> Mat4 {
        let mut r = Mat4::IDENTITY;
        r.m[0][3] = t.x;
        r.m[1][3] = t.y;
        r.m[2][3] = t.z;
        r
    }

    /// Rotation about +Y by `angle` radians (right-handed: +X turns toward -Z).
    pub fn rotation_y(angle: f32) -> Mat4 {
        let (s, c) = angle.sin_cos();
        Mat4 {
            m: [
                [c, 0.0, s, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [-s, 0.0, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Transforms a point by an affine matrix (the last row is ignored).
    pub fn transform_point3(&self, p: Vec3) -> Vec3 {
        let h = self.transform_point(p);
        Vec3::new(h[0], h[1], h[2])
    }

    /// Transforms a direction (the translation is ignored). Correct for normals only if the matrix is a
    /// rotation, possibly with a uniform scale; the caller renormalises.
    pub fn transform_dir(&self, d: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3::new(
            m[0][0] * d.x + m[0][1] * d.y + m[0][2] * d.z,
            m[1][0] * d.x + m[1][1] * d.y + m[1][2] * d.z,
            m[2][0] * d.x + m[2][1] * d.y + m[2][2] * d.z,
        )
    }

    /// Transforms the point (x, y, z, 1) and returns the homogeneous result [x, y, z, w].
    pub fn transform_point(&self, p: Vec3) -> [f32; 4] {
        let v = [p.x, p.y, p.z, 1.0];
        let mut out = [0.0f32; 4];
        for (r, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|k| self.m[r][k] * v[k]).sum();
        }
        out
    }

    /// View matrix for an eye at `eye` looking at `target`. `up` must not be parallel to the view direction.
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
        let f = (target - eye).normalized();
        let s = f.cross(up).normalized();
        let u = s.cross(f);
        Mat4 {
            m: [
                [s.x, s.y, s.z, -s.dot(eye)],
                [u.x, u.y, u.z, -u.dot(eye)],
                [-f.x, -f.y, -f.z, f.dot(eye)],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Perspective projection with a vertical field of view (radians). Clip z runs -w (near) to +w (far).
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let f = 1.0 / (fov_y * 0.5).tan();
        Mat4 {
            m: [
                [f / aspect, 0.0, 0.0, 0.0],
                [0.0, f, 0.0, 0.0],
                [
                    0.0,
                    0.0,
                    (far + near) / (near - far),
                    2.0 * far * near / (near - far),
                ],
                [0.0, 0.0, -1.0, 0.0],
            ],
        }
    }
}

/// A unit quaternion. Components are normalized in f64 to avoid f32 overflow/underflow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    components: [f32; 4],
}

impl Quat {
    pub const IDENTITY: Self = Self {
        components: [0.0, 0.0, 0.0, 1.0],
    };

    /// Returns None for a zero or non-finite quaternion.
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Option<Self> {
        let values = [x, y, z, w];
        if !values.iter().all(|v| v.is_finite()) {
            return None;
        }
        let length = values
            .iter()
            .map(|&v| f64::from(v).powi(2))
            .sum::<f64>()
            .sqrt();
        if length == 0.0 {
            return None;
        }
        Some(Self {
            components: values.map(|v| (f64::from(v) / length) as f32),
        })
    }
    /// The stored unit components, in x, y, z, w order (the order `new` takes and `from_trs` reads).
    pub fn components(self) -> [f32; 4] {
        self.components
    }
}

impl Mat4 {
    /// Scale, then rotate, then translate. Returns None if inputs or result are non-finite.
    pub fn from_trs(t: Vec3, q: Quat, s: Vec3) -> Option<Self> {
        if ![t.x, t.y, t.z, s.x, s.y, s.z].iter().all(|v| v.is_finite()) {
            return None;
        }
        let [x, y, z, w] = q.components.map(f64::from);
        let rotation = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        let mut result = Self::IDENTITY;
        let scales = [s.x, s.y, s.z];
        for (r, row) in rotation.iter().enumerate() {
            for (c, value) in row.iter().enumerate() {
                result.m[r][c] = (value * f64::from(scales[c])) as f32;
            }
        }
        result.m[0][3] = t.x;
        result.m[1][3] = t.y;
        result.m[2][3] = t.z;
        result
            .m
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(result)
    }

    /// Inverts the actual rounded TRS matrix; None for singular or non-finite results.
    pub fn inverse_trs(t: Vec3, q: Quat, s: Vec3) -> Option<Self> {
        let matrix = Self::from_trs(t, q, s)?;
        let mut a = [[0.0_f64; 8]; 4];
        for (r, row) in a.iter_mut().enumerate() {
            for (c, value) in matrix.m[r].iter().enumerate() {
                row[c] = f64::from(*value);
            }
            row[r + 4] = 1.0;
        }
        for c in 0..4 {
            let pivot = (c..4).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
            if a[pivot][c] == 0.0 {
                return None;
            }
            a.swap(c, pivot);
            let divisor = a[c][c];
            for value in &mut a[c] {
                *value /= divisor;
            }
            let pivot_row = a[c];
            for (r, row) in a.iter_mut().enumerate() {
                if r != c {
                    let factor = row[c];
                    for (value, pivot_value) in row.iter_mut().zip(pivot_row) {
                        *value -= factor * pivot_value;
                    }
                }
            }
        }
        let mut result = Self::IDENTITY;
        for (row, source) in result.m.iter_mut().zip(a) {
            for (value, source) in row.iter_mut().zip(&source[4..]) {
                *value = *source as f32;
            }
        }
        result
            .m
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(result)
    }
}
