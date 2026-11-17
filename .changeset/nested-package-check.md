---
verwalter: patch
---

A file in a nested package, such as `webrtc-sys/build` inside `webrtc-sys`, counts as a change of that package only. Before, the changeset check also required a changeset for the package around it.
