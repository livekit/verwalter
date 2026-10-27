//! Spec: value Version, enum BumpLevel, black boxes next_version and
//! next_prerelease_version. Version arithmetic is knope-versioning's
//! `PackageVersions`, built from the current file version plus git tags.

use knope_versioning::semver::{Label, PreVersion, Prerelease, Version};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

fn components(v: &Version) -> (u64, u64, u64) {
    // StableVersion keeps its fields private; Display is the public view.
    let s = v.stable_component().to_string();
    let mut it = s.split('.').map(|n| n.parse::<u64>().expect("stable version components"));
    (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
}

/// Apply one level to a stable version, changesets rules: `0.x` + major
/// gives `1.0.0`, no special cases.
fn bump(v: &Version, level: BumpLevel) -> Version {
    let (major, minor, patch) = components(v);
    match level {
        BumpLevel::Major => Version::new(major + 1, 0, 0, None),
        BumpLevel::Minor => Version::new(major, minor + 1, 0, None),
        BumpLevel::Patch => Version::new(major, minor, patch + 1, None),
    }
}

/// Spec: next_version. The highest pending level wins; no pending bump
/// returns the version unchanged. From a pre-release, the result is the
/// stable version it was leading to (the promote step), whatever the level.
pub fn next_version(current: &Version, pending: impl IntoIterator<Item = BumpLevel>) -> Version {
    match pending.into_iter().max() {
        _ if current.is_prerelease() => Version::Stable(current.stable_component()),
        Some(level) => bump(current, level),
        None => current.clone(),
    }
}

/// Spec: next_prerelease_version. `1.5.1` + minor gives `1.6.0-rc.1`;
/// with `1.6.0-rc.1` already released, `1.6.0-rc.2`. The target is bumped
/// from the latest stable release so that a later `major` request from an
/// rc moves to the next major, not to the rc's own target. `label` is
/// `rc` by default (config: prerelease_label).
pub fn next_prerelease_version(current: &Version, released: &[Version], level: BumpLevel, label: &str) -> Version {
    let base = released.iter().filter(|r| !r.is_prerelease()).max().cloned().unwrap_or_else(|| {
        if current.is_prerelease() { Version::Stable(current.stable_component()) } else { current.clone() }
    });
    let target = bump(&base, level).stable_component();
    let marker = format!("-{label}.");
    let last = released
        .iter()
        .filter(|r| r.is_prerelease() && r.stable_component() == target)
        .filter_map(|r| r.to_string().rsplit_once(marker.as_str()).and_then(|(_, n)| n.parse::<u64>().ok()))
        .max();
    Version::Pre(PreVersion {
        stable_component: target,
        pre_component: Prerelease::new(Label::from(label), last.map_or(1, |n| n + 1)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
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
        let h = v("1.5.1");
        assert_eq!(next_version(&h, [BumpLevel::Patch]), v("1.5.2"));
        assert_eq!(next_version(&h, [BumpLevel::Minor]), v("1.6.0"));
        assert_eq!(next_version(&h, [BumpLevel::Major]), v("2.0.0"));
    }

    #[test]
    fn zero_x_follows_changesets_rules() {
        let h = v("0.7.3");
        assert_eq!(next_version(&h, [BumpLevel::Major]), v("1.0.0"));
        assert_eq!(next_version(&h, [BumpLevel::Minor]), v("0.8.0"));
        assert_eq!(next_version(&h, [BumpLevel::Patch]), v("0.7.4"));
    }

    #[test]
    fn next_version_takes_highest_pending_level() {
        let h = v("1.5.1");
        assert_eq!(next_version(&h, [BumpLevel::Patch, BumpLevel::Minor, BumpLevel::Patch]), v("1.6.0"));
        assert_eq!(next_version(&h, []), v("1.5.1"));
    }

    // Spec: MaintainerRequestsPreRelease. Mirrors python-sdks minor-rc,
    // next-rc and promote.
    #[test]
    fn prerelease_sequence() {
        let released = [v("1.5.1")];
        let h = v("1.5.1");
        assert_eq!(next_prerelease_version(&h, &released, BumpLevel::Minor, "rc"), v("1.6.0-rc.1"));

        // rc.1 merged and tagged: files say 1.6.0-rc.1, tags know 1.5.1 too.
        let released = [v("1.5.1"), v("1.6.0-rc.1")];
        let h = v("1.6.0-rc.1");
        assert_eq!(next_prerelease_version(&h, &released, BumpLevel::Minor, "rc"), v("1.6.0-rc.2"));
        // A different target version restarts at rc.1.
        assert_eq!(next_prerelease_version(&h, &released, BumpLevel::Major, "rc"), v("2.0.0-rc.1"));
        // Another label starts its own sequence.
        assert_eq!(next_prerelease_version(&h, &released, BumpLevel::Minor, "beta"), v("1.6.0-beta.1"));

        // The final release from the rc: the pending minor resolves to 1.6.0.
        assert_eq!(next_version(&h, [BumpLevel::Minor]), v("1.6.0"));
    }
}
