//! Spec: value Version, enum BumpLevel, black boxes next_version and
//! next_prerelease_version.

use semver::{Prerelease, Version};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

/// Apply one bump level to a final version. Bumping a pre-release promotes
/// it: `1.6.0-rc.2` becomes `1.6.0` whatever the level, because the rc
/// already encodes the bump of the pending changesets.
pub fn bump(v: &Version, level: BumpLevel) -> Version {
    let base = Version::new(v.major, v.minor, v.patch);
    if !v.pre.is_empty() {
        return base;
    }
    match level {
        BumpLevel::Major => Version::new(base.major + 1, 0, 0),
        BumpLevel::Minor => Version::new(base.major, base.minor + 1, 0),
        BumpLevel::Patch => Version::new(base.major, base.minor, base.patch + 1),
    }
}

/// Spec: next_version. The highest pending level wins; no pending bump
/// returns the version unchanged.
pub fn next_version(current: &Version, pending: impl IntoIterator<Item = BumpLevel>) -> Version {
    match pending.into_iter().max() {
        Some(level) => bump(current, level),
        None => current.clone(),
    }
}

/// Spec: next_prerelease_version. From a final version, the next version
/// as `-rc.1`. From an rc of the same next version, `rc.N+1`.
pub fn next_prerelease_version(current: &Version, level: BumpLevel) -> Version {
    let n = match current.pre.as_str().strip_prefix("rc.") {
        Some(n) => n.parse::<u64>().unwrap_or(0) + 1,
        None => 1,
    };
    let mut v = bump(current, level);
    v.pre = Prerelease::new(&format!("rc.{n}")).expect("rc.N is a valid prerelease");
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    // obligation: value-equality.Version
    #[test]
    fn version_equality_is_by_value() {
        assert_eq!(v("1.2.3"), v("1.2.3"));
        assert_ne!(v("1.2.3"), v("1.2.3-rc.1"));
        assert!(v("1.2.3-rc.1").pre.as_str() == "rc.1");
    }

    // obligation: enum-comparable.BumpLevel
    #[test]
    fn bump_levels_order_patch_minor_major() {
        assert!(BumpLevel::Patch < BumpLevel::Minor);
        assert!(BumpLevel::Minor < BumpLevel::Major);
    }

    #[test]
    fn bump_each_level() {
        assert_eq!(bump(&v("1.5.1"), BumpLevel::Patch), v("1.5.2"));
        assert_eq!(bump(&v("1.5.1"), BumpLevel::Minor), v("1.6.0"));
        assert_eq!(bump(&v("1.5.1"), BumpLevel::Major), v("2.0.0"));
    }

    #[test]
    fn next_version_takes_highest_pending_level() {
        let pending = [BumpLevel::Patch, BumpLevel::Minor, BumpLevel::Patch];
        assert_eq!(next_version(&v("1.5.1"), pending), v("1.6.0"));
        assert_eq!(next_version(&v("1.5.1"), []), v("1.5.1"));
    }

    // Spec: MaintainerRequestsPreRelease. Mirrors python-sdks minor-rc,
    // next-rc and promote.
    #[test]
    fn prerelease_sequence() {
        let rc1 = next_prerelease_version(&v("1.5.1"), BumpLevel::Minor);
        assert_eq!(rc1, v("1.6.0-rc.1"));
        let rc2 = next_prerelease_version(&rc1, BumpLevel::Minor);
        assert_eq!(rc2, v("1.6.0-rc.2"));
        // A final release from an rc drops the suffix (the promote step).
        assert_eq!(bump(&rc2, BumpLevel::Minor), v("1.6.0"));
        assert_eq!(next_version(&rc2, [BumpLevel::Patch]), v("1.6.0"));
    }
}
