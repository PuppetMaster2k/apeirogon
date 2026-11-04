# Manual release checklist: file transfers

Run with paired machines using Windows/Explorer, macOS/Finder, Linux
X11/Nautilus and Linux Wayland/Nautilus. On Wayland, cover both GNOME's
clipboard portal and a compositor with data-control. Record OS, file manager
and compositor versions in the release notes. These checks need real desktops
and are not covered by CI.

## Copy and paste

- [ ] Copy a file and a folder in the file manager, then paste on a peer.
  They land in `files.receive_dir` with matching contents.
- [ ] Copy on a peer, then paste in Explorer, Finder and Nautilus on X11 and
  Wayland. Explorer copies rather than moves. Finder shows every top-level
  entry: try one file, three files and a directory.
- [ ] Paste a file larger than 1 GB: the paste waits, `a7n status` and the GUI
  show progress, and no timeout occurs before 10 minutes.
- [ ] Cancel or disconnect mid-paste: the paste fails cleanly and the next
  copy works. On reconnect, an explicit send resumes from its verified partial
  data and finishes with matching contents.
- [ ] Copy something else during a pending paste: the stale paste is refused.
- [ ] On Windows, quit the daemon while files are offered: it exits promptly.

## Offers and policy

- [ ] Run `a7n send <peer> <path>...` with the receiver's file policy set to
  `ask`: acceptance downloads the files, rejection and the 60-second timeout
  do not, and the sender's exit code matches the outcome.
- [ ] Accept and reject held offers with `a7n files accept <peer>` and
  `a7n files reject <peer>` and with the GUI.
- [ ] Cancel an active transfer with `a7n files cancel <id>` using the id
  shown by `a7n status`: progress stops and incomplete data is removed.
- [ ] With the receiver's `peers.<id>.files` set to `off` or `send`, copied
  files are not offered for paste and explicit incoming sends are ignored.
- [ ] With `ask`, a local paste downloads files without another confirmation.

## Receive directory and safety

- [ ] With no configured receive directory, files land in Downloads/Apeirogon.
  Set an absolute `files.receive_dir` and verify subsequent files land there.
- [ ] Paste or send a name already present: the existing entry is preserved
  and the new entry receives a collision suffix.
- [ ] Receive is refused when the disk is too full for the remaining bytes
  plus 256 MiB. A paste fails with reason `no_space`; accepting a held offer
  through the CLI or GUI shows "not enough free space in the receive directory".
  An automatic offer is dropped with log kind `file_offer_no_space`.
- [ ] Repeat after disconnecting a partial transfer: only the remaining bytes
  plus the margin are needed. Free space and retry; contents still match.
- [ ] Logs contain event kinds and sizes only, never file names or paths.
