use ecat_registry::ServiceInfo;

pub fn resolve_k8s_endpoint(discovered: &[ServiceInfo], fallback: &str) -> String {
    discovered
        .iter()
        .find_map(|info| info.endpoints.first())
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_discovery_uses_fallback() {
        assert_eq!(
            resolve_k8s_endpoint(&[], "http://localhost:9091"),
            "http://localhost:9091"
        );
    }

    #[test]
    fn first_endpoint_wins() {
        let discovered = vec![
            ServiceInfo::new("superops-k8s", "1.0.0").with_endpoint("http://10.0.0.1:9091"),
            ServiceInfo::new("superops-k8s", "1.0.0").with_endpoint("http://10.0.0.2:9091"),
        ];
        assert_eq!(
            resolve_k8s_endpoint(&discovered, "http://localhost:9091"),
            "http://10.0.0.1:9091"
        );
    }

    #[test]
    fn service_without_endpoints_falls_back() {
        let discovered = vec![ServiceInfo::new("superops-k8s", "1.0.0")];
        assert_eq!(
            resolve_k8s_endpoint(&discovered, "http://localhost:9091"),
            "http://localhost:9091"
        );
    }
}
