use crate::alert::AlertEvent;
use crate::config::SmtpConfig;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
pub struct NotifyTarget {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String, // generic | dingtalk | wecom | email
    pub url: String, // webhook 地址；kind=email 时为收件人（逗号分隔多个）
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

// B4 邮件通知通道：kind=email 时经 SMTP 发送，正文为纯文本逐条告警
pub fn email_body(events: &[AlertEvent]) -> String {
    let mut lines = vec![format!("SuperOps 告警 {} 条", events.len())];
    for e in events {
        lines.push(format!("[{}] {} — {}", e.level, e.title, e.message));
    }
    lines.join("\n")
}

pub async fn send_email(
    smtp: &SmtpConfig,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    let from: Mailbox = smtp
        .from
        .parse()
        .map_err(|e| format!("invalid from addr '{}': {e}", smtp.from))?;
    let mut builder = Message::builder().from(from).subject(subject);
    let mut first = true;
    for addr in to.split(',') {
        let a: Mailbox = addr
            .trim()
            .parse()
            .map_err(|e| format!("invalid to addr '{addr}': {e}"))?;
        if first {
            builder = builder.to(a);
            first = false;
        } else {
            builder = builder.cc(a);
        }
    }
    let mail = builder
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| e.to_string())?;
    let creds = Credentials::new(smtp.username.clone(), smtp.password.clone());
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
        .port(smtp.port)
        .tls(Tls::Opportunistic(
            TlsParameters::new_rustls(smtp.host.clone()).map_err(|e| e.to_string())?,
        ))
        .credentials(creds)
        .build();
    mailer.send(mail).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn dispatch(
    target: &NotifyTarget,
    events: &[AlertEvent],
    http: &reqwest::Client,
    smtp: Option<&SmtpConfig>,
) -> Result<(), String> {
    if target.kind == "email" {
        let Some(s) = smtp else {
            return Err(format!(
                "notify {}: email kind requires smtp config",
                target.name
            ));
        };
        return send_email(
            s,
            &target.url,
            &format!("[SuperOps] 告警 {} 条", events.len()),
            &email_body(events),
        )
        .await;
    }
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

    #[test]
    fn email_body_lists_each_event() {
        let body = email_body(&[ev("pod-not-running"), ev("node-not-ready")]);
        assert!(body.contains("SuperOps 告警 2 条"));
        assert!(body.contains("[WARN] pod-not-running — msg pod-not-running"));
        assert!(body.contains("node-not-ready"));
    }

    #[test]
    fn email_kind_without_smtp_is_error() {
        let target = NotifyTarget {
            name: "mail".into(),
            kind: "email".into(),
            url: "ops@example.com".into(),
        };
        let http = reqwest::Client::new();
        let err = futures::executor::block_on(dispatch(&target, &[ev("x")], &http, None));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("smtp"));
    }
}
