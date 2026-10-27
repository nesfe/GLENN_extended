//! File adapters; importing geometry never starts a solver.
pub use glenn_core::{geometry, model};
pub use glenn_solver_panel::physics;
mod airfoil;
pub use airfoil::*;
pub mod stl;
