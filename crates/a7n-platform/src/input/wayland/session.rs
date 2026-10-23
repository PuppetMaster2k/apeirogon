// SPDX-License-Identifier: GPL-3.0-or-later

//! The two portal sessions of a Wayland desktop and their libei connections.
//!
//! `InputCapture` captures local input at pointer barriers. `RemoteDesktop`
//! injects input from peers. The clipboard portal extends the `RemoteDesktop`
//! session: [`open_desktop`] calls `Clipboard.RequestClipboard` before it starts
//! the session and keeps the resulting clipboard service in
//! [`DesktopSession::clipboard`].

use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
use crate::clipboard::wayland::portal as clipboard_portal;
use crate::input::InputError;

/// Name under which the portals know the daemon; they match it against an
/// installed desktop file of the same name (`packaging/linux`).
const PORTAL_APP_ID: &str = "io.github.PuppetMaster2k.Apeirogon";

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
    pub(super) session: Arc<Session<RemoteDesktop>>,
    pub(super) eis: Eis,
    /// Present when the portal granted clipboard access with the session.
    pub(super) clipboard: Option<clipboard_portal::Service>,
}

/// Tells the portals which application this process is. Once per process,
/// before the first portal session; older portals lack the interface and then
/// identify the process their own way.
pub(super) async fn register_app() {
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(app_id) = ashpd::AppID::try_from(PORTAL_APP_ID) else {
        return;
    };
    if let Err(error) = ashpd::register_host_app(app_id).await {
        eprintln!("kind=portal_app_registration_skipped reason={error}");
    }
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
    let session = Arc::new(session);
    let requested = clipboard_portal::request(&session).await;
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
    let clipboard = connect_clipboard(requested, &session, granted.is_clipboard_enabled()).await;
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
        session,
        eis,
        clipboard,
    })
}

/// Subscribes to the clipboard portal; a failure only loses the clipboard.
async fn connect_clipboard(
    requested: Option<ashpd::desktop::clipboard::Clipboard>,
    session: &Arc<Session<RemoteDesktop>>,
    enabled: bool,
) -> Option<clipboard_portal::Service> {
    let requested = requested.filter(|_| enabled)?;
    match clipboard_portal::Service::connect(requested, Arc::clone(session)).await {
        Ok(service) => Some(service),
        Err(error) => {
            eprintln!("kind=clipboard_wayland_portal_unavailable reason={error}");
            None
        }
    }
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
