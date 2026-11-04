# apeirogon

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

Start the daemon with `a7n daemon start`, then open `a7n-gui`.
Use `a7n daemon enable` to start it automatically at login.

## Files

Copy files or folders in your file manager and paste them on another paired
machine. Files download on paste into `files.receive_dir`, which defaults to
your Downloads directory under `Apeirogon` (`~/Downloads/Apeirogon` as a fallback).

To send files directly:

```sh
a7n send <peer> <path>...
a7n files accept <peer>
a7n files reject <peer>
a7n files cancel <id>
```

`<peer>` is a peer name, machine id or unique hex prefix of at least four digits.
`a7n send` follows the transfer; add `--no-wait` to return after offering it.
Use `a7n status` or the GUI to see held offers, progress and transfer ids for
cancellation. Accept or reject an offer on the receiving machine.

Per-peer `peers.<id>.files` defaults to `ask`: incoming sends need confirmation,
while a local paste counts as consent. Policies are `both`, `send`, `receive`,
`ask` and `off`. Receiving requires enough free space for the remaining download
plus a 256 MiB margin. Files are verified before publishing; existing files
are preserved by choosing a new name on collisions.
