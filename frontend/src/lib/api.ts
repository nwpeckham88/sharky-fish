export interface Job {
	id: number;
	file_path: string;
	status: string;
	created_at: string;
	probe: MediaProbe | null;
	decision: JobDecision | null;
}

export interface MediaProbe {
	format: string;
	duration_secs: number;
	streams: MediaStreamInfo[];
}

export interface JobDecision {
	job_id: number;
	arguments: string[];
	requires_two_pass: boolean;
	rationale: string;
}

export interface IntakeManagedItem {
	relative_path: string;
	file_path: string;
	file_name: string;
	media_type: string;
	size_bytes: number;
	modified_at: number;
	library_id: string | null;
	managed_status: string;
	has_sidecar: boolean;
	selected_metadata: InternetMetadataMatch | null;
	last_decision: JobDecision | null;
}

export interface BacklogSummary {
	total_items: number;
	needs_attention_count: number;
	unprocessed_count: number;
	reviewed_count: number;
	kept_original_count: number;
	awaiting_approval_count: number;
	approved_count: number;
	processed_count: number;
	failed_count: number;
	missing_metadata_count: number;
	missing_sidecar_count: number;
	organize_needed_count: number;
}

export interface BulkFailure {
	path: string;
	error: string;
}

export interface BulkCreateReviewResponse {
	jobs: Job[];
	success_count: number;
	failure_count: number;
	failures: BulkFailure[];
}

export interface BulkManagedStatusResponse {
	success_count: number;
	failure_count: number;
	failures: BulkFailure[];
}

export type BacklogFilter =
	| 'all'
	| 'needs_attention'
	| 'unprocessed'
	| 'failed'
	| 'awaiting_approval'
	| 'approved'
	| 'reviewed'
	| 'missing_metadata'
	| 'missing_sidecar'
	| 'organize_needed';

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
	managed_status: string | null;
	has_sidecar: boolean;
	has_selected_metadata: boolean;
	organize_target_path: string | null;
	organize_needed: boolean;
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
	provider_used: string | null;
	search_candidates: string[];
	providers: InternetMetadataProviderStatus[];
	matches: InternetMetadataMatch[];
	warnings: string[];
}

export interface InternetMetadataProviderStatus {
	provider: string;
	attempted: boolean;
	match_count: number;
	warning: string | null;
}

export interface InternetMetadataBulkItem {
	path: string;
	result: InternetMetadataResponse;
}

export interface InternetMetadataBulkResponse {
	items: InternetMetadataBulkItem[];
}

export interface BulkInternetAutoSelectResponse {
	success_count: number;
	failure_count: number;
	failures: BulkFailure[];
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

export interface OrganizeLibraryResult {
	current_relative_path: string;
	target_relative_path: string;
	changed: boolean;
	applied: boolean;
	target_exists: boolean;
	conflict_path: string | null;
}

export interface RelatedInternetMetadataPathsResponse {
	paths: string[];
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
	default_provider: 'omdb' | 'tvdb';
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
	auto_approve_ai_jobs: boolean;
	libraries: LibraryFolder[];
	internet_metadata: InternetMetadataConfig;
}

export interface LibraryFolder {
	id: string;
	name: string;
	path: string;
	media_type: 'movie' | 'tv';
}

export interface LlmTestResponse {
	ok: boolean;
	provider: string;
	model: string;
	message: string;
}

const BASE = '/api';

export async function fetchJobs(limit = 50, offset = 0): Promise<Job[]> {
	const res = await fetch(`${BASE}/jobs?limit=${limit}&offset=${offset}`);
	if (!res.ok) throw new Error(`Failed to fetch jobs: ${res.status}`);
	return res.json();
}

export async function fetchUnprocessedIntake(limit = 50, offset = 0): Promise<IntakeManagedItem[]> {
	const res = await fetch(`${BASE}/intake/unprocessed?limit=${limit}&offset=${offset}`);
	if (!res.ok) throw new Error(`Failed to fetch unprocessed intake items: ${res.status}`);
	return res.json();
}

export async function fetchBacklogSummary(): Promise<BacklogSummary> {
	const res = await fetch(`${BASE}/backlog/summary`);
	if (!res.ok) throw new Error(`Failed to fetch backlog summary: ${res.status}`);
	return res.json();
}

export async function fetchBacklogItems(
	filter: BacklogFilter = 'needs_attention',
	limit = 50,
	offset = 0
): Promise<IntakeManagedItem[]> {
	const params = new URLSearchParams({
		filter,
		limit: String(limit),
		offset: String(offset)
	});
	const res = await fetch(`${BASE}/backlog/items?${params.toString()}`);
	if (!res.ok) throw new Error(`Failed to fetch backlog items: ${res.status}`);
	return res.json();
}

export async function createIntakeReview(path: string): Promise<Job> {
	const res = await fetch(`${BASE}/intake/review`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ path })
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to create review job: ${res.status}`);
	}
	return res.json();
}

export async function createBulkIntakeReviews(paths: string[]): Promise<BulkCreateReviewResponse> {
	const res = await fetch(`${BASE}/intake/review/bulk`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ paths })
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to create bulk review jobs: ${res.status}`);
	}
	return res.json();
}

