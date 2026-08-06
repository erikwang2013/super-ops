use anyhow::Result;
use k8s_openapi::api::{
    batch::v1::Job,
    core::v1::{Container, PodSpec, PodTemplateSpec},
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, PostParams};

use crate::cluster::client::ClusterClient;

pub fn validate_job(
    cluster_id: &str,
    namespace: &str,
    job_name: &str,
    image: &str,
    command: &str,
) -> Result<()> {
    if cluster_id.is_empty() || namespace.is_empty() || job_name.is_empty() || image.is_empty() {
        return Err(anyhow::anyhow!(
            "cluster_id/namespace/job_name/image must not be empty"
        ));
    }
    if command.is_empty() {
        return Err(anyhow::anyhow!("command must not be empty"));
    }
    if job_name.len() > 63
        || !job_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(anyhow::anyhow!(
            "job_name must be DNS-1123 (<=63, [a-z0-9-])"
        ));
    }
    Ok(())
}

pub async fn run_job(
    client: &ClusterClient,
    namespace: &str,
    job_name: &str,
    image: &str,
    command: &str,
    timeout_s: i32,
) -> Result<bool> {
    let api = Api::<Job>::namespaced(client.client.clone(), namespace);
    let job = Job {
        metadata: ObjectMeta {
            name: Some(job_name.into()),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::batch::v1::JobSpec {
            template: PodTemplateSpec {
                spec: Some(PodSpec {
                    restart_policy: Some("Never".into()),
                    containers: vec![Container {
                        name: "runner".into(),
                        image: Some(image.into()),
                        command: Some(vec!["/bin/sh".into(), "-c".into(), command.into()]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            backoff_limit: Some(0),
            active_deadline_seconds: Some(i64::from(timeout_s.max(1))),
            ..Default::default()
        }),
        ..Default::default()
    };
    api.create(&PostParams::default(), &job).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_fields() -> (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) {
        ("c", "ns", "job-1", "busybox:1.36", "echo hi")
    }

    #[test]
    fn validate_job_rejects_empty_fields() {
        let (c, ns, j, img, cmd) = ok_fields();
        assert!(validate_job("", ns, j, img, cmd).is_err());
        assert!(validate_job(c, "", j, img, cmd).is_err());
        assert!(validate_job(c, ns, "", img, cmd).is_err());
        assert!(validate_job(c, ns, j, "", cmd).is_err());
    }

    #[test]
    fn validate_job_rejects_empty_command() {
        let (c, ns, j, img, _) = ok_fields();
        assert!(validate_job(c, ns, j, img, "").is_err());
    }

    #[test]
    fn validate_job_rejects_uppercase_and_overlong_name() {
        let (c, ns, _, img, cmd) = ok_fields();
        assert!(validate_job(c, ns, "UPPER", img, cmd).is_err());
        // 63 字节上限是字节数：64 字节 ASCII 名必须拒绝
        let long = "a".repeat(64);
        assert!(validate_job(c, ns, &long, img, cmd).is_err());
    }

    #[test]
    fn validate_job_accepts_valid_names() {
        let (c, ns, _, img, cmd) = ok_fields();
        assert!(validate_job(c, ns, "job-1", img, cmd).is_ok());
        let max = "a".repeat(63);
        assert!(validate_job(c, ns, &max, img, cmd).is_ok());
    }
}
