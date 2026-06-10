//! Docker integration via direct Unix socket communication.
//! Avoids the bollard crate and its deep dependency tree.

use anyhow::Result;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerStatus {
    Running,
    Stopped,
    Starting,
    Restarting,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id:     String,
    pub name:   String,
    pub image:  String,
    pub status: ContainerStatus,
}

#[derive(Deserialize)]
struct DockerContainer {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Names")]
    names: Vec<String>,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
}

const SOCKET:  &str = "/var/run/docker.sock";
const REQUEST: &str = "GET /containers/json?all=1 HTTP/1.0\r\nHost: localhost\r\n\r\n";

/// Poll the Docker Unix socket. Returns empty Vec if Docker is unavailable.
pub async fn poll_containers() -> Result<Vec<ContainerInfo>> {
    let Ok(mut stream) = UnixStream::connect(SOCKET).await else {
        return Ok(vec![]);
    };

    stream.write_all(REQUEST.as_bytes()).await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;

    let response = String::from_utf8_lossy(&buf);
    let Some(body_start) = response.find("\r\n\r\n") else {
        return Ok(vec![]);
    };
    let body = &response[body_start + 4..];

    let containers: Vec<DockerContainer> = match serde_json::from_str(body) {
        Ok(v)  => v,
        Err(_) => return Ok(vec![]),
    };

    Ok(containers.into_iter().map(|c| {
        let name = c.names
            .into_iter()
            .next()
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string();

        let status = match c.state.as_str() {
            "running"    => ContainerStatus::Running,
            "exited"     => ContainerStatus::Stopped,
            "restarting" => ContainerStatus::Restarting,
            "created"    => ContainerStatus::Starting,
            other        => ContainerStatus::Other(other.to_string()),
        };

        ContainerInfo {
            id:    c.id[..c.id.len().min(12)].to_string(),
            name,
            image: c.image,
            status,
        }
    }).collect())
}
