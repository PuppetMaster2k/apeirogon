# Apeirogon Link Protocol v1 (draft)

Schema: `apeirogon.proto` (package `apeirogon.v1`). This document defines everything the schema cannot: transport, identity, pairing, timing and behaviour. Derived from the design of Beamer wire v6, with changes noted as *(differs from Beamer)*.

Keywords MUST / SHOULD / MAY as in RFC 2119.

## 1. Overview

Every pair of paired machines keeps one **control link**. One side is the **initiator**, the other the **responder**. At any moment at most one machine **owns** input and drives another; routing messages (`Focus`, `Accept`, `Refuse`, `Handover`) move ownership. Large data (files, big clipboard items) never travels on the control link; it uses a **transfer channel** so that input latency is unaffected.

| Channel | Profile T (TCP) | Profile Q (QUIC) | ALPN |
|---|---|---|---|
| Control | TLS 1.3 over TCP | bidirectional stream 0 | `a7n/1` |
| Motion | on control link | QUIC datagrams | `a7n/1` |
| Transfer | extra TLS connection | unidirectional stream per transfer | `a7n-xfer/1` (T) |
| Pairing | TLS over TCP | separate connection | `a7n-pair/1` |

Profile T is mandatory. Profile Q is optional and negotiated through `CAPABILITY_MOTION_DATAGRAM`. Default port: to be assigned (Beamer uses 24820/24821; do not reuse).

## 2. Framing

Control and transfer streams carry `Envelope` / `TransferFrame` messages, each prefixed by a protobuf varint length.

- Control link: max frame 64 KiB. Larger means protocol violation, close the link.
- Transfer channel: max frame 512 KiB (chunks are at most 256 KiB).
- Frame read timeout once the first byte arrived: 30 s.
- Unknown fields and unknown `oneof` cases MUST be ignored. A receiver MUST NOT close a link because of an unknown case.

## 3. Identity and encryption

### 3.1 Certificates *(differs from Beamer)*

Beamer authenticates with a pre-shared key id plus its own AEAD framing. Apeirogon uses standard TLS 1.3 with **mutual authentication and self-signed per-device certificates**. No CA exists or is needed.

- On first start each device generates a key pair (Ed25519, or ECDSA P-256 where Ed25519 is unavailable) and a self-signed X.509 certificate. Validity: long (for example 100 years). Expiry is not a security mechanism here.
- The **machine id** is the first 16 bytes of the SHA-256 of the certificate's SubjectPublicKeyInfo. It is stable across certificate re-issue with the same key.
- The **pin** is the SHA-256 of the full certificate DER. Pairing stores the peer's pin.
- Both sides present certificates (mutual TLS). Each side accepts a peer only if the certificate's pin equals a stored pin. Hostname, validity dates and chain are ignored. TLS 1.3 only, no session resumption with 0-RTT.
- Revoking a peer = deleting its pin. Replacing a device = pair again.

Benefits over Beamer's scheme: the peer is identified inside the handshake (no key id in the clear), standard audited implementations, forward secrecy, and QUIC support for free. Costs: certificate parsing code and a TLS dependency on every platform; mitigated by taking the library from the platform language (rustls, Go crypto/tls, and so on) and accepting only a fixed profile.

### 3.2 What an observer sees

Initial packets reveal the ALPN and, with TLS 1.3, the server certificate is already encrypted. The client certificate is encrypted too. The observer learns timing and sizes, and that Apeirogon is in use.

## 4. Connection and handshake

1. Initiator opens TLS (or QUIC) with ALPN `a7n/1`, presents its certificate.
2. Responder checks the pin; unknown pin: close with TLS alert `bad_certificate`. No further information is given.
3. Initiator sends `Hello{protocol_version=1, info}`.
4. Responder replies `Welcome{protocol_version, info, accepts_drive}`. On mismatch it sets `error`; both sides then close.
5. Both sides compute the capability intersection from `PeerInfo.capabilities`. A feature is used only if both sides list it.
6. Both send `PairedPeers` and `Arrangement` for known peers; the newest `(set_at, by)` wins.

Limits: handshake timeout 5 s, 4 concurrent unauthenticated connections per listener, first `Hello` max 4096 bytes. If both machines dial each other simultaneously, the link initiated by the larger machine id is kept.

## 5. Pairing

