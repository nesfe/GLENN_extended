//! GLENN Extended: numerical models independent of the desktop interface.
//!
//! Coordinates use metres for the wing mesh and chord fractions for airfoils.
//! Public simulation inputs use SI units, except angles (degrees) and profile
//! camber/thickness (percent). See `docs/NUMERICS.md` for model limitations.
pub mod geometry;
pub mod interchange;
pub mod model;
pub mod physics;
