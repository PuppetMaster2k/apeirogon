# Manual release checklist: Linux Wayland input backend

Run on a real Wayland session, once on GNOME (Mutter) and once on KDE
(KWin), with `apeirogond` paired to a second machine. The backend uses the
`InputCapture` and `RemoteDesktop` portals with libei, so it cannot be tested
in CI. Every item must pass on both desktops before a release; note the
compositor version and the portal versions (`InputCapture`, `RemoteDesktop`)
in the release notes. A portal that is too old must make `a7n status` show the
reason instead of hanging.

## Consent and persistence

- [ ] First start: two consent dialogs appear (input capture, then remote
  control), and `a7n status` reports input as available after both are
  accepted.
- [ ] Second start (restart the daemon): no dialog appears when the portal
  supports persistence (`InputCapture` 2, `RemoteDesktop` 2). A portal without
  it asks again on every start; that is expected and must not fail.
- [ ] With `packaging/linux/io.github.PuppetMaster2k.Apeirogon.desktop`
  installed in `~/.local/share/applications/`, the consent dialogs show
  "Apeirogon" as the requesting application and do not reappear after a
  daemon restart.
- [ ] Declining a dialog: `a7n status` names the missing permission; the
  daemon keeps running without input.
- [ ] Leaving a dialog unanswered for 2 minutes ends the start with a timeout
  reason and the daemon keeps running.
- [ ] `wayland-portal-tokens` in the daemon state directory is readable by the
  user only (mode 0600) and holds one line per session.
- [ ] Revoking the permission in the desktop's settings makes the next start
  prompt again instead of failing.

## Capture (this machine drives a peer)

- [ ] Typing: letters, digits, punctuation, Shift/Caps Lock reach the peer
  unchanged, and nothing typed while driving shows up locally.
- [ ] Shortcuts: Ctrl+C / Ctrl+V, Alt+Tab and a Super combination work on the
  peer and do nothing locally while driving.
- [ ] Mouse: motion is smooth, left/middle/right and back/forward buttons
  work, drag and double click work, slow motion is not lost.
- [ ] Scroll: a wheel scrolls the right way vertically and horizontally by
  whole notches (never twice per notch); a touchpad scrolls smoothly.
- [ ] Media keys: Mute, Volume up/down, Play/Pause, Next, Previous reach the
  peer.
- [ ] Edge crossing out: pushing the pointer against the configured edge
  hands focus to the peer, with edge resistance if configured; the local
  cursor does not move while driving.
- [ ] Edge crossing back: pushing through the peer's edge returns focus; the
  cursor reappears at the matching place and local input works at once.
- [ ] Push without a neighbour: pushing against an edge that has no peer
  gives the cursor back after about one second, at the place the push began.
- [ ] Push then retreat: pushing against an edge and moving back inside gives
  the cursor back at once.
- [ ] Multi-monitor: monitors side by side have no capture trigger on the seam
  between them; every outer edge triggers.
- [ ] Hot-plugging a monitor re-arms the edges without restarting the daemon.

## Injection (a peer drives this machine)

- [ ] Typing, shortcuts, mouse, scroll and media keys from the peer behave as
  above on this machine.
- [ ] The cursor appears where the landing point says when focus arrives
  (needs an absolute pointer device; on a compositor without one, record that
  the cursor stays where it was).
- [ ] Edge crossing: moving the peer's pointer through this machine's edge
  hands focus back.
- [ ] Known limit: while a peer drives this machine, local input is not
  blocked and local activity is not seen (capture starts only at an edge).
  Confirm this is what happens and nothing worse.

## Safety

- [ ] Stuck keys after link loss: hold a key on the peer's driver, pull the
  network cable, and verify no key stays pressed on either machine.
- [ ] Kill the daemon (`kill -9`) while driving: local keyboard and pointer
  work again within 1.5 s.
- [ ] Stop the daemon with Ctrl+C while driving: input returns at once.
- [ ] Logging out and back in starts the backend again with no stale session.