export async function updateIntakeManagedStatus(path: string, status: 'REVIEWED' | 'KEPT_ORIGINAL'): Promise<void> {
	const res = await fetch(`${BASE}/intake/status`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ path, status })
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to update managed status: ${res.status}`);
	}
}

export async function updateBulkIntakeManagedStatus(
	paths: string[],
	status: 'REVIEWED' | 'KEPT_ORIGINAL'
): Promise<BulkManagedStatusResponse> {
	const res = await fetch(`${BASE}/intake/status/bulk`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ paths, status })
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to update bulk managed status: ${res.status}`);
	}
	return res.json();
}

export async function fetchJob(id: number): Promise<{ job_id: number; tasks: Task[] }> {
	const res = await fetch(`${BASE}/jobs/${id}`);
	if (!res.ok) throw new Error(`Failed to fetch job ${id}: ${res.status}`);
	return res.json();
}

export async function approveJob(id: number): Promise<void> {
	const res = await fetch(`${BASE}/jobs/${id}/approve`, { method: 'POST' });
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to approve job ${id}: ${res.status}`);
	}
}

export async function rejectJob(id: number): Promise<void> {
	const res = await fetch(`${BASE}/jobs/${id}/reject`, { method: 'POST' });
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to reject job ${id}: ${res.status}`);
	}
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

export async function searchLibraryInternetMetadata(relativePath: string, queryOverride: string): Promise<InternetMetadataResponse> {
	const params = new URLSearchParams({ path: relativePath, query: queryOverride });
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

export async function autoSelectLibraryInternetMetadataBulk(paths: string[]): Promise<BulkInternetAutoSelectResponse> {
	const res = await fetch(`${BASE}/library/internet/bulk/select`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ paths })
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to auto-select bulk internet metadata: ${res.status}`);
	}
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

export async function fetchRelatedLibraryInternetMetadataPaths(path: string): Promise<RelatedInternetMetadataPathsResponse> {
	const params = new URLSearchParams({ path });
	const res = await fetch(`${BASE}/library/internet/related?${params.toString()}`);
	if (!res.ok) throw new Error(`Failed to fetch related internet metadata paths for ${path}: ${res.status}`);
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

export async function organizeLibraryFile(input: {
	path: string;
	library_id?: string;
	selected?: InternetMetadataMatch;
	season?: number;
	episode?: number;
	scope?: 'file' | 'movie_folder';
	merge_existing?: boolean;
	apply?: boolean;
}): Promise<OrganizeLibraryResult> {
	const res = await fetch(`${BASE}/library/organize`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(input)
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to organize library file: ${res.status}`);
	}
	return res.json();
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

export async function testLlmConnection(llm: LlmConfig): Promise<LlmTestResponse> {
	const res = await fetch(`${BASE}/config/llm/test`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(llm)
	});
	const data = await res.json();
	if (!res.ok) throw new Error(data?.message || `Failed to test LLM connection: ${res.status}`);
	return data;
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

export async function updateLibrary(id: string, folder: LibraryFolder): Promise<LibraryFolder> {
	const res = await fetch(`${BASE}/libraries/${encodeURIComponent(id)}`, {
		method: 'PUT',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(folder)
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(text || `Failed to update library: ${res.status}`);
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
