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
    /// 只发送这些级别的告警；None/空 = 全部级别（告警精细化：按通道分级投递）
    #[serde(default)]
    pub levels: Option<Vec<String>>,
}

fn default_kind() -> String {
    "generic".into()
}

/// 按 target 的 levels 过滤告警；levels 为 None 或空时不限制。
pub fn filter_levels(events: &[AlertEvent], levels: Option<&[String]>) -> Vec<AlertEvent> {
    let Some(levels) = levels else {
        return events.to_vec();
    };
    if levels.is_empty() {
        return events.to_vec();
    }
    events
        .iter()
        .filter(|e| levels.iter().any(|l| l == &e.level))
        .cloned()
        .collect()
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
    email_body_with_oncall(events, None)
}

/// 邮件正文末尾追加值班人（告警精细化：值班联动）
pub fn email_body_with_oncall(events: &[AlertEvent], oncall: Option<&str>) -> String {
    let mut lines = vec![format!("SuperOps 告警 {} 条", events.len())];
    for e in events {
        lines.push(format!("[{}] {} — {}", e.level, e.title, e.message));
    }
    if let Some(assignee) = oncall
        && !assignee.trim().is_empty()
    {
        lines.push(format!("当前值班: {assignee}"));
    }
    lines.join("\n")
}

/// 查询 oncall_schedule 当前值班人；库不可达/无班次时返回 None（联动失败不阻断告警）
pub async fn current_oncall(pool: &sqlx::MySqlPool) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT assignee FROM oncall_schedule WHERE start_at <= NOW() AND end_at >= NOW() \
         ORDER BY start_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
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
    dispatch_with_oncall(target, events, http, smtp, None).await
}

/// dispatch + 值班联动：oncall 为当前值班人，追加进邮件正文/通知内容。
/// 按 target.levels 过滤后无事件时直接跳过（不报错、不发 HTTP）。
pub async fn dispatch_with_oncall(
    target: &NotifyTarget,
    events: &[AlertEvent],
    http: &reqwest::Client,
    smtp: Option<&SmtpConfig>,
    oncall: Option<&str>,
) -> Result<(), String> {
    let filtered = filter_levels(events, target.levels.as_deref());
    if filtered.is_empty() {
        return Ok(());
    }
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
            &format!("[SuperOps] 告警 {} 条", filtered.len()),
            &email_body_with_oncall(&filtered, oncall),
        )
        .await;
    }
    let mut payload = build_payload(&target.kind, &filtered);
    if let Some(assignee) = oncall
        && !assignee.trim().is_empty()
    {
        payload["oncall"] = serde_json::Value::String(assignee.to_string());
    }
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
            levels: None,
        };
        let http = reqwest::Client::new();
        let err = futures::executor::block_on(dispatch(&target, &[ev("x")], &http, None));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("smtp"));
    }

    #[test]
    fn filter_levels_none_keeps_all() {
        let events = vec![ev("a"), ev("b")];
        assert_eq!(filter_levels(&events, None).len(), 2);
        assert_eq!(filter_levels(&events, Some(&[])).len(), 2);
    }

    #[test]
    fn filter_levels_matches_only_listed() {
        let mut warn = ev("a");
        warn.level = "WARN".into();
        let mut crit = ev("b");
        crit.level = "CRITICAL".into();
        let events = vec![warn, crit];
        let out = filter_levels(&events, Some(&["CRITICAL".to_string()]));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "b");
    }

    #[test]
    fn filter_levels_no_match_empties() {
        let events = vec![ev("a")];
        assert!(filter_levels(&events, Some(&["CRITICAL".to_string()])).is_empty());
    }

    #[test]
    fn dispatch_skips_when_filtered_empty() {
        let target = NotifyTarget {
            name: "crit-only".into(),
            kind: "generic".into(),
            url: "http://127.0.0.1:1/never-called".into(),
            levels: Some(vec!["CRITICAL".into()]),
        };
        let http = reqwest::Client::new();
        // WARN 事件被过滤，跳过 HTTP；返回 Ok 且不发起请求
        let res = futures::executor::block_on(dispatch(&target, &[ev("warn-event")], &http, None));
        assert!(res.is_ok());
    }

    #[test]
    fn email_body_appends_oncall_footer() {
        let body = email_body_with_oncall(&[ev("pod-down")], Some("zhang-san"));
        assert!(body.contains("当前值班: zhang-san"));
        assert!(!email_body(&[ev("pod-down")]).contains("值班"));
    }
}
