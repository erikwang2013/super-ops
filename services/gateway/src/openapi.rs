use ecat_openapi::{OpenApiBuilder, OpenApiSpec, string_schema};
use std::collections::HashMap;

pub fn build_spec() -> OpenApiSpec {
    OpenApiBuilder::new("SuperOps Gateway API", env!("CARGO_PKG_VERSION"))
        .add_route("/api/health", "GET", "Health check", vec!["system".into()])
        .add_route(
            "/api/auth/register",
            "POST",
            "Register a new user",
            vec!["auth".into()],
        )
        .add_route(
            "/api/auth/login",
            "POST",
            "Login and get tokens",
            vec!["auth".into()],
        )
        .add_route(
            "/api/k8s/clusters",
            "GET",
            "List clusters",
            vec!["k8s".into()],
        )
        .add_route(
            "/api/k8s/clusters/{cluster_id}/pods",
            "GET",
            "List pods",
            vec!["k8s".into()],
        )
        .add_route(
            "/api/k8s/clusters/{cluster_id}/deployments",
            "GET",
            "List deployments",
            vec!["k8s".into()],
        )
        .add_route(
            "/api/k8s/clusters/{cluster_id}/nodes",
            "GET",
            "List nodes",
            vec!["k8s".into()],
        )
        .add_route(
            "/api/k8s/clusters/{cluster_id}/metrics",
            "GET",
            "Cluster metrics",
            vec!["k8s".into()],
        )
        .add_route("/api/keys", "GET", "List API keys", vec!["auth".into()])
        .add_route("/api/keys", "POST", "Create API key", vec!["auth".into()])
        .add_route(
            "/api/keys/{id}",
            "DELETE",
            "Delete API key",
            vec!["auth".into()],
        )
        .add_route(
            "/api/docs",
            "GET",
            "This OpenAPI document",
            vec!["system".into()],
        )
        .add_schema(
            "LoginRequest",
            HashMap::from([
                ("username".into(), string_schema()),
                ("password".into(), string_schema()),
            ]),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_is_openapi_3_0_3() {
        let spec = build_spec();
        assert_eq!(spec.openapi, "3.0.3");
        assert_eq!(spec.info.title, "SuperOps Gateway API");
    }

    #[test]
    fn spec_covers_core_routes() {
        let spec = build_spec();
        assert!(spec.paths.contains_key("/api/auth/login"));
        assert!(spec.paths.contains_key("/api/k8s/clusters"));
        let login = spec.paths.get("/api/auth/login").unwrap();
        assert!(login.post.is_some());
        let docs = spec.paths.get("/api/docs").unwrap();
        assert!(docs.get.is_some());
    }
}
