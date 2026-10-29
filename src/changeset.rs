//! Spec: entity Changeset, value Bump. Exactly the changesets file format:
//! YAML front matter with one `package: level` line per bump (package names
//! quoted or unquoted), markdown body as the summary.

use crate::version::BumpLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bump {
    pub package_name: String,
    pub level: BumpLevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changeset {
    pub file: String,
    pub bumps: Vec<Bump>,
    pub summary: String,
    /// Changelog suffix naming the PR (or commit) and author that added
    /// the file, in the livekit changesets-changelog-github form:
    /// `- [#12](pr url) ([@user](profile url))`. Set by github::attribute.
    pub attribution: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    MissingFrontMatter,
    BadBumpLine(usize, String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFrontMatter => write!(f, "changeset has no `---` front matter"),
            Self::BadBumpLine(n, l) => write!(
                f,
                "line {n}: expected `package: major|minor|patch`, got `{l}`"
            ),
        }
    }
}

impl std::error::Error for ParseError {}

impl Changeset {
    pub fn parse(file: &str, content: &str) -> Result<Self, ParseError> {
        let rest = content
            .trim_start()
            .strip_prefix("---")
            .ok_or(ParseError::MissingFrontMatter)?;
        let (front, body) = rest
            .split_once("\n---")
            .ok_or(ParseError::MissingFrontMatter)?;

        let mut bumps = Vec::new();
        for (i, line) in front.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // `front` starts right after the opening `---`, so its first
            // line is the rest of file line 1.
            let bad = || ParseError::BadBumpLine(i + 1, line.to_string());
            let (name, level) = line.split_once(':').ok_or_else(bad)?;
            let name = name.trim().trim_matches(|c| c == '"' || c == '\'');
            let level = match level.trim() {
                "major" => BumpLevel::Major,
                "minor" => BumpLevel::Minor,
                "patch" => BumpLevel::Patch,
                _ => return Err(bad()),
            };
            bumps.push(Bump {
                package_name: name.to_string(),
                level,
            });
        }

        Ok(Self {
            file: file.to_string(),
            bumps,
            summary: body.trim().to_string(),
            attribution: None,
        })
    }

    /// Spec: Changeset.unreleased_for(name). A released package is no
    /// longer in the front matter, so this is a plain lookup.
    pub fn unreleased_for(&self, package: &str) -> Option<BumpLevel> {
        self.bumps
            .iter()
            .find(|b| b.package_name == package)
            .map(|b| b.level)
    }

    /// Bump lines naming a package that is not configured: a typo that
    /// would otherwise bump nothing and never be consumed.
    pub fn unknown_packages<'a>(&'a self, known: &[&str]) -> Vec<&'a str> {
        self.bumps
            .iter()
            .map(|b| b.package_name.as_str())
            .filter(|n| !known.contains(n))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUOTED: &str = r#"---
"@livekit/rtc-node": minor
'livekit-server-sdk': patch
---

Add RPC round-trip latency metric.
"#;

    const UNQUOTED: &str =
        "---\nlivekit-protocol: minor\nlivekit-api: patch\n---\nNegotiated data channels\n";

    // obligation: entity-fields.Changeset, value-equality.Bump
    #[test]
    fn parses_quoted_and_unquoted_package_names() {
        let q = Changeset::parse("a.md", QUOTED).unwrap();
        assert_eq!(
            q.bumps[0],
            Bump {
                package_name: "@livekit/rtc-node".into(),
                level: BumpLevel::Minor
            }
        );
        assert_eq!(q.bumps[1].package_name, "livekit-server-sdk");
        assert_eq!(q.summary, "Add RPC round-trip latency metric.");

        let u = Changeset::parse("b.md", UNQUOTED).unwrap();
        assert_eq!(u.bumps.len(), 2);
        assert_eq!(u.unreleased_for("livekit-api"), Some(BumpLevel::Patch));
        assert_eq!(u.unreleased_for("livekit"), None);
    }

    #[test]
    fn unknown_packages_are_listed() {
        let u = Changeset::parse("b.md", UNQUOTED).unwrap();
        assert!(u
            .unknown_packages(&["livekit-protocol", "livekit-api"])
            .is_empty());
        assert_eq!(u.unknown_packages(&["livekit-api"]), ["livekit-protocol"]);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            Changeset::parse("x.md", "no front matter"),
            Err(ParseError::MissingFrontMatter)
        );
        let err = Changeset::parse("x.md", "---\nlivekit: huge\n---\n").unwrap_err();
        assert!(matches!(err, ParseError::BadBumpLine(2, _)));
    }
}
