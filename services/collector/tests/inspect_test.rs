use async_trait::async_trait;
use ecat_lock::{DistributedLock, LockError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use superops_collector::inspect::with_inspect_lock;

struct FakeLock {
    grant: bool,
    acquired: AtomicBool,
    released: AtomicBool,
}

#[async_trait]
impl DistributedLock for FakeLock {
    async fn acquire(&self, _key: &str, _ttl: Duration) -> Result<Option<String>, LockError> {
        if self.grant {
            self.acquired.store(true, Ordering::SeqCst);
            Ok(Some("fake-token".into()))
        } else {
            Ok(None)
        }
    }

    async fn release(&self, _key: &str, _token: &str) -> Result<(), LockError> {
        self.released.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn lock_denied_skips_work() {
    let lock = FakeLock {
        grant: false,
        acquired: AtomicBool::new(false),
        released: AtomicBool::new(false),
    };
    let mut ran = false;
    let out = with_inspect_lock(&lock, || {
        ran = true;
        std::future::ready(())
    })
    .await
    .unwrap();
    assert!(out.is_none());
    assert!(!ran);
    assert!(!lock.acquired.load(Ordering::SeqCst));
    assert!(!lock.released.load(Ordering::SeqCst));
}

#[tokio::test]
async fn lock_granted_runs_work_and_releases() {
    let lock = FakeLock {
        grant: true,
        acquired: AtomicBool::new(false),
        released: AtomicBool::new(false),
    };
    let mut ran = false;
    let out = with_inspect_lock(&lock, || {
        ran = true;
        std::future::ready(42)
    })
    .await
    .unwrap();
    assert_eq!(out, Some(42));
    assert!(ran);
    assert!(lock.acquired.load(Ordering::SeqCst));
    assert!(lock.released.load(Ordering::SeqCst));
}
