# AGENTS.md

Rules for anyone (human or AI agent) writing code in this repository. Read
[`docs/design.md`](docs/design.md) for *what* we build and
[`docs/PROTOCOL.md`](docs/PROTOCOL.md) for the wire protocol. This file says
*how* we write it. When the design and this file disagree, the design wins on
behaviour and this file wins on style.

## 1. Project structure

```
proto/apeirogon/v1/apeirogon.proto   wire schema (source of truth for messages)
crates/
  a7n-proto      generated types, framing, encode/decode (MIT OR Apache-2.0)
  a7n-ipc        daemon <-> CLI/GUI socket API
  a7n-core       focus, layout, desk, clipboard logic; platform-free, no I/O
  a7n-platform   Platform traits + backends (windows, macos, x11, wayland)
  apeirogond     daemon binary: wires core + platform + network together
  a7n            CLI + TUI
  a7n-gui        egui app (thin client of the daemon)
docs/            design.md, PROTOCOL.md
```

Dependency direction is one way. Never add an arrow that points back up.

```
a7n, a7n-gui ──► a7n-ipc ──► a7n-core ──► a7n-proto
apeirogond ──► a7n-core, a7n-platform, a7n-proto, a7n-ipc
a7n-platform ──► a7n-core (traits/types only)
```

Rules:

- **`a7n-core` is pure.** No sockets, no files, no clocks, no OS calls. Time,
  randomness and I/O come in as parameters or traits. That is what makes it
  testable without a desktop.
- **Platform code lives only in `a7n-platform`**, behind traits defined there.
  `#[cfg(target_os = ...)]` appears nowhere else.
- **Protocol constants come from one place.** Limits from PROTOCOL.md section
  14 live in `a7n-proto::limits`; never repeat a number elsewhere.
- One module per concept. A file that grows past ~400 lines is a hint to split.

```rust
// a7n-core/src/layout.rs: pure logic, the clock is a parameter
pub fn apply_edit(current: &Layout, edit: Layout, now: Version) -> Layout { ... }

// apeirogond/src/layout_sync.rs: the I/O shell around it
let merged = layout::apply_edit(&state.layout, incoming, clock.next_version());
```

## 2. Coding style

- Edition 2021, `rustfmt` with default settings. CI fails on unformatted code.
- `cargo clippy --all-targets -- -D warnings` must pass. Enable
  `clippy::pedantic` per crate and allow individual lints only with a reason.
- Every file starts with an SPDX line: `// SPDX-License-Identifier: GPL-3.0-or-later`
  (`MIT OR Apache-2.0` in `a7n-proto`).
- Naming follows the Rust API guidelines: `snake_case` functions and modules,
  `UpperCamelCase` types, `SCREAMING_SNAKE_CASE` constants. Conversions are
  `as_`/`to_`/`into_`; fallible constructors are `try_from`/`parse`.
- Use the domain words from the design and protocol: `owner`, `route`, `desk`,
  `pin`, `machine id`, `generation`, `landing`. Do not invent synonyms.
- Imports grouped std, external, crate; no glob imports except `use super::*`
  in test modules and preludes.

```rust
// Good
pub struct MachineId([u8; 16]);
const MAX_CONTROL_FRAME: usize = 128 * 1024;
fn release_held_keys(&mut self) { ... }

// Bad
pub struct Mid(Vec<u8>);      // abbreviation, unchecked length
const LIMIT: usize = 131072;  // what limit? where does it come from?
fn cleanup(&mut self) { ... } // cleans up what?
```

## 3. Rust best practices

### Types make invalid states unrepresentable

Use newtypes for ids, pins and counters. Validate once at the boundary (when
decoding a message), then pass the checked type around.

```rust
pub struct MachineId([u8; 16]);

impl TryFrom<&[u8]> for MachineId {
    type Error = ProtoError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let array: [u8; 16] = bytes.try_into().map_err(|_| ProtoError::BadMachineId)?;
        Ok(Self(array))
    }
}
```

Prefer enums over booleans and stringly-typed values:

```rust
// Good
pub enum DrivePolicy { Allowed, Denied }
// Bad
pub allow: bool, pub mode: String
```

### Errors

- Libraries define their own error enums with `thiserror`. Binaries may use
  `anyhow` at the top level only.
- **No `unwrap()`/`expect()` on anything that comes from the network, the OS or
  the user.** `expect("reason")` is allowed for true invariants, with the
  invariant as the message.
