//! Shell-line parsing, specs, matching and ranking: everything terme knows
//! about commands, with no terminal or process handling.

pub mod complete;
pub mod context;
pub mod edit;
pub mod lexer;
pub mod matching;
pub mod spec;

pub use complete::{Candidate, Env, Kind, Suffix, Suggestions, complete, worth_showing};
pub use context::{Context, analyze};
pub use spec::SpecSet;
