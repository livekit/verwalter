//! Spec: value Version, enum BumpLevel, black boxes next_version and
//! next_prerelease_version. Version arithmetic is knope-versioning's
//! `PackageVersions`, built from the current file version plus git tags.

use knope_versioning::{
    package::Name,
    release_notes::ReleaseNotes,
    semver::{Label, PreVersion, Prerelease, Rule, StableRule, Version},
    Package,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

impl From<BumpLevel> for StableRule {
    fn from(level: BumpLevel) -> Self {
        match level {
            BumpLevel::Patch => StableRule::Patch,
            BumpLevel::Minor => StableRule::Minor,
            BumpLevel::Major => StableRule::Major,
        }
    }
}

/// The version history of one package: the version in its files plus every
/// version that has a release tag. Tags matter for pre-releases, so that
/// `rc.2` follows `rc.1` even after the files went back to a stable version.
/// knope only exposes version arithmetic through `Package`, so this is one.
pub fn history(current: &Version, released: &[Version]) -> Package {
    let tags: Vec<String> = released.iter().chain(std::iter::once(current)).map(|v| format!("v{v}")).collect();
    Package::new(Name::Default, &tags, Vec::new(), &[], ReleaseNotes::default(), None)
        .expect("a package without versioned files is always valid")
}

/// Spec: next_version. The highest pending level wins; no pending bump
/// returns the latest version unchanged. From a pre-release, the result is
/// the stable version it was leading to (the promote step).
///
/// knope semantics for 0.x: `major` bumps the minor component and `minor`
/// bumps the patch component. rust-sdks releases this way today.
pub fn next_version(history: &Package, pending: impl IntoIterator<Item = BumpLevel>) -> Version {
    let Some(level) = pending.into_iter().max() else {
        return history.latest_version().expect("history holds the current version");
    };
    history.calculate_new_version(Rule::Stable(level.into())).expect("a stable rule never fails")
}

/// Spec: next_prerelease_version. `1.5.1` + minor gives `1.6.0-rc.1`;
/// with `1.6.0-rc.1` already released, `1.6.0-rc.2`. Numbering starts at 1
/// (python-sdks convention); knope's own `Rule::Pre` starts at 0.
pub fn next_prerelease_version(history: &Package, released: &[Version], level: BumpLevel) -> Version {
    let target = next_version(history, [level]).stable_component();
    let last = released
        .iter()
        .filter(|r| r.is_prerelease() && r.stable_component() == target)
        .filter_map(|r| r.to_string().rsplit_once("-rc.").and_then(|(_, n)| n.parse::<u64>().ok()))
        .max();
    Version::Pre(PreVersion {
        stable_component: target,
        pre_component: Prerelease::new(Label::from("rc"), last.map_or(1, |n| n + 1)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    fn hist(current: &str, released: &[&str]) -> Package {
        let released: Vec<Version> = released.iter().map(|t| v(t)).collect();
        history(&v(current), &released)
    }

    // obligation: value-equality.Version
    #[test]
    fn version_equality_is_by_value() {
        assert_eq!(v("1.2.3"), v("1.2.3"));
        assert_ne!(v("1.2.3"), v("1.2.3-rc.1"));
        assert!(v("1.2.3-rc.1").is_prerelease());
        assert_eq!(v("1.2.3-rc.1").to_string(), "1.2.3-rc.1");
    }

    // obligation: enum-comparable.BumpLevel
    #[test]
    fn bump_levels_order_patch_minor_major() {
        assert!(BumpLevel::Patch < BumpLevel::Minor);
        assert!(BumpLevel::Minor < BumpLevel::Major);
    }

    #[test]
    fn bump_each_level() {
        let h = hist("1.5.1", &[]);
        assert_eq!(next_version(&h, [BumpLevel::Patch]), v("1.5.2"));
        assert_eq!(next_version(&h, [BumpLevel::Minor]), v("1.6.0"));
        assert_eq!(next_version(&h, [BumpLevel::Major]), v("2.0.0"));
    }

    #[test]
    fn zero_x_major_bumps_minor() {
        let h = hist("0.7.3", &[]);
        assert_eq!(next_version(&h, [BumpLevel::Major]), v("0.8.0"));
        assert_eq!(next_version(&h, [BumpLevel::Minor]), v("0.7.4"));
    }

    #[test]
    fn next_version_takes_highest_pending_level() {
        let h = hist("1.5.1", &[]);
        assert_eq!(next_version(&h, [BumpLevel::Patch, BumpLevel::Minor, BumpLevel::Patch]), v("1.6.0"));
        assert_eq!(next_version(&h, []), v("1.5.1"));
    }

    // Spec: MaintainerRequestsPreRelease. Mirrors python-sdks minor-rc,
    // next-rc and promote.
    #[test]
    fn prerelease_sequence() {
        let released = [v("1.5.1")];
        let h = history(&v("1.5.1"), &released);
        assert_eq!(next_prerelease_version(&h, &released, BumpLevel::Minor), v("1.6.0-rc.1"));

        // rc.1 merged and tagged: files say 1.6.0-rc.1, tags know 1.5.1 too.
        let released = [v("1.5.1"), v("1.6.0-rc.1")];
        let h = history(&v("1.6.0-rc.1"), &released);
        assert_eq!(next_prerelease_version(&h, &released, BumpLevel::Minor), v("1.6.0-rc.2"));
        // A different target version restarts at rc.1.
        assert_eq!(next_prerelease_version(&h, &released, BumpLevel::Major), v("2.0.0-rc.1"));

        // The final release from the rc: the pending minor resolves to 1.6.0.
        assert_eq!(next_version(&h, [BumpLevel::Minor]), v("1.6.0"));
    }
}
