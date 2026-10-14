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
}
