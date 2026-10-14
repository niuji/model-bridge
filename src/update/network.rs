use super::journal;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub proxy_url: String,
}

impl Settings {
    pub fn validate(&mut self) -> Result<()> {
        self.proxy_url = self.proxy_url.trim().to_owned();
        if self.proxy_url.is_empty() {
            return Ok(());
        }
        let url = reqwest::Url::parse(&self.proxy_url)
            .map_err(|_| anyhow::anyhow!("请输入有效的 HTTP/HTTPS 代理地址"))?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && matches!(url.path(), "" | "/")
                && url.query().is_none()
                && url.fragment().is_none(),
            "代理仅支持 HTTP/HTTPS 地址，不能包含路径、查询参数或片段"
        );
        Ok(())
    }

    pub fn read(root: &Path) -> Result<Self> {
        let mut settings: Self = journal::read_json(&root.join("proxy.json"))
            .context("无法读取更新代理配置")?
            .unwrap_or_default();
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        journal::write_json(&root.join("proxy.json"), self).context("无法保存更新代理配置")
    }

    pub async fn verify(&self) -> Result<()> {
        // Clearing a broken proxy must remain possible even while the network is unavailable.
        if self.proxy_url.is_empty() {
            return Ok(());
        }
        let client = super::release::client_for_settings(self)?;
        verify_request(
            &client,
            "https://github.com/niuji/model-bridge/releases/latest",
        )
        .await
    }
}

async fn verify_request(client: &reqwest::Client, url: &str) -> Result<()> {
    let response = client.get(url)
        .timeout(std::time::Duration::from_secs(8))
        .send().await.map_err(|error| {
            // Do not expose proxy credentials through lower-level connection errors.
            if error.is_timeout() {
                anyhow::anyhow!("代理验证失败：连接 GitHub 超时（8 秒），原配置未修改")
            } else {
                anyhow::anyhow!("代理验证失败：无法通过代理连接 GitHub，请检查代理地址、认证和网络，原配置未修改")
            }
        })?;
    ensure!(
        response.status().is_success(),
        "代理验证失败：GitHub 返回 HTTP {}，原配置未修改",
        response.status().as_u16()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn proxy_probe_requires_success_from_update_source() {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for status in [200, 403, 407, 500] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            let result = verify_request(&client, &server.uri()).await;
            assert_eq!(result.is_ok(), status == 200, "status {status}");
            if let Err(error) = result {
                assert!(error.to_string().contains(&status.to_string()));
            }
        }
    }

    #[tokio::test]
    async fn proxy_probe_bounds_slow_upstream() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(10)))
            .mount(&server)
            .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(9),
            verify_request(&client, &server.uri()),
        )
        .await
        .expect("probe must finish within its timeout");
        assert!(result.unwrap_err().to_string().contains("超时"));
    }
}
