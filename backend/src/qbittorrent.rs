use crate::config::QbittorrentConfig;

use anyhow::{Context, Result};
use reqwest::header::{COOKIE, SET_COOKIE};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct QbittorrentTransferInfo {
    pub dl_info_speed: u64,
    pub up_info_speed: u64,
    pub dl_info_data: u64,
    pub up_info_data: u64,
    pub connection_status: String,
    pub dht_nodes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QbittorrentTorrent {
    pub hash: String,
    pub name: String,
    pub state: String,
    pub progress: f64,
    pub dlspeed: u64,
    pub upspeed: u64,
    pub size: u64,
    pub total_size: u64,
    pub save_path: String,
    pub content_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QbittorrentStatus {
    pub enabled: bool,
    pub connected: bool,
    pub base_url: String,
    pub transfer: Option<QbittorrentTransferInfo>,
    pub torrents: Vec<QbittorrentTorrent>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTransferInfo {
    dl_info_speed: Option<u64>,
    up_info_speed: Option<u64>,
    dl_info_data: Option<u64>,
    up_info_data: Option<u64>,
    connection_status: Option<String>,
    dht_nodes: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawTorrent {
    hash: Option<String>,
    name: Option<String>,
    state: Option<String>,
    progress: Option<f64>,
    dlspeed: Option<u64>,
    upspeed: Option<u64>,
    size: Option<u64>,
    total_size: Option<u64>,
    save_path: Option<String>,
    content_path: Option<String>,
}

fn normalize_base_url(input: &str) -> String {
    input.trim().trim_end_matches('/').to_string()
}

fn extract_cookie(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|cookie| cookie.split(';').next().map(str::trim).map(str::to_string))
}

async fn login(client: &reqwest::Client, cfg: &QbittorrentConfig) -> Result<String> {
    let username = cfg
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("qBittorrent username is required")?;
    let password = cfg
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("qBittorrent password is required")?;

    let login_url = format!("{}/api/v2/auth/login", normalize_base_url(&cfg.base_url));
    let resp = client
        .post(login_url)
        .form(&[("username", username), ("password", password)])
        .send()
        .await
        .context("qBittorrent login request failed")?;

    if !resp.status().is_success() {
        let code = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("qBittorrent login failed with {code}: {text}");
    }

    extract_cookie(resp.headers()).context("qBittorrent auth cookie not returned")
}

pub async fn fetch_status(cfg: &QbittorrentConfig) -> QbittorrentStatus {
    let base_url = normalize_base_url(&cfg.base_url);
    if !cfg.enabled {
        return QbittorrentStatus {
            enabled: false,
            connected: false,
            base_url,
            transfer: None,
            torrents: Vec::new(),
            error: None,
        };
    }

    let timeout = std::time::Duration::from_secs(cfg.request_timeout_secs.max(2));
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(client) => client,
        Err(error) => {
            return QbittorrentStatus {
                enabled: true,
                connected: false,
                base_url,
                transfer: None,
                torrents: Vec::new(),
                error: Some(format!("failed to create HTTP client: {error}")),
            };
        }
    };

    let cookie = match login(&client, cfg).await {
        Ok(cookie) => cookie,
        Err(error) => {
            return QbittorrentStatus {
                enabled: true,
                connected: false,
                base_url,
                transfer: None,
                torrents: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };

    let transfer_url = format!("{}/api/v2/transfer/info", base_url);
    let torrents_url = format!("{}/api/v2/torrents/info?sort=added_on&reverse=true", base_url);

    let transfer = match async {
        let resp = client
            .get(transfer_url)
            .header(COOKIE, &cookie)
            .send()
            .await
            .context("failed to fetch qBittorrent transfer info")?;
        if !resp.status().is_success() {
            anyhow::bail!("qBittorrent transfer info returned {}", resp.status());
        }
        let raw = resp
            .json::<RawTransferInfo>()
            .await
            .context("failed to decode qBittorrent transfer info")?;
        Ok::<RawTransferInfo, anyhow::Error>(raw)
    }
    .await
    {
        Ok(raw) => Some(QbittorrentTransferInfo {
            dl_info_speed: raw.dl_info_speed.unwrap_or(0),
            up_info_speed: raw.up_info_speed.unwrap_or(0),
            dl_info_data: raw.dl_info_data.unwrap_or(0),
            up_info_data: raw.up_info_data.unwrap_or(0),
            connection_status: raw.connection_status.unwrap_or_else(|| "unknown".to_string()),
            dht_nodes: raw.dht_nodes.unwrap_or(0),
        }),
        Err(error) => {
            return QbittorrentStatus {
                enabled: true,
                connected: false,
                base_url,
                transfer: None,
                torrents: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };

    let torrents = match async {
        let resp = client
            .get(torrents_url)
            .header(COOKIE, &cookie)
            .send()
            .await
            .context("failed to fetch qBittorrent torrents")?;
        if !resp.status().is_success() {
            anyhow::bail!("qBittorrent torrents returned {}", resp.status());
        }
        let raw = resp
            .json::<Vec<RawTorrent>>()
            .await
            .context("failed to decode qBittorrent torrents")?;
        Ok::<Vec<RawTorrent>, anyhow::Error>(raw)
    }
    .await
    {
        Ok(raw) => raw
            .into_iter()
            .map(|item| QbittorrentTorrent {
                hash: item.hash.unwrap_or_default(),
                name: item.name.unwrap_or_else(|| "(unnamed torrent)".to_string()),
                state: item.state.unwrap_or_else(|| "unknown".to_string()),
                progress: item.progress.unwrap_or(0.0),
                dlspeed: item.dlspeed.unwrap_or(0),
                upspeed: item.upspeed.unwrap_or(0),
                size: item.size.unwrap_or(0),
                total_size: item.total_size.unwrap_or(0),
                save_path: item.save_path.unwrap_or_default(),
                content_path: item.content_path.unwrap_or_default(),
            })
            .take(cfg.max_torrents.clamp(1, 500))
            .collect::<Vec<_>>(),
        Err(error) => {
            return QbittorrentStatus {
                enabled: true,
                connected: false,
                base_url,
                transfer,
                torrents: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };

    QbittorrentStatus {
        enabled: true,
        connected: true,
        base_url,
        transfer,
        torrents,
        error: None,
    }
}

pub async fn test_connection(cfg: &QbittorrentConfig) -> Result<String> {
    let base_url = normalize_base_url(&cfg.base_url);
    let timeout = std::time::Duration::from_secs(cfg.request_timeout_secs.max(2));
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("failed to create HTTP client")?;

    let cookie = login(&client, cfg).await?;
    let version_url = format!("{}/api/v2/app/version", base_url);
    let version_resp = client
        .get(version_url)
        .header(COOKIE, &cookie)
        .send()
        .await
        .context("failed to fetch qBittorrent app version")?;

    if !version_resp.status().is_success() {
        anyhow::bail!(
            "qBittorrent app/version returned {}",
            version_resp.status()
        );
    }

    let version = version_resp.text().await.unwrap_or_default();
    let version = version.trim();
    if version.is_empty() {
        Ok(format!("Connected to qBittorrent at {base_url}"))
    } else {
        Ok(format!("Connected to qBittorrent {version} at {base_url}"))
    }
}

fn normalize_path_for_compare(input: &str) -> String {
    input.replace('\\', "/").trim_end_matches('/').to_string()
}

fn path_matches(base: &str, target: &str) -> bool {
    if base.is_empty() {
        return false;
    }
    target == base || target.starts_with(&format!("{base}/"))
}

fn torrent_is_actively_downloading(torrent: &QbittorrentTorrent) -> bool {
    if torrent.progress < 0.999 {
        return true;
    }

    let state = torrent.state.to_ascii_lowercase();
    state.contains("down")
        || state.contains("meta")
        || state.contains("check")
        || state.contains("stalled")
        || state.contains("queued")
        || state.contains("alloc")
        || state.contains("move")
}

pub fn path_is_in_active_torrent(status: &QbittorrentStatus, path: &Path) -> bool {
    if !status.connected {
        return false;
    }

    let target = normalize_path_for_compare(&path.to_string_lossy());
    if target.is_empty() {
        return false;
    }

    status.torrents.iter().any(|torrent| {
        if !torrent_is_actively_downloading(torrent) {
            return false;
        }

        let content = normalize_path_for_compare(&torrent.content_path);
        let save = normalize_path_for_compare(&torrent.save_path);
        path_matches(&content, &target) || path_matches(&save, &target)
    })
}

pub async fn path_is_actively_downloading(cfg: &QbittorrentConfig, path: &Path) -> Result<bool> {
    if !cfg.enabled {
        return Ok(false);
    }

    let status = fetch_status(cfg).await;
    if !status.connected {
        if let Some(error) = status.error {
            anyhow::bail!("qBittorrent unavailable: {error}");
        }
        anyhow::bail!("qBittorrent unavailable");
    }

    Ok(path_is_in_active_torrent(&status, path))
}
