//! Spec: rule OpenOrUpdateReleasePullRequest, black box next_version.
//! Pure: the caller reads the repository and GitHub, this module decides
//! what the release PR contains.

use std::collections::BTreeMap;

use crate::{
    changeset::Changeset,
    version::{next_version, BumpLevel},
    Version,
};

/// What the caller knows about one package before planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageState {
    pub name: String,
    pub version: Version,
    /// In-repo packages this one depends on (handler::internal_dependencies).
    pub dependencies: Vec<String>,
}

/// One package's part of the release PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRelease {
    pub package: String,
    pub from: Version,
    pub to: Version,
    pub level: BumpLevel,
    /// Changeset files that name this package.
    pub changesets: Vec<String>,
    /// Planned packages this one depends on, for the "updated dependencies"
    /// changelog line.
    pub bumped_dependencies: Vec<String>,
}

/// Dependencies first: by depth in the dependency graph, then by name.
/// The order releases are applied, tagged, created and handed to the
/// publish workflow.
pub fn dependency_order<'a>(
    names: impl IntoIterator<Item = &'a str>,
    deps: &dyn Fn(&str) -> Vec<&'a str>,
) -> Vec<&'a str> {
    fn depth<'a>(
        name: &'a str,
        deps: &dyn Fn(&str) -> Vec<&'a str>,
        memo: &mut BTreeMap<&'a str, usize>,
    ) -> usize {
        if let Some(d) = memo.get(name) {
            return *d;
        }
        let d = deps(name)
            .into_iter()
            .map(|d| depth(d, deps, memo) + 1)
            .max()
            .unwrap_or(0);
        memo.insert(name, d);
        d
    }
    let mut memo = BTreeMap::new();
    let mut order: Vec<&str> = names.into_iter().collect();
    order.sort_by_key(|n| (depth(n, deps, &mut memo), n.to_string()));
    order
}

/// Every package with a pending bump, dependencies first. Empty when
/// nothing is pending: no release PR.
pub fn plan(
    packages: &[PackageState],
    changesets: &[Changeset],
    dependent_level: BumpLevel,
) -> Vec<PlannedRelease> {
    let by_name: BTreeMap<&str, &PackageState> =
        packages.iter().map(|p| (p.name.as_str(), p)).collect();

    // Direct bumps: the highest level named in a changeset.
    let mut level: BTreeMap<&str, BumpLevel> = BTreeMap::new();
    let mut files: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for p in packages {
        for c in changesets {
            if let Some(l) = c.unreleased_for(&p.name) {
                level
                    .entry(&p.name)
                    .and_modify(|cur| *cur = (*cur).max(l))
                    .or_insert(l);
                files.entry(&p.name).or_default().push(c.file.clone());
            }
        }
    }

    // Dependent bumps, to a fixpoint: a package whose planned dependency is
    // bumped gets at least `dependent_level`.
    loop {
        let before = level.len();
        for p in packages {
            if p.dependencies
                .iter()
                .any(|d| level.contains_key(d.as_str()))
            {
                level.entry(&p.name).or_insert(dependent_level);
            }
        }
        if level.len() == before {
            break;
        }
    }

    let planned_deps = |name: &str| -> Vec<&str> {
        by_name[name]
            .dependencies
            .iter()
            .map(String::as_str)
            .filter(|d| level.contains_key(d))
            .collect()
    };
    dependency_order(level.keys().copied(), &planned_deps)
        .into_iter()
        .map(|name| {
            let p = by_name[name];
            PlannedRelease {
                package: p.name.clone(),
                from: p.version.clone(),
                to: next_version(&p.version, [level[name]]),
                level: level[name],
                changesets: files.remove(name).unwrap_or_default(),
                bumped_dependencies: planned_deps(name).into_iter().map(String::from).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, deps: &[&str]) -> PackageState {
        PackageState {
            name: name.into(),
            version: version.parse().unwrap(),
            dependencies: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn cs(file: &str, bumps: &[(&str, BumpLevel)]) -> Changeset {
        let front: String = bumps.iter().map(|(p, l)| format!("{p}: {l}\n")).collect();
        Changeset::parse(file, &format!("---\n{front}---\n{file}\n")).unwrap()
    }

    fn python_sdks() -> Vec<PackageState> {
        vec![
            pkg("livekit-protocol", "1.1.5", &[]),
            pkg("livekit-api", "1.1.1", &["livekit-protocol"]),
            pkg("livekit", "1.2.0", &["livekit-protocol"]),
        ]
    }

    fn names(plan: &[PlannedRelease]) -> Vec<&str> {
        plan.iter().map(|r| r.package.as_str()).collect()
    }

    #[test]
    fn no_changesets_no_plan() {
        assert!(plan(&python_sdks(), &[], BumpLevel::Patch).is_empty());
    }

    // Spec: dependent bumps are added by the tool, dependencies come first.
    #[test]
    fn protocol_change_fans_out_to_dependents() {
        let changesets = [cs("a.md", &[("livekit-protocol", BumpLevel::Minor)])];
        let p = plan(&python_sdks(), &changesets, BumpLevel::Patch);
        assert_eq!(names(&p), ["livekit-protocol", "livekit", "livekit-api"]);
        assert_eq!(p[0].to.to_string(), "1.2.0");
        assert_eq!(p[0].changesets, vec!["a.md"]);
        assert_eq!(p[1].to.to_string(), "1.2.1");
        assert_eq!(p[1].level, BumpLevel::Patch);
        assert!(p[1].changesets.is_empty());
        assert_eq!(p[1].bumped_dependencies, vec!["livekit-protocol"]);
    }

    #[test]
    fn direct_bump_wins_over_dependent_level() {
        let changesets = [cs(
            "a.md",
            &[
                ("livekit-protocol", BumpLevel::Patch),
                ("livekit", BumpLevel::Major),
            ],
        )];
        let p = plan(&python_sdks(), &changesets, BumpLevel::Patch);
        let rtc = p.iter().find(|r| r.package == "livekit").unwrap();
        assert_eq!(rtc.to.to_string(), "2.0.0");
    }

    #[test]
    fn leaf_only_change_plans_only_the_leaf() {
        let changesets = [cs("a.md", &[("livekit-api", BumpLevel::Patch)])];
        let p = plan(&python_sdks(), &changesets, BumpLevel::Patch);
        assert_eq!(names(&p), ["livekit-api"]);
    }

    #[test]
    fn deep_chain_is_transitive_and_ordered() {
        // rust-sdks shape: protocol <- api <- livekit <- ffi
        let packages = vec![
            pkg("livekit-protocol", "0.3.0", &[]),
            pkg("livekit-api", "0.4.0", &["livekit-protocol"]),
            pkg("livekit", "0.7.0", &["livekit-api", "livekit-protocol"]),
            pkg("livekit-ffi", "0.12.0", &["livekit"]),
        ];
        let changesets = [cs("x.md", &[("livekit-protocol", BumpLevel::Patch)])];
        let p = plan(&packages, &changesets, BumpLevel::Patch);
        assert_eq!(
            names(&p),
            ["livekit-protocol", "livekit-api", "livekit", "livekit-ffi"]
        );
    }

    // Spec: next_version of a pre-release is the promote step.
    #[test]
    fn prerelease_promotes_to_final() {
        let packages = vec![pkg("livekit", "1.6.0-rc.2", &[])];
        let changesets = [cs("x.md", &[("livekit", BumpLevel::Minor)])];
        let p = plan(&packages, &changesets, BumpLevel::Patch);
        assert_eq!(p[0].to.to_string(), "1.6.0");
    }
}
