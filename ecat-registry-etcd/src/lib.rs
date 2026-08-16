// Copyright (c) 2026 erik <erik@erik.xyz> — https://erik.xyz
use async_trait::async_trait;
use ecat_registry::{Registration, Registry, RegistryError, ServiceInfo};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct EtcdRegistry {
    client: reqwest::Client,
    endpoints: Vec<String>,
    prefix: String,
    lease_ttl: u64,
    // id (prefix/name) → 本实例的 lease + 实例级 key + keepalive 停止标志
    leases: Arc<Mutex<HashMap<String, LeaseEntry>>>,
}

struct LeaseEntry {
    key: String,
    stop: Arc<AtomicBool>,
}

impl EtcdRegistry {
    pub fn new(endpoints: Vec<String>, prefix: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoints,
            prefix: prefix.into(),
            lease_ttl: 30,
            leases: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn lease_ttl(mut self, ttl: u64) -> Self {
        self.lease_ttl = ttl;
        self
    }

    fn base_url(&self) -> &str {
        self.endpoints
            .first()
            .map(|s| s.as_str())
            .unwrap_or("http://127.0.0.1:2379")
    }
}

/// 实例级注册键（多实例互不覆盖）。
fn instance_key(prefix: &str, name: &str, uuid: &str) -> String {
    format!("/ecat/services/{prefix}/{name}/{uuid}")
}

/// 某服务的发现前缀（尾部带 `/`，配合 range_end 做前缀范围查询）。
/// name 为空时返回服务层前缀（修复点：避免 `//` 双斜杠导致范围查询永不命中）。
fn service_prefix(prefix: &str, name: &str) -> String {
    if name.is_empty() {
        format!("/ecat/services/{prefix}/")
    } else {
        format!("/ecat/services/{prefix}/{name}/")
    }
}

/// lease 保活循环：按 TTL/3 间隔 keepalive，stop 置位后退出（lease 过期自动清理关联 key）。
fn spawn_keepalive(
    client: reqwest::Client,
    base_url: String,
    lease_id: i64,
    stop: Arc<AtomicBool>,
    ttl: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs((ttl / 3).max(1)));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let _ = client
                .post(format!("{base_url}/v3/lease/keepalive"))
                .json(&serde_json::json!({ "ID": lease_id.to_string() }))
                .send()
                .await;
        }
    });
}

#[async_trait]
impl Registry for EtcdRegistry {
    async fn register(&self, service: ServiceInfo) -> Result<Registration, RegistryError> {
        let id = format!("{}/{}", self.prefix, service.name);
        let lease_id = create_lease(&self.client, self.base_url(), self.lease_ttl)
            .await
            .map_err(RegistryError::Other)?;
        let key = instance_key(
            &self.prefix,
            &service.name,
            &uuid::Uuid::new_v4().to_string(),
        );
        let value = serde_json::to_string(&service)
            .map_err(|e| RegistryError::Other(format!("serialize: {e}")))?;
        let body = serde_json::json!({
            "key": b64(&key),
            "value": b64(&value),
            "lease": lease_id.to_string(),
        });
        self.client
            .post(format!("{}/v3/kv/put", self.base_url()))
            .json(&body)
            .send()
            .await
            .map_err(|e| RegistryError::Other(format!("etcd put: {e}")))?;

        // P0 修复：原实现只创建 lease 不续约，30s 后注册静默过期；现启动保活循环
        let stop = Arc::new(AtomicBool::new(false));
        spawn_keepalive(
            self.client.clone(),
            self.base_url().to_string(),
            lease_id,
            Arc::clone(&stop),
            self.lease_ttl,
        );
        self.leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.clone(),
                LeaseEntry {
                    key: key.clone(),
                    stop,
                },
            );

