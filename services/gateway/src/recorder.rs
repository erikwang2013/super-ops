use base64::Engine as _;
use ecat_data::{DataPoint, FieldValue, TsdbClient};
use ecat_data_clickhouse::ClickhouseClient;
use rand::RngCore;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

pub fn session_id() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    uuid::Builder::from_random_bytes(b).as_uuid().to_string()
}

/// 单帧大小上限校验（仅测试契约使用；record_frame 内部自行截断超限帧）
#[allow(dead_code)]
pub fn frame_valid(frame: &[u8]) -> bool {
    !frame.is_empty() && frame.len() <= 256 * 1024
}

pub fn truncate<'a>(frame: &'a [u8]) -> &'a [u8] {
    &frame[..frame.len().min(256 * 1024)]
}

pub fn valid_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !name.starts_with('.')
        && !name.contains("..")
}

// 录制与 exec 数据通路解耦：信号量限 1 帧在途，CH 忙则丢帧（每 100 帧告警一次），
// 绝不阻塞 ws 桥；写入失败/超时仅告警。
static FRAME_PERMITS: OnceLock<Semaphore> = OnceLock::new();
static DROPPED_FRAMES: AtomicU64 = AtomicU64::new(0);

pub fn record_frame(ch: &Arc<ClickhouseClient>, session_id: &str, node: &str, frame: &[u8]) {
    if frame.is_empty() {
        return;
    }
    let Some(permit) = FRAME_PERMITS
        .get_or_init(|| Semaphore::new(1))
        .try_acquire()
        .ok()
    else {
        let dropped = DROPPED_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        if dropped % 100 == 0 {
            tracing::warn!(dropped, "recording frames dropped: ClickHouse busy");
        }
        return;
    };
    let frame = truncate(frame).to_vec();
    let ch = Arc::clone(ch);
    let sid = session_id.to_string();
    let node = node.to_string();
    tokio::spawn(async move {
        let _permit = permit;
        let point = DataPoint::new("exec_session")
            .with_tag("kind", "recording")
            .with_tag("session_id", sid)
            .with_tag("node", node)
            .with_field(
                "frame_b64",
                FieldValue::String(base64::engine::general_purpose::STANDARD.encode(frame)),
            )
            .with_timestamp(now_secs());
        match tokio::time::timeout(Duration::from_secs(2), ch.write(&[point])).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "recording write failed"),
            Err(_) => tracing::warn!("recording write timed out"),
        }
    });
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
