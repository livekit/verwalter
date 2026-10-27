# Changesets

One Markdown file per change, with the packages it bumps in the front matter:

```md
---
verwalter: minor
---

Short description for the changelog.
```

`verwalter pr` turns pending files into a release PR; the file is deleted when it merges.
