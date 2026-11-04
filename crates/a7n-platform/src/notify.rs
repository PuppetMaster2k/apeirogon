// SPDX-License-Identifier: GPL-3.0-or-later

use notify_rust::Notification;

/// Show a desktop notification on a detached worker, logging only failure kinds.
pub fn notify(title: &str, body: &str) {
    let mut notification = Notification::new();
    notification.appname("Apeirogon").summary(title).body(body);
    if std::thread::Builder::new()
        .name("notification".into())
        .spawn(move || show(&mut notification))
        .is_err()
    {
        eprintln!("kind=notification_worker_failed");
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show(notification: &mut Notification) {
    use std::time::Duration;

    const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);
    const ACTION_TIMEOUT: Duration = Duration::from_secs(60);

    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        eprintln!("kind=notification_worker_failed");
        return;
    };
    runtime.block_on(async {
        notification.action("default", "Open Apeirogon");
        let Ok(Ok(handle)) =
            tokio::time::timeout(DELIVERY_TIMEOUT, notification.show_async()).await
        else {
            eprintln!("kind=notification_show_failed");
            return;
        };
        let _ = tokio::time::timeout(
            ACTION_TIMEOUT,
            handle.wait_for_action_async(|action| {
                if let notify_rust::ActionResponse::Custom(action) = action {
                    if *action == "default" {
                        open_gui();
                    }
                }
            }),
        )
        .await;
    });
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_gui() {
    let result = std::env::current_exe().and_then(|exe| {
        let sibling = exe.with_file_name("a7n-gui");
        super::autostart::start(if sibling.is_file() {
            &sibling
        } else {
            std::path::Path::new("a7n-gui")
        })
    });
    if result.is_err() {
        eprintln!("kind=notification_open_failed");
    }
}

#[cfg(any(target_os = "macos", windows))]
fn show(notification: &mut Notification) {
    if notification.show().is_err() {
        eprintln!("kind=notification_show_failed");
    }
}
