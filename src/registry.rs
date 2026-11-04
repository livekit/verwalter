//! Spec: Registry and the exists checks. verwalter never publishes, but
//! it asks the registry whether a version is there: that is one GET per
//! registry, and it is what makes a publish re-run safe without grepping
//! the publish command's errors. The request goes through `curl`, like
//! git and gh go through their binaries.

use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::{Version, config::Config, handler::to_pep440};

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
    /// The Go module proxy, proxy.golang.org. Asking it for a version makes
    /// it fetch the tag, so the check is the publish: no command runs.
    /// Public modules only.
    Goproxy,
    /// `exists_url` with `{name}` and `{version}` placeholders: Maven, a
    /// private npm registry, anything with a URL that answers 200 or 404.
    Custom,
}

/// An HTTP status for a URL. Injected so tests need no network.
pub type Http<'a> = &'a (dyn Fn(&str) -> Result<u16> + Sync);

/// `curl` on the machine: 200, 404, or an error for anything else.
/// crates.io answers 403 to curl's default User-Agent, so the tool
/// names itself.
pub fn curl(url: &str) -> Result<u16> {
    let agent = format!(
        "verwalter/{} (https://github.com/livekit/verwalter)",
        env!("CARGO_PKG_VERSION")
    );
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
            "-A",
            &agent,
            url,
        ])
        .output()
        .context("curl (is curl installed?)")?;
    ensure!(
        out.status.success(),
        "curl {url}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
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
        Registry::Goproxy => format!("{}/@v/v{v}.info", goproxy_module_url(config, name)?),
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
        Registry::Goproxy => format!("{}/@v/list", goproxy_module_url(config, name)?),
    })
}

const GOPROXY: &str = "https://proxy.golang.org";

/// The proxy's URL for the package's module. The proxy protocol writes an
/// upper-case letter as `!` and the lower-case letter. None without a
/// go.mod, which the config load reports on a goproxy package.
fn goproxy_module_url(config: &Config, name: &str) -> Option<String> {
    let module = config.packages[name].go_module.as_deref()?;
    let escaped: String = module
        .chars()
        .flat_map(|c| {
            if c.is_ascii_uppercase() {
                vec!['!', c.to_ascii_lowercase()]
            } else {
                vec![c]
            }
        })
        .collect();
    Some(format!("{GOPROXY}/{escaped}"))
}

/// Some(true) on 200, Some(false) on 404, None without a registry, an
/// error for any other answer: a registry outage must not look like
/// "not there yet".
pub fn exists(url: Option<String>, http: Http) -> Result<Option<bool>> {
    let Some(url) = url else {
        return Ok(None);
    };
    match http(&url)? {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        // The Go proxy protocol answers "not found" with 404 or 410.
        410 if url.starts_with(GOPROXY) => Ok(Some(false)),
        code => bail!("{url}: HTTP {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn config(top: &str, extra: &str) -> Config {
        Config::parse(&format!(
            "{top}\n[packages.x]\npath = \"x\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\n{extra}\n"
        ))
        .unwrap()
    }

    #[test]
    fn urls_per_registry() {
        let v: Version = "1.6.0-rc.1".parse().unwrap();
        assert_eq!(version_url(&config("", ""), "x", &v), None);
        assert_eq!(
            version_url(&config("", "publish.registry = \"crates\""), "x", &v).unwrap(),
            "https://crates.io/api/v1/crates/x/1.6.0-rc.1"
        );
        // The default registry applies to a package without one.
        assert_eq!(
            version_url(&config("[publish]\nregistry = \"npm\"", ""), "x", &v).unwrap(),
            "https://registry.npmjs.org/x/1.6.0-rc.1"
        );
        assert_eq!(
            version_url(&config("", "publish.registry = \"pypi\""), "x", &v).unwrap(),
            "https://pypi.org/pypi/x/1.6.0rc1/json"
        );
        assert_eq!(
            version_url(
                &config(
                    "",
                    "publish.registry = \"custom\"\npublish.exists_url = \"https://r/{name}/{version}/\""
                ),
                "x",
                &v
            )
            .unwrap(),
            "https://r/x/1.6.0-rc.1/"
        );
        assert_eq!(
            package_url(&config("", "publish.registry = \"pubdev\""), "x").unwrap(),
            "https://pub.dev/api/packages/x"
        );
    }

    // Spec: Package.publish_command resolution.
    #[test]
    fn publish_command_resolution() {
        let c = config(
            "[publish]\nregistry = \"crates\"\ncommand = \"cargo publish --no-verify\"",
            "",
        );
        assert_eq!(
            c.publish_command("x", false, None).as_deref(),
            Some("cargo publish --no-verify")
        );
        // Another registry does not inherit the default command.
        let c = config(
            "[publish]\nregistry = \"crates\"\ncommand = \"cargo publish --no-verify\"",
            "publish.registry = \"npm\"",
        );
        assert_eq!(
            c.publish_command("x", false, None).as_deref(),
            Some("npm publish --provenance --access public")
        );
        // A pnpm workspace takes pnpm, which resolves `workspace:` ranges.
        let c = config("", "publish.registry = \"npm\"");
        assert_eq!(
            c.publish_command("x", true, None).as_deref(),
            Some("pnpm publish --provenance --access public --no-git-checks")
        );
        // An rc goes under the `next` dist-tag on npm.
        assert_eq!(
            c.publish_command("x", true, Some("next")).as_deref(),
            Some("pnpm publish --provenance --access public --no-git-checks --tag next")
        );
        assert_eq!(
            config("", "publish.registry = \"npm\"")
                .publish_command("x", false, Some("next"))
                .as_deref(),
            Some("npm publish --provenance --access public --tag next")
        );
        // A maintenance release goes under its line's dist-tag.
        assert_eq!(
            config("", "publish.registry = \"npm\"")
                .publish_command("x", false, Some("latest-1"))
                .as_deref(),
            Some("npm publish --provenance --access public --tag latest-1")
        );
        // The package's own wins.
        let c = config(
            "[publish]\nregistry = \"npm\"",
            "publish.command = \"pnpm publish --no-git-checks\"",
        );
        assert_eq!(
            c.publish_command("x", false, None).as_deref(),
            Some("pnpm publish --no-git-checks")
        );
        // Nothing applies.
        assert_eq!(config("", "").publish_command("x", false, None), None);
        assert_eq!(
            config("", "publish.registry = \"custom\"").publish_command("x", false, None),
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

    #[test]
    fn goproxy_urls_use_the_escaped_module_path() {
        let mut c = config("", "publish.registry = \"goproxy\"");
        let v: Version = "1.2.0-rc.1".parse().unwrap();
        assert_eq!(version_url(&c, "x", &v), None, "no go.mod, no URL");
        c.packages.get_mut("x").unwrap().go_module = Some("github.com/BurntSushi/toml/v2".into());
        assert_eq!(
            version_url(&c, "x", &v).unwrap(),
            "https://proxy.golang.org/github.com/!burnt!sushi/toml/v2/@v/v1.2.0-rc.1.info"
        );
        assert_eq!(
            package_url(&c, "x").unwrap(),
            "https://proxy.golang.org/github.com/!burnt!sushi/toml/v2/@v/list"
        );
        assert_eq!(c.publish_command("x", false, None), None);
    }
}
