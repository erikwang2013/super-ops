use crate::config::Config;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use superops_protos::k8s::v1::k8s_service_client::K8sServiceClient;

struct ReleaseTarget {
    id: i64,
    cluster_id: String,
    namespace: String,
    name: String,
    old_image: String,
    new_image: String,
}

/// deployment 不可用判定：记录缺失（部署被删）或 ready==0 且声明副本>0
fn unhealthy(dep: Option<(&str, i32, i32)>) -> bool {
    match dep {
        None => true,
        Some((_, replicas, ready)) => ready == 0 && replicas > 0,
    }
}

/// 观察窗口内（delay 秒后 window 秒内）status=ok 且带旧镜像的发布
async fn pending_releases(
    pool: &sqlx::MySqlPool,
    delay_secs: u64,
    window_secs: u64,
) -> sqlx::Result<Vec<ReleaseTarget>> {
    let rows: Vec<(i64, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, cluster_id, namespace, name, old_image, new_image FROM release \
         WHERE status = 'ok' AND old_image != '-' \
           AND created_at < NOW() - INTERVAL ? SECOND \
           AND created_at >= NOW() - INTERVAL ? SECOND",
    )
    .bind(delay_secs as i64)
    .bind((delay_secs + window_secs) as i64)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, cluster_id, namespace, name, old_image, new_image)| ReleaseTarget {
                id,
                cluster_id,
                namespace,
                name,
                old_image,
                new_image,
            },
        )
        .collect())
}

/// 周期任务：观察窗口内已发布的 deployment 若不可用，自动回滚到 old_image 并落记录。
/// mysql 未配置时跳过（与值班联动一致，不阻断其他任务）。
pub async fn rollback_once(cfg: &Config) -> anyhow::Result<()> {
    // 分布式锁：多 collector 实例下只执行一次（防双重回滚）
    let lock = ecat_data_redis::RedisLock::from_config(cfg.lock.clone())
        .await
        .map_err(|e| anyhow::anyhow!("redis lock config: {e}"))?;
    match crate::inspect::with_task_lock(&lock, "superops:rollback:lock", || {
        rollback_once_inner(cfg)
    })
    .await
    {
        Ok(Some(out)) => out,
        Ok(None) => {
            tracing::debug!("rollback skipped: lock held by another instance");
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("rollback lock: {e}")),
    }
}

async fn rollback_once_inner(cfg: &Config) -> anyhow::Result<()> {
    let Some(mysql) = &cfg.mysql else {
        tracing::debug!("auto rollback skipped: mysql not configured");
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
    let targets =
        pending_releases(&pool, cfg.rollback.delay_secs, cfg.rollback.window_secs).await?;
    if targets.is_empty() {
        return Ok(());
    }
    let mut client = K8sServiceClient::connect(cfg.k8s.endpoint.clone()).await?;
    let deps = client
        .list_deployments(superops_protos::k8s::v1::ListDeploymentsRequest::default())
        .await?
        .into_inner()
        .deployments;
    let mut rolled = 0usize;
    for t in &targets {
        let dep = deps
            .iter()
            .find(|d| d.name == t.name && (d.namespace.is_empty() || d.namespace == t.namespace));
        if !unhealthy(dep.map(|d| (d.name.as_str(), d.replicas, d.ready_replicas))) {
            continue;
        }
        match client
            .update_deployment_image(superops_protos::k8s::v1::UpdateDeploymentImageRequest {
                cluster_id: t.cluster_id.clone(),
                namespace: t.namespace.clone(),
                name: t.name.clone(),
                image: t.old_image.clone(),
            })
            .await
        {
            Ok(_) => {
                let _ = sqlx::query("UPDATE release SET status = 'failed' WHERE id = ?")
                    .bind(t.id)
                    .execute(&pool)
                    .await;
                let _ = sqlx::query(
                    "INSERT INTO release \
                     (cluster_id, namespace, name, old_image, new_image, operator, status) \
                     VALUES (?, ?, ?, ?, ?, 'auto-rollback', 'ok')",
                )
                .bind(&t.cluster_id)
                .bind(&t.namespace)
                .bind(&t.name)
                .bind(&t.new_image)
                .bind(&t.old_image)
                .execute(&pool)
                .await;
                rolled += 1;
                tracing::warn!(deployment = %t.name, image = %t.old_image, "auto rollback triggered");
                // 领域事件：自动回滚广播到事件总线（gateway 端落 ClickHouse domain_event）
                let event = superops_protos::events::DomainEvent::new(
                    "rollback",
                    "WARN",
                    "auto-rollback",
                    format!("deployment {} 回滚到 {}", t.name, t.old_image),
                )
                .with_detail(serde_json::json!({
                    "cluster_id": t.cluster_id,
                    "namespace": t.namespace,
                    "deployment": t.name,
                    "image": t.old_image,
                }))
                .with_ts(crate::ch::now_secs());
                if let Err(e) = crate::domain_events::publish_domain_event(cfg, &event).await {
                    tracing::warn!("rollback domain event publish failed: {e}");
                }
            }
            Err(e) => {
                tracing::warn!(deployment = %t.name, error = %e, "auto rollback update failed")
            }
        }
    }
    tracing::info!(count = rolled, "auto rollback cycle done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unhealthy_when_missing() {
        assert!(unhealthy(None));
    }

    #[test]
    fn unhealthy_when_ready_zero_with_replicas() {
        assert!(unhealthy(Some(("web", 2, 0))));
    }

    #[test]
    fn healthy_when_ready_positive() {
        assert!(!unhealthy(Some(("web", 2, 2))));
        assert!(!unhealthy(Some(("web", 0, 0))));
    }
}
