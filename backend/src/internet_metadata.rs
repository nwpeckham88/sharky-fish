use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;

use crate::config::AppConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetMetadataMatch {
    pub provider: String,
    pub title: String,
    pub year: Option<u16>,
    pub media_kind: String,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<u64>,
    pub overview: Option<String>,
    pub rating: Option<f64>,
    pub genres: Vec<String>,
    pub poster_url: Option<String>,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetMetadataResponse {
    pub query: String,
    pub parsed_year: Option<u16>,
    pub media_hint: Option<String>,
    pub matches: Vec<InternetMetadataMatch>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetMetadataBulkItem {
    pub path: String,
    pub result: InternetMetadataResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetMetadataBulkResponse {
    pub items: Vec<InternetMetadataBulkItem>,
}

pub async fn lookup_for_library_path(config: &AppConfig, relative_path: &str) -> Result<InternetMetadataResponse> {
    let (query, parsed_year) = extract_title_and_year(relative_path);
    let media_hint = infer_media_hint(config, relative_path);

    let client = Client::builder()
        .user_agent(config.internet_metadata.user_agent.clone())
        .timeout(std::time::Duration::from_secs(12))
        .build()?;

    let mut matches: Vec<InternetMetadataMatch> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if let Some(key) = config.internet_metadata.omdb_api_key.as_deref().filter(|k| !k.trim().is_empty()) {
        match lookup_omdb(&client, key.trim(), &query, parsed_year, media_hint.as_deref()).await {
            Ok(mut found) => matches.append(&mut found),
            Err(e) => warnings.push(format!("OMDb lookup failed: {}", e)),
        }
    } else {
        warnings.push("OMDb API key not configured".into());
    }

    if let Some(key) = config.internet_metadata.tvdb_api_key.as_deref().filter(|k| !k.trim().is_empty()) {
        match lookup_tvdb(
            &client,
            key.trim(),
            config.internet_metadata.tvdb_pin.as_deref(),
            &query,
            media_hint.as_deref(),
        )
        .await
        {
            Ok(mut found) => matches.append(&mut found),
            Err(e) => warnings.push(format!("TVDB lookup failed: {}", e)),
        }
    } else {
        warnings.push("TVDB API key not configured".into());
    }

    Ok(InternetMetadataResponse {
        query,
        parsed_year,
        media_hint,
        matches,
        warnings,
    })
}

fn extract_title_and_year(relative_path: &str) -> (String, Option<u16>) {
    let file_name = std::path::Path::new(relative_path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(relative_path);

    let stem = file_name.rsplit_once('.').map(|(left, _)| left).unwrap_or(file_name);

    let cleaned = stem
        .replace('.', " ")
        .replace('_', " ")
        .replace('-', " ")
        .replace('[', " ")
        .replace(']', " ")
        .replace('(', " ")
        .replace(')', " ");

    let mut year: Option<u16> = None;
    for token in cleaned.split_whitespace() {
        if token.len() == 4 {
            if let Ok(value) = token.parse::<u16>() {
                if (1900..=2099).contains(&value) {
                    year = Some(value);
                    break;
                }
            }
        }
    }

    let normalized = cleaned
        .split_whitespace()
        .filter(|t| !t.eq_ignore_ascii_case("1080p")
            && !t.eq_ignore_ascii_case("2160p")
            && !t.eq_ignore_ascii_case("720p")
            && !t.eq_ignore_ascii_case("x264")
            && !t.eq_ignore_ascii_case("x265")
            && !t.eq_ignore_ascii_case("h264")
            && !t.eq_ignore_ascii_case("h265")
            && !t.eq_ignore_ascii_case("hevc")
            && !t.eq_ignore_ascii_case("bluray")
            && !t.eq_ignore_ascii_case("webrip")
            && !t.eq_ignore_ascii_case("webdl")
            && !t.eq_ignore_ascii_case("dvdrip")
            && !t.eq_ignore_ascii_case("remux")
            && !t.eq_ignore_ascii_case("hdr"))
        .collect::<Vec<_>>()
        .join(" ");

    let query = normalized.trim();
    if query.is_empty() {
        (stem.to_string(), year)
    } else {
        (query.to_string(), year)
    }
}

fn infer_media_hint(config: &AppConfig, relative_path: &str) -> Option<String> {
    for lib in &config.libraries {
        let prefix = if lib.path.ends_with('/') {
            lib.path.clone()
        } else {
            format!("{}/", lib.path)
        };
        if relative_path.starts_with(&prefix) || relative_path == lib.path {
            return Some(match lib.media_type.as_str() {
                "movie" => "movie".to_string(),
                "tv" => "series".to_string(),
                _ => "movie".to_string(),
            });
        }
    }
    None
}

#[derive(Debug, Deserialize)]
struct OmdbResponse {
    #[serde(rename = "Response")]
    response: Option<String>,
    #[serde(rename = "Title")]
    title: Option<String>,
    #[serde(rename = "Year")]
    year: Option<String>,
    #[serde(rename = "Type")]
    media_type: Option<String>,
    #[serde(rename = "imdbID")]
    imdb_id: Option<String>,
    #[serde(rename = "Plot")]
    plot: Option<String>,
    #[serde(rename = "Genre")]
    genre: Option<String>,
    #[serde(rename = "Poster")]
    poster: Option<String>,
    #[serde(rename = "imdbRating")]
    imdb_rating: Option<String>,
}

async fn lookup_omdb(
    client: &Client,
    api_key: &str,
    query: &str,
    year: Option<u16>,
    media_hint: Option<&str>,
) -> Result<Vec<InternetMetadataMatch>> {
    let mut params: Vec<(String, String)> = vec![
        ("apikey".into(), api_key.to_string()),
        ("t".into(), query.to_string()),
        ("plot".into(), "short".into()),
    ];

    if let Some(y) = year {
        params.push(("y".into(), y.to_string()));
    }
    if let Some(kind) = media_hint {
        let t = if kind == "series" { "series" } else { "movie" };
        params.push(("type".into(), t.to_string()));
    }

    let response = client
        .get("https://www.omdbapi.com/")
        .query(&params)
        .send()
        .await?
        .json::<OmdbResponse>()
        .await?;

    if !response
        .response
        .as_deref()
        .unwrap_or("False")
        .eq_ignore_ascii_case("true")
    {
        return Ok(Vec::new());
    }

    let title = response.title.unwrap_or_else(|| query.to_string());
    let year = response
        .year
        .as_deref()
        .and_then(|v| v.split(' ').next())
        .and_then(|v| v.parse::<u16>().ok());
    let media_kind = response.media_type.unwrap_or_else(|| "unknown".into());
    let genres = response
        .genre
        .unwrap_or_default()
        .split(',')
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect::<Vec<_>>();
    let imdb_id = response.imdb_id.clone();

    Ok(vec![InternetMetadataMatch {
        provider: "omdb".into(),
        title,
        year,
        media_kind,
        imdb_id: imdb_id.clone(),
        tvdb_id: None,
        overview: response.plot,
        rating: response.imdb_rating.and_then(|v| v.parse::<f64>().ok()),
        genres,
        poster_url: response.poster.filter(|v| v != "N/A"),
        source_url: imdb_id.map(|id| format!("https://www.imdb.com/title/{}/", id)),
    }])
}

#[derive(Debug, Deserialize)]
struct TvdbLoginResponse {
    data: Option<TvdbLoginData>,
}

#[derive(Debug, Deserialize)]
struct TvdbLoginData {
    token: String,
}

#[derive(Debug, Deserialize)]
struct TvdbSearchResponse {
    data: Option<Vec<TvdbSearchItem>>,
}

#[derive(Debug, Deserialize)]
struct TvdbSearchItem {
    #[serde(default)]
    tvdb_id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    year: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

async fn lookup_tvdb(
    client: &Client,
    api_key: &str,
    pin: Option<&str>,
    query: &str,
    media_hint: Option<&str>,
) -> Result<Vec<InternetMetadataMatch>> {
    let mut body = serde_json::json!({ "apikey": api_key });
    if let Some(value) = pin.filter(|v| !v.trim().is_empty()) {
        body["pin"] = serde_json::Value::String(value.trim().to_string());
    }

    let login = client
        .post("https://api4.thetvdb.com/v4/login")
        .json(&body)
        .send()
        .await?
        .json::<TvdbLoginResponse>()
        .await?;

    let token = login
        .data
        .map(|v| v.token)
        .ok_or_else(|| anyhow::anyhow!("TVDB login did not return a token"))?;

    let mut req = client
        .get("https://api4.thetvdb.com/v4/search")
        .bearer_auth(token)
        .query(&[("query", query)]);

    if let Some(kind) = media_hint {
        let tvdb_kind = if kind == "series" { "series" } else { "movie" };
        req = req.query(&[("type", tvdb_kind)]);
    }

    let result = req.send().await?.json::<TvdbSearchResponse>().await?;

    let mut matches = Vec::new();
    if let Some(items) = result.data {
        for item in items.into_iter().take(5) {
            let title = item.name.unwrap_or_else(|| query.to_string());
            let year = item.year.as_deref().and_then(|v| v.parse::<u16>().ok());
            let media_kind = media_hint.unwrap_or("unknown").to_string();
            let source_url = item
                .slug
                .as_ref()
                .map(|slug| format!("https://thetvdb.com/{}", slug.trim_start_matches('/')))
                .or_else(|| item.tvdb_id.map(|id| format!("https://thetvdb.com/dereferrer/series/{}", id)));

            matches.push(InternetMetadataMatch {
                provider: "tvdb".into(),
                title,
                year,
                media_kind,
                imdb_id: None,
                tvdb_id: item.tvdb_id,
                overview: item.overview,
                rating: None,
                genres: item
                    .status
                    .as_deref()
                    .map(|v| vec![v.to_string()])
                    .unwrap_or_default(),
                poster_url: item.image_url,
                source_url,
            });
        }
    }

    Ok(matches)
}
