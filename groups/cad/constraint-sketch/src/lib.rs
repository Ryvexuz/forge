//! Standard-library-only constraint-sketch skeleton.
//!
//! Geometry is the initial data layer; the remaining modules reserve the
//! planned constraint, solver, analysis, and file-format boundaries.

pub mod constraint;
pub mod dof;
pub mod format;
pub mod geom;
pub mod solve;
