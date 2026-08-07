use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunbookStep {
    pub name: String,
    pub script_id: i64,
    pub timeout_s: i32,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct RunbookRow {
    pub id: i64,
    pub name: String,
    pub description: String,
    /// steps 原始 JSON（含脚本名快照，供前端展示）
    pub steps: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct RunbookRunRow {
    pub id: i64,
    pub runbook_id: i64,
    pub runbook_name: String,
    pub target_pods: String,
    pub status: String,
    pub output: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// 校验 steps JSON 并返回解析后的步骤（顺序执行前再校验脚本存在性）。
pub fn validate_steps(steps_json: &str) -> Result<Vec<RunbookStep>, String> {
    let steps: Vec<RunbookStep> =
        serde_json::from_str(steps_json).map_err(|e| format!("steps 必须是 JSON 数组: {e}"))?;
    if steps.is_empty() {
        return Err("steps 至少 1 步".into());
    }
    if steps.len() > 20 {
        return Err("steps 最多 20 步".into());
    }
    for s in &steps {
        let n = s.name.chars().count();
        if !(1..=128).contains(&n) {
            return Err(format!("step name 长度需 1..=128，当前 {n}"));
        }
        if s.script_id <= 0 {
            return Err("script_id 必须为正整数".into());
        }
        if !(1..=3600).contains(&s.timeout_s) {
            return Err(format!("step timeout_s 需 1..=3600，当前 {}", s.timeout_s));
        }
    }
    Ok(steps)
}

pub async fn list_runbooks(pool: &MySqlPool, tenant: &str) -> sqlx::Result<Vec<RunbookRow>> {
    sqlx::query_as(
        "SELECT id, name, description, CAST(steps AS CHAR) AS steps, created_by, \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') FROM runbook \
         WHERE tenant_id = ? ORDER BY id DESC LIMIT 200",
    )
    .bind(tenant)
    .fetch_all(pool)
    .await
}

pub async fn get_runbook(
    pool: &MySqlPool,
    tenant: &str,
    id: i64,
) -> sqlx::Result<Option<RunbookRow>> {
    sqlx::query_as(
        "SELECT id, name, description, CAST(steps AS CHAR) AS steps, created_by, \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') FROM runbook WHERE id = ? AND tenant_id = ?",
    )
    .bind(id)
    .bind(tenant)
    .fetch_optional(pool)
    .await
}

pub async fn create_runbook(
    pool: &MySqlPool,
    tenant: &str,
    name: &str,
    description: &str,
    steps_json: &str,
    created_by: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO runbook (tenant_id, name, description, steps, created_by) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(tenant)
    .bind(name)
    .bind(description)
    .bind(steps_json)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn delete_runbook(pool: &MySqlPool, tenant: &str, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM runbook WHERE id = ? AND tenant_id = ?")
        .bind(id)
        .bind(tenant)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn create_runbook_run(
    pool: &MySqlPool,
    runbook_id: i64,
    target_pods: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO runbook_run (runbook_id, target_pods, status) VALUES (?, ?, 'running')",
    )
    .bind(runbook_id)
    .bind(target_pods)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn mark_runbook_run(
    pool: &MySqlPool,
    run_id: i64,
    status: &str,
    output: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE runbook_run SET status = ?, output = ?, \
         finished_at = CASE WHEN ? IN ('ok','failed') THEN NOW() ELSE finished_at END \
         WHERE id = ?",
    )
    .bind(status)
    .bind(output)
    .bind(status)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_runbook_runs(
    pool: &MySqlPool,
    tenant: &str,
    limit: i64,
) -> sqlx::Result<Vec<RunbookRunRow>> {
    sqlx::query_as(
        "SELECT r.id, r.runbook_id, rb.name AS runbook_name, r.target_pods, r.status, r.output, \
         DATE_FORMAT(r.started_at, '%Y-%m-%d %H:%i:%s'), \
         DATE_FORMAT(r.finished_at, '%Y-%m-%d %H:%i:%s') \
         FROM runbook_run r JOIN runbook rb ON rb.id = r.runbook_id \
         WHERE rb.tenant_id = ? ORDER BY r.id DESC LIMIT ?",
    )
    .bind(tenant)
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_steps_accepts_valid() {
        let steps = r#"[{"name":"检查","script_id":1,"timeout_s":30}]"#;
        let parsed = validate_steps(steps).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "检查");
        assert_eq!(parsed[0].script_id, 1);
        assert_eq!(parsed[0].timeout_s, 30);
    }

    #[test]
    fn validate_steps_rejects_bad() {
        assert!(validate_steps("[]").is_err());
        assert!(validate_steps("not-json").is_err());
        assert!(validate_steps(r#"[{"name":"","script_id":1,"timeout_s":30}]"#).is_err());
        assert!(validate_steps(r#"[{"name":"x","script_id":0,"timeout_s":30}]"#).is_err());
        assert!(validate_steps(r#"[{"name":"x","script_id":1,"timeout_s":0}]"#).is_err());
        assert!(validate_steps(r#"[{"name":"x","script_id":1,"timeout_s":3601}]"#).is_err());
        let too_many = (0..21)
            .map(|_| r#"{"name":"x","script_id":1,"timeout_s":30}"#)
            .collect::<Vec<_>>()
            .join(",");
        assert!(validate_steps(&format!("[{too_many}]")).is_err());
    }
}
