//! Core types of verwalter. See versioning.allium for the specification;
//! each module names the spec constructs it implements.
//!
//! Versioned files and semver come from `knope-versioning`; this crate adds
//! the changeset format (with quoted names), the config file, dependency
//! discovery and, later, the release plan.

pub mod apply;
pub mod branches;
pub mod changeset;
pub mod check;
pub mod config;
pub mod git;
pub mod github;
pub mod handler;
pub mod plan;
pub mod prerelease;
pub mod release;
pub mod repo;
pub mod status;
pub mod version;
pub mod workspace;

pub use knope_versioning::semver::Version;
