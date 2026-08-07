use crate::ch::{clickhouse_from, now_secs};
use crate::config::Config;
use ecat_data::{DataPoint, FieldValue};
use ecat_data_redis::RedisLock;
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

pub fn evaluate_rules(
    rules: &[AlertRule],
    nodes: &[Node],
    pods: &[Pod],
    deps: &[Deployment],
) -> ClusterHealth {
    let mut alerts = Vec::new();
    for rule in rules {
        alerts.extend(evaluate_rule(rule, nodes, pods, deps));
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
    client: &mut K8sServiceClient<tonic::transport::Channel>,
    deps: &[Deployment],
    ch: &dyn ecat_data::TsdbClient,
    rules: &[AlertRule],
) {
    let targets = selfheal_targets(rules, deps, cfg.selfheal.max_actions_per_cycle);
    if targets.is_empty() {
        return;
    }
    tracing::info!(count = targets.len(), "selfheal actions to execute");
    for (ns, name, action) in targets {
        let out: Result<(), tonic::Status> = if action == "restart" {
            client
                .restart_deployment(superops_protos::k8s::v1::RestartDeploymentRequest {
                    cluster_id: cfg.selfheal.cluster_id.clone(),
                    namespace: ns.clone(),
                    name: name.clone(),
                })
                .await
                .map(|_| ())
        } else if action == "scale" {
            let replicas = deps
                .iter()
                .find(|d| d.namespace == ns && d.name == name)
                .map(|d| d.replicas + 1)
                .unwrap_or(1);
            client
                .scale_deployment(superops_protos::k8s::v1::ScaleDeploymentRequest {
                    cluster_id: cfg.selfheal.cluster_id.clone(),
                    namespace: ns.clone(),
                    name: name.clone(),
                    replicas,
                })
                .await
                .map(|_| ())
        } else {
            continue;
        };
        match out {
            Ok(_) => {
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
    let mut client = K8sServiceClient::connect(cfg.k8s.endpoint.clone()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest::default())
        .await?
        .into_inner()
        .nodes;
    let pods = client
        .list_pods(ListPodsRequest::default())
        .await?
        .into_inner()
        .pods;
    let deps = client
        .list_deployments(ListDeploymentsRequest::default())
        .await?
        .into_inner()
        .deployments;

    let rules = resolve_rules(cfg).await;
    let health = evaluate_rules(&rules, &nodes, &pods, &deps);
    // 旧实现用 cluster_ok 短路（节点全 Ready 时吞掉 pod/deployment 告警），规则引擎只按告警有无判定
    if health.alerts.is_empty() {
        return Ok(());
    }
    let ch = clickhouse_from(cfg)?;
    let points = health_to_points(&health.alerts, now_secs());
    ecat_data::TsdbClient::write(ch.as_ref(), &points).await?;

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    for t in &cfg.notify.targets {
        let mut silencer = crate::notify::NotifySilencer::default();
        let fresh: Vec<AlertEvent> = health
            .alerts
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
        if let Err(e) = crate::notify::dispatch(t, &fresh, &http, cfg.smtp.as_ref()).await {
            tracing::warn!("notify target {} failed: {e}", t.name);
        }
    }
    if cfg.selfheal.enabled {
        execute_selfheal(cfg, &mut client, &deps, ch.as_ref(), &rules).await;
    }
    Ok(())
}
