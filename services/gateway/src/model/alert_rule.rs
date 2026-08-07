use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const METRICS: [&str; 6] = [
    "node_not_ready",
    "node_ready_pct",
    "pod_not_running",
    "pod_running_pct",
    "deployment_unavailable",
    "deployment_ready_pct",
];
pub const OPERATORS: [&str; 2] = ["ge", "le"];
pub const LEVELS: [&str; 3] = ["INFO", "WARN", "CRIT"];
pub const ACTIONS: [&str; 3] = ["notify", "restart", "scale"];

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct AlertRuleRow {
    pub id: i64,
    pub name: String,
    pub metric: String,
    pub operator: String,
    pub threshold: String,
    pub level: String,
    pub action: String,
    pub enabled: bool,
}

pub fn validate_rule(
    name: &str,
    metric: &str,
    op: &str,
    threshold: &str,
    level: &str,
    action: &str,
) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > 128 {
        return Err("name must be 1..=128 chars".into());
    }
    if !METRICS.contains(&metric) {
        return Err(format!("metric must be one of {:?}", METRICS));
    }
    if !OPERATORS.contains(&op) {
        return Err(format!("operator must be one of {:?}", OPERATORS));
    }
    let v: f64 = threshold
        .parse()
        .map_err(|_| "threshold must be a number".to_string())?;
    if !v.is_finite() || v < 0.0 {
        return Err("threshold must be a finite non-negative number".into());
    }
    let pct = metric.ends_with("_pct");
    if pct && v > 100.0 {
        return Err("threshold for pct metrics must be 0..=100".into());
    }
    if !LEVELS.contains(&level) {
        return Err(format!("level must be one of {:?}", LEVELS));
    }
    if !ACTIONS.contains(&action) {
        return Err(format!("action must be one of {:?}", ACTIONS));
    }
    Ok(())
}

pub async fn list_rules(pool: &MySqlPool) -> sqlx::Result<Vec<AlertRuleRow>> {
    sqlx::query_as::<_, AlertRuleRow>(
        "SELECT id, name, metric, operator, threshold, level, action, enabled \
         FROM alert_rule ORDER BY id",
    )
    .fetch_all(pool)
    .await
}

pub struct CreateRuleArgs<'a> {
    pub name: &'a str,
    pub metric: &'a str,
    pub op: &'a str,
    pub threshold: &'a str,
    pub level: &'a str,
    pub action: &'a str,
    pub enabled: bool,
}

pub async fn create_rule(pool: &MySqlPool, args: CreateRuleArgs<'_>) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "INSERT INTO alert_rule (name, metric, operator, threshold, level, action, enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(args.name)
    .bind(args.metric)
    .bind(args.op)
    .bind(args.threshold)
    .bind(args.level)
    .bind(args.action)
    .bind(args.enabled)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn set_rule_enabled(pool: &MySqlPool, id: i64, enabled: bool) -> sqlx::Result<bool> {
    let r = sqlx::query("UPDATE alert_rule SET enabled = ? WHERE id = ?")
        .bind(enabled)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete_rule(pool: &MySqlPool, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM alert_rule WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_bad_fields() {
        assert!(validate_rule("", "node_not_ready", "ge", "1", "WARN", "notify").is_err());
        assert!(validate_rule("x", "vm_down", "ge", "1", "WARN", "notify").is_err());
        assert!(validate_rule("x", "node_not_ready", "xx", "1", "WARN", "notify").is_err());
        assert!(validate_rule("x", "node_not_ready", "ge", "abc", "WARN", "notify").is_err());
        assert!(validate_rule("x", "node_not_ready", "ge", "-1", "WARN", "notify").is_err());
        assert!(validate_rule("x", "node_ready_pct", "le", "120", "WARN", "notify").is_err());
        assert!(validate_rule("x", "node_not_ready", "ge", "1", "FATAL", "notify").is_err());
        assert!(validate_rule("x", "node_not_ready", "ge", "1", "WARN", "email").is_err());
    }

    #[test]
    fn validate_accepts_valid_rule() {
        assert!(validate_rule("r", "node_ready_pct", "le", "80", "CRIT", "restart").is_ok());
        assert!(validate_rule("r", "pod_not_running", "ge", "5", "INFO", "notify").is_ok());
    }
}
