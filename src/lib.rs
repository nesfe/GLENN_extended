//! Compatibility facade. Implementations live in independent workspace crates.
pub use glenn_core::mesh;
pub use glenn_core::{geometry, model};
pub use glenn_engine as engine;
pub use glenn_io as interchange;
pub use glenn_solver_cfd as cfd;
pub use glenn_solver_panel::physics;
