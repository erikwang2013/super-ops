use crate::alert::AlertEvent;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
pub struct NotifyTarget {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String, // generic | dingtalk | wecom
    pub url: String,
}

fn default_kind() -> String {
    "generic".into()
}

pub fn build_payload(kind: &str, events: &[AlertEvent]) -> serde_json::Value {
    match kind {
        "dingtalk" => serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": format!("SuperOps 告警 {} 条", events.len()),
                "text": events.iter().map(|e| format!("**{}** {} — {}", e.level, e.title, e.message)).collect::<Vec<_>>().join("\n\n")
            }
        }),
        "wecom" => serde_json::json!({
            "msgtype": "text",
            "text": { "content": format!("[SuperOps] 告警 {} 条: {}", events.len(),
                events.iter().map(|e| format!("{} {}", e.title, e.message)).collect::<Vec<_>>().join("; ")) }
        }),
        _ => serde_json::json!({
            "level": events.first().map(|e| e.level.clone()).unwrap_or_default(),
            "title": events.first().map(|e| e.title.clone()).unwrap_or_default(),
            "message": events.first().map(|e| e.message.clone()).unwrap_or_default(),
            "count": events.len()
        }),
    }
}

#[derive(Default)]
pub struct NotifySilencer {
    last_sent: HashMap<String, Instant>,
}

impl NotifySilencer {
    pub fn should_send(&mut self, key: &str, silence_secs: u64) -> bool {
        let now = Instant::now();
        let ok = match self.last_sent.get(key) {
            Some(t) => now.duration_since(*t) >= Duration::from_secs(silence_secs),
            None => true,
        };
        if ok {
            self.last_sent.insert(key.to_string(), now);
        }
        ok
    }
}

pub async fn dispatch(
    target: &NotifyTarget,
    events: &[AlertEvent],
    http: &reqwest::Client,
) -> Result<(), String> {
    let payload = build_payload(&target.kind, events);
    let resp = http
        .post(&target.url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("notify {}: http {}", target.name, resp.status()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(title: &str) -> AlertEvent {
        AlertEvent {
            level: "WARN".into(),
            title: title.into(),
            message: format!("msg {title}"),
            node: None,
        }
    }

    #[test]
    fn generic_payload_has_count_and_first_event() {
        let p = build_payload("generic", &[ev("pod-not-running"), ev("node-not-ready")]);
        assert_eq!(p["count"], 2);
        assert_eq!(p["title"], "pod-not-running");
    }

    #[test]
    fn dingtalk_payload_is_markdown() {
        let p = build_payload("dingtalk", &[ev("pod-not-running")]);
        assert_eq!(p["msgtype"], "markdown");
        assert!(
            p["markdown"]["text"]
                .as_str()
                .unwrap()
                .contains("pod-not-running")
        );
    }

    #[test]
    fn wecom_payload_is_text() {
        let p = build_payload("wecom", &[ev("node-not-ready")]);
        assert_eq!(p["msgtype"], "text");
        assert!(
            p["text"]["content"]
                .as_str()
                .unwrap()
                .contains("node-not-ready")
        );
    }

    #[test]
    fn silencer_respects_window() {
        let mut s = NotifySilencer::default();
        assert!(s.should_send("pod-not-running:node-1", 300));
        assert!(!s.should_send("pod-not-running:node-1", 300));
        assert!(s.should_send("node-not-ready:node-2", 300)); // 不同 key 不受影响
    }
}
