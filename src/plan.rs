//! Spec: rule OpenOrUpdateReleasePullRequests, black boxes next_version,
//! stack_base and invariant StackCoversDependencies. Pure: the caller reads
//! the repository and GitHub, this module decides what the release PRs are.

use std::collections::{BTreeMap, BTreeSet};

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

/// One release PR to open or update. `stacked_on` names the package whose
/// release PR branch this PR targets; `None` targets the base branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRelease {
    pub package: String,
    pub from: Version,
    pub to: Version,
    pub level: BumpLevel,
    /// Changeset files that name this package and are not yet released for it.
    pub changesets: Vec<String>,
    /// Planned packages this one depends on, for the "updated dependencies"
    /// changelog line.
    pub bumped_dependencies: Vec<String>,
    pub stacked_on: Option<String>,
}

/// Dependencies first: by depth in the dependency graph, then by name.
/// Used for the stack and for the order releases are tagged, created and
/// handed to the publish workflow.
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

/// The release plan in stack order: a PR always comes after the PR it is
/// stacked on.
pub fn plan(
    packages: &[PackageState],
    changesets: &[Changeset],
    dependent_level: BumpLevel,
) -> Vec<PlannedRelease> {
    let by_name: BTreeMap<&str, &PackageState> =
        packages.iter().map(|p| (p.name.as_str(), p)).collect();

    // Direct bumps: the highest unreleased level named in a changeset.
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

    // Stack order: dependency depth among planned packages, then name.
    let planned: BTreeSet<&str> = level.keys().copied().collect();
    let planned_deps = |name: &str| -> Vec<&str> {
        by_name[name]
            .dependencies
            .iter()
            .map(String::as_str)
            .filter(|d| planned.contains(d))
            .collect()
    };
    let order = dependency_order(planned.iter().copied(), &planned_deps);
    let position: BTreeMap<&str, usize> = order.iter().enumerate().map(|(i, n)| (*n, i)).collect();

    // Tree: stack on the planned dependency that comes last in the order.
    let closure = |name: &str| -> BTreeSet<&str> {
        let mut seen = BTreeSet::new();
        let mut todo = planned_deps(name);
        while let Some(d) = todo.pop() {
            if seen.insert(d) {
                todo.extend(planned_deps(d));
            }
        }
        seen
    };
    let mut parent: BTreeMap<&str, Option<&str>> = order
        .iter()
        .map(|n| (*n, closure(n).into_iter().max_by_key(|d| position[d])))
        .collect();

    // A diamond (two unrelated planned dependencies) is not a tree. The
    // connected component holding it becomes one chain in stack order,
    // which always satisfies StackCoversDependencies; unrelated components
    // keep their trees.
    fn ancestors<'a>(name: &str, parent: &BTreeMap<&'a str, Option<&'a str>>) -> BTreeSet<&'a str> {
        let mut out = BTreeSet::new();
        let mut cur = parent[name];
        while let Some(p) = cur {
            out.insert(p);
            cur = parent[p];
        }
        out
    }
    let mut component: BTreeMap<&str, usize> = BTreeMap::new();
    for (id, start) in order.iter().enumerate() {
        if component.contains_key(start) {
            continue;
        }
        let mut todo = vec![*start];
        while let Some(n) = todo.pop() {
            if component.insert(n, id).is_none() {
                todo.extend(planned_deps(n));
                todo.extend(
                    order
                        .iter()
                        .copied()
                        .filter(|m| planned_deps(m).contains(&n)),
                );
            }
        }
    }
    let covered = |n: &&&str| closure(n).is_subset(&ancestors(n, &parent));
    let diamonds: BTreeSet<usize> = order
        .iter()
        .filter(|n| !covered(n))
        .map(|n| component[*n])
        .collect();
    for id in diamonds {
        let members: Vec<&str> = order
            .iter()
            .copied()
            .filter(|n| component[n] == id)
            .collect();
        for (i, n) in members.iter().enumerate() {
            parent.insert(n, i.checked_sub(1).map(|j| members[j]));
        }
    }

    order
        .iter()
        .map(|name| {
            let p = by_name[name];
            PlannedRelease {
                package: p.name.clone(),
                from: p.version.clone(),
                to: next_version(&p.version, [level[name]]),
                level: level[name],
                changesets: files.remove(name).unwrap_or_default(),
                bumped_dependencies: planned_deps(name).into_iter().map(String::from).collect(),
                stacked_on: parent[name].map(String::from),
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
        let front: String = bumps
            .iter()
            .map(|(p, l)| format!("{p}: {}\n", format!("{l:?}").to_lowercase()))
            .collect();
        Changeset::parse(file, &format!("---\n{front}---\n{file}\n")).unwrap()
    }

    fn python_sdks() -> Vec<PackageState> {
        vec![
            pkg("livekit-protocol", "1.1.5", &[]),
            pkg("livekit-api", "1.1.1", &["livekit-protocol"]),
            pkg("livekit", "1.2.0", &["livekit-protocol"]),
        ]
    }

    fn stack(plan: &[PlannedRelease]) -> Vec<(&str, Option<&str>)> {
        plan.iter()
            .map(|r| (r.package.as_str(), r.stacked_on.as_deref()))
            .collect()
    }

    #[test]
    fn no_changesets_no_plan() {
        assert!(plan(&python_sdks(), &[], BumpLevel::Patch).is_empty());
    }

    // Spec: dependent bumps are added by the tool; siblings form a tree.
    #[test]
    fn protocol_change_fans_out_to_a_tree() {
        let changesets = [cs("a.md", &[("livekit-protocol", BumpLevel::Minor)])];
        let p = plan(&python_sdks(), &changesets, BumpLevel::Patch);
        assert_eq!(
            stack(&p),
            vec![
                ("livekit-protocol", None),
                ("livekit", Some("livekit-protocol")),
                ("livekit-api", Some("livekit-protocol"))
            ]
        );
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
        assert_eq!(rtc.stacked_on.as_deref(), Some("livekit-protocol"));
    }

    #[test]
    fn leaf_only_change_is_a_single_base_pr() {
        let changesets = [cs("a.md", &[("livekit-api", BumpLevel::Patch)])];
        let p = plan(&python_sdks(), &changesets, BumpLevel::Patch);
        assert_eq!(stack(&p), vec![("livekit-api", None)]);
    }

    // Spec: a changeset released for one package still drives the other.
    // A package whose line a release branch removed is no longer planned.
    #[test]
    fn removed_lines_are_not_planned() {
        let c = cs(
            "a.md",
            &[
                ("livekit-protocol", BumpLevel::Patch),
                ("livekit", BumpLevel::Major),
            ],
        );
        let c = Changeset::parse("a.md", &c.without(&["livekit-protocol"]).unwrap()).unwrap();
        let packages = vec![
            pkg("livekit-protocol", "0.3.0", &[]),
            pkg("livekit", "0.7.0", &["livekit-protocol"]),
            pkg("livekit-ffi", "0.12.0", &["livekit"]),
        ];
        let p = plan(&packages, &[c], BumpLevel::Patch);
        let names: Vec<&str> = p.iter().map(|r| r.package.as_str()).collect();
        assert_eq!(names, ["livekit", "livekit-ffi"]);
    }

    // Spec: invariant StackCoversDependencies, diamond case. Only the
    // diamond's component is chained; an unrelated package stays a root.
    #[test]
    fn diamond_falls_back_to_a_chain() {
        let packages = vec![
            pkg("a", "1.0.0", &[]),
            pkg("b", "1.0.0", &[]),
            pkg("c", "1.0.0", &["a", "b"]),
            pkg("z", "1.0.0", &[]),
        ];
        let changesets = [cs(
            "x.md",
            &[
                ("a", BumpLevel::Patch),
                ("b", BumpLevel::Patch),
                ("z", BumpLevel::Patch),
            ],
        )];
        let p = plan(&packages, &changesets, BumpLevel::Patch);
        assert_eq!(
            stack(&p),
            vec![("a", None), ("b", Some("a")), ("z", None), ("c", Some("b"))]
        );
    }

    #[test]
    fn deep_chain_is_transitive() {
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
            stack(&p),
            vec![
                ("livekit-protocol", None),
                ("livekit-api", Some("livekit-protocol")),
                ("livekit", Some("livekit-api")),
                ("livekit-ffi", Some("livekit")),
            ]
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
