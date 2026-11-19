# Apeirogon

Apeirogon is a free, open-source software KVM that lets one keyboard and
mouse control several computers. Move the pointer off the edge of one screen
and it continues on the next machine, with the clipboard following along.
It runs on Windows, macOS and Linux and connects machines directly over the
local network with encrypted, mutually authenticated links — no cloud service
or account involved.

## Features

- Share keyboard and mouse input across Windows, macOS, Linux/X11 and
  Linux/Wayland (with the required desktop portals).
- Arrange machines on a shared layout, including offsets and blocked edges.
- Return home, lock the cursor or jump to a machine with configurable hotkeys.
- Share text, HTML and images through the clipboard, with per-peer policies
  and automatic exclusion of content marked sensitive by the OS.
- Copy and paste files and folders, or send them directly with confirmation
  and transfer progress.
- Wake paired machines with Wake-on-LAN when their hardware supports it.
- Manage machines, layout and settings through the CLI and GUI.

## Install

On macOS (Apple Silicon) or Linux (x86_64), use Homebrew:

```sh
brew install puppetmaster2k/tap/apeirogon
```

Releases are unsigned. On macOS, grant Accessibility and Input Monitoring
permissions again after every update.

On Windows (x86_64), use Scoop:

```sh
scoop bucket add apeirogon https://github.com/PuppetMaster2k/scoop-bucket
scoop install apeirogon
```

On Arch Linux (x86_64), use the AUR package:

```sh
yay -S apeirogon-bin
```

Download the x86_64 `.deb` or `.rpm` package from the
[latest release](https://github.com/PuppetMaster2k/apeirogon/releases/latest).
Each package includes the daemon, CLI, GUI and a desktop launcher.

```sh
sudo apt install ./apeirogon_*.deb
# Fedora and other RPM-based distributions:
sudo dnf install ./apeirogon-*.rpm
```

Or install from crates.io with a recent stable Rust toolchain:

```sh
cargo install a7n apeirogond a7n-gui --locked
```

## Quick start

Install Apeirogon on both machines and keep them on the same local network.
Run the daemon in your desktop session on each machine:

```sh
a7n daemon start
```

Grant the requested OS permissions. On macOS, allow Accessibility and Input
Monitoring for the daemon. On Wayland, approve the desktop portal dialogs.
See [troubleshooting](docs/guide.md#troubleshooting) if input is unavailable.

### Pair two machines

On the first machine, request a one-time pairing code:

```sh
a7n pair
```

On the second machine, use the first machine's IP address (replace the example):

```sh
a7n pair 192.168.1.10
```

Enter the code at the prompt. Check the connection on either machine:

```sh
a7n status
```

For GUI pairing, open `a7n-gui` on both machines. On **Machines**, click
**Host** on the first. On the second, enter the first machine's **Address**
and **Pairing code**, then click **Join**.

### Arrange the layout and move across

Open **Layout** in the GUI. Drag the machine rectangles to match your desk,
with the edges you want to cross touching. Click **Save**. The layout is
shared with the other machines.

You can also arrange machines in the terminal:

```sh
a7n layout
```

Use Tab to select a machine, arrow keys to move it, and Enter to save.
Push the pointer against a connected edge to move to the next machine.
Double-tap Right Alt (Right Option on macOS) to return home.

To start the daemon automatically at login, run this on each machine:

```sh
a7n daemon enable
```

See the [user guide](docs/guide.md) for hotkeys, clipboard and file policies,
settings, autostart and troubleshooting.

## Documentation

- [User guide](docs/guide.md): setup and everyday use.
- [Design](docs/design.md): behaviour and architecture.
- [Protocol](docs/PROTOCOL.md): wire format and security rules.
- [UI specification](docs/ui.md): interface design.
- [Changelog](CHANGELOG.md): release history.

## Contributing

Read [AGENTS.md](AGENTS.md) for the branch and pull request workflow.
Link your issue, use Conventional Commits and sign off every commit for the DCO.
Before opening a PR, run `cargo fmt --all`,
`cargo clippy --all-targets -- -D warnings` and `cargo test --workspace`.

## License

[GPL-3.0-or-later](LICENSE); `a7n-proto` is MIT OR Apache-2.0.
