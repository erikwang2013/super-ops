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
pub struct MetricsQuery {
    pub measurement: String,
    pub series: String,
    pub field: String,
    #[serde(default = "default_window")]
    pub window_secs: i64,
}

fn default_window() -> i64 {
    3600
}

pub fn quote_ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

pub fn build_query_sql(measurement: &str, series: &str, field: &str, window_secs: i64) -> String {
    format!(
        "SELECT {series}, argMax({field}, timestamp) AS value, max(timestamp) AS ts \
         FROM {measurement} WHERE timestamp > now() - {window_secs} \
         GROUP BY {series} ORDER BY {series}",
        series = quote_ident(series),
        field = quote_ident(field),
        measurement = quote_ident(measurement),
    )
}

pub async fn metrics_query(
    State(state): State<AppState>,
    Query(q): Query<MetricsQuery>,
) -> impl IntoResponse {
    if q.measurement.is_empty() || q.series.is_empty() || q.field.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "measurement/series/field are required",
        )
            .into_response();
    }
    if !(60..=86_400).contains(&q.window_secs) {
        return (StatusCode::BAD_REQUEST, "window_secs must be in 60..86400").into_response();
    }
    let sql = build_query_sql(&q.measurement, &q.series, &q.field, q.window_secs);
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
    fn query_sql_groups_by_series_latest() {
        let sql = build_query_sql("resource_snapshot", "node", "ready", 3600);
        assert!(sql.contains("argMax(`ready`, timestamp)"));
        assert!(sql.contains("FROM `resource_snapshot`"));
        assert!(sql.contains("WHERE timestamp > now() - 3600"));
        assert!(sql.contains("GROUP BY `node`"));
    }

    #[test]
    fn window_defaults_to_3600() {
        assert_eq!(default_window(), 3600);
    }
}
