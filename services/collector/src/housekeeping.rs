use crate::ch::{clickhouse_from, now_secs};
use crate::config::{Config, MysqlConfig};
use ecat_data::{DataPoint, FieldValue};
use superops_protos::k8s::v1::ListNodesRequest;
use superops_protos::k8s::v1::k8s_service_client::K8sServiceClient;

pub const DEFAULT_CPU_PRICE: f64 = 0.5;
pub const DEFAULT_MEM_PRICE: f64 = 0.25;
const BACKUP_KEEP: usize = 7;
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// 小时价 → 每天 (¥/day)
pub fn estimate_cost(cpu_cores: f64, mem_gib: f64, cpu_price: f64, mem_price: f64) -> f64 {
    (cpu_cores * cpu_price + mem_gib * mem_price) * 24.0
}

/// 单引号包裹 + 转义, 防止 shell 元字符注入
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn is_backup_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".sql.gz") else {
        return false;
    };
    stem.starts_with("superops-") && !stem.contains('/') && !stem.contains("..")
}

/// "2500m" → 2.5 核；"2" → 2.0 核；无法解析或非有限值（NaN/Inf）→ 0.0
pub fn parse_cpu_cores(s: &str) -> f64 {
    let s = s.trim();
    if let Some(v) = s.strip_suffix('m') {
        v.parse::<f64>()
            .ok()
            .map(|n| n / 1000.0)
            .filter(|n| n.is_finite())
            .unwrap_or(0.0)
    } else {
        s.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .unwrap_or(0.0)
    }
}

/// "8Gi"/"8192Mi"/"8192Ki"/"8589934592" → GiB
pub fn parse_mem_gib(s: &str) -> f64 {
    let s = s.trim();
    let (num, mult) = if let Some(v) = s.strip_suffix("Ki") {
        (v, 1024.0)
    } else if let Some(v) = s.strip_suffix("Mi") {
        (v, 1024.0 * 1024.0)
    } else if let Some(v) = s.strip_suffix("Gi") {
        (v, GIB)
    } else if let Some(v) = s.strip_suffix("Ti") {
        (v, GIB * 1024.0)
    } else if let Some(v) = s.strip_suffix('K') {
        (v, 1000.0)
    } else if let Some(v) = s.strip_suffix('M') {
        (v, 1_000_000.0)
    } else if let Some(v) = s.strip_suffix('G') {
        (v, 1_000_000_000.0)
    } else if let Some(v) = s.strip_suffix('T') {
        (v, 1_000_000_000_000.0)
    } else {
        (s, 1.0)
    };
    num.parse::<f64>()
        .ok()
        .map(|n| n * mult / GIB)
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}

/// epoch 秒 → UTC "%Y-%m-%d-%H%M%S"（workspace 无 chrono，自实现）
pub fn utc_timestamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}-{h:02}{m:02}{s:02}")
}

/// Hinnant 民用历算法: days since epoch → (year, month, day)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// mysqldump | gzip 备份; 失败仅告警跳过, 绝不崩溃 collector
pub fn backup_mysql(cfg: &MysqlConfig) -> anyhow::Result<()> {
    std::fs::create_dir_all(&cfg.backup_dir)
        .map_err(|e| anyhow::anyhow!("backup dir create failed: {e}"))?;
    let path = format!(
        "{}/superops-{}.sql.gz",
        cfg.backup_dir,
        utc_timestamp(now_secs())
    );
    // pipefail: mysqldump 中途失败会让整条管道非零退出, 损坏备份不被当作成功
    let cmd = format!(
        "set -o pipefail; mysqldump -h {} -P {} -u {} {} | gzip > {}",
        sh_quote(&cfg.host),
        cfg.port,
        sh_quote(&cfg.user),
        sh_quote(&cfg.database),
        sh_quote(&path),
    );
    // 密码走 MYSQL_PWD 环境变量, 不进 argv (ps 可见) 也不做 shell 插值
    let out = match std::process::Command::new("sh")
        .args(["-c", &cmd])
        .env("MYSQL_PWD", &cfg.password)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("mysqldump unavailable, backup skipped: {e}");
            return Ok(());
        }
    };
    if !out.status.success() {
        tracing::warn!(
            "mysqldump failed, backup skipped: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        // 管道失败会留下半成品 .sql.gz（仍匹配备份命名规则），删除以免被保留/误当有效备份
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    tracing::info!(path = %path, "mysql backup written");
    prune_backups(&cfg.backup_dir, BACKUP_KEEP);
    Ok(())
}

/// 按 mtime 保留最新 `keep` 个匹配备份命名规则的文件, 其余删除
fn prune_backups(dir: &str, keep: usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("backup prune: cannot read dir {dir}: {e}");
            return;
        }
    };
    let mut files: Vec<(std::path::PathBuf, i64)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !is_backup_file(&name) {
                return None;
            }
            let mtime = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Some((e.path(), mtime))
        })
        .collect();
    files.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    for (path, _) in files.into_iter().skip(keep) {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!("backup prune failed for {}: {e}", path.display());
        }
    }
}

/// 汇总全部 Node allocatable 容量并写入 capacity_snapshot
pub async fn collect_capacity(cfg: &Config) -> anyhow::Result<()> {
    let mut client = K8sServiceClient::connect(cfg.k8s.endpoint.clone()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest::default())
        .await?
        .into_inner()
        .nodes;
    let mut cores = 0.0f64;
    let mut gib = 0.0f64;
    for n in &nodes {
        cores += parse_cpu_cores(&n.cpu);
        gib += parse_mem_gib(&n.memory);
    }
    let ch = clickhouse_from(cfg)?;
    let point = DataPoint::new("capacity_snapshot")
        .with_tag("cluster", "default")
        .with_field("cpu_cores", FieldValue::Float(cores))
        .with_field("mem_gib", FieldValue::Float(gib))
        .with_field("node_count", FieldValue::Int(nodes.len() as i64))
        .with_timestamp(now_secs());
    ecat_data::TsdbClient::write(ch.as_ref(), &[point]).await?;
    tracing::info!(
        nodes = nodes.len(),
        cpu_cores = cores,
        mem_gib = gib,
        est_cost_yuan_day = estimate_cost(cores, gib, DEFAULT_CPU_PRICE, DEFAULT_MEM_PRICE),
        "capacity snapshot written"
    );
    Ok(())
}

pub async fn housekeeping_once(cfg: &Config) -> anyhow::Result<()> {
    if let Some(mysql) = &cfg.mysql {
        backup_mysql(mysql)?;
    }
    collect_capacity(cfg).await?;
    Ok(())
}
