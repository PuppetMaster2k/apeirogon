Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

# Code signing policy

Windows code signing is being set up. Releases are currently unsigned.
The signing scope is the Windows release binaries `apeirogond.exe`, `a7n.exe`
and `a7n-gui.exe` built by the [release workflow](../.github/workflows/release.yml).
The certificate is issued to SignPath Foundation, which will appear as the
publisher of signed binaries.

## Team roles

- Committers and reviewers: [PuppetMaster2k](https://github.com/PuppetMaster2k).
- Approvers: [PuppetMaster2k](https://github.com/PuppetMaster2k).

Only approvers can approve a signing request. Contributors with signing access
must use multi-factor authentication on both GitHub and SignPath.

## Build and release process

All changes reach `main` through reviewed pull requests. Under this policy,
only builds produced by GitHub Actions from version tags on `main`, created
by release-please releases, may be submitted for signing. Locally built
binaries are not signed.

## Privacy policy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

Operating Apeirogon involves these network communications:

- Paired machines on the local network exchange input, clipboard contents,
  files and the control information needed to share them. These connections
  are encrypted and mutually authenticated.
- While the daemon runs, mDNS discovery automatically announces the machine
  ID, protocol version, local IP addresses and listening port on the local
  network. This lets paired machines find each other and lists nearby machines
  for pairing. The OS hostname is not announced.
- Waking a paired machine sends Wake-on-LAN UDP broadcast packets on the local
  network, containing that machine's MAC address. Waking can be requested
  explicitly or triggered by switching to an offline machine when wake is enabled.
- The GUI can check for updates with an HTTPS GET request to
  `https://api.github.com/repos/PuppetMaster2k/apeirogon/releases/latest`.
  Checks run on demand, or automatically when enabled in settings. Automatic
  checks are off by default; when enabled, they run on GUI startup and at most
  once every 24 hours while it runs. The request uses the `apeirogon-gui`
  User-Agent and sends no input, clipboard contents, files or machine identifiers.
  GitHub receives the source IP address and ordinary HTTPS request metadata.