- Network input never panics. A malformed frame is a `ProtoError` that closes
  the link, as PROTOCOL.md requires.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("control frame of {0} bytes exceeds the limit")]
    FrameTooLarge(usize),
    #[error("machine id must be 16 bytes")]
    BadMachineId,
    #[error(transparent)]
    Decode(#[from] prost::DecodeError),
}
```

### Ownership and allocation

- Borrow by default (`&str`, `&[u8]`); take ownership only when you store it.
- The input hot path (motion, keys) does not allocate per event. Reuse buffers.
- Use wrapping arithmetic explicitly where the protocol says so; never rely on
  overflow behaviour.

```rust
let dx = new.x.wrapping_sub(last.x);
```

### Async and concurrency

- `tokio` in binaries. `a7n-core` stays synchronous; it is a state machine fed
  by events.
- One task owns each piece of mutable state; others talk to it through
  channels. Avoid `Arc<Mutex<_>>` across await points.
- Every network read has a timeout taken from `a7n-proto::limits`.

### `unsafe`

Only inside `a7n-platform`, only for FFI, each block with a `// SAFETY:`
comment explaining why it is sound. Every crate except `a7n-platform` sets
`#![forbid(unsafe_code)]`.

```rust
// SAFETY: `event` was created by CGEventCreateKeyboardEvent above and is
// released exactly once by the CFRelease at the end of this function.
unsafe { CGEventPost(kCGHIDEventTap, event) };
```

### Security and privacy

- Never log key usages, characters or clipboard contents, at any level. Log
  event kinds and sizes. Key codes only behind `--trace-input`.
- Treat every peer message as hostile: check sizes before allocating, reject
  `..` and absolute paths, never follow symlinks.
- Crypto comes from audited crates (`rustls`, `quinn`, RustCrypto). Never
  implement primitives ourselves.

```rust
// Good
tracing::debug!(kind = "key", count = batch.events.len(), "input batch sent");
// Bad
tracing::debug!("sent key {:?}", key);
```

### Dependencies

Add a crate only when it saves real work and is maintained. Prefer crates
already in the workspace. Pin versions in the workspace `Cargo.toml` and use
`workspace = true` in members.

## 4. Clean code rules

1. **One function, one job.** If you need "and" to describe it, split it.
   Aim for functions under ~40 lines.
2. **Names explain intent;** comments explain *why*, never *what*.
3. **No magic numbers.** Every literal with meaning is a named constant.
4. **Early return over nesting.** Guard clauses first, happy path last.
5. **No dead code,** commented-out code or `TODO` without an issue link.
6. **Keep functions pure where possible;** push I/O to the edges.
7. **Don't repeat knowledge.** Duplicated logic is extracted; duplicated
   test setup is fine if it keeps the test readable.
8. **Small public surface.** Default to private; `pub(crate)` before `pub`.
9. **No premature abstraction.** A trait needs a second implementation or a
   test seam that actually exists.

```rust
// Bad: nested, magic number, comment says what
fn handle(frame: &[u8]) -> Result<Envelope, ProtoError> {
    // check length
    if frame.len() <= 131072 {
        if !frame.is_empty() {
            return Ok(Envelope::decode(frame)?);
        }
    }
    Err(ProtoError::FrameTooLarge(frame.len()))
}

// Good: guard clause, named limit, no noise
fn decode_control_frame(frame: &[u8]) -> Result<Envelope, ProtoError> {
    if frame.len() > limits::MAX_CONTROL_FRAME {
        return Err(ProtoError::FrameTooLarge(frame.len()));
    }
    Ok(Envelope::decode(frame)?)
}
```

```rust
// Comment explains why, not what
// The responder waits longer than the initiator so that both sides do not
// drop the link at the same instant and race on reconnect (PROTOCOL.md 9).
const RESPONDER_SILENCE_DROP: Duration = Duration::from_millis(2500);
```

## 5. Testing

### What we test

**Only the essentials.** A test must protect behaviour that would hurt if it
broke. Test:

- Protocol rules: framing limits, version checks, ordering rules (route,
  generation, layout version), wrapping arithmetic.
- Security rules: pin checks, pairing failures, path validation, sensitive
  clipboard exclusion, settings allow-list.
- State machines in `a7n-core`: focus take/let go/handover, release on end,
  idle takeover, desk merge and revocation, layout edge mapping.
- Bugs: every fixed bug gets one test that reproduces it.

**Do not test:**

- Getters, setters, constructors, `Default`, derived traits.
- Generated protobuf code or third-party crates.
- Trivial delegation or glue code with no logic.
- The same rule twice through different layers.
- OS backends in CI (they get the manual release checklist instead).

If you cannot say in one sentence what a test protects, delete it.

### Naming: `method_behavior_expectedBehavior`

Test names have three parts in `snake_case`:
`<method>_<behavior>_<expected_behavior>`: the method under test, the
situation or input it is exercised with, and what must happen.

```rust
decode_control_frame_oversized_returns_frame_too_large()
apply_motion_counter_wrapped_moves_by_small_delta()
merge_desk_revoked_member_removes_pin()
take_focus_older_route_is_ignored()
validate_manifest_path_with_parent_dir_is_rejected()
```

