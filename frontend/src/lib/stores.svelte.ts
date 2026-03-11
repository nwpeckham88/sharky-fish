// Shared reactive state for the entire app (Svelte 5 Runes).

import { fetchBacklogSummary, type BacklogSummary, type Job, type LibraryChangeEvent } from './api';
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

const EMPTY_BACKLOG_SUMMARY: BacklogSummary = {
	total_items: 0,
	needs_attention_count: 0,
	unprocessed_count: 0,
	reviewed_count: 0,
	kept_original_count: 0,
	awaiting_approval_count: 0,
	approved_count: 0,
	processed_count: 0,
	failed_count: 0,
	missing_metadata_count: 0,
	missing_sidecar_count: 0,
	organize_needed_count: 0
};

// Jobs shared across dashboard, intake, forge
export const jobStore = $state<{ jobs: Job[]; loading: boolean }>({
	jobs: [],
	loading: true
});

// Live FFmpeg progress keyed by job_id
export const progressStore = $state<Record<number, SseProgress>>({});

// Backend health
export const healthStore = $state<{ connected: boolean }>({ connected: false });

// Lightweight managed-item state used for backlog badges and quick refreshes.
export const managedItemStore = $state<{
	summary: BacklogSummary;
	loading: boolean;
}>({
	summary: { ...EMPTY_BACKLOG_SUMMARY },
	loading: true
});


export function getReviewState() {
	const awaitingApproval = jobStore.jobs.filter((job) => job.status === 'AWAITING_APPROVAL');

	return {
		awaitingApproval,
		counts: {
			awaitingApproval: awaitingApproval.length
		}
	};
}

export function getExecutionState() {
	const approved = jobStore.jobs.filter((job) => job.status === 'APPROVED');
	const processing = jobStore.jobs.filter((job) => job.status === 'PROCESSING');
	const completed = jobStore.jobs.filter((job) => job.status === 'COMPLETED');
	const failed = jobStore.jobs.filter((job) => job.status === 'FAILED');
	const jobs = [...approved, ...processing, ...completed, ...failed].sort((left, right) =>
		right.created_at.localeCompare(left.created_at)
	);

	return {
		jobs,
		approved,
		processing,
		completed,
		failed,
		counts: {
			approved: approved.length,
			processing: processing.length,
			completed: completed.length,
			failed: failed.length
		}
	};
}

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
			...jobStore.jobs.filter((job) => job.id !== event.job_id)
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

export async function refreshManagedItemStore() {
	managedItemStore.loading = true;
	try {
		managedItemStore.summary = await fetchBacklogSummary();
	} catch {
		managedItemStore.summary = { ...EMPTY_BACKLOG_SUMMARY };
	} finally {
		managedItemStore.loading = false;
	}
}
