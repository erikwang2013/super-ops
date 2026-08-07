use crate::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_data::TsdbClient;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MetricsTrendQuery {
    pub measurement: String,
    pub series: String,
    pub field: String,
    #[serde(default = "default_window")]
    pub window_secs: i64,
    #[serde(default = "default_bucket")]
    pub bucket_minutes: i64,
}

fn default_window() -> i64 {
    3600
}

fn default_bucket() -> i64 {
    60
}

pub fn quote_ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

/// 时间桶趋势：每个 (bucket, series) 取桶内最新值，前端按 series 分组画多序列折线/柱状。
pub fn build_trend_sql(
    measurement: &str,
    series: &str,
    field: &str,
    window_secs: i64,
    bucket_minutes: i64,
) -> String {
    format!(
        "SELECT toStartOfInterval(timestamp, INTERVAL {bucket} MINUTE) AS bucket, {series}, \
         argMax({field}, timestamp) AS value \
         FROM {measurement} WHERE timestamp > now() - {window_secs} \
         GROUP BY bucket, {series} ORDER BY bucket",
        bucket = bucket_minutes,
        series = quote_ident(series),
        field = quote_ident(field),
        measurement = quote_ident(measurement),
    )
}

pub async fn metrics_trend(
    State(state): State<AppState>,
    Query(q): Query<MetricsTrendQuery>,
) -> impl IntoResponse {
    if q.measurement.is_empty() || q.series.is_empty() || q.field.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "measurement/series/field are required",
        )
            .into_response();
    }
    if !(60..=604_800).contains(&q.window_secs) {
        return (StatusCode::BAD_REQUEST, "window_secs must be in 60..604800").into_response();
    }
    let bucket = q.bucket_minutes.clamp(1, 1440);
    let sql = build_trend_sql(&q.measurement, &q.series, &q.field, q.window_secs, bucket);
    match TsdbClient::query(state.ch.as_ref(), &sql).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("ch query failed: {e}"),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_doubles_backticks() {
        assert_eq!(quote_ident("a`b"), "`a``b`");
    }

    #[test]
    fn trend_sql_buckets_by_interval_and_series() {
        let sql = build_trend_sql("resource_snapshot", "node", "ready", 3600, 5);
        assert!(sql.contains("INTERVAL 5 MINUTE) AS bucket"));
        assert!(sql.contains("argMax(`ready`, timestamp) AS value"));
        assert!(sql.contains("FROM `resource_snapshot`"));
        assert!(sql.contains("WHERE timestamp > now() - 3600"));
        assert!(sql.contains("GROUP BY bucket, `node` ORDER BY bucket"));
    }

    #[test]
    fn window_defaults_to_3600() {
        assert_eq!(default_window(), 3600);
    }

    #[test]
    fn bucket_defaults_to_60() {
        assert_eq!(default_bucket(), 60);
    }
}
