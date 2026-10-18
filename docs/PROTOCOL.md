# Apeirogon Link Protocol v1 (draft)

Schema: [`proto/apeirogon/v1/apeirogon.proto`](../proto/apeirogon/v1/apeirogon.proto) (package `apeirogon.v1`). This document defines everything the schema cannot: transport, identity, pairing, timing and behaviour. Product-level decisions live in [`design.md`](design.md); where this spec and the design disagree, the design wins and this spec is corrected.

Keywords MUST / SHOULD / MAY as in RFC 2119.

## 1. Overview

Every pair of machines on a desk keeps one **control link** (full mesh). One side is the **initiator**, the other the **responder**. At any moment exactly one machine **owns** input: all physical input on the desk goes to the machine holding the cursor (single global focus). Routing messages (`Focus`, `Accept`, `Refuse`, `Handover`) move ownership. Large data (files, big clipboard items) never travels on the control stream; it uses **transfer streams** so that input latency is unaffected.

All traffic between two machines uses **one QUIC connection** (RFC 9000) with the DATAGRAM extension (RFC 9221). There is no TCP profile.

| Channel | Carrier | Reliability | ALPN |
|---|---|---|---|
| Control (keys, buttons, scroll, routing, metadata) | bidirectional stream 0 | reliable, ordered | `a7n/1` |
| Motion | QUIC datagrams | unreliable | `a7n/1` |
| Transfer (clipboard data, files) | one unidirectional stream per transfer, opened by the source | reliable | `a7n/1` |
| Pairing | separate QUIC connection, bidirectional stream 0 | reliable, ordered | `a7n-pair/1` |

Scope is the local network. Default UDP port: **42042** for both `a7n/1` and `a7n-pair/1` (distinguished by ALPN); configurable. If the port is taken, the daemon MAY bind another and advertise it through mDNS and `PeerInfo.listen_port`.

### 1.1 Discovery

Machines advertise over mDNS / DNS-SD as service `_apeirogon._udp` with TXT keys `id` (hex machine id) and `v` (protocol major version). Paired machines are found by id; unpaired ones are listed for `a7n pair`. A machine may also be addressed by IP.

## 2. Framing

The control stream, the pairing stream and transfer streams carry `Envelope` / `PairingMessage` / `TransferFrame` messages, each prefixed by a protobuf varint length. A datagram carries exactly one `Envelope` with no length prefix; receivers MUST ignore datagrams whose body is not `Motion`.

- Control stream: max frame 128 KiB. Larger means protocol violation, close the link.
- Transfer stream: max frame 512 KiB (chunks are at most 256 KiB).
- Frame read timeout once the first byte arrived: 30 s.
- Unknown fields and unknown `oneof` cases MUST be ignored. A receiver MUST NOT close a link because of an unknown case.

## 3. Identity and encryption

Encryption is mandatory. There is no plaintext mode.

### 3.1 Certificates

QUIC uses TLS 1.3 with **mutual authentication and self-signed per-device certificates**. No CA exists or is needed.

- On first start each device generates a key pair (Ed25519, or ECDSA P-256 where Ed25519 is unavailable) and a self-signed X.509 certificate. Validity: long (for example 100 years). Expiry is not a security mechanism here.
- The **machine id** is the first 16 bytes of the SHA-256 of the certificate's SubjectPublicKeyInfo. It is stable across certificate re-issue with the same key. Being derived from the key, it cannot be claimed by another device and needs no separate binding to the pin; a new key means a new id and pairing again.
- The **pin** is the SHA-256 of the full certificate DER. Pairing and desk membership store the peer's pin.
- Both sides present certificates. Each side accepts a peer only if the certificate's pin equals a stored pin. Hostname, validity dates and chain are ignored. No 0-RTT.
- Revoking a peer = deleting its pin (and announcing a `DeskRevocation`, section 6). Replacing a device = pair again.

### 3.2 What an observer sees

Initial packets reveal the ALPN; with TLS 1.3 both certificates are encrypted. The observer learns timing and sizes, and that Apeirogon is in use.

## 4. Connection and handshake

