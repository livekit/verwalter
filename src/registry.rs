//! Spec: Registry and the exists checks. verwalter never publishes, but
//! it asks the registry whether a version is there: that is one GET per
//! registry, and it is what makes a publish re-run safe without grepping
//! the publish command's errors. The request goes through `curl`, like
//! git and gh go through their binaries.

use std::process::Command;

use serde::Deserialize;

use crate::{config::Config, handler::to_pep440, Error, Version};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Registry {
    /// No check. The publish command handles it, or the package is private.
    #[default]
    None,
    Crates,
    Npm,
    Pypi,
    Pubdev,
    /// `exists_url` with `{name}` and `{version}` placeholders: Maven, a
    /// private npm registry, anything with a URL that answers 200 or 404.
    Custom,
}

/// An HTTP status for a URL. Injected so tests need no network.
pub type Http<'a> = &'a (dyn Fn(&str) -> Result<u16, Error> + Sync);

/// `curl` on the machine: 200, 404, or an error for anything else.
pub fn curl(url: &str) -> Result<u16, Error> {
    let out = Command::new("curl")
        .args([
            "-sS",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-L",
            "--max-time",
            "30",
            url,
        ])
        .output()
        .map_err(|e| format!("curl: {e} (is curl installed?)"))?;
    if !out.status.success() {
        return Err(format!(
            "curl {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().parse()?)
}

/// The URL that answers 200 when `version` of `name` is on the registry.
/// None when the package has no registry configured.
pub fn version_url(config: &Config, name: &str, version: &Version) -> Option<String> {
    let v = version.to_string();
    Some(match config.registry_of(name) {
        Registry::None => return None,
        Registry::Crates => format!("https://crates.io/api/v1/crates/{name}/{v}"),
        Registry::Npm => format!("https://registry.npmjs.org/{name}/{v}"),
        Registry::Pypi => format!("https://pypi.org/pypi/{name}/{}/json", to_pep440(version)),
        Registry::Pubdev => format!("https://pub.dev/api/packages/{name}/versions/{v}"),
        Registry::Custom => config
            .exists_url_of(name)?
            .replace("{name}", name)
            .replace("{version}", &v),
    })
}

/// The URL that answers 200 when any version of `name` is on the registry:
/// Spec Package.first_release.
pub fn package_url(config: &Config, name: &str) -> Option<String> {
    Some(match config.registry_of(name) {
        Registry::None | Registry::Custom => return None,
        Registry::Crates => format!("https://crates.io/api/v1/crates/{name}"),
        Registry::Npm => format!("https://registry.npmjs.org/{name}"),
        Registry::Pypi => format!("https://pypi.org/pypi/{name}/json"),
        Registry::Pubdev => format!("https://pub.dev/api/packages/{name}"),
    })
}

/// Some(true) on 200, Some(false) on 404, None without a registry, an
/// error for any other answer: a registry outage must not look like
/// "not there yet".
pub fn exists(url: Option<String>, http: Http) -> Result<Option<bool>, Error> {
    let Some(url) = url else {
        return Ok(None);
    };
    match http(&url)? {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        code => Err(format!("{url}: HTTP {code}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn config(top: &str, extra: &str) -> Config {
        Config::parse(&format!(
            "{top}\n[packages.x]\npath = \"x\"\nversioned_files = [\"x/Cargo.toml\"]\nchangelog = \"x/CHANGELOG.md\"\n{extra}\n"
        ))
        .unwrap()
    }

    #[test]
    fn urls_per_registry() {
        let v: Version = "1.6.0-rc.1".parse().unwrap();
        assert_eq!(version_url(&config("", ""), "x", &v), None);
        assert_eq!(
            version_url(&config("", "registry = \"crates\""), "x", &v).unwrap(),
            "https://crates.io/api/v1/crates/x/1.6.0-rc.1"
        );
        // The default registry applies to a package without one.
        assert_eq!(
            version_url(&config("[publish]\nregistry = \"npm\"", ""), "x", &v).unwrap(),
            "https://registry.npmjs.org/x/1.6.0-rc.1"
        );
        assert_eq!(
            version_url(&config("", "registry = \"pypi\""), "x", &v).unwrap(),
            "https://pypi.org/pypi/x/1.6.0rc1/json"
        );
        assert_eq!(
            version_url(
                &config(
                    "",
                    "registry = \"custom\"\nexists_url = \"https://r/{name}/{version}/\""
                ),
                "x",
                &v
            )
            .unwrap(),
            "https://r/x/1.6.0-rc.1/"
        );
        assert_eq!(
            package_url(&config("", "registry = \"pubdev\""), "x").unwrap(),
            "https://pub.dev/api/packages/x"
        );
    }

    // Spec: Package.publish_command resolution.
    #[test]
    fn publish_command_resolution() {
        let c = config(
            "[publish]\nregistry = \"crates\"\npublish = \"cargo publish --no-verify\"",
            "",
        );
        assert_eq!(
            c.publish_command("x").as_deref(),
            Some("cargo publish --no-verify")
        );
        // Another registry does not inherit the default command.
        let c = config(
            "[publish]\nregistry = \"crates\"\npublish = \"cargo publish --no-verify\"",
            "registry = \"npm\"",
        );
        assert_eq!(
            c.publish_command("x").as_deref(),
            Some("npm publish --provenance --access public")
        );
        // The package's own wins.
        let c = config(
            "[publish]\nregistry = \"npm\"",
            "publish = \"pnpm publish --no-git-checks\"",
        );
        assert_eq!(
            c.publish_command("x").as_deref(),
            Some("pnpm publish --no-git-checks")
        );
        // Nothing applies.
        assert_eq!(config("", "").publish_command("x"), None);
        assert_eq!(
            config("", "registry = \"custom\"").publish_command("x"),
            None
        );
    }

    #[test]
    fn exists_maps_200_404_and_errors() {
        let ok = |_: &str| Ok(200);
        let missing = |_: &str| Ok(404);
        let down = |_: &str| Ok(503);
        assert_eq!(exists(Some("u".into()), &ok).unwrap(), Some(true));
        assert_eq!(exists(Some("u".into()), &missing).unwrap(), Some(false));
        assert!(exists(Some("u".into()), &down).is_err());
        assert_eq!(exists(None, &down).unwrap(), None);
    }
}
