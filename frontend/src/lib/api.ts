export interface Job {
	id: number;
	file_path: string;
	status: string;
	created_at: string;
}

export interface Task {
	id: number;
	job_id: number;
	step_order: number;
	task_type: string;
	payload: string | null;
	status: string;
}

export interface LibraryRoots {
	library_path: string;
	ingest_path: string;
}

export interface LibrarySummary {
	total_items: number;
	total_bytes: number;
	video_items: number;
	audio_items: number;
	other_items: number;
}

export interface LibraryEntry {
	relative_path: string;
	file_name: string;
	extension: string;
	media_type: string;
	size_bytes: number;
	modified_at: number | null;
	library_id: string | null;
}

export interface LibraryScanStatus {
	status: string;
	scanned_items: number;
	total_items: number;
	started_at: number | null;
	completed_at: number | null;
	last_scan_at: number | null;
	last_error: string | null;
}

export interface LibraryResponse {
	items: LibraryEntry[];
	total_items: number;
	limit: number;
	offset: number;
	summary: LibrarySummary;
	roots: LibraryRoots;
	scan: LibraryScanStatus;
}

export interface StreamDisposition {
	default: boolean;
	forced: boolean;
	hearing_impaired: boolean;
}

export interface MediaStreamInfo {
	index: number;
	codec_type: string;
	codec_name: string;
	width?: number;
	height?: number;
	channels?: number;
	sample_rate?: number;
	bit_rate?: number;
	language?: string;
	title?: string;
	disposition: StreamDisposition;
}

export interface LibraryMetadata {
	file_path: string;
	relative_path: string;
	size_bytes: number;
	modified_at: number;
	format: string;
	duration_secs: number;
	stream_count: number;
	video_codec?: string;
	audio_codec?: string;
	width?: number;
	height?: number;
	audio_channels?: number;
	subtitle_count: number;
	subtitle_languages: string[];
	probe: {
		format: string;
		duration_secs: number;
		streams: MediaStreamInfo[];
	};
	cached: boolean;
}

export interface InternetMetadataMatch {
	provider: string;
	title: string;
	year: number | null;
	media_kind: string;
	imdb_id: string | null;
	tvdb_id: number | null;
	overview: string | null;
	rating: number | null;
	genres: string[];
	poster_url: string | null;
	source_url: string | null;
}

export interface InternetMetadataResponse {
	query: string;
	parsed_year: number | null;
	media_hint: string | null;
	matches: InternetMetadataMatch[];
	warnings: string[];
}

export interface InternetMetadataBulkItem {
	path: string;
	result: InternetMetadataResponse;
}

export interface InternetMetadataBulkResponse {
	items: InternetMetadataBulkItem[];
}

export interface SelectedInternetMetadataResponse {
	path: string;
	selected: InternetMetadataMatch;
}

export interface LibraryChangeEvent {
	relative_path: string;
	path: string;
	change: string;
	occurred_at: number;
}

export interface VideoStandards {
	codec: string;
	max_bitrate_mbps: number;
	resolution_ceiling: string;
}

export interface AudioStandards {
	target_lufs: number;
	target_true_peak: number;
	max_channels: string;
}

export interface SubtitleStandards {
	mode: string;
	preferred_languages: string[];
	keep_forced: boolean;
	keep_sdh: boolean;
}

export interface GoldenStandards {
	video: VideoStandards;
	audio: AudioStandards;
	subtitle: SubtitleStandards;
}

export interface LlmConfig {
	provider: string;
	base_url: string;
	model: string;
	api_key: string | null;
}

export interface InternetMetadataConfig {
	omdb_api_key: string | null;
	tvdb_api_key: string | null;
	tvdb_pin: string | null;
	user_agent: string;
}

export interface AppConfig {
	data_path: string;
	ingest_path: string;
	config_path: string;
	port: number;
	llm: LlmConfig;
	max_io_concurrency: number;
	metadata_prewarm_limit: number;
	scan_exclude_patterns: string[];
	scan_concurrency: number;
	scan_queue_capacity: number;
	bulk_metadata_concurrency: number;
	bulk_metadata_max_inflight: number;
	golden_standards: GoldenStandards;
	system_prompt: string;
	libraries: LibraryFolder[];
	internet_metadata: InternetMetadataConfig;
}

export interface LibraryFolder {
	id: string;
	name: string;
	path: string;
	media_type: 'movie' | 'tv';
}

const BASE = '/api';

export async function fetchJobs(limit = 50, offset = 0): Promise<Job[]> {
	const res = await fetch(`${BASE}/jobs?limit=${limit}&offset=${offset}`);
	if (!res.ok) throw new Error(`Failed to fetch jobs: ${res.status}`);
	return res.json();
}

