use crate::alert::AlertEvent;
use crate::ch::{clickhouse_from, now_secs};
use crate::config::Config;
use ecat_data::{DataPoint, FieldValue, TsdbClient};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};

/// 配置漂移：集群里存在但 CMDB 未登记（或 status != active）的 deployment。
/// 对比输入为 CMDB 登记的 deployment 名集合（仅 active）与集群实际 deployment 名集合。
pub fn diff_deployments(cfg_names: &[String], k8s_names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = k8s_names
        .iter()
        .filter(|n| !cfg_names.contains(n))
        .cloned()
        .collect();
    out.sort();
    out
}

fn drift_points(events: &[AlertEvent], ts: i64) -> Vec<DataPoint> {
    events
        .iter()
        .map(|e| {
            let mut p = DataPoint::new("drift_event")
                .with_tag("level", e.level.clone())
                .with_field("title", FieldValue::String(e.title.clone()))
                .with_field("message", FieldValue::String(e.message.clone()))
                .with_timestamp(ts);
            if let Some(n) = &e.node {
                p = p.with_tag("deployment", n.clone());
            }
            p
        })
        .collect()
}

/// 周期任务：CMDB deployment 资产 vs 集群实际 deployment，差异写 ClickHouse drift_event。
/// mysql 未配置时跳过（与值班联动一致，不阻断其他任务）。
pub async fn drift_once(cfg: &Config) -> anyhow::Result<()> {
    // 分布式锁：多 collector 实例下只执行一次（防重复漂移告警）
    let lock = ecat_data_redis::RedisLock::from_config(cfg.lock.clone())
        .await
        .map_err(|e| anyhow::anyhow!("redis lock config: {e}"))?;
    match crate::inspect::with_task_lock(&lock, "superops:drift:lock", || drift_once_inner(cfg))
        .await
    {
        Ok(Some(out)) => out,
        Ok(None) => {
            tracing::debug!("drift skipped: lock held by another instance");
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("drift lock: {e}")),
    }
}

async fn drift_once_inner(cfg: &Config) -> anyhow::Result<()> {
    let Some(mysql) = &cfg.mysql else {
        tracing::debug!("drift check skipped: mysql not configured");
        return Ok(());
    };
    let opts = MySqlConnectOptions::new()
        .host(&mysql.host)
        .port(mysql.port)
        .username(&mysql.user)
        .password(&mysql.password)
        .database(&mysql.database);
    let pool = MySqlPoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    let rows: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT name FROM cmdb_asset WHERE asset_type = 'deployment' AND status = 'active'",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| anyhow::anyhow!("cmdb_asset query failed: {e}"))?;

    let Some(cluster_id) = crate::cluster::resolve_cluster_id(cfg).await? else {
        tracing::warn!("no registered cluster; skipping drift round");
        return Ok(());
    };
    let mut client = crate::cluster::k8s_client(cfg).await?;
    let deps = client
        .list_deployments(superops_protos::k8s::v1::ListDeploymentsRequest {
            cluster_id: cluster_id.clone(),
            ..Default::default()
        })
        .await?
        .into_inner()
        .deployments;
    let k8s_names: Vec<String> = deps.iter().map(|d| d.name.clone()).collect();
    let drifted = diff_deployments(&rows, &k8s_names);
    if drifted.is_empty() {
        return Ok(());
    }
    let events: Vec<AlertEvent> = drifted
        .iter()
        .map(|name| AlertEvent {
            level: "WARN".into(),
            title: "config-drift".into(),
            message: format!("deployment {name} 未在 CMDB 登记"),
            node: Some(name.clone()),
        })
        .collect();
    let ch = clickhouse_from(cfg)?;
    TsdbClient::write(ch.as_ref(), &drift_points(&events, now_secs())).await?;
    tracing::info!(count = drifted.len(), "config drift detected");
    // 领域事件：配置漂移同步广播到事件总线
    for name in &drifted {
        let event = superops_protos::events::DomainEvent::new(
            "drift",
            "WARN",
            "config-drift",
            format!("deployment {name} 未在 CMDB 登记"),
        )
        .with_detail(serde_json::json!({ "deployment": name }))
        .with_ts(now_secs());
        if let Err(e) = crate::domain_events::publish_domain_event(cfg, &event).await {
            tracing::warn!("drift domain event publish failed: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_finds_untracked_deployments() {
        let cfg_names = vec!["api-server".to_string(), "worker".to_string()];
        let k8s_names = vec![
            "worker".to_string(),
            "api-server".to_string(),
            "rogue".to_string(),
        ];
        assert_eq!(
            diff_deployments(&cfg_names, &k8s_names),
            vec!["rogue".to_string()]
        );
    }

    #[test]
    fn diff_empty_when_consistent() {
        let names = vec!["a".to_string()];
        assert!(diff_deployments(&names, &names).is_empty());
        assert!(diff_deployments(&names, &[]).is_empty());
    }

    #[test]
    fn drift_points_carry_tags() {
        let e = AlertEvent {
            level: "WARN".into(),
            title: "config-drift".into(),
            message: "deployment rogue 未在 CMDB 登记".into(),
            node: Some("rogue".into()),
        };
        let pts = drift_points(&[e], 1_700_000_000);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].measurement, "drift_event");
        assert_eq!(
            pts[0].tags.get("deployment").map(String::as_str),
            Some("rogue")
        );
    }
}
