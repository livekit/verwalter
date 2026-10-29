---
verwalter: minor
---
A changeset that names a package missing from `verwalter.toml` is an error, reported by every command and by `verwalter check`. The release branch deletes changesets outright instead of editing their front matter.
