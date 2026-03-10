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
}

export interface LibraryResponse {
	items: LibraryEntry[];
	total_items: number;
	limit: number;
	offset: number;
	summary: LibrarySummary;
	roots: LibraryRoots;
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
	probe: {
		format: string;
		duration_secs: number;
		streams: MediaStreamInfo[];
	};
	cached: boolean;
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

export interface GoldenStandards {
	video: VideoStandards;
	audio: AudioStandards;
}

export interface LlmConfig {
	provider: string;
	base_url: string;
	model: string;
	api_key: string | null;
}

export interface AppConfig {
	data_path: string;
	ingest_path: string;
	config_path: string;
	port: number;
	llm: LlmConfig;
	max_io_concurrency: number;
	metadata_prewarm_limit: number;
	golden_standards: GoldenStandards;
	system_prompt: string;
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

export async function fetchLibrary(query = '', limit = 40, offset = 0): Promise<LibraryResponse> {
	const params = new URLSearchParams({
		limit: String(limit),
		offset: String(offset)
	});

	if (query.trim()) {
		params.set('q', query.trim());
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

export async function fetchLibraryEvents(limit = 24): Promise<LibraryChangeEvent[]> {
	const params = new URLSearchParams({ limit: String(limit) });
	const res = await fetch(`${BASE}/library/events?${params.toString()}`);
	if (!res.ok) throw new Error(`Failed to fetch library events: ${res.status}`);
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

export async function fetchHealth(): Promise<boolean> {
	try {
		const res = await fetch(`${BASE}/health`);
		return res.ok;
	} catch {
		return false;
	}
}
