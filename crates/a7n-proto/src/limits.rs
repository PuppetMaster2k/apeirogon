// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every number from PROTOCOL.md sections 2, 4, 9 and 14. Nothing else in the
//! workspace repeats these values.

use std::time::Duration;

/// Default UDP port (section 14).
pub const DEFAULT_UDP_PORT: u16 = 42042;
/// Time allowed for TLS plus `Hello`/`Welcome` (section 4).
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Concurrent unauthenticated connections per listener (section 4).
pub const UNAUTHENTICATED_SLOTS: usize = 4;
/// Largest first `Hello` frame in bytes (section 4).
pub const FIRST_HELLO_MAX: usize = 4096;
/// Largest control or pairing frame in bytes (section 2).
pub const CONTROL_FRAME_MAX: usize = 128 * 1024;
/// Largest transfer frame in bytes (section 2).
pub const TRANSFER_FRAME_MAX: usize = 512 * 1024;
/// Largest file chunk payload in bytes (section 2).
pub const CHUNK_MAX: usize = 256 * 1024;
/// Time to finish reading a frame once its first byte arrived (section 2).
pub const FRAME_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Interval between `Ping` messages (section 9).
pub const PING_INTERVAL: Duration = Duration::from_secs(1);
/// Silence after which the initiator drops the link (section 9).
pub const INITIATOR_SILENCE_DROP: Duration = Duration::from_secs(2);
/// Silence after which the responder drops the link (section 9). Longer than
/// the initiator's so both sides do not drop at the same instant and race on
/// reconnect.
pub const RESPONDER_SILENCE_DROP: Duration = Duration::from_millis(2500);
/// An `InputBatch` unacknowledged for this long drops the link (section 9).
pub const UNACKED_INPUT_DROP: Duration = Duration::from_millis(1500);
/// Wait for `Accept`/`Refuse` before taking focus home (section 8).
pub const TAKE_TIMEOUT: Duration = Duration::from_secs(1);
/// Owner silence after which another peer may take over (section 8).
pub const IDLE_TAKEOVER: Duration = Duration::from_secs(3);
/// Digits in a pairing code (section 14).
pub const PAIRING_CODE_LENGTH: usize = 6;
/// Lifetime of a pairing code (section 14).
pub const PAIRING_CODE_DURATION: Duration = Duration::from_secs(60);
/// Guesses allowed per pairing code (section 14).
pub const PAIRING_CODE_GUESSES: usize = 1;
/// Pairing attempts per minute from one source (section 14).
pub const PAIRING_ATTEMPTS_PER_MINUTE_PER_SOURCE: usize = 5;
/// Concurrent transfers per link (section 14).
pub const CONCURRENT_TRANSFERS_PER_LINK: usize = 4;
/// Largest text clipboard item in bytes (section 14).
pub const TEXT_CLIPBOARD_MAX: usize = 256 * 1024;
/// Largest image clipboard item in bytes (section 14).
pub const IMAGE_CLIPBOARD_MAX: usize = 8 * 1024 * 1024;
/// Inline clipboard bytes per announce, summed over items (section 14).
pub const INLINE_CLIPBOARD_TOTAL_PER_ANNOUNCE: usize = 64 * 1024;
