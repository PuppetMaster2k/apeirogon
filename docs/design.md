# Apeirogon — Design Decisions

> One keyboard and mouse. Every system. No borders.

Apeirogon is a cross-platform software KVM: one keyboard and mouse drive every
machine on your desk, regardless of OS. This document records the decisions
agreed during the initial design review.

## Naming

| Thing   | Name         |
| ------- | ------------ |
| Project | Apeirogon    |
| CLI     | `a7n`        |
| Daemon  | `apeirogond` |
| GUI     | `a7n-gui`    |

`apeirogon` and `a7n` are unclaimed on crates.io, npm, and PyPI (checked
2026-10-17; Homebrew not checked). **Action:** publish `0.0.0` placeholders on
crates.io (optionally npm/PyPI) before the repo goes public.

## Positioning

- **Wedge:** zero-config, secure-by-default pairing **plus** clipboard and file
  transfer done properly everywhere (images, files, drag-and-drop). Input
  sharing alone is a solved problem (Deskflow, Input Leap, lan-mouse).
- **Wayland is table stakes**, not the pitch.
- **No wire compatibility** with Synergy/Deskflow/Input Leap.
- **Open source only.** License **GPL-3.0-or-later**; reusable library crates
  (e.g. `a7n-proto`) may be MIT/Apache-2.0. Contributions under **DCO**
  sign-off, no CLA.
- **No telemetry, ever.** No update pings unless opted in.

## Platforms

v1 supports **Windows, macOS, Linux/X11, and Linux/Wayland**.

- Wayland: InputCapture portal + libei on GNOME/KDE; wlroots compositors
  best-effort via their own protocols.
- macOS: requires Accessibility and Input Monitoring permissions.

## Architecture

- **Language:** Rust.
- **Topology:** peer-to-peer. Any machine's keyboard/mouse can drive any other.
- **Focus:** single global focus. All physical input, from any machine, goes to
  the machine currently holding the cursor; focus moves by edge crossing.
- **Daemon:** `apeirogond` runs per user session (input and clipboard live
  there). The CLI and GUI talk to it over a local socket. No login-screen /
  UAC secure-desktop / pre-login support in v1.

### Workspace layout

```
crates/
  a7n-proto      wire protocol + message types (MIT/Apache, reusable)
  a7n-ipc        daemon <-> CLI/GUI local socket API
  a7n-core       focus, layout, clipboard logic (platform-free)
  a7n-platform   trait + backends: windows, macos, x11, wayland
  apeirogond     daemon binary
  a7n            CLI + TUI
  a7n-gui        egui app
```

## Networking & security

- **Scope:** LAN only, discovery via mDNS. Cross-network use via
  Tailscale/WireGuard, not built in.
- **Encryption:** mandatory. No plaintext mode.
- **Pairing:** `a7n pair` shows a 6-digit code on both sides; SPAKE2 PAKE
  exchanges long-term keys, which are pinned. Only pinned keys are accepted
  afterwards. No CAs, no certificate files.
- **Desk membership:** pairing with any member of a *desk* admits the new
  machine to the whole desk. The member list is signed and synced.
  `a7n unpair <machine>` revokes everywhere.
- **Transport:** one QUIC connection per peer pair, mutual TLS with pinned keys.
  - Pointer motion: unreliable QUIC datagrams.
  - Keys and buttons: reliable ordered stream (never lose a key-up).
  - Clipboard / files: separate streams, so bulk transfer never blocks input.
- **Versioning:** protocol version exchanged at handshake. Major mismatch means
  refuse with a clear "update X" message; minors negotiate optional features.

## Input

- **Keys:** send physical keys (USB HID usage codes); the receiver applies its
  own keyboard layout. Optional per-target modifier remap (e.g. Mac ⌘ → Ctrl),
  off by default.
- **Pointer:** relative deltas; the receiver applies its own speed and
  acceleration. Per-machine speed multiplier in config.
- **Layout:** 2D canvas of whole-machine rectangles (each machine's virtual
  desktop). Edges connect where rectangles touch, so offsets and partial
  overlaps work. Synced to all peers; concurrent edits resolve by
  last-writer-wins with a version counter and a notice to the loser.
- **Safety hotkeys** (all rebindable):
  - Return to the local machine: Scroll Lock / `Ctrl+Alt+Shift+Home`
  - Lock cursor to the current machine: `Ctrl+Alt+Shift+L`
  - Jump to machine N: `Ctrl+Alt+Shift+1…9`
  - Peer disconnect auto-releases all held keys and buttons on it.

## Clipboard

- **Model:** announce formats on copy, transfer on paste. Small text (< ~64 KB)
  is pushed eagerly to avoid paste latency.
- **Text milestone** includes plain text + HTML (no RTF).
- **Exclusion:** always honor OS sensitivity markers (cannot be disabled):
  - macOS `org.nspasteboard.ConcealedType`
  - Windows `ExcludeClipboardContentFromMonitorProcessing`
  - KDE `x-kde-passwordManagerHint`

  Plus a configurable app deny-list (where the source app is identifiable; on
  Wayland it usually is not, so markers only).

## User interface

- **CLI** (`a7n`): `init`, `pair`, `unpair`, `layout`, `switch`, `status`,
  `clip`, `send`, `config`, `daemon`, `logs`. Config is TOML, managed via
  `a7n config`. `a7n layout` is an interactive TUI.
- **GUI** (`a7n-gui`, egui/eframe) for Windows and macOS:
  - Thin client of the daemon over `a7n-ipc`; no logic of its own.
  - Tray / menu-bar icon (`tray-icon`): status, focused machine, pause,
    settings, quit. Daemon starts at login (LaunchAgent / Run key).
  - Guided first-run macOS permission flow, with deep links to System Settings
    and grant detection.
  - Builds on Linux best-effort and unsupported; CLI/TUI is the primary Linux
    interface.

## Logging & privacy

- Logs never contain key codes or clipboard contents, even at debug level;
  only event types and sizes.
- Opt-in `--trace-input` for key-code debugging, with a warning.

## Distribution

- **Signing:** Apple Developer ID + notarization (needed so macOS permissions
  survive updates); Windows via Azure Trusted Signing (or OV cert). Wired into
  CI from the first public release.
- **Channels:** Homebrew cask, winget, Scoop, `.deb`, `.rpm`, AUR,
  `cargo install`. No Flatpak or Snap (sandboxing fights input capture and the
  daemon).

## Testing

- GitHub Actions matrix: Windows, macOS, Linux.
- Unit tests for `a7n-core` and `a7n-proto`; protocol integration tests with
  simulated in-process peers.
- Platform backends: manual per-release test checklist.

## Roadmap

| Version | Scope |
| ------- | ----- |
| v0.1 | Pairing, input sharing, text clipboard (plain + HTML) on all 4 platforms; CLI + TUI |
| v0.2 | GUI + tray, signed installers, image clipboard |
| v0.3 | File clipboard, `a7n send` |
| Later | Drag-and-drop (per platform); login-screen support |

## Prior art

Barrier, Input Leap, Deskflow, lan-mouse, ShareMouse.
