//! Spec: value Version, enum BumpLevel, black boxes next_version and
//! next_prerelease_version. Version arithmetic is knope-versioning's
//! `PackageVersions`, built from the current file version plus git tags.

use knope_versioning::semver::{Label, PreVersion, Prerelease, Version};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

/// The changeset spelling: `patch`, `minor`, `major`.
impl std::fmt::Display for BumpLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
        })
    }
}

impl std::str::FromStr for BumpLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "major" => Ok(Self::Major),
            "minor" => Ok(Self::Minor),
            "patch" => Ok(Self::Patch),
            _ => Err(()),
        }
    }
}

fn components(v: &Version) -> (u64, u64, u64) {
    // StableVersion keeps its fields private; Display is the public view.
    let s = v.stable_component().to_string();
    let mut it = s
        .split('.')
        .map(|n| n.parse::<u64>().expect("stable version components"));
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
/// stable version it was leading to (the promote step) when that version
/// already covers the pending level, else that version bumped by the
/// level: `1.6.0-rc.2` + major gives `2.0.0`, + minor or patch `1.6.0`.
/// A release is never lower than the rc that preceded it.
pub fn next_version(current: &Version, pending: impl IntoIterator<Item = BumpLevel>) -> Version {
    let level = pending.into_iter().max();
    if current.is_prerelease() {
        let target = Version::Stable(current.stable_component());
        let (_, minor, patch) = components(&target);
        let covered = match level {
            None | Some(BumpLevel::Patch) => true,
            Some(BumpLevel::Minor) => patch == 0,
            Some(BumpLevel::Major) => minor == 0 && patch == 0,
        };
        return if covered {
            target
        } else {
            bump(&target, level.unwrap())
        };
    }
    match level {
        Some(level) => bump(current, level),
        None => current.clone(),
    }
}

/// Spec: next_prerelease_version. `1.5.1` + minor gives `1.6.0-rc.1`;
/// with `1.6.0-rc.1` already released, `1.6.0-rc.2`. The target is bumped
/// from the latest stable version, the higher of the current one and the
/// newest stable tag, so that a later `major` request from an rc moves
/// to the next major, not to the rc's own target, and a version that
/// moved past its tags, by hand or because tags are missing, is never
/// pulled back. `label` is `rc` by default (config: prerelease_label).
pub fn next_prerelease_version(
    current: &Version,
    released: &[Version],
    level: BumpLevel,
    label: &str,
) -> Version {
    let current_stable = if current.is_prerelease() {
        // An rc's own target is not a base: 1.6.0-rc.1 + major is 2.0.0,
        // not 2.0.0 from 1.6.0 plus the rc's stable component 1.6.0.
        None
    } else {
        Some(current.clone())
    };
    let base = released
        .iter()
        .filter(|r| !r.is_prerelease())
        .max()
        .cloned()
        .into_iter()
        .chain(current_stable)
        .max()
        .unwrap_or_else(|| Version::Stable(current.stable_component()));
    let target = bump(&base, level).stable_component();
    let marker = format!("-{label}.");
    let last = released
        .iter()
        .filter(|r| r.is_prerelease() && r.stable_component() == target)
        .filter_map(|r| {
            r.to_string()
                .rsplit_once(marker.as_str())
                .and_then(|(_, n)| n.parse::<u64>().ok())
        })
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
        assert_eq!(
            next_version(&h, [BumpLevel::Patch, BumpLevel::Minor, BumpLevel::Patch]),
            v("1.6.0")
        );
        assert_eq!(next_version(&h, []), v("1.5.1"));
    }

    // Spec: MaintainerRequestsPreRelease. Mirrors python-sdks minor-rc,
    // next-rc and promote.
    #[test]
    fn prerelease_sequence() {
        let released = [v("1.5.1")];
        let h = v("1.5.1");
        assert_eq!(
            next_prerelease_version(&h, &released, BumpLevel::Minor, "rc"),
            v("1.6.0-rc.1")
        );

        // rc.1 merged and tagged: files say 1.6.0-rc.1, tags know 1.5.1 too.
        let released = [v("1.5.1"), v("1.6.0-rc.1")];
        let h = v("1.6.0-rc.1");
        assert_eq!(
            next_prerelease_version(&h, &released, BumpLevel::Minor, "rc"),
            v("1.6.0-rc.2")
        );
        // A different target version restarts at rc.1.
        assert_eq!(
            next_prerelease_version(&h, &released, BumpLevel::Major, "rc"),
            v("2.0.0-rc.1")
        );
        // Another label starts its own sequence.
        assert_eq!(
            next_prerelease_version(&h, &released, BumpLevel::Minor, "beta"),
            v("1.6.0-beta.1")
        );

        // The final release from the rc: the pending minor resolves to 1.6.0.
        assert_eq!(next_version(&h, [BumpLevel::Minor]), v("1.6.0"));

        // The version moved past its tags, by hand or with tags missing:
        // the rc is bumped from the version, never pulled back to a tag.
        let released = [v("0.3.27")];
        assert_eq!(
            next_prerelease_version(&v("0.3.54"), &released, BumpLevel::Patch, "rc"),
            v("0.3.55-rc.1")
        );
    }

    // Spec: the pending level wins over the rc's target. A release is
    // never lower than its rc.
    #[test]
    fn promote_from_rc_follows_the_pending_level() {
        let rc = v("1.6.0-rc.2");
        assert_eq!(next_version(&rc, [BumpLevel::Patch]), v("1.6.0"));
        assert_eq!(next_version(&rc, [BumpLevel::Minor]), v("1.6.0"));
        assert_eq!(next_version(&rc, [BumpLevel::Major]), v("2.0.0"));
        assert_eq!(next_version(&rc, []), v("1.6.0"));
        // A patch rc with a minor changeset added after it.
        assert_eq!(
            next_version(&v("1.5.2-rc.1"), [BumpLevel::Minor]),
            v("1.6.0")
        );
        assert_eq!(
            next_version(&v("2.0.0-rc.1"), [BumpLevel::Major]),
            v("2.0.0")
        );
        assert_eq!(
            next_version(&v("0.4.0-rc.1"), [BumpLevel::Major]),
            v("1.0.0")
        );
    }
}
