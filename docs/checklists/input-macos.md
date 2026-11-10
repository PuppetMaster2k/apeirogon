# Manual release checklist: macOS input backend

Run on a real macOS desktop, with `apeirogond` paired to a second machine.
Include a non-Mac peer for modifier remapping, a wheel mouse and a trackpad.
Every item must pass before a release.

## Permissions

- [ ] Grant Accessibility and Input Monitoring in System Settings > Privacy
  & Security for the process running the daemon, then restart the daemon;
  input capture and injection work.
- [ ] Start without Accessibility: `a7n status` names the missing permission,
  the log reports `kind=input_unavailable`, and the daemon keeps running
  without capturing or injecting input; local input still works.
- [ ] With Accessibility granted but Input Monitoring missing, repeat the
  previous check: the reason names Input Monitoring. Grant it and restart;
  input works again.

- [ ] Remove Accessibility while idle and while driving another machine: input never freezes;
  re-granting restarts input without restarting the daemon.

## Capture (this machine drives a peer)

- [ ] Typing: letters, digits, punctuation, Shift/Caps Lock reach the peer
  according to its keyboard layout; no key repeats on the peer beyond its
  own OS repeat.
- [ ] Shortcuts: Command (⌘), Ctrl, Option and Shift combinations work on the
  peer and do nothing locally while driving. Hold both left and right
  modifiers, release one, and check the other stays active until released.
- [ ] Modifier remap: for a non-Mac peer, set
  `peers.<id>.modifier_remap` to map `left_meta` to `left_ctrl` and
  `right_meta` to `right_ctrl`. Take focus again; ⌘+C / ⌘+V act as Ctrl+C /
  Ctrl+V there, and releasing ⌘ releases Ctrl. A peer without this remap
  still receives Command as Meta.
- [ ] Mouse: motion is smooth, left/middle/right and back/forward buttons
  work, drag and double click work, slow motion is not lost.
- [ ] Scroll: wheel line scroll and trackpad pixel scroll work vertically
  and horizontally, with no missing or doubled wheel notches. Judge pixel
  smoothness against what the receiving backend supports.
- [ ] Natural scrolling: repeat both axes with macOS Natural scrolling on
  and off; with `input.natural_scrolling=false`, direction follows the
  captured macOS events. Setting it to `true` reverses both outgoing axes
  once and leaves local scrolling unchanged.
- [ ] Media keys: Mute, Volume up/down, Play/Pause, Next, Previous reach the
  peer.
- [ ] Edge crossing out: pushing the pointer through the configured edge
  hands focus to the peer; the local cursor is hidden while driving.
- [ ] Edge crossing back: pushing through the peer's edge returns focus; the
  cursor reappears at the right place and local input works at once.
- [ ] Grab does not leak: after the grab ends, local typing and clicking go
  to local apps, and no key typed while driving shows up locally.

## Injection (a peer drives this machine)

- [ ] Typing, shortcuts, mouse buttons, drag, double click and media keys
  from the peer behave as above on this machine, using its keyboard layout.
  Injected events do not feed back to the peer as physical input.
- [ ] Scroll: line and pixel input from the peer work on both axes in the
  expected direction. Known limit: pixel deltas accumulate into whole line
  steps (15 pixels per line); tiny movements build up rather than being
  lost, and native pixel-smooth scrolling is not expected here.
- [ ] Speed multiplier: compare `input.speed=1.0`, `0.5` and `2.0` on this
  Mac with the same peer motion; lower values move less and higher values
  move more, with slow motion still accumulating. Restore the original
  value after the check.
- [ ] Edge crossing: moving the peer's pointer through this machine's edge
  hands focus back.
- [ ] Multi-monitor or a non-zero screen origin: the pointer lands where the
  peer expects and stays within the combined active display bounds. Restart
  the daemon after changing displays; bounds are sampled at start.

## Failure handling

- [ ] Stuck keys: while a peer drives this Mac, hold a key, a modifier and a
  mouse button, then cut the link (pull the cable or block the port); all
  release on this Mac. Repeat with the Mac driving and check the peer too.
