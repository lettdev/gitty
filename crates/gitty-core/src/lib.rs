//! Repository logic shared by every gitty frontend.
//!
//! Reads and writes both go through the `git` executable so results match what
//! the user would get typing the command themselves (same config, hooks,
//! credentials). Every mutating operation is recorded in an operation log so it
//! can be undone.

pub mod branch;
pub mod conflict;
pub mod diff;
pub mod git;
pub mod graph;
pub mod ignore;
pub mod insight;
pub mod log;
pub mod oplog;
pub mod ops;
pub mod project;
pub mod rebase;
pub mod repo;
pub mod stash;
pub mod status;
pub mod unity;
pub mod workspace;

pub use git::{Error, Result};
pub use repo::{Repo, RepoState};
