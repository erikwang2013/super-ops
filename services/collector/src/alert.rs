use crate::ch::{clickhouse_from, now_secs};
use crate::config::Config;
use ecat_data::Cache as _;
use ecat_data::{DataPoint, FieldValue};
use ecat_data_redis::{RedisCache, RedisLock};
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions};
use superops_protos::k8s::v1::k8s_service_client::K8sServiceClient;
use superops_protos::k8s::v1::{
    Deployment, ListDeploymentsRequest, ListNodesRequest, ListPodsRequest, Node, Pod,
};

pub const METRICS: [&str; 6] = [
    "node_not_ready",
    "node_ready_pct",
    "pod_not_running",
    "pod_running_pct",
    "deployment_unavailable",
    "deployment_ready_pct",
];

#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub level: String,
    pub title: String,
    pub message: String,
    pub node: Option<String>,
}

#[derive(Debug, Default)]
pub struct ClusterHealth {
    pub alerts: Vec<AlertEvent>,
    pub cluster_ok: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertRule {
    pub id: i64,
    pub name: String,
    pub metric: String,
    pub operator: String,
    pub threshold: f64,
    pub level: String,
    pub action: String,
}

/// 无 MySQL 时的内置默认规则，语义与旧硬编码 evaluate_health 等价
pub fn default_rules() -> Vec<AlertRule> {
    vec![
        AlertRule {
            id: 0,
            name: "node-not-ready".into(),
            metric: "node_not_ready".into(),
            operator: "ge".into(),
            threshold: 1.0,
            level: "CRIT".into(),
            action: "notify".into(),
        },
        AlertRule {
            id: 0,
            name: "pod-not-running".into(),
            metric: "pod_not_running".into(),
            operator: "ge".into(),
            threshold: 1.0,
            level: "WARN".into(),
            action: "notify".into(),
        },
        AlertRule {
            id: 0,
            name: "deployment-unavailable".into(),
            metric: "deployment_unavailable".into(),
            operator: "ge".into(),
            threshold: 1.0,
            level: "WARN".into(),
            action: "notify".into(),
        },
    ]
}

fn cmp_op(operator: &str, value: f64, threshold: f64) -> bool {
    match operator {
        "ge" => value >= threshold,
        "le" => value <= threshold,
        _ => false,
    }
}

/// 单条规则求值：计数类按目标逐条告警（需 count 过阈值），百分比类按集群或逐 deployment 告警
pub fn evaluate_rule(
    rule: &AlertRule,
    nodes: &[Node],
    pods: &[Pod],
    deps: &[Deployment],
) -> Vec<AlertEvent> {
    let mut alerts = Vec::new();
    match rule.metric.as_str() {
        "node_not_ready" => {
            let bad: Vec<&Node> = nodes.iter().filter(|n| n.status != "Ready").collect();
            if cmp_op(&rule.operator, bad.len() as f64, rule.threshold) {
                for n in bad {
                    alerts.push(AlertEvent {
                        level: rule.level.clone(),
                        title: rule.name.clone(),
                        message: format!("node {} status={}", n.name, n.status),
                        node: Some(n.name.clone()),
                    });
                }
            }
        }
        "pod_not_running" => {
            let bad: Vec<&Pod> = pods
                .iter()
                .filter(|p| p.status != "Running" && p.status != "Succeeded")
                .collect();
            if cmp_op(&rule.operator, bad.len() as f64, rule.threshold) {
                for p in bad {
                    alerts.push(AlertEvent {
                        level: rule.level.clone(),
                        title: rule.name.clone(),
                        message: format!("pod {}/{} status={}", p.namespace, p.name, p.status),
                        node: None,
                    });
                }
            }
        }
        "deployment_unavailable" => {
            let bad: Vec<&Deployment> = deps
                .iter()
                .filter(|d| d.ready_replicas < d.replicas)
                .collect();
            if cmp_op(&rule.operator, bad.len() as f64, rule.threshold) {
                for d in bad {
                    alerts.push(AlertEvent {
                        level: rule.level.clone(),
                        title: rule.name.clone(),
                        message: format!(
                            "deployment {}/{} ready {}/{}",
                            d.namespace, d.name, d.ready_replicas, d.replicas
                        ),
                        node: None,
                    });
                }
            }
        }
        "node_ready_pct" => {
            let pct = if nodes.is_empty() {
                100.0
            } else {
                nodes.iter().filter(|n| n.status == "Ready").count() as f64 / nodes.len() as f64
                    * 100.0
            };
            if cmp_op(&rule.operator, pct, rule.threshold) {
                alerts.push(AlertEvent {
                    level: rule.level.clone(),
                    title: rule.name.clone(),
                    message: format!("node ready {pct:.1}% threshold {}%", rule.threshold),
                    node: None,
                });
            }
        }
        "pod_running_pct" => {
            let pct = if pods.is_empty() {
                100.0
            } else {
                pods.iter().filter(|p| p.status == "Running").count() as f64 / pods.len() as f64
                    * 100.0
            };
            if cmp_op(&rule.operator, pct, rule.threshold) {
                alerts.push(AlertEvent {
                    level: rule.level.clone(),
                    title: rule.name.clone(),
                    message: format!("pod running {pct:.1}% threshold {}%", rule.threshold),
                    node: None,
                });
            }
        }
        "deployment_ready_pct" => {
            for d in deps {
                let pct = if d.replicas == 0 {
                    100.0
                } else {
                    d.ready_replicas as f64 / d.replicas as f64 * 100.0
                };
                if cmp_op(&rule.operator, pct, rule.threshold) {
                    alerts.push(AlertEvent {
                        level: rule.level.clone(),
                        title: rule.name.clone(),
                        message: format!(
                            "deployment {}/{} ready {:.1}% threshold {}%",
                            d.namespace, d.name, pct, rule.threshold
                        ),
                        node: None,
                    });
                }
            }
        }
        _ => {}
    }
    alerts
}

/// 连续超标计数推进：返回 (是否达到连续阈值, 新计数)。
/// `consecutive=1` 时任何超标立即通过（等价于未开启降噪）。
pub fn streak_progress(current: i64, consecutive: u64) -> (bool, i64) {
    let next = current + 1;
    (next >= consecutive as i64, next)
}

/// 告警降噪：Redis streak 计数过滤，连续 `consecutive` 个周期超标才放行。
/// 键 TTL 超时自动重置（告警中断后自动归零）；Redis 不可用时降级为全放行（保告警不丢）。
async fn filter_by_streak(
    cfg: &Config,
    alerts: &[AlertEvent],
    consecutive: u64,
    ttl: std::time::Duration,
) -> Vec<AlertEvent> {
    if consecutive <= 1 {
        return alerts.to_vec();
    }
    let cache = match RedisCache::from_config(cfg.lock.clone()).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "alert streak cache unavailable; bypassing dedup");
            return alerts.to_vec();
        }
    };
    let mut out = Vec::new();
    for alert in alerts {
        let key = format!(
            "alert:streak:{}:{}",
            alert.title,
            alert.node.as_deref().unwrap_or("-")
        );
        let cur: i64 = cache
            .get(&key)
            .await
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let (pass, next) = streak_progress(cur, consecutive);
        if let Err(e) = cache.set(&key, next.to_string().as_bytes(), ttl).await {
            tracing::warn!(error = %e, key, "streak set failed");
        }
        if pass {
            out.push(alert.clone());
        }
    }
    out
}

