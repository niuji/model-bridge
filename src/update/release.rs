use anyhow::{bail, ensure, Context, Result};
use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

pub const ASSET: &str = "model-bridge-linux-amd64.tar.gz";
pub const TARGET: &str = "x86_64-unknown-linux-musl";
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag: String,
    pub asset_url: String,
    pub sha256: String,
    pub size: u64,
    pub html_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    target: String,
    asset: String,
    sha256: String,
    size: u64,
    update_protocol: u32,
}

struct Source {
    api: String,
    web: String,
}

impl Source {
    fn github() -> Self {
        Self {
            api: "https://api.github.com".into(),
            web: "https://github.com".into(),
        }
    }

    #[cfg(test)]
    fn test(base: &str) -> Self {
        Self {
            api: base.into(),
            web: base.into(),
        }
    }

    fn asset_url(&self, url: &str, tag: &str, name: &str) -> Result<()> {
        ensure!(
            url == format!(
                "{}/niuji/model-bridge/releases/download/{tag}/{name}",
                self.web
            ),
            "unexpected release asset URL"
        );
        Ok(())
    }
}

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .https_only(true)
        .user_agent(concat!("model-bridge-updater/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many release redirects");
            }
            if attempt.url().scheme() != "https"
                || !matches!(
                    attempt.url().host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                return attempt.error("unexpected release redirect host");
            }
            attempt.follow()
        }))
        .build()?)
}

fn stable_version(version: &str) -> Result<Version> {
    let parsed = Version::parse(version).context("invalid release version")?;
    ensure!(
        parsed.pre.is_empty() && parsed.build.is_empty() && parsed.to_string() == version,
        "expected canonical stable version"
    );
    Ok(parsed)
}

async fn bytes(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    let mut result = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= limit.saturating_sub(result.len()),
            "release metadata exceeds size limit"
        );
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}

async fn metadata(client: &Client, source: &Source, endpoint: &str) -> Result<GithubRelease> {
    let url = format!(
        "{}/repos/niuji/model-bridge/releases/{endpoint}",
        source.api
    );
    Ok(serde_json::from_slice(
        &bytes(client, &url, 2 * 1024 * 1024).await?,
    )?)
}

pub async fn check_release(client: &Client, current: &str) -> Result<Option<ReleaseInfo>> {
    check_from(client, current, &Source::github()).await
}

async fn check_from(
    client: &Client,
    current: &str,
    source: &Source,
) -> Result<Option<ReleaseInfo>> {
    let current = stable_version(current)?;
    let release = metadata(client, source, "latest").await?;
    ensure!(
        !release.draft && !release.prerelease,
        "latest release is not stable"
    );
    let version = release
        .tag_name
        .strip_prefix('v')
        .context("release tag must begin with v")?;
    if stable_version(version)? <= current {
        return Ok(None);
    }
    Ok(Some(validate(client, source, release).await?))
}

pub async fn pinned_release(client: &Client, version: &str) -> Result<ReleaseInfo> {
    pinned_from(client, version, &Source::github()).await
}

async fn pinned_from(client: &Client, version: &str, source: &Source) -> Result<ReleaseInfo> {
    stable_version(version)?;
    let tag = format!("v{version}");
    let release = metadata(client, source, &format!("tags/{tag}")).await?;
    ensure!(
        release.tag_name == tag,
        "release tag does not match requested version"
    );
    validate(client, source, release).await
}

async fn validate(client: &Client, source: &Source, release: GithubRelease) -> Result<ReleaseInfo> {
    ensure!(
        !release.draft && !release.prerelease,
        "release is not stable"
    );
    let version = release
        .tag_name
        .strip_prefix('v')
        .context("release tag must begin with v")?;
    stable_version(version)?;
    let asset = |name: &str| -> Result<String> {
        let matches: Vec<_> = release
            .assets
            .iter()
            .filter(|asset| asset.name == name)
            .collect();
        ensure!(
            matches.len() == 1,
            "release must contain exactly one {name}"
        );
        let url = &matches[0].browser_download_url;
        source.asset_url(url, &release.tag_name, name)?;
        Ok(url.clone())
    };
    let asset_url = asset(ASSET)?;
    let manifest_url = asset("update-manifest.json")?;
    let checksums_url = asset("SHA256SUMS")?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes(client, &manifest_url, 64 * 1024).await?)?;
    ensure!(
        manifest.version == version
            && manifest.target == TARGET
            && manifest.asset == ASSET
            && manifest.update_protocol == 1,
        "release manifest does not match supported update protocol, version, target or asset"
    );
    ensure!(
        manifest.size > 0 && manifest.size <= MAX_BYTES,
        "release archive size is outside allowed limits"
    );
    ensure!(
        manifest.sha256.len() == 64
            && manifest
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid SHA-256 digest"
    );
    let sums = String::from_utf8(bytes(client, &checksums_url, 1024 * 1024).await?)?;
    let mut found = None;
    for line in sums.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 2 && fields[1].trim_start_matches('*') == ASSET {
            ensure!(found.is_none(), "duplicate archive checksum");
            found = Some(fields[0]);
        }
    }
    ensure!(
        found == Some(manifest.sha256.as_str()),
        "SHA256SUMS does not match release manifest"
    );
    Ok(ReleaseInfo {
        version: version.into(),
        html_url: format!(
            "{}/niuji/model-bridge/releases/tag/{}",
            source.web, release.tag_name
        ),
        tag: release.tag_name,
        asset_url,
        sha256: manifest.sha256,
        size: manifest.size,
    })
}

