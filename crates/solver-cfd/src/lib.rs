//! OpenFOAM adapter for viscous volume flow. All displayed fields and loads are
//! read from the same case/time; no panel field or synthetic wake is mixed in.
mod case;
mod field;
mod runner;
mod runtime;
pub use case::{Config, FlowModel, Resolution, write_case};
pub use field::{VolumeField, read_field};
pub use runner::{Backend, RunResult, load_result, run};
