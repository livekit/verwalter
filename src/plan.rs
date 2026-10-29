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
    /// Config: the other members of this package's fixed group.
    pub fixed_with: Vec<String>,
}

impl PackageState {
    pub fn new(name: &str, version: Version, dependencies: Vec<String>) -> Self {
        Self {
            name: name.to_string(),
            version,
            dependencies,
            fixed_with: Vec::new(),
        }
    }
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
    /// The other members of its fixed group, released at the same version.
    pub fixed_with: Vec<String>,
}

/// Fixed groups whose members are not at one version: a warning until the
/// next release aligns them.
pub fn fixed_group_drift(packages: &[PackageState]) -> Vec<String> {
    let mut out = Vec::new();
    for p in packages {
        if p.fixed_with.iter().any(|o| o.as_str() < p.name.as_str()) {
            continue; // reported once, by the first member in name order
        }
        let members: Vec<&PackageState> = std::iter::once(p)
            .chain(
                p.fixed_with
                    .iter()
                    .filter_map(|o| packages.iter().find(|q| &q.name == o)),
            )
            .collect();
        if members.iter().any(|m| m.version != p.version) {
            let list: Vec<String> = members
                .iter()
                .map(|m| format!("{} {}", m.name, m.version))
                .collect();
            out.push(format!(
                "fixed group is not at one version ({}); the next release aligns it",
                list.join(", ")
            ));
        }
    }
    out
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

    // Spec: has_pending_bump. A package left at a pre-release is promoted
    // even without changesets; patch resolves to the rc's target.
    for p in packages.iter().filter(|p| p.version.is_prerelease()) {
        level.entry(&p.name).or_insert(BumpLevel::Patch);
    }

    // Dependent bumps and fixed groups, to a fixpoint: a package whose
    // planned dependency is bumped gets at least `dependent_level`; a
    // fixed group with any planned member is planned whole, at the highest
    // level in it.
    loop {
        let before = level.clone();
        for p in packages {
            if p.dependencies
                .iter()
                .any(|d| level.contains_key(d.as_str()))
            {
                level.entry(&p.name).or_insert(dependent_level);
            }
        }
        for p in packages.iter().filter(|p| !p.fixed_with.is_empty()) {
            let members =
                std::iter::once(p.name.as_str()).chain(p.fixed_with.iter().map(String::as_str));
            let Some(l) = members.clone().filter_map(|m| level.get(m).copied()).max() else {
                continue;
            };
            for m in members {
                level.insert(&by_name[m].name, l);
            }
        }
        if level == before {
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
            // A fixed group moves from its highest version.
            let base = p
                .fixed_with
                .iter()
                .map(|o| &by_name[o.as_str()].version)
                .chain(std::iter::once(&p.version))
                .max()
                .unwrap();
            PlannedRelease {
                package: p.name.clone(),
                from: p.version.clone(),
                to: next_version(base, [level[name]]),
                level: level[name],
                changesets: files.remove(name).unwrap_or_default(),
                bumped_dependencies: planned_deps(name).into_iter().map(String::from).collect(),
                fixed_with: p.fixed_with.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, deps: &[&str]) -> PackageState {
        PackageState::new(
            name,
            version.parse().unwrap(),
            deps.iter().map(|d| d.to_string()).collect(),
        )
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

    // Spec: config.fixed. The group moves from its highest version by the
    // highest level in it; a member without a changeset comes along.
    #[test]
    fn fixed_group_releases_at_one_version() {
        let mut packages = vec![
            pkg("core", "0.3.1", &[]),
            pkg("ffi", "0.2.0", &["core"]),
            pkg("other", "1.0.0", &[]),
        ];
        packages[0].fixed_with = vec!["ffi".into()];
        packages[1].fixed_with = vec!["core".into()];
        let changesets = [cs("x.md", &[("ffi", BumpLevel::Minor)])];
        let p = plan(&packages, &changesets, BumpLevel::Patch);
        let got: Vec<(&str, &str, &str)> = p
            .iter()
            .map(|r| {
                (
                    r.package.as_str(),
                    r.to.to_string().leak() as &str,
                    r.level.to_string().leak() as &str,
                )
            })
            .collect();
        assert_eq!(got, [("core", "0.4.0", "minor"), ("ffi", "0.4.0", "minor")]);
        assert_eq!(p[0].fixed_with, ["ffi"]);
        assert!(p[0].changesets.is_empty());
    }

    #[test]
    fn drift_is_reported_once_per_group() {
        let mut packages = vec![pkg("core", "0.3.1", &[]), pkg("ffi", "0.2.0", &[])];
        assert!(fixed_group_drift(&packages).is_empty());
        packages[0].fixed_with = vec!["ffi".into()];
        packages[1].fixed_with = vec!["core".into()];
        let w = fixed_group_drift(&packages);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("core 0.3.1, ffi 0.2.0"), "{}", w[0]);
    }

    // Spec: next_version of a pre-release is the promote step, unless a
    // higher level landed after the rc.
    #[test]
    fn prerelease_promotes_to_final() {
        let packages = vec![pkg("livekit", "1.6.0-rc.2", &[])];
        let changesets = [cs("x.md", &[("livekit", BumpLevel::Minor)])];
        let p = plan(&packages, &changesets, BumpLevel::Patch);
        assert_eq!(p[0].to.to_string(), "1.6.0");
        let changesets = [cs("x.md", &[("livekit", BumpLevel::Major)])];
        let p = plan(&packages, &changesets, BumpLevel::Patch);
        assert_eq!(p[0].to.to_string(), "2.0.0");
        // No changeset at all: the rc is still promoted.
        let p = plan(&packages, &[], BumpLevel::Patch);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].to.to_string(), "1.6.0");
        assert!(p[0].changesets.is_empty());
    }
}
