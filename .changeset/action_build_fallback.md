---
verwalter: patch
---

The action builds verwalter from its own checkout with `cargo` when it cannot download the release binary. `livekit/verwalter` is internal, so the `github.token` of another repository gets "release not found" for its releases. The build needs no token.
