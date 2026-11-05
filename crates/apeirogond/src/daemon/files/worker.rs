// SPDX-License-Identifier: GPL-3.0-or-later

use std::future::Future;

use super::{Failure, Result};

pub(super) async fn run<T: Send + 'static>(
    work: impl Future<Output = Result<T>> + Send + 'static,
) -> Result<T> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || runtime.block_on(work))
        .await
        .unwrap_or(Err(Failure::Io))
}
