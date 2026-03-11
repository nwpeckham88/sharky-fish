// Shared reactive state for the entire app (Svelte 5 Runes).

import type { Job, LibraryChangeEvent } from './api';
import type { SseEvent, SseProgress } from './sse';

export interface LibraryScanState {
	status: string;
	scanned_items: number;
	total_items: number;
	started_at: number | null;
	completed_at: number | null;
	last_scan_at: number | null;
	last_error: string | null;
}

// Jobs shared across dashboard, intake, forge
export const jobStore = $state<{ jobs: Job[]; loading: boolean }>({
	jobs: [],
	loading: true
});

// Live FFmpeg progress keyed by job_id
export const progressStore = $state<Record<number, SseProgress>>({});

// Backend health
export const healthStore = $state<{ connected: boolean }>({ connected: false });

// Library change signals from SSE (used by dashboard + library pages)
export const libraryState = $state<{
	changeCount: number;
	latestChange: LibraryChangeEvent | null;
	recentChanges: LibraryChangeEvent[];
	scan: LibraryScanState;
}>({
	changeCount: 0,
	latestChange: null,
	recentChanges: [],
	scan: {
		status: 'idle',
		scanned_items: 0,
		total_items: 0,
		started_at: null,
		completed_at: null,
		last_scan_at: null,
		last_error: null
	}
});

/** Central SSE event handler — called from the layout's EventSource. */
export function handleSseEvent(event: SseEvent) {
	if (event.type === 'job_created') {
		jobStore.jobs = [
			{
				id: event.job_id,
				file_path: event.file_path,
				status: event.status,
				created_at: new Date().toISOString(),
				probe: null,
				decision: null
			},
			...jobStore.jobs
		];
		return;
	}
	if (event.type === 'job_status') {
		jobStore.jobs = jobStore.jobs.map((job) =>
			job.id === event.job_id ? { ...job, status: event.status } : job
		);
		return;
	}
	if (event.type === 'library_change') {
		const change: LibraryChangeEvent = {
			relative_path: event.relative_path,
			path: event.path,
			change: event.change,
			occurred_at: event.occurred_at
		};
		libraryState.latestChange = change;
		libraryState.recentChanges = [change, ...libraryState.recentChanges].slice(0, 24);
		libraryState.changeCount++;
		return;
	}
	if (event.type === 'library_scan_progress') {
		libraryState.scan = {
			status: event.status,
			scanned_items: event.scanned_items,
			total_items: event.total_items,
			started_at: event.started_at ?? null,
			completed_at: event.completed_at ?? null,
			last_scan_at: event.last_scan_at ?? null,
			last_error: event.last_error ?? null
		};
		return;
	}
	if (event.type === 'progress') {
		jobStore.jobs = jobStore.jobs.map((job) =>
			job.id === event.job_id ? { ...job, status: 'PROCESSING' } : job
		);
		progressStore[event.job_id] = event;
		return;
	}
	if (event.type === 'job_completed') {
		jobStore.jobs = jobStore.jobs.map((job) =>
			job.id === event.job_id ? { ...job, status: event.success ? 'COMPLETED' : 'FAILED' } : job
		);
		delete progressStore[event.job_id];
	}
}
