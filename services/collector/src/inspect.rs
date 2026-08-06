use ecat_lock::{DistributedLock, LockError};
use std::future::Future;
use std::time::Duration;

pub const INSPECT_LOCK_KEY: &str = "superops:inspect:lock";
pub const INSPECT_LOCK_TTL_SECS: u64 = 300;

pub async fn with_inspect_lock<L, F, Fut>(
    lock: &L,
    work: F,
) -> Result<Option<Fut::Output>, LockError>
where
    L: DistributedLock,
    F: FnOnce() -> Fut,
    Fut: Future,
{
    let Some(token) = lock
        .acquire(INSPECT_LOCK_KEY, Duration::from_secs(INSPECT_LOCK_TTL_SECS))
        .await?
    else {
        return Ok(None);
    };
    let out = work().await;
    if let Err(e) = lock.release(INSPECT_LOCK_KEY, &token).await {
        tracing::warn!("inspect lock release failed (ttl will expire it): {e}");
    }
    Ok(Some(out))
}
