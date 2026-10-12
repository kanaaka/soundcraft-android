# Attribution

## Upstream

**SoundCraft** — <https://github.com/storytold/soundcraft> — pinned here at
`acb08ff5f00d654a5303194ad04dad799c68a9fb` (version `0.5.0`).

Upstream is dual-licensed **Apache-2.0 OR MIT**. Both licence texts are reproduced in this
repository as `LICENSE-APACHE` and `LICENSE-MIT`, as the licences require. SoundCraft and its
engine, session model, mix engine and egui interface are the work of its authors; nothing in this
repository changes their copyright.

## This repository

The Android layer — the `apps/soundcraft-android/` shell crate, `patches/`, `docs/` and `scripts/` —
is an unofficial modification by [@kanaaka](https://github.com/kanaaka), written in October 2026,
offered under the same upstream terms (Apache-2.0 OR MIT).

It is **unofficial**: not produced, reviewed or endorsed by the upstream project, and upstream issues
should not be filed against it without saying so. The APK in Releases is signed with a throwaway
local test key — not with any official release key — and carries the upstream package name
`ai.storyteller.soundcraft` only so the port is recognisable as that application. It is not an official
SoundCraft release.