pub fn evaluate_rules(
    rules: &[AlertRule],
    nodes: &[Node],
    pods: &[Pod],
    deps: &[Deployment],
    max_not_ready: usize,
) -> ClusterHealth {
    let mut alerts = Vec::new();
    for rule in rules {
        let mut ra = evaluate_rule(rule, nodes, pods, deps);
        // 节点级告警风暴截断：单规则节点告警数超过 max_not_ready 时保留前 N 条 + 汇总一条
        if rule.metric == "node_not_ready" && ra.len() > max_not_ready {
            let total = ra.len();
            ra.truncate(max_not_ready);
            ra.push(AlertEvent {
                level: rule.level.clone(),
                title: rule.name.clone(),
                message: format!("{total} 个节点未就绪（超上限 {max_not_ready}，已截断展示）"),
                node: None,
            });
        }
        alerts.extend(ra);
    }
    let not_ready = nodes.iter().filter(|n| n.status != "Ready").count();
    ClusterHealth {
        alerts,
        cluster_ok: not_ready == 0,
    }
}

pub async fn mysql_pool(cfg: &crate::config::MysqlConfig) -> anyhow::Result<MySqlPool> {
    let opts = MySqlConnectOptions::new()
        .host(&cfg.host)
        .port(cfg.port)
        .username(&cfg.user)
        .password(&cfg.password)
        .database(&cfg.database);
    Ok(MySqlPoolOptions::new().connect_with(opts).await?)
}

