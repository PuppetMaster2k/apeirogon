# apeirogon

## Install

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
