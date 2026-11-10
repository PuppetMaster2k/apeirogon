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

- [ ] Ctrl+Alt+Shift+1…9 jumps to a paired layout machine.
  After unpairing, edge pushes and jumps skip that placement without waking it.
- [ ] Stuck keys: hold a key and a modifier on the peer, cut the link (pull
  the cable or block the port); the keys release on this machine and do not
  stay down.
- [ ] Kill the daemon while driving (`kill -9 apeirogond`): local keyboard
  and mouse work again within 1.5 s and the cursor is visible.
- [ ] Start the daemon with `XDG_SESSION_TYPE=wayland` set and `WAYLAND_DISPLAY` unset: it reports `kind=input_unavailable` and does not grab anything.

## Full-screen edge hold (#92)

- [ ] Enable `input.fullscreen_hold` (or “Keep the pointer in full-screen apps”)
  and verify a full-screen game, video and presentation each hold edge pushes,
  both with local input and while driven. Windowed apps still allow crossing.
- [ ] Shortcuts, jump keys, shake-to-return and lock still work while held.
- [ ] A continuous held push logs `kind=edge_held_fullscreen` only once; moving
  away and pushing again rechecks focus and full-screen state.
- [ ] Full-screen `a7n-gui` does not hold edges.
- [ ] With multiple monitors, full-screen on one monitor does not hold a push
  on another; check displays with negative desktop coordinates and mixed scaling.
- [ ] Disable the setting and reload: edge crossings work as before. Re-enable
  and reload: holding works without restarting. Other machines keep their own setting.
- [ ] In a Wayland session the toggle is disabled with “Not available on Wayland”; setting it through the CLI has no effect.
- [ ] An EWMH full-screen window without `_NET_WM_PID`, or with an unreadable
  `/proc/<pid>/exe`, still holds. A borderless game without the EWMH full-screen
  state does not hold.

## Edge and landing feedback (#187)

- [ ] Pushing against a screen edge with an edge crossing configured shows a
  glow strip along that edge that grows (longer) as the push builds up.
- [ ] The glow is click-through: clicking and scrolling at the strip reaches the
  window underneath, and it never takes keyboard focus or changes the active window.
- [ ] With several RandR monitors the glow appears on the monitor containing the
  pointer, along its own edge.
- [ ] The glow disappears on crossing, when the push stops, and when `feedback.edge` is turned off.
- [ ] After a take, a small accent-coloured square marks the landing point and
  disappears after about half a second; turning `feedback.landing` off hides it.
- [ ] Neither window shows in the task bar, pager or window switcher, and both
  appear above other windows including full-screen ones.
- [ ] In a Wayland session the feedback settings show "Not available on Wayland".
