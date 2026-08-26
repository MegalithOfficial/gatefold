use std::fmt::Display;
use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use librespot::core::error::ErrorKind;
use tokio::sync::Semaphore;

const CONCURRENCY: usize = 32;
const RETRIES: u32 = 3;

static PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(CONCURRENCY));

pub trait Recoverable {
    fn recoverable(&self) -> bool;
}

impl Recoverable for librespot::core::Error {
    fn recoverable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::ResourceExhausted | ErrorKind::Unavailable | ErrorKind::DeadlineExceeded
        )
    }
}

pub async fn fetch<T, E, F, Fut>(op: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: Recoverable + Display,
{
    let _permit = PERMITS.acquire().await.expect("fetch semaphore closed");

    let mut attempt = 0;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(error) if attempt < RETRIES && error.recoverable() => {
                attempt += 1;
                let wait = Duration::from_millis(500 * 4u64.pow(attempt - 1));
                tracing::warn!("request limited ({error}), retrying in {wait:?}");
                tokio::time::sleep(wait).await;
            }
            Err(error) => return Err(error),
        }
    }
}
