use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const SCRIPT_LANGUAGES: [&str; 3] = ["shell", "python", "go"];

pub fn validate_script_input(name: &str, language: &str, content: &str) -> Result<(), String> {
    let n = name.chars().count();
    if !(1..=128).contains(&n) {
        return Err(format!("name must be 1..=128 chars, got {n}"));
    }
    if !SCRIPT_LANGUAGES.contains(&language) {
        return Err(format!("language must be one of {:?}", SCRIPT_LANGUAGES));
    }
    if content.is_empty() {
        return Err("content must not be empty".into());
    }
    if content.len() > 65536 {
        return Err(format!(
            "content too large ({} bytes, max 65536)",
            content.len()
        ));
    }
    Ok(())
}

pub fn validate_timeout_s(timeout_s: i32) -> Result<(), String> {
    if !(1..=3600).contains(&timeout_s) {
        return Err(format!("timeout_s must be in 1..=3600, got {timeout_s}"));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ScriptRow {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub language: String,
    pub timeout_s: i32,
    pub created_by: String,
    // sqlx 的 chrono 仅启用 clock 特性（无 serde），故时间列用字符串 + SQL 格式化（同 user.rs 约定）
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ScriptRunRow {
    pub id: i64,
    pub script_id: i64,
    pub target_pods: String,
    pub status: String,
    pub output: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

pub async fn list_scripts(pool: &MySqlPool, tenant: &str) -> sqlx::Result<Vec<ScriptRow>> {
    sqlx::query_as(
        "SELECT id, name, description, language, timeout_s, created_by, \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') FROM script \
         WHERE tenant_id = ? ORDER BY id DESC LIMIT 500",
    )
    .bind(tenant)
    .fetch_all(pool)
    .await
}

pub async fn get_script(
    pool: &MySqlPool,
    tenant: &str,
    id: i64,
) -> sqlx::Result<Option<ScriptRow>> {
    sqlx::query_as(
        "SELECT id, name, description, language, timeout_s, created_by, \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') FROM script WHERE id = ? AND tenant_id = ?",
    )
    .bind(id)
    .bind(tenant)
    .fetch_optional(pool)
    .await
}

pub async fn create_script(
    pool: &MySqlPool,
    tenant: &str,
    name: &str,
    description: &str,
    language: &str,
    content: &str,
    timeout_s: i32,
    created_by: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO script (tenant_id, name, description, language, content, timeout_s, created_by) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(tenant)
    .bind(name)
    .bind(description)
    .bind(language)
    .bind(content)
    .bind(timeout_s)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn get_script_content(
    pool: &MySqlPool,
    tenant: &str,
    id: i64,
) -> sqlx::Result<Option<String>> {
    sqlx::query_as::<_, (String,)>("SELECT content FROM script WHERE id = ? AND tenant_id = ?")
        .bind(id)
        .bind(tenant)
        .fetch_optional(pool)
        .await
        .map(|r| r.map(|(c,)| c))
}

pub async fn delete_script(pool: &MySqlPool, tenant: &str, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM script WHERE id = ? AND tenant_id = ?")
        .bind(id)
        .bind(tenant)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn create_run(pool: &MySqlPool, script_id: i64, target_pods: &str) -> sqlx::Result<i64> {
    let r = sqlx::query("INSERT INTO script_run (script_id, target_pods) VALUES (?, ?)")
        .bind(script_id)
        .bind(target_pods)
        .execute(pool)
        .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn mark_run_status(pool: &MySqlPool, run_id: i64, status: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE script_run SET status = ?, \
         finished_at = CASE WHEN ? IN ('failed','succeeded') THEN NOW() ELSE finished_at END \
         WHERE id = ?",
    )
    .bind(status)
    .bind(status)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_runs(
    pool: &MySqlPool,
    tenant: &str,
    script_id: Option<i64>,
    limit: i64,
) -> sqlx::Result<Vec<ScriptRunRow>> {
    // script_run 表不加 tenant_id 列（计划决策），运行记录经 script 表 JOIN 按租户过滤
    let mut b = sqlx::QueryBuilder::new(
        "SELECT r.id, r.script_id, r.target_pods, r.status, r.output, \
         DATE_FORMAT(r.started_at, '%Y-%m-%d %H:%i:%s'), \
         DATE_FORMAT(r.finished_at, '%Y-%m-%d %H:%i:%s') FROM script_run r \
         JOIN script s ON s.id = r.script_id WHERE s.tenant_id = ",
    );
    b.push_bind(tenant);
    if let Some(id) = script_id {
        b.push(" AND r.script_id = ").push_bind(id);
    }
    b.push(" ORDER BY r.id DESC LIMIT ").push_bind(limit);
    b.build_query_as::<ScriptRunRow>().fetch_all(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_bad_input() {
        assert!(validate_script_input("", "shell", "x").is_err());
        assert!(validate_script_input(&"a".repeat(129), "shell", "x").is_err());
        assert!(validate_script_input("ok", "powershell", "x").is_err());
        assert!(validate_script_input("ok", "shell", "").is_err());
        assert!(validate_script_input("ok", "shell", &"x".repeat(65537)).is_err());
    }

    #[test]
    fn validate_accepts_valid_input() {
        assert!(validate_script_input("备份脚本", "python", "print(1)").is_ok());
        assert!(validate_script_input(&"a".repeat(128), "go", &"x".repeat(65536)).is_ok());
    }

    #[test]
    fn validate_timeout_rejects_out_of_range() {
        assert!(validate_timeout_s(0).is_err());
        assert!(validate_timeout_s(3601).is_err());
        assert!(validate_timeout_s(-5).is_err());
        assert!(validate_timeout_s(1).is_ok());
        assert!(validate_timeout_s(300).is_ok());
        assert!(validate_timeout_s(3600).is_ok());
    }
}