1. Initiator opens a QUIC connection with ALPN `a7n/1`, presents its certificate.
2. Responder checks the pin; unknown pin: close with TLS alert `bad_certificate`. No further information is given.
3. Initiator opens stream 0 and sends `Hello{protocol_version=1, info}`.
4. Responder replies `Welcome{protocol_version, info, accepts_drive}`. On a major-version mismatch it sets `error = UNSUPPORTED_VERSION`; both sides then close and tell the user which machine runs the older version (by `info.name`) and needs updating.
5. Both sides compute the capability intersection from `PeerInfo.capabilities`. Capabilities are the minor-version features: one is used only if both sides list it.
6. Both send `Desk` and `Layout` (sections 6 and 7).

Limits: handshake timeout 5 s, 4 concurrent unauthenticated connections per listener, first `Hello` max 4096 bytes. If both machines dial each other simultaneously, the link initiated by the larger machine id is kept.

## 5. Pairing

ALPN `a7n-pair/1`. Goal: exchange pins with a human-checked secret and defeat a man in the middle even though neither certificate is known yet.

1. `a7n pair` on the **host** shows a 6-digit code, valid 60 s, accepting at most one guess per attempt. Optionally a QR code adds a 128-bit secret (`PairStart.qr = true`).
2. The **requester** (the user types the code there) connects with ALPN `a7n-pair/1`. Certificates are presented but not verified (neither side has a pin).
3. Both derive `ctx = TLS-Exporter("a7n-pair/1 binding", 32 bytes)`.
4. Run **SPAKE2** (RFC 9382, suite edwards25519 / SHA-256 / HKDF / HMAC) with password = code (concatenated with the QR secret if present), identity A = requester pin, identity B = host pin, and `ctx` as additional authenticated data. Messages: `PairStart` (pA), `PairAnswer` (pB + host confirmation MAC), `PairConfirm` (requester confirmation MAC), `PairDone` (host MAC over `"stored"` with the confirmation key).
5. Because the pins are the SPAKE2 identities and `ctx` is bound in, a man in the middle presenting different certificates on each leg yields different transcripts and the MACs cannot match.
6. On success each side stores the other's pin, name and machine id. `PairDone.tag` proves the host stored the pin, so the requester only stores after receiving it.
7. The host then admits the requester to its desk (section 6).

Failure handling: every failure sends `PAIR_ERROR_REFUSED` without saying which check failed. A wrong code ends pairing mode; the user must restart it. Rate limit: 5 pairing attempts per minute per source address.

## 6. Desk membership

Pairing any machine with any member of a desk admits it to the whole desk; the user pairs once per new machine, not once per pair.

- After a successful pairing the host creates a `DeskMember` for the new machine, signed with its certificate key, and sends `Desk` to every connected member and to the new machine.
- Signature input: the ASCII string `a7n desk member v1`, a zero byte, then `id || pin || u16be(len(name)) || name || introduced_by || u64be(added_at)`. Revocations sign `a7n desk revoke v1`, a zero byte, then `id || revoked_by || u64be(revoked_at)`.
- A receiver accepts a `DeskMember` only if `introduced_by` is a machine it already trusts and the signature verifies against that machine's pinned certificate key. Accepted members' pins are stored, and the receiver may dial them.
- `a7n unpair <machine>` creates a `DeskRevocation`. A revocation verified against a trusted member's key deletes the pin of `id` and drops any link to it. It overrides every `DeskMember` for that id with `added_at <= revoked_at`; re-admitting requires pairing again.
- Merge rule: union of members and revocations, then apply revocations. `Desk` is sent after every handshake, so members that were offline catch up.

A paired machine is trusted to inject input; any member can admit or revoke others. Per-peer `allow_drive` and clipboard/file policy (section 13) limit what each member may do.

## 7. Layout

The layout is a 2D canvas on which each machine's whole virtual desktop (all its monitors) is one rectangle (`MachineRect`). Edges connect only where rectangles touch, so offsets and partial overlaps work. Individual monitors are not placed separately.

