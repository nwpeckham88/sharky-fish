// SSE client for real-time events from the Rust backend.

export type SseJobCreated = {
	type: 'job_created';
	job_id: number;
	file_path: string;
	status: string;
	group_key: string | null;
	group_label: string | null;
	group_kind: string;
};
export type SseJobStatus = { type: 'job_status'; job_id: number; status: string };
export type SseLibraryChange = {
	type: 'library_change';
	relative_path: string;
	path: string;
	change: string;
	occurred_at: number;
};
export type SseLibraryScanProgress = {
	type: 'library_scan_progress';
	status: string;
	scanned_items: number;
	total_items: number;
	started_at?: number;
	completed_at?: number;
	last_scan_at?: number;
	last_error?: string;
};
export type SseProgress = {
	type: 'progress';
	job_id: number;
	frame?: number;
	fps?: number;
	speed?: string;
	time_secs?: number;
	percent?: number;
};
export type SseJobCompleted = { type: 'job_completed'; job_id: number; success: boolean };
export type SseEvent =
	| SseJobCreated
	| SseJobStatus
	| SseLibraryChange
	| SseLibraryScanProgress
	| SseProgress
	| SseJobCompleted;

export function createEventSource(
	onEvent: (event: SseEvent) => void,
	onError?: (err: Event) => void
): EventSource {
	const es = new EventSource('/api/events');

	es.onmessage = (msg) => {
		try {
			const data: SseEvent = JSON.parse(msg.data);
			onEvent(data);
		} catch (error) {
			if (import.meta.env.DEV) {
				console.warn('Ignoring malformed SSE message', { error, payload: msg.data });
			}
		}
	};

	es.onerror = (err) => {
		onError?.(err);
	};

	return es;
}
