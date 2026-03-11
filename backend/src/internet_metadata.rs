use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

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
    pub provider_used: Option<String>,
    pub search_candidates: Vec<String>,
    pub providers: Vec<InternetMetadataProviderStatus>,
    pub matches: Vec<InternetMetadataMatch>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetMetadataProviderStatus {
    pub provider: String,
    pub attempted: bool,
    pub match_count: usize,
    pub warning: Option<String>,
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

#[derive(Debug, Clone)]
struct SearchCandidate {
    query: String,
    parsed_year: Option<u16>,
}

pub async fn lookup_for_library_path(
    config: &AppConfig,
    relative_path: &str,
) -> Result<InternetMetadataResponse> {
    lookup_for_library_path_with_query(config, relative_path, None).await
}

pub async fn lookup_for_library_path_with_query(
    config: &AppConfig,
    relative_path: &str,
    query_override: Option<&str>,
) -> Result<InternetMetadataResponse> {
    let search_candidates = if let Some(query) = query_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        vec![build_search_candidate(query)]
    } else {
        extract_search_candidates(relative_path)
    };
    let (query, parsed_year) = search_candidates
        .first()
        .map(|candidate| (candidate.query.clone(), candidate.parsed_year))
        .unwrap_or_else(|| {
            let fallback = std::path::Path::new(relative_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or(relative_path)
                .to_string();
            (fallback, None)
        });
    let media_hint = infer_media_hint(config, relative_path);

    let client = Client::builder()
        .user_agent(config.internet_metadata.user_agent.clone())
        .timeout(std::time::Duration::from_secs(12))
        .build()?;

    let mut matches: Vec<InternetMetadataMatch> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut providers: Vec<InternetMetadataProviderStatus> = Vec::new();

    let omdb_key = config
        .internet_metadata
        .omdb_api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    let tvdb_key = config
        .internet_metadata
        .tvdb_api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());

    let provider_order = choose_provider_order(
        omdb_key.is_some(),
        tvdb_key.is_some(),
        &config.internet_metadata.default_provider,
    );

    if provider_order.is_empty() {
        warnings.push("No metadata provider API key configured".into());
    }

    for provider in &provider_order {
        match provider.as_str() {
            "omdb" => {
                if let Some(key) = omdb_key {
                    let mut provider_matches = Vec::new();
                    let mut provider_warning = None;
                    for candidate in &search_candidates {
                        match lookup_omdb(
                            &client,
                            key,
                            &candidate.query,
                            candidate.parsed_year,
                            media_hint.as_deref(),
                        )
                        .await
                        {
                            Ok(found) if !found.is_empty() => {
                                provider_matches = found;
                                break;
                            }
                            Ok(_) => {}
                            Err(error) => provider_warning = Some(error.to_string()),
                        }
                    }
                    if let Some(warning) = provider_warning.clone() {
                        warnings.push(format!("OMDb lookup failed: {}", warning));
                    }
                    providers.push(InternetMetadataProviderStatus {
                        provider: "omdb".into(),
                        attempted: true,
                        match_count: provider_matches.len(),
                        warning: provider_warning,
                    });
                    matches.extend(provider_matches);
                }
            }
            "tvdb" => {
                if let Some(key) = tvdb_key {
                    let mut provider_matches = Vec::new();
                    let mut provider_warning = None;
                    for candidate in &search_candidates {
                        match lookup_tvdb(
                            &client,
                            key,
                            config.internet_metadata.tvdb_pin.as_deref(),
                            &candidate.query,
                            media_hint.as_deref(),
                        )
                        .await
                        {
                            Ok(found) if !found.is_empty() => {
                                provider_matches = found;
                                break;
                            }
                            Ok(_) => {}
                            Err(error) => provider_warning = Some(error.to_string()),
                        }
                    }
                    if let Some(warning) = provider_warning.clone() {
                        warnings.push(format!("TVDB lookup failed: {}", warning));
                    }
                    providers.push(InternetMetadataProviderStatus {
                        provider: "tvdb".into(),
                        attempted: true,
                        match_count: provider_matches.len(),
                        warning: provider_warning,
                    });
                    matches.extend(provider_matches);
                }
            }
            _ => warnings.push("Unknown default metadata provider configured".into()),
        }
    }

    dedupe_matches(&mut matches);

    Ok(InternetMetadataResponse {
        query,
        parsed_year,
        media_hint,
        provider_used: provider_order.first().cloned(),
        search_candidates: search_candidates
            .into_iter()
            .map(|candidate| candidate.query)
            .collect(),
        providers,
        matches,
        warnings,
    })
}