Alias ALPN `a7n-pair/1`. Goal: exchange pins with a human-checked secret and defeat a man in the middle even though neither certificate is known yet.

1. **Host** shows a 6-digit code, valid 60 s, accepting at most one guess per attempt. Optionally a QR code adds a 128-bit secret (`PairStart.qr = true`).
2. **Requester** connects over TLS. Certificates are not verified at this point (neither side has a pin).
3. Both derive `ctx = TLS-Exporter("a7n-pair/1 binding", 32 bytes)` plus the two certificate pins (requester first).
4. Run **CPace** with password = code (and QR secret if present) and `ctx` as session id. Messages: `PairStart` (requester share), `PairAnswer` (host share + confirmation tag Tb), `PairConfirm` (requester tag Ta), `PairDone` (host tag Td).
5. Tags are MACs over the transcript, `ctx` and both pins. A man in the middle presents different certificates on each leg, so the pins and exporters differ and the tags cannot match.
6. On success each side stores the other's pin, name and machine id. `PairDone.tag` proves the host stored the pin, so the requester only stores after receiving it.

Failure handling: every failure sends `PAIR_ERROR_REFUSED` without saying which check failed. A wrong code ends pairing mode; the user must restart it. Rate limit: 5 pairing attempts per minute per source address.

Discovery of unpaired machines (mDNS or UDP broadcast) is out of scope for v1 of this spec; the host may also be addressed by IP.

## 6. Ownership and routing

Semantics follow Beamer; only the wire format changed.

- `route` is a number that rises with every move. Messages with an older route are ignored.
- **Take**: owner sends `Focus{target = responder id}` with an optional `Landing`. Responder answers `Accept{route}` or `Refuse{route, reason, other}`.
- **Let go**: `Focus` with another target. The responder releases all held keys and buttons.
- **Handover**: the responder asks its owner to send input onward or home (`next` = owner id).
- **Idle takeover**: an owner that has sent nothing for 3 s can be replaced by another peer's `Focus`. The old owner learns this through `Handover.taken_over_by`.
- **Release on end**: whenever ownership ends for any reason (link loss, takeover, let go) the responder MUST synthesise key-up and button-up for everything still held.
- **Chains**: A drives B; B sends `Handover{next = C}` to A; A takes C directly. There is no relay through B. Full-mesh links.

## 7. Input

- `InputBatch` is reliable and ordered. Every batch carries the cumulative pointer counters so the pointer is resynchronised before events apply.
- `Motion` is sent between batches. On Profile T it travels on the control stream (cheap, 13 bytes). On Profile Q it is a datagram. The receiver drops a `Motion` whose `motion_seq` is not newer (wrapping comparison) and applies `new - last` with 32-bit wrapping arithmetic.
- Keys are USB HID usages, so layouts need not match. `character` is advisory for platforms that cannot synthesise from a usage.
- `Ack{seq, held_us}`: the sender computes `rtt = now - sent_at - held_us`.
- Timing: `Ping` every 1 s. The initiator drops the link after 2 s of silence, the responder after 2.5 s. An `InputBatch` unacknowledged for 1.5 s also drops the link.
- `Ack.locked = true`: the responder is on a lock screen and drops input; the owner SHOULD take focus home.

## 8. Clipboard

Differs from Beamer: Beamer moves clipboard content only at the switch moment. Apeirogon makes it *live*.

- Every local clipboard change creates a new generation: `generation = max(seen) + 1`, origin = own id.
- The machine sends `ClipboardAnnounce` to every connected peer whose policy allows it. Order: higher `generation` wins, tie broken by larger `origin`.
- Items of at most 64 KiB carry `inline_data`. Larger items carry only `size` and `sha256` and are **fetched on paste**: the OS paste request on the receiving machine triggers `TransferRequest{clipboard = (origin, generation), item_index}`. Where the OS cannot delay a paste (older platforms), the receiver fetches eagerly when the item is below a configurable limit.
- Limits (defaults, as in Beamer): text 256 KiB, image 8 MiB with validation before decode (decompression-bomb checks). Files are limited only by policy.
- A source that no longer holds the generation answers `ABORT_REASON_NOT_FOUND`.
- Files: the item with MIME `application/x-apeirogon-files` carries a `FileManifest`. Paths are relative, `/`-separated, NFC normalised, and the receiver MUST reject `..`, absolute paths, and reserved names.

