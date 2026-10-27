//! Core types of verwalter. See versioning.allium for the specification;
//! each module names the spec constructs it implements.
//!
//! Versioned files and semver come from `knope-versioning`; this crate adds
//! the changeset format (with quoted names), the config file, dependency
//! discovery and, later, the release plan.

pub mod apply;
pub mod changeset;
pub mod config;
pub mod handler;
pub mod plan;
pub mod version;

pub use knope_versioning::semver::Version;