fn choose_provider_order(has_omdb: bool, has_tvdb: bool, configured_default: &str) -> Vec<String> {
    match (has_omdb, has_tvdb) {
        (false, false) => Vec::new(),
        (true, false) => vec!["omdb".into()],
        (false, true) => vec!["tvdb".into()],
        (true, true) => {
            let normalized = configured_default.trim().to_ascii_lowercase();
            if normalized == "tvdb" {
                vec!["tvdb".into(), "omdb".into()]
            } else {
                vec!["omdb".into(), "tvdb".into()]
            }
        }
    }
}

fn extract_search_candidates(relative_path: &str) -> Vec<SearchCandidate> {
    let path = std::path::Path::new(relative_path);
    let mut raw_candidates = Vec::new();

    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
        raw_candidates.push(stem.to_string());
    }
    if let Some(parent) = path
        .parent()
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str())
    {
        raw_candidates.push(parent.to_string());
    }
    if let Some(grandparent) = path
        .parent()
        .and_then(|value| value.parent())
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str())
    {
        raw_candidates.push(grandparent.to_string());
    }

    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in raw_candidates {
        let Some((query, parsed_year)) = normalize_title_and_year(&raw) else {
            continue;
        };
        let key = format!(
            "{}|{}",
            query.to_ascii_lowercase(),
            parsed_year
                .map(|value| value.to_string())
                .unwrap_or_default()
        );
        if seen.insert(key) {
            candidates.push(SearchCandidate { query, parsed_year });
        }
    }

    candidates
}

fn build_search_candidate(value: &str) -> SearchCandidate {
    if let Some((query, parsed_year)) = normalize_title_and_year(value) {
        SearchCandidate { query, parsed_year }
    } else {
        SearchCandidate {
            query: value.trim().to_string(),
            parsed_year: None,
        }
    }
}

fn normalize_title_and_year(input: &str) -> Option<(String, Option<u16>)> {
    let stem = input
        .rsplit_once('.')
        .map(|(left, _)| left)
        .unwrap_or(input);

    let cleaned = stem
        .replace('.', " ")
        .replace('_', " ")
        .replace('-', " ")
        .replace('[', " ")
        .replace(']', " ")
        .replace('(', " ")
        .replace(')', " ");

    let tokens = cleaned
        .split_whitespace()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();

    let mut year: Option<u16> = None;
    for token in &tokens {
        if token.len() == 4 {
            if let Ok(value) = token.parse::<u16>() {
                if (1900..=2099).contains(&value) {
                    year = Some(value);
                    break;
                }
            }
        }
    }

    let year_str = year.map(|y| y.to_string());
    let mut normalized_tokens: Vec<String> = Vec::new();
    let mut skip_next_if_numeric_id = false;

    for token in &tokens {
        let lowered = token.to_ascii_lowercase();

        if skip_next_if_numeric_id {
            if lowered.chars().all(|c| c.is_ascii_digit()) {
                skip_next_if_numeric_id = false;
                continue;
            }
            skip_next_if_numeric_id = false;
        }

        if is_metadata_noise_token(&lowered) {
            continue;
        }

        if year_str.as_deref() == Some(token) {
            continue;
        }

        if is_external_id_marker(&lowered) {
            skip_next_if_numeric_id = true;
            continue;
        }

        if is_external_id_token(&lowered) {
            continue;
        }

        if lowered.chars().all(|c| c.is_ascii_digit()) && lowered.len() > 4 {
            continue;
        }

        normalized_tokens.push((*token).to_string());
    }

    let normalized = normalized_tokens.join(" ");

    let query = normalized.trim();
    if query.is_empty() {
        None
    } else {
        Some((query.to_string(), year))
    }
}

