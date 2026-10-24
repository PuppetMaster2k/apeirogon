# Manual release checklist: Linux Wayland clipboard backend

Run on real Wayland sessions with `apeirogond` paired to a second machine.
Two paths exist and each needs its own run:

- GNOME (Mutter): no data-control; the clipboard rides on the `RemoteDesktop`
  portal session of the input backend (`Clipboard` portal).
- KDE (KWin) and Sway/wlroots: `ext_data_control_v1` or
  `zwlr_data_control_unstable_v1`, no consent dialog.

Note the compositor version and, on GNOME, the `Clipboard` and `RemoteDesktop`
portal versions in the release notes. The backend cannot be tested in CI.

## GNOME (portal)

- [ ] The remote control consent dialog also covers the clipboard; after
  accepting it `a7n status` reports clipboard text as available.
- [ ] Denying clipboard access or a portal without the `Clipboard` interface
  leaves input working and `a7n status` shows clipboard as unavailable.
- [ ] Copy text on this machine, paste on the peer; then the other way.
- [ ] Copy rich text (browser selection): plain text and HTML both arrive.
- [ ] Copy more than 64 KiB of text: the peer pastes it all (lazy item).
- [ ] Paste a large lazy item while the peer is unreachable: the paste ends
  empty after about 10 s and the desktop stays responsive.
- [ ] Copy a password from a password manager that sets
  `x-kde-passwordManagerHint`: nothing appears on the peer, and the previous
  clipboard is no longer offered.
- [ ] Restart the daemon: the clipboard works again (with a dialog only where
  the portal cannot persist consent).

## KDE and Sway (data-control)

- [ ] No dialog appears; `a7n status` reports clipboard text as available.
- [ ] Same items as above: text both ways, rich text, large lazy item,
  unreachable peer, password-manager entry not announced.
- [ ] On Sway, `wl-copy` and `wl-paste` interoperate with pastes from the peer
  (data offered by this backend is readable without a focused window).
- [ ] A clipboard manager running next to the daemon does not make copies
  bounce between the machines.

## Echo (all desktops)

- [ ] Copy on machine A: machine B receives it once, and A does not receive
  its own copy back (check `a7n clipboard` or the daemon log for a single
  announce per copy).
- [ ] Paste on B, then copy something new on B: A receives it once.
- [ ] Copy the same text twice in a row: each copy is announced, none loops.

## Safety

Wayland provides no source app identity, so `clipboard.deny_apps` has no effect
there. Sensitive-content markers still prevent clipboard announcements.

- [ ] Primary selection (middle click) is never synchronised.
- [ ] Logs contain event kinds and sizes only, never clipboard text.
