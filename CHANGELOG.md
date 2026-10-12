# Changelog

## v0.1.0 — first published port (SoundCraft 0.5.0)

| | |
|---|---|
| APK | `soundcraft-0.5.0-arm64-v8a.apk` (10.2 MB) |
| sha256 | `fccb35698ea1d25f3cd2c6705f7f088f4cc70597869aab7e68fb6e78eb0ba39e` |
| Built from | upstream `storytold/soundcraft` @ `acb08ff5f00d654a5303194ad04dad799c68a9fb` + `patches/0001-soundcraft-android-port.patch` + `apps/soundcraft-android/` |
| Package | `ai.storyteller.soundcraft` — versionName `0.5.0`, minSdk 26, targetSdk 35 |
| ABI | `arm64-v8a` only |
| Signature | throwaway local test key, not an official key |

### In this release

- Android shell crate: NativeActivity entry point, `Services` implementation, file logger,
  preferences written while the app runs (atomic), modifier-key synthesis, Activity not recreated on
  configuration change, launcher icon, edge-to-edge theme variant.
- **The file dialogs work.** SoundCraft asks for a path with a synchronous picker, which no
  draw-your-own-dialog host can answer in one call, so the shell splits it in two: `Browse…` records
  what was asked, the shell shows its own picker above the app's dialog, and the chosen path is
  written into the dialog's path box, where the app's own `OK` picks it up. The app's dialog, its
  extension defaulting and its *"already exists — replace?"* warning are untouched.
- **Audio plays.** `cpal` reaches Android through its AAudio host, so the real player opens the
  platform's output device (the emulator log reports `audio device: default @ 48000 Hz, 2 ch`).
- **Ctrl / Shift / Alt work** — the platform reports no modifiers, so they are tracked from key events
  and re-injected; covered by five unit tests, including the cross-frame case.
- **Quit ends the process,** rather than leaving a stale close request that looks like a hang and
  cannot be reopened until the app is cleared from Recents.
- The platform UI font is loaded from `/system/fonts`, instead of falling back to egui's bundled
  defaults — the one upstream change in this port.

### Known issues

See *Known gaps* in the [README](README.md) and [`docs/PORT-NOTES.md`](docs/PORT-NOTES.md).
