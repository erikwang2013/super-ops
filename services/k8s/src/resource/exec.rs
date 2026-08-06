use anyhow::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::{AttachParams, AttachedProcess};

use crate::cluster::client::ClusterClient;

pub fn parse_command(command: &str) -> Vec<String> {
    command.split_whitespace().map(String::from).collect()
}

pub fn exec_params(container: Option<String>) -> AttachParams {
    AttachParams {
        container,
        stdin: true,
        stdout: true,
        stderr: true,
        tty: true,
        ..Default::default()
    }
}

pub async fn exec_pod(
    client: &ClusterClient,
    namespace: String,
    pod_name: String,
    container: Option<String>,
    command: &str,
) -> Result<AttachedProcess> {
    let api: Api<Pod> = Api::namespaced(client.client.clone(), &namespace);
    api.exec(&pod_name, parse_command(command), &exec_params(container))
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_splits_whitespace() {
        assert_eq!(parse_command("ls -la /tmp"), vec!["ls", "-la", "/tmp"]);
        assert_eq!(parse_command(""), Vec::<String>::new());
    }

    #[test]
    fn exec_params_forces_tty_and_three_streams() {
        let p = exec_params(Some("sidecar".into()));
        assert_eq!(p.container.as_deref(), Some("sidecar"));
        assert!(p.stdin && p.stdout && p.stderr && p.tty);
        let p = exec_params(None);
        assert!(p.container.is_none());
        assert!(p.tty);
    }
}
