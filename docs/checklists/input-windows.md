# Manual release checklist: Windows input backend

Run on a real Windows user session, with `apeirogond` paired to a second
machine. Every item must pass before a release; record the Windows version
and whether the daemon and target apps run as administrator. Login screens
and secure desktops are out of scope; input must release there.

## Capture (this machine drives a peer)

- [ ] Typing: letters, digits, punctuation, Shift/Caps Lock reach the peer
  as physical keys using its keyboard layout; holding a key does not send
  duplicate key-downs beyond the peer's own OS repeat.
- [ ] Extended keys: left/right Ctrl and Alt, navigation keys, keypad Enter,
  Num Lock, Pause and Print Screen reach the peer as distinct keys.
- [ ] Shortcuts: Ctrl+C / Ctrl+V, Alt+Tab and a Win-key combination work on
  the peer and do nothing locally while driving.
- [ ] Mouse: motion is smooth, left/middle/right and back/forward buttons
  work, drag and double click work, slow motion is not lost.
- [ ] Motion while driving is unaccelerated: toggle "Enhance pointer precision"
  on the driving machine; peer motion speed must not change.
- [ ] Scroll: line scroll from a wheel goes the right way on both axes;
  high-resolution wheel or touchpad deltas keep fractional lines. Windows
  capture sends line units, not native pixel scroll.
- [ ] Media keys: Mute, Volume up/down, Play/Pause, Stop, Next, Previous
  reach the peer.
- [ ] Edge crossing out: pushing through the configured edge hands focus
  to the peer; the local pointer stays at its capture position while driving.
- [ ] Edge crossing back: pushing through the peer's edge returns focus;
  the pointer lands at the matching place and local input works at once.
- [ ] Grab does not leak: after the grab ends, local typing and clicking go
  to local apps, and no key typed while driving shows up locally.

## Injection (a peer drives this machine)

- [ ] Typing, shortcuts, mouse, drag and media keys from the peer behave as
  above on this machine; injected input does not echo back to the peer.
- [ ] Scroll: line and pixel scroll from the peer work on both axes in both
  directions. Pixel scroll accumulates into wheel notches (15 pixels per
  notch), keeping remainders; do not expect native smooth pixel scrolling.
- [ ] Speed multiplier: compare `input.speed` values 0.5, 1.0 and 2.0 on
  this machine with the same peer motion; movement scales accordingly and
  repeated small movements still move the pointer. Restore the setting.
- [ ] Edge crossing: moving the peer's pointer through this machine's edge
  hands focus back.
- [ ] Multi-monitor, mixed DPI or a negative screen origin: the pointer
  lands at the expected place across the virtual desktop. Restart the daemon
  after changing the display arrangement; bounds are sampled at start.
- [ ] Elevated target windows: compare typing and clicking in an ordinary
  app and an app run as administrator, with the daemon unelevated and then
  elevated. Record any blocked injection and `kind=input_inject_failed`;
  do not assume an unelevated daemon can drive an admin app. Local input
  must remain usable after returning home or stopping the daemon.

## Failure handling

- [ ] Stuck keys after link loss: while a peer drives this machine, hold a
  key, a modifier and a mouse button on its driver, then cut the link (pull
  the cable or block the port); all held inputs release on this machine.
  Repeat with this machine driving; neither machine keeps an input down.
- [ ] Kill the daemon while driving: local keyboard and pointer come back within 1.5 s
  (force termination through Task Manager). This is the dead-man switch
  requirement (`DEADMAN_TIMEOUT`); also suspend the daemon while driving to
  check missing keepalives release capture without process exit.
- [ ] Safety hotkeys: while driving, double-tap Right Alt within 400 ms,
  press Scroll Lock, and press Ctrl+Alt+Shift+Home in separate runs; each
  returns focus home and releases held inputs on the peer.
- [ ] Cursor lock: Ctrl+Alt+Shift+L prevents edge crossing; press it again
  to unlock. Ctrl+Alt+Shift+1…9 jumps to an available paired layout machine.
  Windows' Alt+Shift input-language hotkey also fires on this chord; turn it off under Settings > Time & language > Typing > Advanced keyboard settings > Input language hot keys, or pick other jump keys.
- [ ] Unpair a machine: it disappears from Paired machines immediately (it
  may remain under Nearby machines). Its retained layout placement says
  “Unpaired machine”; edge pushes and jumps skip it without a wake attempt.
  Jumping to its slot does nothing; later machines keep their numbers.
- [ ] Secure desktop: open a UAC prompt or press Ctrl+Alt+Del locally while
  driving, then repeat while this machine is driven. Remote control of
  these screens is out of scope; input must release, with local keyboard
  and pointer usable and no held inputs left after returning to the session.

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
- [ ] The desktop, taskbar and shell windows do not hold edges.
- [ ] A borderless game slightly larger than its monitor still holds. An ordinary
  maximized window does not hold, including with an auto-hidden taskbar.

## Edge glow and landing marker (#185)

- [ ] Push the pointer against an edge that has a neighbour: a thin accent
  strip appears along that edge, centred on the pointer, and grows longer and
  stronger as the push builds up.
- [ ] The strip never takes focus, never moves the active window's title bar
  to inactive, and clicks, scrolls and drags pass straight through it. It does
  not show in the taskbar or Alt+Tab.
- [ ] The strip disappears at once when the pointer crosses, when the push
  stops, and when the setting is turned off.
- [ ] After crossing to this machine, a ring marks the landing point and fades
  out within half a second.
- [ ] Turn off Settings > Accessibility > Visual effects > Animation effects:
  the ring appears at full strength for the same short time and vanishes
  without fading; the strip still works.
- [ ] With two monitors at different scaling (for example 100% and 175%), the
  strip appears on the monitor holding the pointer, hugs the right edge, and
  has the same apparent thickness on both; the ring is centred on the landing
  point on either monitor. Repeat with a monitor at negative coordinates.
- [ ] Take a screenshot and start screen sharing while the strip and ring are
  showing: neither appears in the capture (Windows 10 2004 or later).