        Ok(Registration::new(id, service, Arc::new(self.clone())))
    }

    async fn deregister(&self, id: &str) -> Result<(), RegistryError> {
        // P0 修复：原实现按前缀范围删除该服务全部实例（多实例互相误删）；
        // 现删除本实例 key（精确范围 = key..key+1），并停止保活让 lease 过期兜底
        let entry = self
            .leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(entry) = entry {
            entry.stop.store(true, Ordering::Relaxed);
            let key = entry.key;
            let _ = self
                .client
                .post(format!("{}/v3/kv/deleterange", self.base_url()))
                .json(&serde_json::json!({
                    "key": b64(&key),
                    "range_end": b64(&prefix_end(&key)),
                }))
                .send()
                .await;
        }
        Ok(())
    }

    async fn discover(&self, name: &str) -> Result<Vec<ServiceInfo>, RegistryError> {
        let prefix = service_prefix(&self.prefix, name);
        let resp = self
            .client
            .post(format!("{}/v3/kv/range", self.base_url()))
            .json(&serde_json::json!({"key": b64(&prefix), "range_end": b64(&prefix_end(&prefix))}))
            .send()
            .await
            .map_err(|e| RegistryError::Other(format!("etcd range: {e}")))?;
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| RegistryError::Other(format!("etcd parse: {e}")))?;
        let mut services = Vec::new();
        if let Some(kvs) = result.get("kvs").and_then(|v| v.as_array()) {
            for kv in kvs {
                if let Some(v) = kv.get("value").and_then(|v| v.as_str())
                    && let Ok(svc) = decode_b64_str(v).and_then(|s| {
                        serde_json::from_str::<ServiceInfo>(&s).map_err(|e| e.to_string())
                    })
                {
                    services.push(svc);
                }
            }
        }
        Ok(services)
    }

    async fn list_services(&self) -> Result<Vec<String>, RegistryError> {
        // P0 修复：原实现 discover("") 产生双斜杠前缀（/ecat/services/{prefix}//）永不命中；
        // 现按服务层前缀直接范围查询，从 key 中提取 name 段
        let prefix = service_prefix(&self.prefix, "");
        let resp = self
            .client
            .post(format!("{}/v3/kv/range", self.base_url()))
            .json(&serde_json::json!({"key": b64(&prefix), "range_end": b64(&prefix_end(&prefix))}))
            .send()
            .await
            .map_err(|e| RegistryError::Other(format!("etcd range: {e}")))?;
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| RegistryError::Other(format!("etcd parse: {e}")))?;
        let mut names = Vec::new();
        if let Some(kvs) = result.get("kvs").and_then(|v| v.as_array()) {
            for kv in kvs {
                if let Some(k) = kv.get("key").and_then(|v| v.as_str())
                    && let Ok(k) = decode_b64_str(k)
                {
                    // key 形如 /ecat/services/{prefix}/{name}/{uuid}，取 name 段
                    let segs: Vec<&str> = k.split('/').filter(|s| !s.is_empty()).collect();
                    if segs.len() >= 3 {
                        names.push(segs[segs.len() - 2].to_string());
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }
}

async fn create_lease(client: &reqwest::Client, base_url: &str, ttl: u64) -> Result<i64, String> {
    let resp = client
        .post(format!("{base_url}/v3/lease/grant"))
        .json(&serde_json::json!({"TTL": ttl.to_string()}))
        .send()
        .await
        .map_err(|e| format!("etcd lease: {e}"))?;
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("etcd parse: {e}"))?;
    body.get("ID")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "no lease ID".into())
}

fn prefix_end(prefix: &str) -> String {
    let mut bytes = prefix.as_bytes().to_vec();
    for i in (0..bytes.len()).rev() {
        if bytes[i] < 0xff {
            bytes[i] += 1;
            bytes.truncate(i + 1);
            return String::from_utf8_lossy(&bytes).into_owned();
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

use base64::Engine;

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

fn decode_b64_str(s: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("base64: {e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etcd_registry_constructs() {
        let _reg = EtcdRegistry::new(vec!["http://localhost:2379".into()], "ecat").lease_ttl(60);
    }

    #[test]
    fn b64_roundtrip() {
        let input = "hello-world";
        let encoded = b64(input);
        let decoded = decode_b64_str(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn instance_key_and_service_prefix_formats() {
        assert_eq!(
            instance_key("superops", "collector", "abc-123"),
            "/ecat/services/superops/collector/abc-123"
        );
        // 修复点：list_services 前缀不再因 name="" 产生双斜杠
        assert_eq!(service_prefix("superops", ""), "/ecat/services/superops/");
        assert_eq!(
            service_prefix("superops", "collector"),
            "/ecat/services/superops/collector/"
        );
    }

    #[test]
    fn prefix_end_is_strictly_greater_than_prefix() {
        let p = "/ecat/services/superops/";
        let end = prefix_end(p);
        assert!(end.as_str() > p);
        assert!(end.starts_with("/ecat/services/superops"));
        // 单字节 +1 语义：范围查询覆盖全部子键
        let child = format!("{p}collector/uuid1");
        assert!(child.as_str() < end.as_str());
    }
}