### Structure: Arrange, Act, Assert

Every test has three blocks marked with comments, in this order, separated by
a blank line. One act per test. Assert on behaviour, not internals.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_control_frame_oversized_returns_frame_too_large() {
        // Arrange
        let frame = vec![0u8; limits::MAX_CONTROL_FRAME + 1];

        // Act
        let result = decode_control_frame(&frame);

        // Assert
        assert!(matches!(result, Err(ProtoError::FrameTooLarge(_))));
    }

    #[test]
    fn apply_motion_counter_wrapped_moves_by_small_delta() {
        // Arrange
        let mut pointer = PointerTracker::new();
        pointer.apply(Motion { motion_seq: 1, x: i32::MAX - 2, y: 0, ..Default::default() });

        // Act
        let delta = pointer.apply(Motion { motion_seq: 2, x: i32::MIN + 2, y: 0, ..Default::default() });

        // Assert
        assert_eq!(delta, Some((5, 0)));
    }

    #[test]
    fn end_ownership_keys_held_releases_all() {
        // Arrange
        let mut session = DrivenSession::new(owner_id());
        session.press(Key::A);
        session.press(Key::LeftShift);

        // Act
        let released = session.end_ownership();

        // Assert
        assert_eq!(released, vec![Key::A, Key::LeftShift]);
    }
}
```

### Test style rules

- Unit tests live next to the code in `#[cfg(test)] mod tests`. Cross-crate
  protocol tests live in `crates/<crate>/tests/` and use simulated in-process
  peers, never real input or real network.
- Tests are deterministic: inject clocks and randomness, never `sleep`.
- Small helper builders (`owner_id()`, `layout_of(...)`) are fine; shared
  fixtures that hide what a test depends on are not.
- Use table-driven tests when the same rule has several inputs:

```rust
#[test]
fn validate_manifest_path_unsafe_paths_are_rejected() {
    // Arrange
    let unsafe_paths = ["../etc/passwd", "/abs/path", "a/../../b", "CON"];

    for path in unsafe_paths {
        // Act
        let result = validate_manifest_path(path);

        // Assert
        assert!(result.is_err(), "{path} should be rejected");
    }
}
```

## 6. Workflow

### Branching: scaled trunk-based development

- `main` is the trunk and is always releasable. Nobody pushes to it directly.
- Work happens on **short-lived branches** (at most 1–2 days) that merge into
  `main` through a pull request. Branch names: `<type>/<issue>-<slug>`, e.g.
  `feat/12-quic-handshake`.
- A PR is small enough to review in one sitting. Split large features into
  several PRs (sub-issues) rather than keeping a long-running branch.
- **Unfinished features merge behind a flag** (a Cargo feature or a config
  switch that defaults to off), so `main` stays shippable.
- PRs merge by **squash merge** only, once CI is green. The PR title becomes
  the commit on `main`, so it must be a valid Conventional Commit.
- **Releases** branch off `main` as `release/vX.Y` when a milestone is
  complete. Fixes land on `main` first and are cherry-picked to the release
  branch; nothing is developed on a release branch.
- Every PR links its issue (`Closes #12`). Features are tracked as GitHub
  issues grouped by milestone (`v0.1`, `v0.2`, ...).

### Commits: Conventional Commits

Format: `<type>(<scope>): <description>`, imperative, lower case, no period.

| Type | Use for |
|---|---|
| `feat` | new user-visible behaviour |
| `fix` | bug fix |
| `perf` | performance improvement |
| `refactor` | code change without behaviour change |
| `test` | adding or fixing tests |
| `docs` | documentation only |
| `build` | Cargo, dependencies, packaging |
| `ci` | CI configuration |
| `chore` | anything else that touches no product code |

Scopes: `proto`, `ipc`, `core`, `platform`, `daemon`, `cli`, `gui`, `docs`,
`ci`. Omit the scope when a change spans many crates.

A change that breaks the wire protocol or the config format adds `!` after
the scope and a `BREAKING CHANGE:` footer.

```text
feat(core): take focus on idle takeover after 3 s
fix(proto): reject control frames above 128 KiB
refactor(platform): extract key mapping from the macOS backend
feat(proto)!: replace Arrangement with desk-wide Layout

BREAKING CHANGE: peers running protocol v1 drafts before this change cannot
exchange layouts.
```

Every commit is signed off (`git commit -s`, DCO). No CLA.

### Before you open a PR

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

- Changing the wire format means changing `apeirogon.proto` **and**
  `PROTOCOL.md` in the same PR, following the schema's evolution rules
  (never reuse or renumber a field).
- A change that contradicts `docs/design.md` updates the design first, or is
  not made.
