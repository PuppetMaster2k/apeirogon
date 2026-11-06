# Apeirogon — Design Decisions

> One keyboard and mouse. Every system. No borders.

Apeirogon is a cross-platform software KVM: one keyboard and mouse drive every
machine on your desk, regardless of OS. This document records the decisions
agreed during the initial design review. The wire protocol is specified in
[`PROTOCOL.md`](PROTOCOL.md) with the schema in
[`proto/apeirogon/v1/apeirogon.proto`](../proto/apeirogon/v1/apeirogon.proto);
where they disagree with this document, this document wins.

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

- **Wedge:** input sharing alone is a solved problem (Deskflow, Input Leap,
  lan-mouse), and TwinDesk already pairs two machines with clipboard and files.
  Apeirogon differs on:
  - **A desk, not a pair:** any number of machines in a full mesh, admitted
    once through desk membership.
  - **Native and lean:** one Rust binary per role (daemon, CLI, GUI) instead
    of an Electron app, scriptable through `a7n`.
  - **Secure by default:** mutual TLS with pinned keys, SPAKE2 pairing,
    sensitive clipboard content that can never be synced.
  - **Wayland without root:** portals and libei only.
- Clipboard and file transfer done properly everywhere (images and files) are
  expected, not the pitch. So is Wayland support. Files move via the clipboard
  or `a7n send`.
- **No wire compatibility** with Synergy/Deskflow/Input Leap.
- **Open source only.** License **GPL-3.0-or-later**; reusable library crates
  (e.g. `a7n-proto`) may be MIT/Apache-2.0. Contributions under **DCO**
  sign-off, no CLA.
- **No telemetry, ever.** No update pings unless opted in.

## Platforms

v1 supports **Windows, macOS, Linux/X11, and Linux/Wayland**. Mobile is out of
scope.

- Wayland: InputCapture portal + libei on GNOME/KDE; wlroots compositors
  best-effort via their own protocols. **No evdev/uinput fallback:** read
  access to `/dev/input` lets a process log every keystroke system-wide, and
  granting it needs root.
- Wayland capture only starts when the pointer hits a barrier at the edge of
  the screens. While a peer drives a Wayland machine, its local keyboard and
  mouse are therefore not blocked and the daemon does not see that local
  activity: idle takeover does not apply there, and local and remote input
  can mix. Accepted for v1; the portal offers no way to watch local input
  without capturing it.
- Wayland clipboard: `ext-data-control-v1`, falling back to
  `wlr-data-control-unstable-v1` (wlroots, KDE). Wayland is lazy by design:
  the compositor asks the offering client to write into a pipe on paste, so
  that request is our "paste happened" signal and needs no key snooping. We
  implement watching, reading and offering ourselves on one `wayland-client`
  connection (`wayland-protocols`, `wayland-protocols-wlr`), because
  `wl-clipboard-rs` and `arboard` only accept data up front. Our own
  selections carry the private format `application/x-apeirogon-clipboard`,
  so they are not announced back.
