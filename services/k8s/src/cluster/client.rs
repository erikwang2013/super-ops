use anyhow::Result;
use kube::Client;

#[derive(Clone)]
pub struct ClusterClient {
    pub id: String,
    pub name: String,
    pub client: Client,
}

impl ClusterClient {
    pub async fn new(id: String, name: String, kubeconfig: &[u8]) -> Result<Self> {
        let kubeconfig: kube::config::Kubeconfig = serde_yaml::from_slice(kubeconfig)?;
        let config = kube::Config::from_custom_kubeconfig(kubeconfig, &Default::default()).await?;
        let client = Client::try_from(config)?;
        Ok(Self { id, name, client })
    }
}
