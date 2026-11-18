//! Spec: surface WorkflowReleaseGate. Whether a workflow run is a release
//! run, decided from the GitHub event. The release workflow asks this in
//! its first job, so the condition is in one place and not in every
//! repository.

use anyhow::Result;
use serde_json::Value;

use crate::{branches, config::Config, github::MARKER, prerelease};

#[derive(Debug, PartialEq, Eq)]
pub struct Decision {
    pub release: bool,
    pub reason: String,
}

/// `event` is the payload of the event `event_name`. `config` is called
/// only for a merged PR with the marker: a closed PR from a fork can
/// carry any `verwalter.toml`, and gets its "no" before the file is read.
pub fn decide(
    event_name: &str,
    event: &Value,
    config: impl FnOnce() -> Result<Config>,
) -> Result<Decision> {
    let answer = |release, reason: String| Ok(Decision { release, reason });
    if event_name == "workflow_dispatch" {
        return answer(true, "a run by hand".into());
    }
    if !matches!(event_name, "pull_request" | "pull_request_target") {
        return answer(false, format!("the event is {event_name}"));
    }
    let pr = &event["pull_request"];
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    if pr["merged"] != true {
        return answer(false, "the pull request is not merged".into());
    }
    if !text(&pr["body"]).contains(MARKER) {
        return answer(false, format!("the pull request body has no {MARKER}"));
    }
    let config = config()?;
    let (head, base) = (text(&pr["head"]["ref"]), text(&pr["base"]["ref"]));
    let release = branches::branch_name(&config);
    let rc = prerelease::branch_name(&config, &base);
    if head != release && head != rc {
        return answer(
            false,
            format!("the branch is {head}, not {release} or {rc}"),
        );
    }
    let author = text(&pr["user"]["login"]);
    if author != config.release.author {
        return answer(
            false,
            format!("the author is {author}, not {}", config.release.author),
        );
    }
    answer(true, format!("the merged pull request from {head}"))
}

/// Appends to `$GITHUB_OUTPUT` when set.
pub fn write_output(decision: &Decision) -> Result<()> {
    crate::github::append_output(&format!("release={}\n", decision.release))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BOT: &str = "verwalter-release[bot]";
    const BODY: &str = "<!-- verwalter release -->\nMerging this PR releases:\n";

    fn pr(merged: bool, head: &str, base: &str, author: &str, body: &str) -> Value {
        json!({ "pull_request": {
            "merged": merged,
            "head": { "ref": head },
            "base": { "ref": base },
            "user": { "login": author },
            "body": body,
        }})
    }

    const PACKAGE: &str = "[packages.a]\npath = \".\"\nversioning.files = [\"Cargo.toml\"]\n";

    fn decide_with(event_name: &str, event: &Value, release: &str) -> Decision {
        let config = format!("{PACKAGE}{release}");
        decide(event_name, event, || Config::parse(&config)).unwrap()
    }

    #[test]
    fn release_runs() {
        let custom = "[release]\nbranch_prefix = \"rel-\"\nauthor = \"github-actions[bot]\"\n";
        let actions = "github-actions[bot]";
        #[rustfmt::skip]
        let cases = [
            // (event name, event, [release] config, release run?)
            ("workflow_dispatch", json!({}), "", true),
            ("pull_request", pr(true, "release/main", "main", BOT, BODY), "", true),
            ("pull_request_target", pr(true, "release/main", "main", BOT, BODY), "", true),
            // An rc PR is named after the branch it targets.
            ("pull_request", pr(true, "release/v1-rc", "v1", BOT, BODY), "", true),
            ("pull_request", pr(true, "release/v1-rc", "main", BOT, BODY), "", false),
            ("pull_request", pr(false, "release/main", "main", BOT, BODY), "", false),
            ("pull_request", pr(true, "release/other", "main", BOT, BODY), "", false),
            ("pull_request", pr(true, "release/main", "main", "mallory", BODY), "", false),
            ("pull_request", pr(true, "release/main", "main", BOT, "no marker"), "", false),
            ("pull_request", json!({ "pull_request": { "merged": true, "body": null } }), "", false),
            ("push", json!({}), "", false),
            ("pull_request", pr(true, "rel-main", "main", actions, BODY), custom, true),
            ("pull_request", pr(true, "rel-main-rc", "main", actions, BODY), custom, true),
            ("pull_request", pr(true, "release/main", "main", BOT, BODY), custom, false),
        ];
        for (event_name, event, config, expected) in cases {
            let d = decide_with(event_name, &event, config);
            assert_eq!(d.release, expected, "{event_name} {event}: {}", d.reason);
        }
    }

    // A closed PR from a fork can carry a broken or hostile verwalter.toml.
    #[test]
    fn config_is_read_only_for_a_merged_pull_request_with_the_marker() {
        let events = [
            ("workflow_dispatch", json!({})),
            ("push", json!({})),
            ("pull_request", pr(false, "release/main", "main", BOT, BODY)),
            ("pull_request", pr(true, "release/main", "main", BOT, "")),
        ];
        for (event_name, event) in events {
            decide(event_name, &event, || panic!("the config was read")).unwrap();
        }
        // The merged PR with the marker needs the config: a broken one is
        // an error, not a "no".
        let merged = pr(true, "release/main", "main", BOT, BODY);
        assert!(decide("pull_request", &merged, || Config::parse("release = 1")).is_err());
    }

    #[test]
    fn the_answer_says_why() {
        let event = pr(true, "release/main", "main", "mallory", BODY);
        assert_eq!(
            decide_with("pull_request", &event, "").reason,
            "the author is mallory, not verwalter-release[bot]"
        );
    }
}
