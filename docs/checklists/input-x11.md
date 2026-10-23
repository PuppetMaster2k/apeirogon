# Manual release checklist: Linux X11 input backend

Run on a real X11 session (not Wayland, not XWayland), with `apeirogond`
paired to a second machine. Every item must pass before a release.

## Capture (this machine drives a peer)

- [ ] Typing: letters, digits, punctuation, Shift/Caps Lock reach the peer
  unchanged; no key repeats on the peer beyond its own OS repeat.
- [ ] Shortcuts: Ctrl+C / Ctrl+V, Alt+Tab, and a Meta (Super) combination
  work on the peer and do nothing locally while driving.
- [ ] Mouse: motion is smooth, left/middle/right and back/forward buttons
  work, drag and double click work, slow motion is not lost.
- [ ] Scroll: wheel notches scroll the right way vertically and
  horizontally; a touchpad scrolls smoothly.
- [ ] Media keys: Mute, Volume up/down, Play/Pause, Next, Previous reach the
  peer.
- [ ] Edge crossing out: pushing the pointer through the configured edge
  hands focus to the peer; the local cursor is hidden while driving.
- [ ] Edge crossing back: pushing through the peer's edge returns focus; the
  cursor reappears at the right place and local input works at once.
- [ ] Grab does not leak: after the grab ends, local typing and clicking go
  to local apps, and no key typed while driving shows up locally.

## Injection (a peer drives this machine)

- [ ] Typing, shortcuts, mouse, scroll and media keys from the peer behave as
  above on this machine.
- [ ] Edge crossing: moving the peer's pointer through this machine's edge
  hands focus back.
- [ ] Multi-monitor or a non-zero screen origin: the pointer lands where the
  peer expects (bounds are the root window at start; RandR changes need a
  daemon restart).

## Failure handling

- [ ] Stuck keys: hold a key and a modifier on the peer, cut the link (pull
  the cable or block the port); the keys release on this machine and do not
  stay down.
- [ ] Kill the daemon while driving (`kill -9 apeirogond`): local keyboard
  and mouse work again within 1.5 s and the cursor is visible.
- [ ] Start the daemon with `XDG_SESSION_TYPE=wayland` set and `WAYLAND_DISPLAY` unset: it reports `kind=input_unavailable` and does not grab anything.
