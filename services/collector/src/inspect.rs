use ecat_lock::{DistributedLock, LockError};
use std::future::Future;
use std::time::Duration;

pub const INSPECT_LOCK_KEY: &str = "superops:inspect:lock";
pub const INSPECT_LOCK_TTL_SECS: u64 = 300;

/// 通用周期任务分布式锁：`key` 锁内执行 work，未获取到锁返回 None（跳过本轮）。
/// 用于 drift/rollback/housekeeping 等任务在多 collector 实例下只执行一次。
pub async fn with_task_lock<L, F, Fut>(
    lock: &L,
    key: &str,
    work: F,
) -> Result<Option<Fut::Output>, LockError>
where
    L: DistributedLock,
    F: FnOnce() -> Fut,
    Fut: Future,
{
    let Some(token) = lock
        .acquire(key, Duration::from_secs(INSPECT_LOCK_TTL_SECS))
        .await?
    else {
        return Ok(None);
    };
    let out = work().await;
    if let Err(e) = lock.release(key, &token).await {
        tracing::warn!("task lock release failed (ttl will expire it): {e}");
    }
    Ok(Some(out))
}

pub async fn with_inspect_lock<L, F, Fut>(
    lock: &L,
    work: F,
) -> Result<Option<Fut::Output>, LockError>
where
    L: DistributedLock,
    F: FnOnce() -> Fut,
    Fut: Future,
{
    with_task_lock(lock, INSPECT_LOCK_KEY, work).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_lock_key_constants() {
        assert_eq!(INSPECT_LOCK_KEY, "superops:inspect:lock");
        assert_eq!(INSPECT_LOCK_TTL_SECS, 300);
    }
}