- GNOME (Mutter) has no data-control: use the xdg-desktop-portal Clipboard
  portal, which also delivers on request (`SelectionTransfer` asks for a
  MIME type, we answer with `SelectionWrite` on a fresh fd). Decided by the
  spike (#25) from the portal specification:
  - The Clipboard portal only extends sessions of other portals:
    RemoteDesktop, or InputCapture since version 2. On an InputCapture
    session the clipboard works only while capture is active, which is too
    late to announce local copies. **Clipboard rides on the RemoteDesktop
    session**, which we need anyway to inject input through libei
    (`ConnectToEIS`).
  - So a portal desktop runs two sessions: **InputCapture** (capture, pointer
    barriers at layout edges) and **RemoteDesktop** (injection + clipboard).
    `Clipboard.RequestClipboard` is called before the RemoteDesktop session
    starts; `clipboard_enabled` in the start response says whether it worked.
  - Prompts: each session shows one consent dialog on first use. Both use
    `persist_mode = 2` and store the single-use `restore_token` in the
    daemon's state directory, replacing it with the new token after every
    start, so later starts need no dialog. Portals without persistence
    (InputCapture < 2, RemoteDesktop < 2) prompt on every daemon start.
  - The input backend owns the RemoteDesktop session, so it requests
    clipboard access and hands the clipboard channels to
    `clipboard::start()`, which therefore runs after `input::start()`. Echo
    of our own selection is detected through `session_is_owner`.
  - Outside Flatpak the daemon registers the host app id
    `io.github.PuppetMaster2k.Apeirogon` before the first portal call and
    ships a matching desktop file (`packaging/linux/`), so dialogs show
    "Apeirogon" and remembered consent belongs to it.
  - Not yet verified on a live GNOME or KDE session; which compositor
    versions implement InputCapture 2 and the Clipboard portal is part of
    the manual checklist.
- macOS: requires Accessibility and Input Monitoring permissions.

## Architecture

- **Language:** Rust.
- **Topology:** peer-to-peer, full mesh: every pair of machines on a desk keeps
  one link. Any machine's keyboard/mouse can drive any other. If two machines
  dial each other at once, the link started by the larger machine id is kept.
- **Focus:** single global focus. All physical input, from any machine, goes to
  the machine currently holding the cursor; focus moves by edge crossing.
  - Ownership moves with `Focus` / `Accept` / `Refuse` / `Handover`, ordered by
    a rising route number. Chains go direct (A drives B, B hands over to C, A
    takes C itself; no relaying through B).
  - **Take timeout:** if the target answers a take with neither accept nor
    refuse within 1 s, the owner takes focus home.
  - **Idle takeover:** if the owner has sent nothing for 3 s, typing on
    another machine takes focus.
  - **Release on end:** whenever ownership ends (link loss, takeover, let go),
    the driven machine releases every held key and button.
  - **Lock screen:** a driven machine on its lock screen reports it and drops
    input; the owner takes focus home.
- **Daemon:** `apeirogond` runs per user session (input and clipboard live
  there). The CLI and GUI talk to it over a local socket. No login-screen /
  UAC secure-desktop / pre-login support in v1.

### Workspace layout

```
proto/apeirogon/v1/apeirogon.proto   language-neutral wire schema
crates/
  a7n-proto      generated protocol types + framing (MIT/Apache, reusable)
  a7n-ipc        daemon <-> CLI/GUI local socket API
  a7n-core       focus, layout, clipboard logic (platform-free)
  a7n-platform   trait + backends: windows, macos, x11, wayland
  apeirogond     daemon binary
  a7n            CLI + TUI
  a7n-gui        egui app
```

## Networking & security

- **Scope:** LAN only. Discovery via mDNS / DNS-SD service `_apeirogon._udp`
  (TXT: machine id, protocol major version); addressing by IP also works.
  Cross-network use via Tailscale/WireGuard, not built in. Default UDP port
  42042 (configurable).
- **Encryption:** mandatory. No plaintext mode.
- **Identity:** each device generates an Ed25519 key (ECDSA P-256 fallback) and
  a long-lived self-signed certificate. Machine id = first 16 bytes of
  SHA-256(SubjectPublicKeyInfo); pin = SHA-256 of the certificate. Mutual TLS
  accepts only pinned certificates; no CAs, no certificate files, no 0-RTT.
- **Pairing:** `a7n pair` shows a 6-digit code (valid 60 s, one guess; a wrong
  code ends pairing mode). SPAKE2 (RFC 9382) over a separate QUIC connection,
  with both pins as identities and the TLS exporter bound in, so a man in the
  middle cannot complete it. Optional QR code adds a 128-bit secret. Rate
  limit 5 attempts per minute per source; failures never say which check
  failed.
- **Desk membership:** pairing with any member of a *desk* admits the new
  machine to the whole desk. The admitting member signs a `DeskMember` record
  with its key; any member forwards it and others accept it if the signer is
  trusted. `a7n unpair <machine>` issues a signed `DeskRevocation` that removes
  the pin everywhere; re-admitting requires pairing again. Members that were
  offline catch up at their next handshake.
- **Transport:** one QUIC connection per peer pair (no TCP fallback).
  - Pointer motion: unreliable QUIC datagrams (RFC 9221).
  - Keys, buttons, scroll and control messages: reliable ordered stream 0
    (never lose a key-up).
  - Clipboard / file data: one unidirectional stream per transfer, so bulk
    data never blocks input.
- **Wire format:** protobuf messages with varint length prefixes, chosen for
  no parsing or allocation per mouse event (a motion update is ~13 bytes).
- **Versioning:** the protocol major version is exchanged at handshake; a
  mismatch is refused with a clear "update X" message. Minor features are
  capability flags, used only if both sides list them.
- **Liveness:** ping every 1 s; a link is dropped after 2–2.5 s of silence or
  1.5 s of unacknowledged input. The ping also measures round-trip time.
- **Per-peer policy:** `allow_drive` toggle (may this peer drive me?) and
  separate clipboard and file policies (`both`, `send`, `receive`, `ask`, `off`).
  File policy is local `peers.<id>.files`, defaults to `ask`, and never syncs.
- **Settings sync:** convenience settings (hotkeys, speed multipliers) can sync
  across the desk through an allow-list. Security settings (`allow_drive`,
  policies, deny-list, sensitive-content check) are never synced.

## Input

- **Keys:** send physical keys (USB HID usage codes); the receiver applies its
  own keyboard layout. Cmd↔Ctrl swap by default between macOS and other systems,
  per peer off or custom, applied by the sender. Per-peer `modifier_mode` is
  `auto` (default), `off`, or `custom`; only `custom` uses `modifier_remap`.
  Option is already the HID Alt key and needs no remap.
- **Media keys:** sent as HID consumer-page usages.
- **Pointer:** relative deltas, sent as loss-tolerant cumulative counters so a
  dropped datagram is repaired by the next; every reliable input batch
  resyncs the pointer so clicks land where the user saw it. The receiver
  applies its own speed and acceleration, plus a per-machine speed multiplier
  from config.
- **Scroll:** pixel (trackpad) or line (wheel) units; the sender applies its
  natural-scrolling preference first.
- **Gestures:** swipes, pinch and spread forwarded where both sides support
  them.
- **Layout:** 2D canvas of whole-machine rectangles (each machine's virtual
  desktop). Known physical sizes use 0.1 mm canvas units; “Use real sizes”
  resizes and lines up the draft before Save. Edges connect where rectangles
  touch, so offsets and partial overlaps work. One desk-wide layout is synced to all members; concurrent
  edits resolve by last-writer-wins with a version counter (ties by machine
  id) and a notice to the loser. Each leaving edge can disable individual thirds;
  diagonal pushes within an 8-pixel corner box reach corner-only neighbours after edge exits.
- **Edge resistance:** while any captured local button (or the remote owner's
  injected button on a driven machine) is held, edge pushes cannot cross and
  discard accumulated resistance; after release a full push is required again.
- **Safety hotkeys** (all rebindable):
  - Return to the local machine: double-tap the trigger key (Right Alt / Right
    Option by default, within 400 ms), or Scroll Lock / `Ctrl+Alt+Shift+Home`.
    `hotkeys.double_tap` selects the trigger: modifier sides (`left_*`,
    `right_*`, `either_*`), F1–F24, Scroll Lock, Pause, Menu, or `none`.
    `hotkeys.trigger_mode` selects `double_tap` (default), `hold`, or `both`.
    `hotkeys.hold_ms` defaults to 800 ms (300–3000 ms); releasing the trigger
    or pressing another key cancels a hold. Repeats do not restart it.
    Non-modifier trigger keys are consumed; modifier triggers keep forwarding.
    The GUI can record a trigger key from local capture before saving it.
  - Shake the mouse left and right while driving a peer to return home: by
    default 4 horizontal reversals within 600 ms, each after at least 40 px
    of travel. `hotkeys.shake` accepts 3–10 reversals, or 0 to turn it off.
    Shaking at home or while driven does nothing; focus changes reset it.
  - Lock cursor to the current machine: `Ctrl+Alt+Shift+L`
  - Jump to machine N: `Ctrl+Alt+Shift+1…9`
  - Peer disconnect auto-releases all held keys and buttons on it.
- **Capture dead-man switch:** the platform capture releases the local
  keyboard and mouse by itself when the daemon's keepalive (every 500 ms) is
  missing for 1.5 s, so a hung daemon can never lock the user out.

## Wake-on-LAN

Each paired link advertises the MAC of the interface carrying it, when known.
The receiver validates it and remembers the last valid address beside the pin,
without logging it. Switching to an offline paired machine by edge push, jump
key or `a7n switch` sends a magic packet and shows “Waking <name>…”. Edge
resistance still applies and the cursor stays where it is while waiting. If
the peer connects within 60 s, the daemon retries the take once, preserving
an edge landing. Another user switch, pause, revocation or timeout cancels it.

Local `peers.<id>.wake` defaults to true and never syncs; false disables only
automatic wake. `a7n wake <machine>` and the Machines page's Wake button send
without moving focus. A MAC must have been learned on a previous awake link.
Packets go to IPv4 limited and directed broadcasts on UDP port 9, at most once
per peer every 5 s. Wake requires BIOS/OS support enabled and works only within
the same broadcast domain. Wired PCs usually answer; MacBooks on Wi-Fi usually
do not.

## Clipboard

- **Model:** announce formats on copy, transfer on paste. Small items (up to
  64 KiB per copy) are pushed eagerly to avoid paste latency. Where an OS
  cannot delay a paste, the receiver fetches eagerly below a configurable
  size.
- **Ordering:** each copy gets a Lamport generation; the highest wins, ties by
  machine id. A stale paste request is answered "not found".
- **Text milestone** includes plain text + HTML (no RTF).
- **Limits** (defaults): text 256 KiB, image 8 MiB with validation before
  decode (decompression-bomb checks). Files limited only by policy.
- **Exclusion:** always honor OS sensitivity markers (cannot be disabled):
  - macOS `org.nspasteboard.ConcealedType`
  - Windows `ExcludeClipboardContentFromMonitorProcessing`
  - KDE `x-kde-passwordManagerHint`

  Plus a configurable app deny-list (where the source app is identifiable; on
  Wayland it usually is not, so markers only). Marked content is never even
  announced.

## Files

- **Transfer:** a manifest lists every file with size and SHA-256; data is
  sent in chunks of up to 256 KiB. The receiver verifies each hash and deletes
  a file that fails. Interrupted transfers resume from an offset after the
  receiver re-hashes the partial file. At most 4 concurrent transfers per peer.
- **Safety:** received files go only into a dedicated directory unless the user
  picks a target; never overwrite; never follow symlinks; reject `..`,
  absolute paths and reserved names.
- **Receive directory:** local `files.receive_dir` is an absolute path, defaulting
  to the OS downloads directory plus `Apeirogon` (or `~/Downloads/Apeirogon`
  if unavailable). It never syncs and is created on the first receive.
- **`a7n send`** uses the same path as a file clipboard.
- **File notifications:** receivers show a system notification at the start and
  completion or failure of transfers of at least 100 MiB, and for held `ask`
  offers of any size. They contain only peer name, file count, size and a
  failure reason, never file names or paths. Local `notifications.files`
  defaults to true and never syncs, since desktop permissions and notification
  preferences are machine-specific. Set it with `a7n config set notifications.files false`.
  Linux notifications open the GUI on click for up to 60 seconds where the
  desktop supports actions; macOS and Windows have no click action.
- **Files on Linux paste:** apps expect `text/uri-list` with local paths, so
  the paste triggers the download into the receive directory and the paths
  are handed out afterwards; above a size threshold a progress indicator is
  shown. Copied files travel with the clipboard when both peers advertise
  `CAPABILITY_CLIPBOARD_FILES`; the source builds the manifest (it hashes every
  file) before announcing, and only while a peer that could paste is linked.
  A paste is an explicit local action, so the `ask` files policy does not hold
  it: `receive`, `both` and `ask` allow it, `send` and `off` hide the files
  from the clipboard. Sending a copy needs `send`, `both` or `ask`.

## User interface

- **CLI** (`a7n`): `init`, `pair`, `unpair`, `layout`, `switch`, `status`,
  `wake`, `clip`, `send`, `files`, `config`, `daemon`, `logs`, `pause`, `resume`. Config is TOML, managed via
  `a7n config`. `a7n layout` is an interactive TUI.
- **GUI** (`a7n-gui`, egui/eframe) for Windows and macOS:
  - Thin client of the daemon over `a7n-ipc`; no logic of its own.
  - Pause returns outgoing focus home and blocks edge, hotkey and explicit
    switches to peers until resumed. Receiving input remains governed by
    `allow_drive`. Pause is session state and resets at daemon startup.
  - Local IPC version 2 adds `SetPaused { paused }` and status fields `paused`
    and `focused` (machine id). Update the daemon and local clients together;
    the peer wire protocol is unchanged.
  - Local IPC version 4 adds explicit file offers, accept/reject and cancellation,
    plus held offer counts, active file/byte progress (sends carry their
    offer id) and the outcome of recently ended sends (`finished_sends`, session
    state, at most 16; link loss is not an outcome because the offer resumes). File offers use an
    independent generation namespace and do not change the receiving clipboard.
  - Local IPC version 6 adds `Wake`, a `Waking` switch response, and per-peer
    `can_wake` and `waking` status flags. Hardware addresses stay inside the daemon.
  - Local IPC version 10 adds `RecordTriggerKey` and `TriggerKey { name }`.
    Recording waits up to 10 seconds for the next captured local key-down,
    consumes that key through release, and returns its trigger name or an
    error. At home this is available on Windows, macOS and X11; inactive
    Wayland capture cannot observe local keys and recording times out.
    Update the daemon and local clients together; the peer protocol is unchanged.
  - Local IPC version 9 carries `blocked` edge thirds in placed machines and
    set-layout requests. Update the daemon and local clients together.
  - Local IPC version 8 adds optional `screen_mm` dimensions to placed and unplaced
    machines. Update the daemon and local clients together.
  - Local IPC version 7 exposes `offer_id` on held files and `generation` on held
    clipboard copies. Accept requests carry these identities (peer and offer id
    for files, origin machine and generation for clipboard) and are refused if
    the held item changed. Update the daemon and local clients together.
  - Local IPC version 5 adds a distinct `no_space` error when accepting files.
    Before downloading, the receiver checks the remaining bytes after rehashing
    partial data plus a 256 MiB safety margin. A failed free-space query is
    logged and receiving proceeds.
  - Local IPC version 3 adds status field `missing_permissions` and
    `RequestPermission { permission }`. The daemon owns the macOS
    Accessibility and Input Monitoring grants, so it reports what is missing,
    opens the matching System Settings pane on request, and starts input by
    itself once both are granted.
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

- **Signing:** none. Releases are unsigned because Apple Developer ID and
  Windows code signing cost more than this project wants to spend. On macOS,
  Accessibility and Input Monitoring are tied to the code signature, so users
  grant them again after every update. Packages install through channels that
  do not quarantine downloads (Homebrew formula, Scoop), which avoids
  Gatekeeper and SmartScreen prompts.
- **Channels:** Homebrew formula in our own tap, Scoop bucket, `.deb`, `.rpm`,
  AUR, `cargo install`; winget later if users ask. No Flatpak or Snap
  (sandboxing fights input capture and the daemon).

## Testing

- GitHub Actions matrix: Windows, macOS, Linux.
- Unit tests for `a7n-core` and `a7n-proto`; protocol integration tests with
  simulated in-process peers.
- Platform backends: manual per-release test checklist.

## Roadmap

| Version | Scope |
| ------- | ----- |
| v1 | Pairing, input sharing, text clipboard (plain + HTML) on all 4 platforms; CLI + TUI |
| v2 | GUI + tray, packages, image clipboard |
| v3 | File clipboard, `a7n send`, wake-on-LAN |
| Later | Login-screen support |

## Prior art

Barrier, Input Leap, Deskflow, lan-mouse, ShareMouse, Beamer, TwinDesk
(GPL-3.0; source of the take timeout, double-tap hotkey and capture
dead-man switch ideas).
