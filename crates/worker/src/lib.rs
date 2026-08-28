#![recursion_limit = "512"]
//! Worker: Oxana consume + probe. In-memory drain is gone; jobs run via `knowledge::pipeline`.

pub mod consume;
pub mod probe;