/// 从 MySQL 加载启用的规则；表存在但全禁用时返回空（= 不告警）
pub async fn load_rules(pool: &MySqlPool) -> sqlx::Result<Vec<AlertRule>> {
    let rows: Vec<(i64, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, name, metric, operator, threshold, level, action \
         FROM alert_rule WHERE enabled = 1 ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, name, metric, operator, threshold, level, action)| {
            let threshold = threshold.parse::<f64>().ok()?;
            Some(AlertRule {
                id,
                name,
                metric,
                operator,
                threshold,
                level,
                action,
            })
        })
        .collect())
}

/// MySQL 不可用或表缺失时回退内置默认规则
/// 自愈目标：仅 deployment 类规则且 action=restart/scale；返回 (namespace, name, action)
pub fn selfheal_targets(
    rules: &[AlertRule],
    deps: &[Deployment],
    limit: usize,
) -> Vec<(String, String, String)> {
    let mut targets = Vec::new();
    for rule in rules {
        if rule.action != "restart" && rule.action != "scale" {
            continue;
        }
        let hits: Vec<(String, String)> = match rule.metric.as_str() {
            "deployment_unavailable" => deps
                .iter()
                .filter(|d| d.ready_replicas < d.replicas)
                .map(|d| (d.namespace.clone(), d.name.clone()))
                .collect(),
            "deployment_ready_pct" => deps
                .iter()
                .filter(|d| {
                    let pct = if d.replicas == 0 {
                        100.0
                    } else {
                        d.ready_replicas as f64 / d.replicas as f64 * 100.0
                    };
                    cmp_op(&rule.operator, pct, rule.threshold)
                })
                .map(|d| (d.namespace.clone(), d.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        for (ns, name) in hits {
            targets.push((ns, name, rule.action.clone()));
            if targets.len() >= limit {
                return targets;
            }
        }
    }
    targets
}

async fn record_selfheal(
    ch: &dyn ecat_data::TsdbClient,
    level: &str,
    action: &str,
    ns: &str,
    name: &str,
    detail: &str,
) {
    let event = AlertEvent {
        level: level.into(),
        title: format!("selfheal-{action}"),
        message: format!("deployment {ns}/{name} {action}: {detail}"),
        node: None,
    };
    if let Err(e) = ch.write(&health_to_points(&[event], now_secs())).await {
        tracing::warn!("selfheal record write failed: {e}");
    }
}

/// 巡检后执行自愈（cfg.selfheal.enabled 开启时）；动作成功/失败均写 alert_event 审计
async fn execute_selfheal(
    cfg: &Config,
    client: &mut K8sServiceClient<crate::cluster::BearerChannel>,
    deps: &[Deployment],
    ch: &dyn ecat_data::TsdbClient,
    rules: &[AlertRule],
    cluster_id: &str,
) {
    let targets = selfheal_targets(rules, deps, cfg.selfheal.max_actions_per_cycle);
    if targets.is_empty() {
        return;
    }
    // 冷却与上限：Redis 冷却键 + 副本上限，防反复触发/无限扩容；Redis 不可用时降级执行
    let cache = match RedisCache::from_config(cfg.lock.clone()).await {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, "selfheal cache unavailable; cooldown disabled");
            None
        }
    };
    let cooldown_secs = cfg.selfheal.cooldown_secs;
    let max_replicas = cfg.selfheal.max_replicas;
    tracing::info!(count = targets.len(), "selfheal actions to execute");
    for (ns, name, action) in targets {
        let cooldown_key = format!("selfheal:cooldown:{ns}:{name}");
        if let Some(cache) = &cache
            && cache.get(&cooldown_key).await.ok().flatten().is_some()
        {
            tracing::debug!(ns, name, action, "selfheal skipped: in cooldown");
            continue;
        }
        let out: Result<(), tonic::Status> = if action == "restart" {
            client
                .restart_deployment(superops_protos::k8s::v1::RestartDeploymentRequest {
                    cluster_id: cluster_id.to_string(),
                    namespace: ns.clone(),
                    name: name.clone(),
                })
                .await
                .map(|_| ())
        } else if action == "scale" {
            let current = deps
                .iter()
                .find(|d| d.namespace == ns && d.name == name)
                .map(|d| d.replicas)
                .unwrap_or(0);
            if current >= max_replicas {
                tracing::warn!(
                    ns,
                    name,
                    current,
                    max_replicas,
                    "selfheal scale skipped: at max replicas"
                );
                continue;
            }
            client
                .scale_deployment(superops_protos::k8s::v1::ScaleDeploymentRequest {
                    cluster_id: cluster_id.to_string(),
                    namespace: ns.clone(),
                    name: name.clone(),
                    replicas: current + 1,
                })
                .await
                .map(|_| ())
        } else {
            continue;
        };
        match out {
            Ok(_) => {
                if let Some(cache) = &cache {
                    let _ = cache
                        .set(
                            &cooldown_key,
                            b"1".as_slice(),
                            std::time::Duration::from_secs(cooldown_secs),
                        )
                        .await;
                }
                tracing::info!(ns, name, action, "selfheal executed");
                record_selfheal(ch, "INFO", &action, &ns, &name, "ok").await;
            }
            Err(e) => {
                tracing::warn!(ns, name, action, %e, "selfheal failed");
                record_selfheal(ch, "WARN", &action, &ns, &name, &format!("failed: {e}")).await;
            }
        }
    }
}

pub async fn resolve_rules(cfg: &Config) -> Vec<AlertRule> {
    let Some(mysql) = &cfg.mysql else {
        return default_rules();
    };
    match mysql_pool(mysql).await {
        Ok(pool) => match load_rules(&pool).await {
            Ok(rules) => rules,
            Err(e) => {
                tracing::warn!("alert rules load failed, fallback to defaults: {e}");
                default_rules()
            }
        },
        Err(e) => {
            tracing::warn!("mysql connect failed, fallback to default alert rules: {e}");
            default_rules()
        }
    }
}

pub fn evaluate_health(
    nodes: &[Node],
    pods: &[Pod],
    deps: &[Deployment],
    max_not_ready: usize,
) -> ClusterHealth {
    let mut alerts = Vec::new();
    let mut not_ready_nodes = 0usize;
    for n in nodes {
        if n.status != "Ready" {
            not_ready_nodes += 1;
            alerts.push(AlertEvent {
                level: "CRIT".into(),
                title: "node-not-ready".into(),
                message: format!("node {} status={}", n.name, n.status),
                node: Some(n.name.clone()),
            });
        }
    }
    for p in pods
        .iter()
        .filter(|p| p.status != "Running" && p.status != "Succeeded")
    {
        alerts.push(AlertEvent {
            level: "WARN".into(),
            title: "pod-not-running".into(),
            message: format!("pod {}/{} status={}", p.namespace, p.name, p.status),
            node: None,
        });
    }
    for d in deps {
        if d.ready_replicas < d.replicas {
            alerts.push(AlertEvent {
                level: "WARN".into(),
                title: "deployment-unavailable".into(),
                message: format!(
                    "deployment {}/{} ready {}/{}",
                    d.namespace, d.name, d.ready_replicas, d.replicas
                ),
                node: None,
            });
        }
    }
    ClusterHealth {
        alerts,
        cluster_ok: not_ready_nodes <= max_not_ready,
    }
}

pub fn health_to_points(events: &[AlertEvent], ts: i64) -> Vec<DataPoint> {
    events
        .iter()
        .map(|e| {
            let mut p = DataPoint::new("alert_event")
                .with_tag("level", e.level.clone())
                .with_field("title", FieldValue::String(e.title.clone()))
                .with_field("message", FieldValue::String(e.message.clone()))
                .with_timestamp(ts);
            if let Some(n) = &e.node {
                p = p.with_tag("node", n.clone());
            }
            p
        })
        .collect()
}

#[tracing::instrument(skip_all)]
pub async fn inspect_once(cfg: &Config) -> anyhow::Result<()> {
    let lock = RedisLock::from_config(cfg.lock.clone())
        .await
        .map_err(|e| anyhow::anyhow!("redis lock config: {e}"))?;
    let ran = crate::inspect::with_inspect_lock(&lock, || inspect_work(cfg))
        .await
        .map_err(|e| anyhow::anyhow!("inspect lock: {e}"))?;
    if ran.is_none() {
        tracing::debug!("inspect skipped: lock held by another instance");
    }
    Ok(())
}

async fn inspect_work(cfg: &Config) -> anyhow::Result<()> {
    let Some(cluster_id) = crate::cluster::resolve_cluster_id(cfg).await? else {
        tracing::warn!("no registered cluster; skipping inspect round");
        return Ok(());
    };
    let mut client = crate::cluster::k8s_client(cfg).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {
            cluster_id: cluster_id.clone(),
        })
        .await?
        .into_inner()
        .nodes;
    let pods = client
        .list_pods(ListPodsRequest {
            cluster_id: cluster_id.clone(),
            ..Default::default()
        })
        .await?
        .into_inner()
        .pods;
    let deps = client
        .list_deployments(ListDeploymentsRequest {
            cluster_id: cluster_id.clone(),
            ..Default::default()
        })
        .await?
        .into_inner()
        .deployments;

    let rules = resolve_rules(cfg).await;
    let health = evaluate_rules(&rules, &nodes, &pods, &deps, cfg.collector.max_not_ready);
    // 降噪：连续 N 周期超标才告警（alert_consecutive，Redis streak 计数，TTL 超时自动重置）
    let consecutive = cfg.collector.alert_consecutive.max(1) as u64;
    let streak_ttl = std::time::Duration::from_secs(
        cfg.collector
            .inspect_interval_secs
            .saturating_mul(consecutive + 2),
    );
    let alerts = filter_by_streak(cfg, &health.alerts, consecutive, streak_ttl).await;
    // 告警去重：open 状态窗口内同一目标不重复落库（Redis 键 TTL 自动过期 ≈ 恢复清除）
    let open_ttl = streak_ttl;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    // 值班联动：查一次当前值班人，追加进通知；库不可达仅告警不阻断
    let oncall = oncall_assignee(cfg).await;
    if let Ok(cache) = RedisCache::from_config(cfg.lock.clone()).await {
        let mut deduped = Vec::new();
        for a in &alerts {
            let key = format!(
                "alert:open:{}:{}",
                a.title,
                a.node.as_deref().unwrap_or("-")
            );
            if cache.get(&key).await.ok().flatten().is_some() {
                continue; // 已 open，跳过重复写入
            }
            deduped.push(a.clone());
            if let Err(e) = cache.set(&key, b"1".as_slice(), open_ttl).await {
                tracing::warn!(error = %e, key, "alert open set failed");
            }
        }
        // 去重后为空说明均为已 open 告警，本周期无需写入/通知
        if deduped.is_empty() {
            return Ok(());
        }
        let alerts = deduped;
        // 旧实现用 cluster_ok 短路（节点全 Ready 时吞掉 pod/deployment 告警），规则引擎只按告警有无判定
        let ch = clickhouse_from(cfg)?;
        let points = health_to_points(&alerts, now_secs());
        ecat_data::TsdbClient::write(ch.as_ref(), &points).await?;
        // 领域事件：告警同步广播到事件总线（gateway 端落 ClickHouse domain_event）
        for a in &alerts {
            let event =
                superops_protos::events::DomainEvent::new("alert", &a.level, &a.title, &a.message)
                    .with_detail(serde_json::json!({ "node": a.node }))
                    .with_ts(now_secs());
            if let Err(e) = crate::domain_events::publish_domain_event(cfg, &event).await {
                tracing::warn!("alert domain event publish failed: {e}");
            }
        }
        notify_alerts(cfg, &alerts, &http, oncall.as_deref()).await;
        if cfg.selfheal.enabled {
            execute_selfheal(cfg, &mut client, &deps, ch.as_ref(), &rules, &cluster_id).await;
        }
        return Ok(());
    }
    // Redis 不可用：降级为直接写入（不丢告警）
    let ch = clickhouse_from(cfg)?;
    let points = health_to_points(&alerts, now_secs());
    ecat_data::TsdbClient::write(ch.as_ref(), &points).await?;
    for a in &alerts {
        let event =
            superops_protos::events::DomainEvent::new("alert", &a.level, &a.title, &a.message)
                .with_detail(serde_json::json!({ "node": a.node }))
                .with_ts(now_secs());
        if let Err(e) = crate::domain_events::publish_domain_event(cfg, &event).await {
            tracing::warn!("alert domain event publish failed: {e}");
        }
    }
    notify_alerts(cfg, &alerts, &http, oncall.as_deref()).await;
    if cfg.selfheal.enabled {
        execute_selfheal(cfg, &mut client, &deps, ch.as_ref(), &rules, &cluster_id).await;
    }
    Ok(())
}

/// 告警通知分发（webhook/SMTP），去重后的告警按 target 静默窗口过滤
async fn notify_alerts(
    cfg: &Config,
    alerts: &[AlertEvent],
    http: &reqwest::Client,
    oncall: Option<&str>,
) {
    for t in &cfg.notify.targets {
        let mut silencer = crate::notify::NotifySilencer::default();
        let fresh: Vec<AlertEvent> = alerts
            .iter()
            .filter(|e| {
                let key = format!("{}:{}", e.title, e.node.as_deref().unwrap_or(""));
                silencer.should_send(&key, cfg.notify.silence_secs)
            })
            .cloned()
            .collect();
        if fresh.is_empty() {
            continue;
        }
        if let Err(e) =
            crate::notify::dispatch_with_oncall(t, &fresh, http, cfg.smtp.as_ref(), oncall).await
        {
            tracing::warn!("notify target {} failed: {e}", t.name);
        }
    }
}

/// 查询当前值班人；mysql 未配置或查询失败返回 None（联动失败不阻断告警）
async fn oncall_assignee(cfg: &Config) -> Option<String> {
    let Some(mysql) = &cfg.mysql else {
        return None;
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
        .await
        .ok()?;
    crate::notify::current_oncall(&pool).await
}
