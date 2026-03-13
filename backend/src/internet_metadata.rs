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
    pub backdrop_url: Option<String>,
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
    pub top_match_title: Option<String>,
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
    let mut search_candidates = if let Some(query) = query_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        vec![build_search_candidate(query)]
    } else {
        extract_search_candidates(relative_path)
    };
    let media_hint = infer_media_hint(config, relative_path);

    // TV file names are commonly noisy (`Show - S01E02 - Episode Title ...`).
    // Prefer show-level folder candidates first to make TMDb matching reliable.
    if query_override.is_none() && media_hint.as_deref() == Some("series") {
        search_candidates.reverse();
    }

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
    let tmdb_key = config
        .internet_metadata
        .tmdb_api_key
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
        tmdb_key.is_some(),
        omdb_key.is_some(),
        tvdb_key.is_some(),
        &config.internet_metadata.default_provider,
    );

    if provider_order.is_empty() {
        warnings.push("No metadata provider API key configured".into());
    }

    for provider in &provider_order {
        match provider.as_str() {
            "tmdb" => {
                if let Some(key) = tmdb_key {
                    let mut provider_matches = Vec::new();
                    let mut provider_warning = None;
                    for candidate in &search_candidates {
                        match lookup_tmdb(
                            &client,
                            key,
                            &candidate.query,
                            candidate.parsed_year,
                            media_hint.as_deref(),
                        )
                        .await
                        {
                            Ok(found) if !found.is_empty() => {
                                provider_matches.extend(found);
                            }
                            Ok(_) => {}
                            Err(error) => provider_warning = Some(error.to_string()),
                        }
                    }
                    if !provider_matches.is_empty() {
                        dedupe_matches(&mut provider_matches);
                        rank_tmdb_matches(
                            &mut provider_matches,
                            search_candidates.first(),
                            media_hint.as_deref(),
                        );
                    }
                    if let Some(warning) = provider_warning.clone() {
                        warnings.push(format!("TMDb lookup failed: {}", warning));
                    }
                    providers.push(InternetMetadataProviderStatus {
                        provider: "tmdb".into(),
                        attempted: true,
                        match_count: provider_matches.len(),
                        top_match_title: provider_matches.first().map(|item| {
                            if let Some(year) = item.year {
                                format!("{} ({year})", item.title)
                            } else {
                                item.title.clone()
                            }
                        }),
                        warning: provider_warning,
                    });
                    matches.extend(provider_matches);
                }
            }
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
                        top_match_title: provider_matches.first().map(|item| {
                            if let Some(year) = item.year {
                                format!("{} ({year})", item.title)
                            } else {
                                item.title.clone()
                            }
                        }),
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
                        top_match_title: provider_matches.first().map(|item| {
                            if let Some(year) = item.year {
                                format!("{} ({year})", item.title)
                            } else {
                                item.title.clone()
                            }
                        }),
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

fn choose_provider_order(
    has_tmdb: bool,
    has_omdb: bool,
    has_tvdb: bool,
    configured_default: &str,
) -> Vec<String> {
    let mut configured = Vec::new();
    if has_tmdb {
        configured.push("tmdb");
    }
    if has_omdb {
        configured.push("omdb");
    }
    if has_tvdb {
        configured.push("tvdb");
    }

    if configured.is_empty() {
        return Vec::new();
    }

    let normalized = configured_default.trim().to_ascii_lowercase();
    let mut ordered = Vec::new();
    if configured.contains(&normalized.as_str()) {
        ordered.push(normalized.clone());
    }

    for provider in configured {
        if !ordered.iter().any(|existing| existing == provider) {
            ordered.push(provider.to_string());
        }
    }

    ordered
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

    let cleaned = stem.replace(['.', '_', '-', '[', ']', '(', ')'], " ");

    let tokens = cleaned
        .split_whitespace()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();

    let mut year: Option<u16> = None;
    for token in &tokens {
        if token.len() == 4
            && let Ok(value) = token.parse::<u16>()
            && (1900..=2099).contains(&value)
        {
            year = Some(value);
            break;
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

        if is_season_episode_token(&lowered) {
            continue;
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

fn is_season_episode_token(token: &str) -> bool {
    if token == "season" || token == "episode" || token == "ep" {
        return true;
    }

    let chars = token.as_bytes();

    // Matches patterns like s01e02, s1e9, s001e010.
    if chars.len() >= 4 && chars[0] == b's' {
        let mut index = 1;
        let mut season_digits = 0;
        while index < chars.len() && chars[index].is_ascii_digit() {
            season_digits += 1;
            index += 1;
        }
        if season_digits > 0 && index < chars.len() && chars[index] == b'e' {
            index += 1;
            let mut episode_digits = 0;
            while index < chars.len() && chars[index].is_ascii_digit() {
                episode_digits += 1;
                index += 1;
            }
            if episode_digits > 0 && index == chars.len() {
                return true;
            }
        }
    }

    // Matches short forms like e02 and s03.
    if token.len() >= 2
        && (token.starts_with('e') || token.starts_with('s'))
        && token[1..].chars().all(|c| c.is_ascii_digit())
    {
        return true;
    }

    false
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
struct TmdbSearchResponse {
    results: Option<Vec<TmdbSearchItem>>,
}

#[derive(Debug, Deserialize)]
struct TmdbSearchItem {
    id: u64,
    title: Option<String>,
    name: Option<String>,
    media_type: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    overview: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    vote_average: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct TmdbGenre {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TmdbExternalIds {
    imdb_id: Option<String>,
    tvdb_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TmdbDetailsResponse {
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    overview: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    vote_average: Option<f64>,
    genres: Option<Vec<TmdbGenre>>,
    imdb_id: Option<String>,
    external_ids: Option<TmdbExternalIds>,
}

async fn lookup_tmdb(
    client: &Client,
    api_key: &str,
    query: &str,
    year: Option<u16>,
    media_hint: Option<&str>,
) -> Result<Vec<InternetMetadataMatch>> {
    let kind = if media_hint == Some("series") { "tv" } else { "movie" };
    let mut payload = tmdb_search(client, api_key, kind, query, year).await?;

    // Some releases are tagged with the wrong year in file names; fall back to
    // unscoped search when strict year filtering yields no results.
    if payload.results.as_ref().is_none_or(|results| results.is_empty()) && year.is_some() {
        payload = tmdb_search(client, api_key, kind, query, None).await?;
    }

    let mut matches = Vec::new();

    for item in payload.results.unwrap_or_default().into_iter().take(5) {
        let details_kind = item
            .media_type
            .as_deref()
            .map(|value| if value.eq_ignore_ascii_case("tv") { "tv" } else { "movie" })
            .unwrap_or(kind);
        let detail_url = format!(
            "https://api.themoviedb.org/3/{details_kind}/{}",
            item.id
        );
        let detail_response = client
            .get(detail_url)
            .query(&[
                ("api_key", api_key),
                ("append_to_response", "external_ids"),
            ])
            .send()
            .await?
            .error_for_status()?;
        let details = detail_response.json::<TmdbDetailsResponse>().await?;

        let title = details
            .title
            .or(details.name)
            .or(item.title)
            .or(item.name)
            .unwrap_or_else(|| query.to_string());
        let date = details
            .release_date
            .or(details.first_air_date)
            .or(item.release_date)
            .or(item.first_air_date);
        let tmdb_year = date
            .as_deref()
            .and_then(|value| value.get(..4))
            .and_then(|value| value.parse::<u16>().ok());
        let poster_path = details.poster_path.or(item.poster_path);
        let backdrop_path = details.backdrop_path.or(item.backdrop_path);
        let poster_url = poster_path.map(|path| format!("https://image.tmdb.org/t/p/original{path}"));
        let backdrop_url =
            backdrop_path.map(|path| format!("https://image.tmdb.org/t/p/original{path}"));
        let genres = details
            .genres
            .unwrap_or_default()
            .into_iter()
            .filter_map(|genre| genre.name)
            .map(|genre| genre.trim().to_string())
            .filter(|genre| !genre.is_empty())
            .collect::<Vec<_>>();
        let external_ids = details.external_ids;
        let imdb_id = details
            .imdb_id
            .or_else(|| external_ids.as_ref().and_then(|value| value.imdb_id.clone()));
        let tvdb_id = external_ids.as_ref().and_then(|value| value.tvdb_id);
        let media_kind = if details_kind == "tv" { "series".to_string() } else { "movie".to_string() };

        matches.push(InternetMetadataMatch {
            provider: "tmdb".into(),
            title,
            year: tmdb_year,
            media_kind,
            imdb_id,
            tvdb_id,
            overview: details.overview.or(item.overview),
            rating: details.vote_average.or(item.vote_average),
            genres,
            poster_url,
            backdrop_url,
            source_url: Some(format!("https://www.themoviedb.org/{details_kind}/{}", item.id)),
        });
    }

    Ok(matches)
}

async fn tmdb_search(
    client: &Client,
    api_key: &str,
    kind: &str,
    query: &str,
    year: Option<u16>,
) -> Result<TmdbSearchResponse> {
    let search_url = format!("https://api.themoviedb.org/3/search/{kind}");
    let mut request = client.get(search_url).query(&[
        ("api_key", api_key),
        ("query", query),
        ("include_adult", "false"),
    ]);

    if let Some(value) = year {
        request = request.query(&[(
            if kind == "tv" {
                "first_air_date_year"
            } else {
                "year"
            },
            &value.to_string(),
        )]);
    }

    let response = request.send().await?.error_for_status()?;
    Ok(response.json::<TmdbSearchResponse>().await?)
}

fn rank_tmdb_matches(
    matches: &mut [InternetMetadataMatch],
    primary_candidate: Option<&SearchCandidate>,
    media_hint: Option<&str>,
) {
    let Some(candidate) = primary_candidate else {
        return;
    };

    let normalized_query = candidate.query.to_ascii_lowercase();
    let query_tokens = tokenize_for_match_score(&normalized_query);

    matches.sort_by(|left, right| {
        let left_score = tmdb_match_score(left, &normalized_query, &query_tokens, candidate.parsed_year, media_hint);
        let right_score = tmdb_match_score(right, &normalized_query, &query_tokens, candidate.parsed_year, media_hint);

        right_score
            .cmp(&left_score)
            .then_with(|| left.title.len().cmp(&right.title.len()))
    });
}

fn tmdb_match_score(
    item: &InternetMetadataMatch,
    normalized_query: &str,
    query_tokens: &[String],
    parsed_year: Option<u16>,
    media_hint: Option<&str>,
) -> i32 {
    let normalized_title = item.title.to_ascii_lowercase();
    let title_tokens = tokenize_for_match_score(&normalized_title);
    let overlap = query_tokens
        .iter()
        .filter(|token| title_tokens.iter().any(|candidate| candidate == *token))
        .count() as i32;

    let mut score = overlap * 10;

    if normalized_title == normalized_query {
        score += 120;
    } else if normalized_title.starts_with(normalized_query) {
        score += 60;
    }

    if let Some(year) = parsed_year {
        if item.year == Some(year) {
            score += 30;
        }
    }

    if media_hint == Some("series") {
        if item.media_kind == "series" {
            score += 25;
        }
        if normalized_title.contains("season") {
            score -= 20;
        }
    }

    score
}

fn tokenize_for_match_score(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .map(str::trim)
        .filter(|value| value.len() >= 2)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
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
            backdrop_url: None,
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
            backdrop_url: None,
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
                backdrop_url: None,
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
            Value::Object(map) => map
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .and_then(|text| (!text.is_empty()).then(|| text.to_string())),
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

    let path_kind = if media_kind == "movie" {
        "movie"
    } else {
        "series"
    };
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
        assert_eq!(
            matches[0].source_url.as_deref(),
            Some("https://thetvdb.com/series/will-trent")
        );
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
        assert_eq!(
            matches[0].poster_url.as_deref(),
            Some("https://images.example/thumb.jpg")
        );
        assert_eq!(
            matches[0].overview.as_deref(),
            Some("Nested search response")
        );
        assert_eq!(
            matches[0].source_url.as_deref(),
            Some("https://thetvdb.com/dereferrer/series/67890")
        );
    }
}
