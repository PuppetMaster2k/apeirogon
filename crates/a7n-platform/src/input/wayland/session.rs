// SPDX-License-Identifier: GPL-3.0-or-later

//! The two portal sessions of a Wayland desktop and their libei connections.
//!
//! `InputCapture` captures local input at pointer barriers. `RemoteDesktop`
//! injects input from peers. The clipboard portal (#22) will extend the
//! `RemoteDesktop` session: it must call `Clipboard.RequestClipboard` on
//! [`DesktopSession::session`] before [`open_desktop`] starts it, so that
//! function is where that hook belongs.

use std::os::unix::net::UnixStream;

use ashpd::Error as PortalFailure;
use ashpd::desktop::input_capture::{
    Capabilities, ConnectToEISOptions as CaptureEisOptions, CreateSession2Options,
    CreateSessionOptions as CreateCaptureOptions, InputCapture, StartOptions as CaptureStart,
};
use ashpd::desktop::remote_desktop::{
    ConnectToEISOptions as DesktopEisOptions, DeviceType, RemoteDesktop, SelectDevicesOptions,
    StartOptions as DesktopStart,
};
use ashpd::desktop::{CreateSessionOptions, PersistMode, ResponseError, Session};
use reis::ei::{self, handshake::ContextType};
use reis::event::Connection;
use reis::tokio::EiConvertEventStream;

use super::tokens::{TokenKind, TokenStore};
use crate::input::InputError;

/// Name we give the EIS server in the libei handshake.
const EI_CLIENT_NAME: &str = "apeirogon";
const CAPTURE_PERMISSION: &str = "Wayland input capture";
const INJECT_PERMISSION: &str = "Wayland input injection";

/// A libei connection after its handshake.
pub(super) struct Eis {
    pub(super) connection: Connection,
    pub(super) events: EiConvertEventStream,
}

pub(super) struct CaptureSession {
    pub(super) portal: InputCapture,
    pub(super) session: Session<InputCapture>,
    pub(super) eis: Eis,
}

pub(super) struct DesktopSession {
    // Kept for the clipboard, which talks to the portal through this session.
    #[allow(dead_code, reason = "used by the clipboard backend (#22)")]
    pub(super) portal: RemoteDesktop,
    pub(super) session: Session<RemoteDesktop>,
    pub(super) eis: Eis,
}

/// Creates and starts the `InputCapture` session, which may show the one-time
/// consent dialog, and connects to its EIS.
///
/// # Errors
/// A declined dialog is [`InputError::MissingPermission`]; a missing portal or
/// compositor failure is [`InputError::Portal`].
pub(super) async fn open_capture(
    tokens: &mut Option<TokenStore>,
) -> Result<CaptureSession, InputError> {
    let fail = |error| portal_error(CAPTURE_PERMISSION, error);
    let portal = InputCapture::new().await.map_err(fail)?;
    let capabilities = Capabilities::Keyboard | Capabilities::Pointer;
    let session = match portal
        .create_session2(CreateSession2Options::default())
        .await
    {
        Ok(session) => {
            let options = CaptureStart::default()
                .set_capabilities(capabilities)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(restore_token(tokens.as_ref(), TokenKind::InputCapture));
            let response = portal
                .start(&session, None, options)
                .await
                .and_then(|request| request.response())
                .map_err(fail)?;
            remember(tokens, TokenKind::InputCapture, response.restore_token());
            session
        }
        // Version 1 portals (GNOME so far) start the session implicitly and
        // cannot persist consent: they prompt on every daemon start.
        Err(PortalFailure::RequiresVersion(..)) => {
            let options = CreateCaptureOptions::default().set_capabilities(capabilities);
            let (session, _granted) = portal.create_session(None, options).await.map_err(fail)?;
            session
        }
        Err(error) => return Err(fail(error)),
    };
    let fd = portal
        .connect_to_eis(&session, CaptureEisOptions::default())
        .await
        .map_err(fail)?;
    let eis = handshake(fd, ContextType::Receiver).await?;
    Ok(CaptureSession {
        portal,
        session,
        eis,
    })
}

/// Creates and starts the `RemoteDesktop` session for keyboard and pointer
/// injection and connects to its EIS.
///
/// # Errors
/// As [`open_capture`]; the portal must also implement `RemoteDesktop` 2.
pub(super) async fn open_desktop(
    tokens: &mut Option<TokenStore>,
) -> Result<DesktopSession, InputError> {
    let fail = |error| portal_error(INJECT_PERMISSION, error);
    let portal = RemoteDesktop::new().await.map_err(fail)?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await
        .map_err(fail)?;
    let token = restore_token(tokens.as_ref(), TokenKind::RemoteDesktop);
    let options = SelectDevicesOptions::default()
        .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
        .set_persist_mode(PersistMode::ExplicitlyRevoked)
        .set_restore_token(token.as_deref());
    portal
        .select_devices(&session, options)
        .await
        .and_then(|request| request.response())
        .map_err(fail)?;
    let granted = portal
        .start(&session, None, DesktopStart::default())
        .await
        .and_then(|request| request.response())
        .map_err(fail)?;
    remember(tokens, TokenKind::RemoteDesktop, granted.restore_token());
    if !granted
        .devices()
        .contains(DeviceType::Keyboard | DeviceType::Pointer)
    {
        return Err(InputError::MissingPermission(INJECT_PERMISSION));
    }
    let fd = portal
        .connect_to_eis(&session, DesktopEisOptions::default())
        .await
        .map_err(fail)?;
    let eis = handshake(fd, ContextType::Sender).await?;
    Ok(DesktopSession {
        portal,
        session,
        eis,
    })
}

async fn handshake(fd: std::os::fd::OwnedFd, context_type: ContextType) -> Result<Eis, InputError> {
    let context = ei::Context::new(UnixStream::from(fd))
        .map_err(|error| InputError::Portal(format!("cannot use the EI socket: {error}")))?;
    let (connection, events) = context
        .handshake_tokio(EI_CLIENT_NAME, context_type)
        .await
        .map_err(|error| InputError::Portal(format!("EI handshake failed: {error}")))?;
    Ok(Eis { connection, events })
}

pub(super) fn portal_error(permission: &'static str, error: PortalFailure) -> InputError {
    match error {
        PortalFailure::Response(ResponseError::Cancelled) => {
            InputError::MissingPermission(permission)
        }
        error => InputError::Portal(format!("{permission}: {error}")),
    }
}

fn restore_token(tokens: Option<&TokenStore>, kind: TokenKind) -> Option<String> {
    tokens?.get(kind).map(str::to_owned)
}

/// A restore token is single use, so a start that returns none leaves the
/// stored one dead and it is forgotten.
fn remember(tokens: &mut Option<TokenStore>, kind: TokenKind, token: Option<&str>) {
    let Some(store) = tokens else {
        return;
    };
    if store.set(kind, token).is_err() {
        eprintln!("kind=input_wayland_token_save_failed");
    }
}
