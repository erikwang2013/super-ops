use crate::ch::{clickhouse_from, now_secs};
use crate::config::Config;
use ecat_data::{Cache as _, DataPoint, FieldValue, TsdbClient};
use ecat_data_redis::RedisCache;
use std::sync::Arc;
use std::time::Duration;
use superops_protos::k8s::v1::{GetPodLogsRequest, ListPodsRequest};

pub fn truncate_line(line: &str, max_bytes: usize) -> String {
    if line.len() <= max_bytes {
        line.to_string()
    } else {
        // 截断到 ≤ max_bytes 的最后一个字符边界，避免多字节字符被切断
        let end = line
            .char_indices()
            .take_while(|(i, _)| *i <= max_bytes)
            .map(|(i, _)| i)
            .last()
            .unwrap_or(0);
        format!("{}…(truncated)", &line[..end])
    }
}

pub fn dedup_continuous(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if out.last().map(|p| p != l).unwrap_or(true) {
            out.push(l.clone());
        }
    }
    out
}

/// 从最后一次出现的游标行之后续采；游标行不存在视为日志轮转，返回全部。
pub fn skip_after_cursor(lines: &[String], cursor: Option<&str>) -> Vec<String> {
    let Some(cur) = cursor.filter(|c| !c.is_empty()) else {
        return lines.to_vec();
    };
    match lines.iter().rposition(|l| l == cur) {
        Some(idx) => lines[idx + 1..].to_vec(),
        None => lines.to_vec(),
    }
}

fn cursor_key(cluster_id: &str, ns: &str, pod: &str) -> String {
    format!("logtail:cursor:{cluster_id}:{ns}:{pod}")
}

