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