export async function fetchJob(id: number): Promise<{ job_id: number; tasks: Task[] }> {
	const res = await fetch(`${BASE}/jobs/${id}`);
	if (!res.ok) throw new Error(`Failed to fetch job ${id}: ${res.status}`);
	return res.json();
}

export async function fetchLibrary(
	query = '',
	limit = 40,
	offset = 0,
	libraryId?: string
): Promise<LibraryResponse> {
	const params = new URLSearchParams({
		limit: String(limit),
		offset: String(offset)
	});

	if (query.trim()) {
		params.set('q', query.trim());
	}
	if (libraryId) {
		params.set('library_id', libraryId);
	}

	const res = await fetch(`${BASE}/library?${params.toString()}`);
	if (!res.ok) throw new Error(`Failed to fetch library: ${res.status}`);
	return res.json();
}

export async function fetchLibraryMetadata(relativePath: string): Promise<LibraryMetadata> {
	const params = new URLSearchParams({ path: relativePath });
	const res = await fetch(`${BASE}/library/metadata?${params.toString()}`);
	if (!res.ok) throw new Error(`Failed to fetch metadata for ${relativePath}: ${res.status}`);
	return res.json();
}

export async function fetchLibraryInternetMetadata(relativePath: string): Promise<InternetMetadataResponse> {
	const params = new URLSearchParams({ path: relativePath });
	const res = await fetch(`${BASE}/library/internet?${params.toString()}`);
	if (!res.ok) throw new Error(`Failed to fetch internet metadata for ${relativePath}: ${res.status}`);
	return res.json();
}

export async function fetchLibraryInternetMetadataBulk(paths: string[]): Promise<InternetMetadataBulkResponse> {
	const res = await fetch(`${BASE}/library/internet/bulk`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ paths })
	});
	if (!res.ok) throw new Error(`Failed to fetch bulk internet metadata: ${res.status}`);
	return res.json();
}

export async function saveSelectedLibraryInternetMetadata(path: string, selected: InternetMetadataMatch): Promise<SelectedInternetMetadataResponse> {
	const res = await fetch(`${BASE}/library/internet`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ path, selected })
	});
	if (!res.ok) throw new Error(`Failed to save selected internet metadata for ${path}: ${res.status}`);
	return res.json();
}

export async function fetchSelectedLibraryInternetMetadata(path: string): Promise<SelectedInternetMetadataResponse | null> {
	const params = new URLSearchParams({ path });
	const res = await fetch(`${BASE}/library/internet/selected?${params.toString()}`);
	if (res.status === 204) return null;
	if (!res.ok) throw new Error(`Failed to fetch selected internet metadata for ${path}: ${res.status}`);
	return res.json();
}

export async function fetchLibraryEvents(limit = 24): Promise<LibraryChangeEvent[]> {
	const params = new URLSearchParams({ limit: String(limit) });
	const res = await fetch(`${BASE}/library/events?${params.toString()}`);
	if (!res.ok) throw new Error(`Failed to fetch library events: ${res.status}`);
	return res.json();
}

export async function triggerLibraryRescan(): Promise<void> {
	const res = await fetch(`${BASE}/library/rescan`, { method: 'POST' });
	if (!res.ok) throw new Error(`Failed to trigger library rescan: ${res.status}`);
}

export async function fetchConfig(): Promise<AppConfig> {
	const res = await fetch(`${BASE}/config`);
	if (!res.ok) throw new Error(`Failed to fetch config: ${res.status}`);
	return res.json();
}

export async function saveConfig(config: AppConfig): Promise<AppConfig> {
	const res = await fetch(`${BASE}/config`, {
		method: 'PUT',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(config)
	});
	if (!res.ok) throw new Error(`Failed to save config: ${res.status}`);
	return res.json();
}

export async function fetchHealth(): Promise<boolean> {
	try {
		const res = await fetch(`${BASE}/health`);
		return res.ok;
	} catch {
		return false;
	}
}

// ---------------------------------------------------------------------------
// Library folder CRUD
// ---------------------------------------------------------------------------

export async function fetchLibraries(): Promise<LibraryFolder[]> {
	const res = await fetch(`${BASE}/libraries`);
	if (!res.ok) throw new Error(`Failed to fetch libraries: ${res.status}`);
	return res.json();
}

export async function addLibrary(folder: LibraryFolder): Promise<LibraryFolder> {
	const res = await fetch(`${BASE}/libraries`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(folder)
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to add library: ${res.status}`);
	}
	return res.json();
}

export async function removeLibrary(id: string): Promise<void> {
	const res = await fetch(`${BASE}/libraries/${encodeURIComponent(id)}`, {
		method: 'DELETE'
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to remove library: ${res.status}`);
	}
}