pub async fn collect_once(cfg: &Config) -> anyhow::Result<()> {
    if cfg.logtail.namespaces.is_empty() {
        return Ok(());
    }
    let Some(cluster_id) = crate::cluster::resolve_cluster_id(cfg).await? else {
        tracing::warn!("no registered cluster; skipping logtail round");
        return Ok(());
    };
    let mut client = crate::cluster::k8s_client(cfg).await?;
    let ch = clickhouse_from(cfg)?;
    // 日志检索后端：search 段配置时，同一批日志同时索引到 ES/OpenSearch
    let search_client: Option<(Arc<dyn ecat_data::SearchClient>, String)> = match &cfg.search {
        Some(s) => {
            let client: Arc<dyn ecat_data::SearchClient> = match s.provider.as_str() {
                "opensearch" => Arc::new(
                    ecat_data_opensearch::OpenSearchClient::from_config(
                        ecat_data_opensearch::OpenSearchConfig {
                            base_url: s.base_url.clone(),
                            username: s.username.clone(),
                            password: s.password.clone(),
                            tls: None,
                        },
                    )
                    .map_err(|e| anyhow::anyhow!("opensearch config: {e}"))?,
                ),
                _ => Arc::new(
                    ecat_data_elasticsearch::ElasticsearchClient::from_config(
                        ecat_data_elasticsearch::ElasticsearchConfig {
                            base_url: s.base_url.clone(),
                            username: s.username.clone(),
                            password: s.password.clone(),
                            tls: None,
                        },
                    )
                    .map_err(|e| anyhow::anyhow!("elasticsearch config: {e}"))?,
                ),
            };
            Some((client, s.index.clone()))
        }
        None => None,
    };
    let cache = if cfg.lock.url.is_empty() {
        tracing::warn!("logtail cursor skipped: redis url empty");
        None
    } else {
        match RedisCache::from_config(cfg.lock.clone()).await {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!(error = %e, "logtail cursor cache unavailable");
                None
            }
        }
    };
    let cursor_ttl = Duration::from_secs(cfg.logtail.cursor_ttl_secs.max(1));
    let batch = cfg.logtail.write_batch_size.max(1);
    for ns in &cfg.logtail.namespaces {
        let pods = client
            .list_pods(ListPodsRequest {
                cluster_id: cluster_id.clone(),
                namespace: ns.clone(),
                ..Default::default()
            })
            .await?
            .into_inner()
            .pods;
        for pod in pods {
            let stream = match client
                .get_pod_logs(GetPodLogsRequest {
                    cluster_id: cluster_id.clone(),
                    namespace: ns.clone(),
                    pod_name: pod.name.clone(),
                    container: String::new(),
                    tail_lines: cfg.logtail.tail_lines,
                    follow: false,
                })
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    // 单 Pod 失败不影响其余 Pod 的采集
                    tracing::warn!("logs for {}/{}: {e}", ns, pod.name);
                    continue;
                }
            };
            let mut lines = Vec::new();
            let mut stream = stream.into_inner();
            loop {
                match stream.message().await {
                    Ok(Some(line)) => lines.push(line.content),
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("log stream for {}/{}: {e}", ns, pod.name);
                        break;
                    }
                }
            }
            let cursor_line = if let Some(cache) = &cache {
                let key = cursor_key(&cluster_id, ns, &pod.name);
                match cache.get(&key).await {
                    Ok(Some(bytes)) => serde_json::from_slice::<serde_json::Value>(&bytes)
                        .ok()
                        .and_then(|v| {
                            v.get("last_line")
                                .and_then(|s| s.as_str())
                                .map(str::to_string)
                        }),
                    Ok(None) => None,
                    Err(e) => {
                        tracing::warn!(error = %e, "logtail cursor get failed");
                        None
                    }
                }
            } else {
                None
            };
            let new_lines = skip_after_cursor(&lines, cursor_line.as_deref());
            let last_raw = new_lines.last().cloned();
            let points: Vec<DataPoint> = dedup_continuous(&new_lines)
                .into_iter()
                .map(|content| {
                    let content = truncate_line(&content, cfg.logtail.max_line_bytes as usize);
                    DataPoint::new("pod_log")
                        .with_tag("namespace", ns.clone())
                        .with_tag("pod", pod.name.clone())
                        .with_field("content", FieldValue::String(content))
                        .with_timestamp(now_secs())
                })
                .collect();
            if !points.is_empty() {
                let mut write_ok = true;
                for chunk in points.chunks(batch) {
                    if let Err(e) = TsdbClient::write(ch.as_ref(), chunk).await {
                        tracing::warn!("logtail CH write {}/{}: {e}", ns, pod.name);
                        write_ok = false;
                        break;
                    }
                }
                if write_ok {
                    if let (Some(cache), Some(last)) = (&cache, last_raw) {
                        let key = cursor_key(&cluster_id, ns, &pod.name);
                        let body = serde_json::json!({ "last_line": last });
                        if let Err(e) = cache
                            .set(&key, &body.to_string().into_bytes(), cursor_ttl)
                            .await
                        {
                            tracing::warn!(error = %e, "logtail cursor set failed");
                        }
                    }
                    if let Some((search, index)) = &search_client {
                        let ts = now_secs();
                        for (i, content) in new_lines.iter().enumerate() {
                            let id = format!("{ns}/{}/{ts}-{i}", pod.name);
                            let doc = serde_json::json!({
                                "namespace": ns,
                                "pod": pod.name,
                                "content": truncate_line(content, cfg.logtail.max_line_bytes as usize),
                                "timestamp": ts,
                            });
                            if let Err(e) = search.index(index, &id, &doc).await {
                                tracing::warn!("log search index {id} failed: {e}");
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_line_cuts_long_lines() {
        let long = "x".repeat(1000);
        assert_eq!(truncate_line(&long, 64).len(), 64 + "…(truncated)".len());
        assert_eq!(truncate_line("short", 64), "short");
    }

    #[test]
    fn truncate_line_handles_multibyte_boundary() {
        let long = "中".repeat(500);
        let out = truncate_line(&long, 64);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        assert!(out.len() <= 64 + "…(truncated)".len());
    }

    #[test]
    fn dedup_continuous_keeps_only_runs() {
        let lines = vec![
            "a".into(),
            "a".into(),
            "b".into(),
            "b".into(),
            "b".into(),
            "a".into(),
        ];
        assert_eq!(
            dedup_continuous(&lines),
            vec!["a".to_string(), "b".to_string(), "a".to_string()]
        );
    }

    #[test]
    fn skip_after_cursor_last_occurrence() {
        let lines = vec!["a".into(), "b".into(), "a".into(), "c".into()];
        assert_eq!(skip_after_cursor(&lines, Some("a")), vec!["c".to_string()]);
        assert_eq!(skip_after_cursor(&lines, None), lines);
        assert_eq!(skip_after_cursor(&lines, Some("missing")), lines);
    }
}
