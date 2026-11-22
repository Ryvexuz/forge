//! W3C separable blending on unassociated channels.
//!
//! Channel functions require finite inputs in `[0, 1]` (including subnormals).
//! They do not clamp or validate inputs outside that domain. Source-over instead
//! takes premultiplied RGBA: each colour channel must be in `[0, alpha]`,
//! with finite alpha in `[0, 1]`. No colour-space conversion is performed.

pub fn normal(_backdrop: f32, source: f32) -> f32 {
    source
}

pub fn multiply(backdrop: f32, source: f32) -> f32 {
    backdrop * source
}

pub fn screen(backdrop: f32, source: f32) -> f32 {
    backdrop + source - backdrop * source
}

pub fn overlay(backdrop: f32, source: f32) -> f32 {
    hard_light(source, backdrop)
}

pub fn darken(backdrop: f32, source: f32) -> f32 {
    backdrop.min(source)
}

pub fn lighten(backdrop: f32, source: f32) -> f32 {
    backdrop.max(source)
}

pub fn color_dodge(backdrop: f32, source: f32) -> f32 {
    if backdrop == 0.0 {
        0.0
    } else if source == 1.0 {
        1.0
    } else {
        (backdrop / (1.0 - source)).min(1.0)
    }
}

pub fn color_burn(backdrop: f32, source: f32) -> f32 {
    if backdrop == 1.0 {
        1.0
    } else if source == 0.0 {
        0.0
    } else {
        // Compare before dividing: subnormal sources must not overflow.
        let complement = 1.0 - backdrop;
        if complement >= source {
            0.0
        } else {
            1.0 - complement / source
        }
    }
}

pub fn hard_light(backdrop: f32, source: f32) -> f32 {
    if source <= 0.5 {
        2.0 * backdrop * source
    } else {
        1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source)
    }
}

pub fn soft_light(backdrop: f32, source: f32) -> f32 {
    if source <= 0.5 {
        backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop)
    } else {
        let d = if backdrop <= 0.25 {
            ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop
        } else {
            backdrop.sqrt()
        };
        backdrop + (2.0 * source - 1.0) * (d - backdrop)
    }
}

pub fn difference(backdrop: f32, source: f32) -> f32 {
    (backdrop - source).abs()
}

pub fn exclusion(backdrop: f32, source: f32) -> f32 {
    backdrop + source - 2.0 * backdrop * source
}

/// Porter-Duff source-over on premultiplied RGBA, including alpha.
pub fn source_over(backdrop: [f32; 4], source: [f32; 4]) -> [f32; 4] {
    let remaining = 1.0 - source[3];
    std::array::from_fn(|i| source[i] + backdrop[i] * remaining)
}
