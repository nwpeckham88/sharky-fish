// Shared reactive state for the entire app (Svelte 5 Runes).

import type { Job, LibraryChangeEvent } from './api';
import type { SseEvent, SseProgress } from './sse';

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
}>({
	changeCount: 0,
	latestChange: null,
	recentChanges: []
});

/** Central SSE event handler — called from the layout's EventSource. */
export function handleSseEvent(event: SseEvent) {
	if (event.type === 'job_created') {
		jobStore.jobs = [
			{ id: event.job_id, file_path: event.file_path, status: 'PENDING', created_at: new Date().toISOString() },
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
