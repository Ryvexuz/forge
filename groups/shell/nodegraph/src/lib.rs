//! Shared node values and kind interfaces for a dependency-free node graph.
//!
//! Graph execution, persistence, editing, presentation, and the C interface are
//! reserved modules in this initial skeleton.

// This boundary is reserved for the future C interface.
pub mod capi;

#[forbid(unsafe_code)]
pub mod editor;
#[forbid(unsafe_code)]
pub mod eval;
#[forbid(unsafe_code)]
pub mod format;
#[forbid(unsafe_code)]
pub mod graph;
#[forbid(unsafe_code)]
pub mod history;
#[forbid(unsafe_code)]
pub mod kind;
#[forbid(unsafe_code)]
pub mod kinds;
#[forbid(unsafe_code)]
pub mod subgraph;
#[forbid(unsafe_code)]
pub mod value;
#[forbid(unsafe_code)]
pub mod view;
