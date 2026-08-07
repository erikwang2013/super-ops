//! 跨服务领域事件（ecat-events 事件总线载荷）。
//!
//! 事件总线 topic = 类型完整路径（`std::any::type_name`），发布方与订阅方
//! 必须引用同一类型定义，因此事件类型放在共享 crate 中。

use serde::{Deserialize, Serialize};

pub fn event_topic<T>() -> String {
    std::any::type_name::<T>().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEvent {
    pub event_type: String,
    pub level: String,
    pub title: String,
    pub message: String,
    #[serde(default)]
    pub detail: serde_json::Value,
    #[serde(default)]
    pub ts: i64,
}

impl DomainEvent {
    pub fn new(
        event_type: impl Into<String>,
        level: impl Into<String>,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            level: level.into(),
            title: title.into(),
            message: message.into(),
            detail: serde_json::Value::Null,
            ts: 0,
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = detail;
        self
    }

    pub fn with_ts(mut self, ts: i64) -> Self {
        self.ts = ts;
        self
    }
}
