//! Core types of verwalter. See versioning.allium for the specification;
//! each module names the spec constructs it implements.

pub mod changeset;
pub mod config;
pub mod handler;
pub mod version;

pub use semver::Version;
