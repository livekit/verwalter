//! Core types of verwalter. See specs/versioning.allium for the specification;
//! each module names the spec constructs it implements.
//!
//! Versioned files and semver come from `knope-versioning`; this crate adds
//! the changeset format (with quoted names), the config file, dependency
//! discovery, the release plan and the commands built on it.

// Pure: values, the plan, and file edits on a directory.
pub mod apply;
pub mod changeset;
pub mod config;
pub mod handler;
pub mod plan;
pub mod version;

// Adapters: the checkout, git, GitHub and the workspace manifests.
pub mod git;
pub mod github;
pub mod repo;
pub mod workspace;

// Commands, one per subcommand of the binary.
pub mod branches;
pub mod check;
pub mod prerelease;
pub mod publish;
pub mod registry;
pub mod release;
pub mod status;

pub use knope_versioning::semver::Version;
