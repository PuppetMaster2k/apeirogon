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