- One `Layout` describes the whole desk and is sent to every member.
- A local edit sets `version = max(seen) + 1`, `by = own id`. The newest `(version, by)` wins; a machine whose unsent edit loses shows a notice.
- When the pointer leaves the owner's current machine across a shared edge segment, the owner computes `Landing` (edge and offset) on the target from the layout.

## 8. Ownership and routing

- `route` is a number that rises with every move. Messages with an older route are ignored.
- **Take**: owner sends `Focus{target = responder id}` with an optional `Landing`. Responder answers `Accept{route}` or `Refuse{route, reason, other}`.
- **Let go**: `Focus` with another target. The responder releases all held keys and buttons.
- **Handover**: the responder asks its owner to send input onward or home (`next` = owner id).
- **Idle takeover**: an owner that has sent nothing for 3 s can be replaced by another peer's `Focus` (someone started typing on a different machine). The old owner learns this through `Handover.taken_over_by`.
- **Release on end**: whenever ownership ends for any reason (link loss, takeover, let go) the responder MUST synthesise key-up and button-up for everything still held.
- **Chains**: A drives B; B sends `Handover{next = C}` to A; A takes C directly. There is no relay through B.
- **Hotkeys** (handled locally by the owner): return home, jump to machine N (a direct take), and lock the cursor to the current machine.

## 9. Input

- `InputBatch` is reliable and ordered on the control stream. Every batch carries the cumulative pointer counters so the pointer is resynchronised before events apply.
- `Motion` is sent as a datagram between batches. The receiver drops a `Motion` whose `motion_seq` is not newer (wrapping comparison) and applies `new - last` with 32-bit wrapping arithmetic.
- Motion values are relative deltas before the sender's acceleration where the OS exposes them; the receiver injects relative motion so its own pointer speed and acceleration apply, scaled by a per-machine multiplier from local config.
- Keys are USB HID usages, so layouts need not match; the receiver applies its own layout. `character` is advisory for platforms that cannot synthesise from a usage. Per-target modifier remaps (for example ⌘ → Ctrl) are applied by the sender before encoding.
- `Ack{seq, held_us}`: the sender computes `rtt = now - sent_at - held_us`.
- Timing: `Ping` every 1 s. The initiator drops the link after 2 s of silence, the responder after 2.5 s. An `InputBatch` unacknowledged for 1.5 s also drops the link.
- `Ack.locked = true`: the responder is on a lock screen and drops input; the owner SHOULD take focus home. Login screens and secure desktops are not driven in v1.

## 10. Clipboard

The clipboard is *live*: every copy is announced; data moves when someone pastes.

- Every local clipboard change creates a new generation: `generation = max(seen) + 1`, origin = own id.
- The machine sends `ClipboardAnnounce` to every connected peer whose policy allows it. Order: higher `generation` wins, tie broken by larger `origin`.
- **Sensitive content is never announced.** The source MUST check the OS markers (macOS `org.nspasteboard.ConcealedType`, Windows `ExcludeClipboardContentFromMonitorProcessing`, KDE `x-kde-passwordManagerHint`) and the local app deny-list; marked content creates no announce. This check cannot be disabled.
- Small items are pushed eagerly in `inline_data`, as long as the announce's total inline bytes stay at or below 64 KiB. Other items carry only `size` and `sha256` and are **fetched on paste**: the OS paste request on the receiving machine triggers `TransferRequest{clipboard = (origin, generation), item_index}`. Where the OS cannot delay a paste, the receiver fetches eagerly when the item is below a configurable limit.
- Text is offered as `text/plain;charset=utf-8` and, when the source has it, `text/html;charset=utf-8`.
- Limits (defaults): text 256 KiB, image 8 MiB with validation before decode (decompression-bomb checks). Files are limited only by policy.
- A source that no longer holds the generation answers `ABORT_REASON_NOT_FOUND`.
- Files: the item with MIME `application/x-apeirogon-files` carries a `FileManifest`. Paths are relative, `/`-separated, NFC normalised, and the receiver MUST reject `..`, absolute paths, and reserved names.

## 11. File transfer

