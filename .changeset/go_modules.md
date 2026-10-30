---
verwalter: minor
---

Go modules: `go.mod` carries the version in a module-line comment, `go.work` members are packages named by directory, `require` lines move with each release, tags default to `<path>/vX.Y.Z`, and `publish.registry = "goproxy"` publishes through the Go module proxy.
