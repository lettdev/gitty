//! Repository logic shared by every gitty frontend.
//!
//! Reads and writes both go through the `git` executable so results match what
//! the user would get typing the command themselves (same config, hooks,
//! credentials). Every mutating operation is recorded in an operation log so it
//! can be undone.

pub mod branch;
pub mod diff;
pub mod git;
pub mod graph;
pub mod log;
pub mod oplog;
pub mod ops;
pub mod repo;
pub mod status;

pub use git::{Error, Result};
pub use repo::{Repo, RepoState};