- [ ] Kill the daemon while driving: local keyboard and pointer come back within 1.5 s
  (`kill -9` the daemon process); the cursor is visible again. This is the
  capture dead-man safety requirement (`DEADMAN_TIMEOUT` in `input.rs`).
- [ ] Safety hotkeys: double-tap Right Option within 400 ms while driving;
  focus returns home and no modifier stays held. Repeat with the configured
  `hotkeys.home` chord (default Ctrl+Option+Shift+Home; Scroll Lock also
  works on keyboards that have it).
- [ ] Cursor lock: Ctrl+Option+Shift+L prevents edge crossing; pressing it
  again restores crossing. Ctrl+Option+Shift+1…9 jumps to the configured
  machine. Repeat with rebound `hotkeys.lock` and `hotkeys.jump` settings.

## Trackpad swipe shortcuts (#97)

Build this branch on both machines. Pair the Mac with Windows, grant the Mac
Accessibility and Input Monitoring permissions, and verify input can drive
Windows. Keep the default Windows gesture bindings for the guided test.
From the repository root on the Mac, run:

```sh
./scripts/test-macos-gestures.sh
# Or select the CLI built from this branch:
A7N="$PWD/target/release/a7n" ./scripts/test-macos-gestures.sh
```

Each Enter starts an eight-second window: switch input to Windows, perform
only the named action, then return home for the next prompt. Lift all fingers
between steps. Enable the matching three-/four-finger system settings yourself
when needed; the script only reads them. Only swipes captured while driving
a peer with negotiated gesture support produce gesture log entries; swipes
at home or to unsupported peers are not counted. PASS requires one new entry per swipe, zero for false-trigger checks, and yes to each visual check.
Zero delivery stops with “macOS does not deliver touches to the event tap”;
other count mismatches or negative answers produce FAIL. A log-read failure
or rotation blocks the test rather than claiming no touches arrived.

Attach the generated `gesture-test-<UTC timestamp>.md` to #97 on failure,
not raw logs. State whether the daemon ran by hand or at login; the report
includes OS/build, model, gesture settings and results without peer identities
or touch data. Complete the remaining checklist yourself: Windows binding
edits, GUI, capability peers, finger-count changes, and dead-man/permission
recovery are not covered by the guide.

- [ ] Pair the Mac with Windows; enable three-finger, then four-finger system
  swipes in macOS Trackpad settings. Test the built-in trackpad and an external
  Magic Trackpad if available, with the daemon running in the background.
- [ ] At home, swipes still operate macOS normally. While driving Windows,
  each up/down/left/right swipe performs exactly one Task View/show desktop/
  previous desktop/next desktop action. macOS must not also open Mission
  Control or switch Spaces. Check logs for `kind=gesture_swipe` only, never
  touch coordinates. If no swipe arrives, verify whether public event-tap
  NSEvents contain touches; record OS version, device and finger setting.
- [ ] Hold fingers after crossing the threshold, reverse direction, and add or
  remove a finger: no second shortcut until all fingers lift. Short movements,
  two-finger scrolling, pinch and spread must not activate a shortcut.
- [ ] Change each Windows `gestures.swipe_*` binding using config get/set and
  reload; confirm the new chord fires without restarting. Clear a binding:
  only that swipe stops. Invalid chords must fail without changing settings.
- [ ] In the Windows GUI, open Hotkeys → Trackpad gestures; edit, clear, save,
  discard/reload, and try an invalid chord. macOS GUI must not show the card.
- [ ] Drive another Mac with the gesture capability: swipes are safely ignored
  by its injector. Drive Linux or a peer without that capability: no gesture
  is sent. Ordinary keyboard input and two-finger scrolling still work.
- [ ] Return home or pause mid-swipe and start driving again: no stale swipe
  or held shortcut modifiers. Revoke permissions or stop the daemon during
  a grab and confirm the dead-man release restores local input.

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
- [ ] Holding works without Screen Recording permission or a new permission prompt.
- [ ] The front layer-zero window decides the hold; a full-screen window behind
  a windowed foreground window must not hold, even if both belong to the same app.