## 9. File transfer

- The **sink** sends `TransferRequest` on the control link. In Profile T the sink then dials the source's listen port with ALPN `a7n-xfer/1` (same certificates, same pin check) and the source sends the data on it. If the sink cannot reach the source (`listen_port = 0` or a firewall), the source dials the sink instead. In Profile Q the source opens a unidirectional stream.
- The source writes `TransferOpen{transfer_id}`, then per file `FileChunk` frames (offset based, at most 256 KiB), `FileEnd{sha256}`, finally `TransferDone`.
- The sink verifies every file's SHA-256 against the manifest. A mismatch aborts with `ABORT_REASON_INTEGRITY` and the partial file is deleted.
- **Resume**: `TransferRequest.resume` lists `(file_index, offset)`; the source seeks. The sink MUST re-hash the partial file before trusting it.
- `a7n send <peer> <path…>` creates a manifest and an offer exactly like a clipboard announce with a private generation.
- Flow control: provided by TCP/QUIC. At most 4 concurrent transfers per link.

## 10. Drag and drop

1. The source OS starts a drag containing files and the pointer reaches an edge with a peer behind it. The source takes focus on the sink as usual (`Focus`) and sends `DragOffer{drag_id, files, items}`.
2. The sink starts a synthetic OS drag session using promised files (no data yet) and shows the drag.
3. The owner continues sending input. The drop is the button-up on the sink; the sink resolves the promise by sending `TransferRequest{drag_id}`.
4. If the pointer returns or the user cancels, the source sends `DragEnd{CANCELLED}`; the sink tears down its drag. An unused offer expires after 60 s (`EXPIRED`).
5. Platform notes: macOS (NSDraggingSession with file promises), Windows (IDataObject with delay-rendered CFSTR_FILEDESCRIPTOR / FILECONTENTS), Linux X11 (XDND), Wayland (drag via the compositor, limited by portal support). This is the riskiest component; the protocol only fixes the exchange.

## 11. Limits and defaults

| Item | Value |
|---|---|
| Handshake timeout | 5 s |
| Unauthenticated slots | 4 |
| Control frame max | 64 KiB |
| Transfer frame max | 512 KiB |
| Chunk max | 256 KiB |
| Ping interval | 1 s |
| Silence drop | 2 s / 2.5 s |
| Unacked input drop | 1.5 s |
| Idle takeover | 3 s |
| Pairing code | 6 digits, 60 s, 1 guess |
| Text clipboard | 256 KiB |
| Image clipboard | 8 MiB |
| Inline clipboard item | 64 KiB |

## 12. Security notes

- A paired peer is trusted to inject input. A stolen device can type on every other machine; the user needs a per-peer `allow_drive` toggle and a quick revoke.
- Clipboard and file policy are separate per peer (receive, send, ask).
- Sinks write received files only below a dedicated directory unless the user picks a target; never overwrite, never follow symlinks from the manifest.
- Keep a hard cap on resources per unauthenticated connection.
- Beamer's documented weakness (key id in the clear) disappears with TLS 1.3 and client certificates.

## 13. Size comparison (measured)

Serialised protobuf `Envelope` including the 1-byte length prefix, against compact JSON in the style of Beamer's `{"type","data"}` frames (the JSON field names are my approximation, not copied from Beamer):

| Message | Protobuf | JSON | Ratio |
|---|---|---|---|
| Motion | 13 B | 63 B | 4.8x |
| InputBatch with one key | 25 B | 112 B | 4.5x |
| Ack | 8 B | 46 B | 5.8x |
| Ping | 5 B | 34 B | 6.8x |

The AEAD or TLS record overhead (about 22 bytes per record in TLS 1.3 with a 16-byte tag) is added to both and narrows the real gap. Motion batching into the next `InputBatch` or record keeps this low. The more important gain is not bytes but the hot path: no JSON parsing and no allocation per mouse event.

## 14. Open questions

1. Host/client versus peer-to-peer: this draft keeps Beamer's per-pair links with an owner role.
2. QUIC library maturity per platform (Profile Q is optional for that reason).
3. Wayland drag and drop and clipboard delayed rendering.
4. Port numbers and mDNS service name.
5. Whether the machine id should be a hash of the key (as here) or a random id bound to the pin.