pub async fn download(
    client: &Client,
    release: &ReleaseInfo,
    destination: &Path,
    progress: impl FnMut(u64),
) -> Result<()> {
    download_from(client, release, destination, &Source::github(), progress).await
}

async fn download_from(
    client: &Client,
    release: &ReleaseInfo,
    destination: &Path,
    source: &Source,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    stable_version(&release.version)?;
    ensure!(
        release.tag == format!("v{}", release.version),
        "release tag/version mismatch"
    );
    source.asset_url(&release.asset_url, &release.tag, ASSET)?;
    ensure!(
        release.size > 0 && release.size <= MAX_BYTES,
        "release archive size is outside allowed limits"
    );
    progress(0);
    let mut response = client
        .get(&release.asset_url)
        .send()
        .await?
        .error_for_status()?;
    if let Some(length) = response.content_length() {
        ensure!(
            length == release.size,
            "release archive content length mismatch"
        );
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await?;
    let result = async {
        let mut size = 0u64;
        let mut digest = Sha256::new();
        while let Some(chunk) = response.chunk().await? {
            size += chunk.len() as u64;
            ensure!(
                size <= release.size && size <= MAX_BYTES,
                "release archive exceeds size limit"
            );
            digest.update(&chunk);
            file.write_all(&chunk).await?;
            progress(size);
        }
        ensure!(size == release.size, "release archive size mismatch");
        if format!("{:x}", digest.finalize()) != release.sha256 {
            bail!("release archive SHA-256 mismatch");
        }
        file.sync_all().await?;
        Ok(())
    }
    .await;
    drop(file);
    if result.is_err() {
        let _ = tokio::fs::remove_file(destination).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

    async fn fixture(
        manifest_change: Option<(&str, Value)>,
        checksums: Option<String>,
    ) -> (MockServer, Client) {
        let server = MockServer::start().await;
        let digest = format!("{:x}", Sha256::digest(b"archive"));
        let assets: Vec<Value> = [ASSET, "update-manifest.json", "SHA256SUMS"].iter().map(|name| json!({"name":name,"browser_download_url":format!("{}/niuji/model-bridge/releases/download/v1.2.3/{}", server.uri(),name)})).collect();
        Mock::given(path("/repos/niuji/model-bridge/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":assets}),
            ))
            .mount(&server)
            .await;
        let mut manifest = json!({"version":"1.2.3","target":TARGET,"asset":ASSET,"sha256":digest,"size":7,"update_protocol":1});
        if let Some((key, value)) = manifest_change {
            manifest[key] = value;
        }
        Mock::given(path(
            "/niuji/model-bridge/releases/download/v1.2.3/update-manifest.json",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
        .mount(&server)
        .await;
        Mock::given(path(
            "/niuji/model-bridge/releases/download/v1.2.3/SHA256SUMS",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(checksums.unwrap_or(format!("{digest}  {ASSET}\n"))),
        )
        .mount(&server)
        .await;
        (server, Client::new())
    }

    #[test]
    fn strict_stable_versions() {
        for bad in [
            "v1.2.3",
            "01.2.3",
            "1.2",
            "1.2.3-rc.1",
            "1.2.3+build",
            "1.2.3/../../evil",
        ] {
            assert!(stable_version(bad).is_err(), "{bad}");
        }
        assert!(stable_version("1.2.3").is_ok());
    }

    #[tokio::test]
    async fn checks_newer_release_and_ignores_same_or_older() {
        let (server, client) = fixture(None, None).await;
        let source = Source::test(&server.uri());
        let release = check_from(&client, "1.2.2", &source)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(release.version, "1.2.3");
        assert!(check_from(&client, "1.2.3", &source)
            .await
            .unwrap()
            .is_none());
        assert!(check_from(&client, "2.0.0", &source)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn rejects_manifest_target_protocol_and_checksum_mismatches() {
        for change in [
            ("target", json!("wrong")),
            ("update_protocol", json!(2)),
            ("asset", json!("other.tar.gz")),
            ("version", json!("1.2.4")),
            ("sha256", json!("f".repeat(64))),
            ("size", json!(MAX_BYTES + 1)),
        ] {
            let (server, client) = fixture(Some(change), None).await;
            assert!(check_from(&client, "1.0.0", &Source::test(&server.uri()))
                .await
                .is_err());
        }
        let (server, client) = fixture(None, Some(String::new())).await;
        assert!(check_from(&client, "1.0.0", &Source::test(&server.uri()))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_missing_assets_and_http_errors() {
        for response in [
            ResponseTemplate::new(503),
            ResponseTemplate::new(200).set_body_json(
                json!({"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[]}),
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(path("/repos/niuji/model-bridge/releases/latest"))
                .respond_with(response)
                .mount(&server)
                .await;
            assert!(
                check_from(&Client::new(), "1.0.0", &Source::test(&server.uri()))
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn download_verifies_size_and_hash() {
        let (server, client) = fixture(None, None).await;
        let source = Source::test(&server.uri());
        let mut release = check_from(&client, "1.0.0", &source)
            .await
            .unwrap()
            .unwrap();
        Mock::given(path(format!(
            "/niuji/model-bridge/releases/download/v1.2.3/{ASSET}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"archive"))
        .mount(&server)
        .await;
        let destination =
            std::env::temp_dir().join(format!("mb-release-test-{}", uuid::Uuid::new_v4()));
        let mut progress = Vec::new();
        download_from(&client, &release, &destination, &source, |bytes| {
            progress.push(bytes)
        })
        .await
        .unwrap();
        assert_eq!(progress.first(), Some(&0));
        assert_eq!(progress.last(), Some(&release.size));
        assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(progress.iter().all(|bytes| *bytes <= release.size));
        assert_eq!(std::fs::read(&destination).unwrap(), b"archive");
        std::fs::remove_file(&destination).unwrap();
        release.sha256 = "f".repeat(64);
        assert!(
            download_from(&client, &release, &destination, &source, |_| {})
                .await
                .is_err()
        );
        assert!(!destination.exists());
        release.size = 6;
        assert!(
            download_from(&client, &release, &destination, &source, |_| {})
                .await
                .is_err()
        );
        assert!(!destination.exists());
    }

    #[tokio::test]
    async fn pinned_release_validates_exact_tag_and_rejects_unsafe_versions() {
        let (server, client) = fixture(None, None).await;
        let source = Source::test(&server.uri());
        let release = metadata(&client, &source, "latest").await.unwrap();
        let assets: Vec<_> = release.assets.iter().map(|asset| json!({"name":asset.name,"browser_download_url":asset.browser_download_url})).collect();
        Mock::given(path("/repos/niuji/model-bridge/releases/tags/v1.2.3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":assets}),
            ))
            .mount(&server)
            .await;
        assert_eq!(
            pinned_from(&client, "1.2.3", &source)
                .await
                .unwrap()
                .version,
            "1.2.3"
        );
        for version in ["1.2.3/evil", "1.2.3-rc.1", "01.2.3"] {
            assert!(pinned_from(&client, version, &source).await.is_err());
        }
        Mock::given(path("/repos/niuji/model-bridge/releases/tags/v1.2.4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[]}),
            ))
            .mount(&server)
            .await;
        assert!(pinned_from(&client, "1.2.4", &source).await.is_err());
    }

    #[tokio::test]
    async fn updater_client_rejects_plain_http() {
        assert!(client()
            .unwrap()
            .get("http://127.0.0.1:1/")
            .send()
            .await
            .is_err());
    }

    #[test]
    fn rejects_asset_urls_outside_fixed_repository() {
        let source = Source::github();
        for url in ["https://evil.example/archive", "https://github.com/other/model-bridge/releases/download/v1.2.3/model-bridge-linux-amd64.tar.gz", "http://github.com/niuji/model-bridge/releases/download/v1.2.3/model-bridge-linux-amd64.tar.gz"] {
            assert!(source.asset_url(url,"v1.2.3",ASSET).is_err());
        }
    }
}