- The **sink** sends `TransferRequest` on the control stream. The source opens a unidirectional stream on the same connection.
- The source writes `TransferOpen{transfer_id}`, then per file `FileChunk` frames (offset based, at most 256 KiB), `FileEnd{sha256}`, finally `TransferDone`.
- The sink verifies every file's SHA-256 against the manifest. A mismatch aborts with `ABORT_REASON_INTEGRITY` and the partial file is deleted.
- **Resume**: `TransferRequest.resume` lists `(file_index, offset)`; the source seeks. The sink MUST re-hash the partial file before trusting it.
- `a7n send <peer> <path…>` creates a manifest and an offer exactly like a clipboard announce with a private generation.
- Flow control: provided by QUIC. At most 4 concurrent transfers per link. Transfer streams SHOULD get lower priority than stream 0.

## 12. Drag and drop

1. The source OS starts a drag containing files and the pointer reaches an edge with a peer behind it. The source takes focus on the sink as usual (`Focus`) and sends `DragOffer{drag_id, files, items}`.
2. The sink starts a synthetic OS drag session using promised files (no data yet) and shows the drag.
3. The owner continues sending input. The drop is the button-up on the sink; the sink resolves the promise by sending `TransferRequest{drag_id}`.
4. If the pointer returns or the user cancels, the source sends `DragEnd{CANCELLED}`; the sink tears down its drag. An unused offer expires after 60 s (`EXPIRED`).
5. Platform notes: macOS (NSDraggingSession with file promises), Windows (IDataObject with delay-rendered CFSTR_FILEDESCRIPTOR / FILECONTENTS), Linux X11 (XDND), Wayland (drag via the compositor, limited by portal support). This is the riskiest component; the protocol only fixes the exchange.

## 13. Settings sync and policy

- `SettingsSync` shares convenience settings (hotkeys, speed multipliers and the like) across the desk. Receivers MUST apply only keys on a fixed allow-list and ignore everything else.
- Security settings are never synced and never changeable by a peer: `allow_drive`, per-peer clipboard and file policy, the clipboard app deny-list, and the sensitive-content check.
- Per peer, the user controls `allow_drive` (announced with `Accepts`) and clipboard and file policy (receive, send, ask).

## 14. Limits and defaults

| Item | Value |
|---|---|
| Default UDP port | 42042 |
| Handshake timeout | 5 s |
| Unauthenticated slots | 4 |
| Control frame max | 128 KiB |
| Transfer frame max | 512 KiB |
| Chunk max | 256 KiB |
| Ping interval | 1 s |
| Silence drop | 2 s / 2.5 s |
| Unacked input drop | 1.5 s |
| Idle takeover | 3 s |
| Pairing code | 6 digits, 60 s, 1 guess |
| Pairing attempts | 5 per minute per source |
| Concurrent transfers | 4 per link |
| Text clipboard | 256 KiB |
| Image clipboard | 8 MiB |
| Inline clipboard total per announce | 64 KiB |

## 15. Security notes

- A paired peer is trusted to inject input. A stolen device can type on every other machine; the user needs a per-peer `allow_drive` toggle and a quick revoke (`a7n unpair`, propagated desk-wide).
- Sinks write received files only below a dedicated directory unless the user picks a target; never overwrite, never follow symlinks from the manifest.
- Keep a hard cap on resources per unauthenticated connection.
- Logs never contain key usages or clipboard contents (see design.md).

## 16. Encoding size

Serialised protobuf `Envelope` including the 1-byte length prefix (datagrams carry no prefix, so `Motion` is one byte smaller), measured with small counter values, against compact JSON frames of the form `{"type","data"}` (field names approximated):

| Message | Protobuf | JSON | Ratio |
|---|---|---|---|
| Motion | 13 B | 63 B | 4.8x |
| InputBatch with one key | 25 B | 112 B | 4.5x |
| Ack | 8 B | 46 B | 5.8x |
| Ping | 5 B | 34 B | 6.8x |

Large counter values (late in a long session) add up to about 4 bytes per message. QUIC packet overhead narrows the real gap. The more important gain is the hot path: no JSON parsing and no allocation per mouse event.

## 17. Open questions

1. Wayland drag and drop and clipboard delayed rendering.