fn is_metadata_noise_token(token: &str) -> bool {
    matches!(
        token,
        "1080p"
            | "2160p"
            | "720p"
            | "480p"
            | "x264"
            | "x265"
            | "h264"
            | "h265"
            | "hevc"
            | "bluray"
            | "webrip"
            | "webdl"
            | "dvdrip"
            | "remux"
            | "hdr"
            | "uhd"
            | "proper"
            | "repack"
            | "extended"
            | "director"
            | "cut"
    )
}

fn is_external_id_marker(token: &str) -> bool {
    token == "tmdbid" || token == "tvdbid" || token == "imdbid"
}

fn is_external_id_token(token: &str) -> bool {
    token.starts_with("tmdbid")
        || token.starts_with("tvdbid")
        || token.starts_with("imdbid")
        || (token.starts_with("tt") && token[2..].chars().all(|c| c.is_ascii_digit()))
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

fn dedupe_matches(matches: &mut Vec<InternetMetadataMatch>) {
    let mut seen = std::collections::HashSet::new();
    matches.retain(|item| {
        let key = format!(
            "{}|{}|{}|{}|{}",
            item.provider,
            item.imdb_id.clone().unwrap_or_default(),
            item.tvdb_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            item.title.to_ascii_lowercase(),
            item.year.map(|value| value.to_string()).unwrap_or_default(),
        );
        seen.insert(key)
    });
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
    let mut exact_params: Vec<(String, String)> = vec![
        ("apikey".into(), api_key.to_string()),
        ("t".into(), query.to_string()),
        ("plot".into(), "short".into()),
    ];

    if let Some(y) = year {
        exact_params.push(("y".into(), y.to_string()));
    }
    if let Some(kind) = media_hint {
        let t = if kind == "series" { "series" } else { "movie" };
        exact_params.push(("type".into(), t.to_string()));
    }

    let response = client
        .get("https://www.omdbapi.com/")
        .query(&exact_params)
        .send()
        .await?
        .json::<OmdbResponse>()
        .await?;

    if response
        .response
        .as_deref()
        .unwrap_or("False")
        .eq_ignore_ascii_case("true")
    {
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

        return Ok(vec![InternetMetadataMatch {
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
        }]);
    }

    let mut search_params: Vec<(String, String)> = vec![
        ("apikey".into(), api_key.to_string()),
        ("s".into(), query.to_string()),
    ];
    if let Some(y) = year {
        search_params.push(("y".into(), y.to_string()));
    }
    if let Some(kind) = media_hint {
        let t = if kind == "series" { "series" } else { "movie" };
        search_params.push(("type".into(), t.to_string()));
    }

    let response = client
        .get("https://www.omdbapi.com/")
        .query(&search_params)
        .send()
        .await?
        .json::<OmdbSearchResponse>()
        .await?;

    Ok(response
        .search
        .unwrap_or_default()
        .into_iter()
        .take(5)
        .map(|item| InternetMetadataMatch {
            provider: "omdb".into(),
            title: item.title.unwrap_or_else(|| query.to_string()),
            year: item
                .year
                .and_then(|value| value.split(' ').next().and_then(|v| v.parse::<u16>().ok())),
            media_kind: item.media_type.unwrap_or_else(|| "unknown".into()),
            imdb_id: item.imdb_id.clone(),
            tvdb_id: None,
            overview: None,
            rating: None,
            genres: Vec::new(),
            poster_url: item.poster.filter(|value| value != "N/A"),
            source_url: item
                .imdb_id
                .map(|id| format!("https://www.imdb.com/title/{}/", id)),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct OmdbSearchResponse {
    #[serde(rename = "Search")]
    search: Option<Vec<OmdbSearchItem>>,
}

#[derive(Debug, Deserialize)]
struct OmdbSearchItem {
    #[serde(rename = "Title")]
    title: Option<String>,
    #[serde(rename = "Year")]
    year: Option<String>,
    #[serde(rename = "Type")]
    media_type: Option<String>,
    #[serde(rename = "imdbID")]
    imdb_id: Option<String>,
    #[serde(rename = "Poster")]
    poster: Option<String>,
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

    let login_response = client
        .post("https://api4.thetvdb.com/v4/login")
        .json(&body)
        .send()
        .await?;
    let login_status = login_response.status();
    let login_body = login_response.text().await?;
    if !login_status.is_success() {
        return Err(anyhow::anyhow!(
            "TVDB login failed ({}): {}",
            login_status,
            summarize_tvdb_body(&login_body)
        ));
    }

    let login_json: Value = serde_json::from_str(&login_body).map_err(|error| {
        anyhow::anyhow!(
            "TVDB login decode failed: {}; body: {}",
            error,
            summarize_tvdb_body(&login_body)
        )
    })?;

    let token = login_json
        .get("data")
        .and_then(|value| value.get("token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            login_json
                .get("token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow::anyhow!("TVDB login did not return a token"))?;

    let mut req = client
        .get("https://api4.thetvdb.com/v4/search")
        .bearer_auth(&token)
        .query(&[("query", query)]);

    if let Some(kind) = media_hint {
        let tvdb_kind = if kind == "series" { "series" } else { "movie" };
        req = req.query(&[("type", tvdb_kind)]);
    }

    let response = req.send().await?;
    let search_status = response.status();
    let search_body = response.text().await?;
    if !search_status.is_success() {
        return Err(anyhow::anyhow!(
            "TVDB search failed ({}): {}",
            search_status,
            summarize_tvdb_body(&search_body)
        ));
    }

    let search_json: Value = serde_json::from_str(&search_body).map_err(|error| {
        anyhow::anyhow!(
            "TVDB search decode failed: {}; body: {}",
            error,
            summarize_tvdb_body(&search_body)
        )
    })?;

    Ok(parse_tvdb_matches(&search_json, query, media_hint))
}

fn parse_tvdb_matches(
    response: &Value,
    query: &str,
    media_hint: Option<&str>,
) -> Vec<InternetMetadataMatch> {
    tvdb_result_items(response)
        .into_iter()
        .take(5)
        .map(|item| {
            let tvdb_id = first_present_u64(&item, &["tvdb_id", "id", "objectID"]);
            let title = first_present_string(&item, &["name", "title", "slugName"])
                .unwrap_or_else(|| query.to_string());
            let year = parse_tvdb_year(&item);
            let media_kind = parse_tvdb_media_kind(&item, media_hint);
            let source_url = build_tvdb_source_url(&item, tvdb_id, &media_kind);
            let overview = first_present_string(&item, &["overview", "description"]);
            let poster_url = first_present_string(&item, &["image_url", "image", "thumbnail"]);
            let status = first_present_string(&item, &["status"]);

            InternetMetadataMatch {
                provider: "tvdb".into(),
                title,
                year,
                media_kind,
                imdb_id: None,
                tvdb_id,
                overview,
                rating: None,
                genres: status.into_iter().collect(),
                poster_url,
                source_url,
            }
        })
        .collect()
}

fn tvdb_result_items(response: &Value) -> Vec<Value> {
    response
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| {
            response
                .get("data")
                .and_then(|value| value.get("results"))
                .and_then(Value::as_array)
        })
        .or_else(|| response.get("results").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default()
}

fn first_present_string(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        item.get(*key).and_then(|value| match value {
            Value::String(text) => {
                let trimmed = text.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            Value::Number(number) => Some(number.to_string()),
            Value::Object(map) => map.get("name").and_then(Value::as_str).map(str::trim).and_then(
                |text| (!text.is_empty()).then(|| text.to_string()),
            ),
            _ => None,
        })
    })
}

fn first_present_u64(item: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        item.get(*key).and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.trim().parse::<u64>().ok(),
            _ => None,
        })
    })
}

fn parse_tvdb_year(item: &Value) -> Option<u16> {
    first_present_string(item, &["year", "release_year"])
        .and_then(|value| value.parse::<u16>().ok())
        .or_else(|| {
            first_present_string(item, &["first_air_time", "firstAired", "release_date"])
                .and_then(|value| value.get(..4).and_then(|year| year.parse::<u16>().ok()))
        })
}

fn parse_tvdb_media_kind(item: &Value, media_hint: Option<&str>) -> String {
    first_present_string(item, &["type", "type_name"])
        .map(|kind| {
            let normalized = kind.trim().to_ascii_lowercase();
            if normalized.contains("movie") {
                "movie".to_string()
            } else if normalized.contains("series") || normalized.contains("show") {
                "series".to_string()
            } else {
                normalized
            }
        })
        .unwrap_or_else(|| media_hint.unwrap_or("unknown").to_string())
}

fn build_tvdb_source_url(item: &Value, tvdb_id: Option<u64>, media_kind: &str) -> Option<String> {
    if let Some(slug) = first_present_string(item, &["slug"]) {
        if slug.starts_with("http://") || slug.starts_with("https://") {
            return Some(slug);
        }
        return Some(format!(
            "https://thetvdb.com/{}",
            slug.trim_start_matches('/')
        ));
    }

    let path_kind = if media_kind == "movie" { "movie" } else { "series" };
    tvdb_id.map(|id| format!("https://thetvdb.com/dereferrer/{}/{}", path_kind, id))
}

fn summarize_tvdb_body(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact = if compact.is_empty() {
        "<empty response body>".to_string()
    } else {
        compact
    };

    const LIMIT: usize = 240;
    if compact.len() <= LIMIT {
        compact
    } else {
        format!("{}...", &compact[..LIMIT])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_tvdb_matches_accepts_array_payloads_with_numeric_fields() {
        let payload = json!({
            "data": [
                {
                    "id": 12345,
                    "name": "Will Trent",
                    "year": 2023,
                    "overview": "Special Agent Will Trent investigates.",
                    "image_url": "https://images.example/will-trent.jpg",
                    "slug": "series/will-trent",
                    "type": "series",
                    "status": "Continuing"
                }
            ]
        });

        let matches = parse_tvdb_matches(&payload, "Will Trent", Some("series"));

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].title, "Will Trent");
        assert_eq!(matches[0].tvdb_id, Some(12345));
        assert_eq!(matches[0].year, Some(2023));
        assert_eq!(matches[0].media_kind, "series");
        assert_eq!(matches[0].source_url.as_deref(), Some("https://thetvdb.com/series/will-trent"));
    }

    #[test]
    fn parse_tvdb_matches_accepts_nested_results_and_string_ids() {
        let payload = json!({
            "data": {
                "results": [
                    {
                        "tvdb_id": "67890",
                        "title": "Will Trent",
                        "first_air_time": "2023-01-03",
                        "thumbnail": "https://images.example/thumb.jpg",
                        "type": "series",
                        "description": "Nested search response"
                    }
                ]
            }
        });

        let matches = parse_tvdb_matches(&payload, "Will Trent", Some("series"));

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].tvdb_id, Some(67890));
        assert_eq!(matches[0].year, Some(2023));
        assert_eq!(matches[0].poster_url.as_deref(), Some("https://images.example/thumb.jpg"));
        assert_eq!(matches[0].overview.as_deref(), Some("Nested search response"));
        assert_eq!(
            matches[0].source_url.as_deref(),
            Some("https://thetvdb.com/dereferrer/series/67890")
        );
    }
}
