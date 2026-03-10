<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import {
		fetchLibraryEvents,
		fetchLibrary,
		type LibraryChangeEvent,
		type LibraryRoots,
		type LibrarySummary
	} from '$lib/api';
	import { jobStore, progressStore, libraryState } from '$lib/stores.svelte';

	let recentChanges = $state<LibraryChangeEvent[]>([]);
	let librarySummary = $state<LibrarySummary>({
		total_items: 0,
		total_bytes: 0,
		video_items: 0,
		audio_items: 0,
		other_items: 0
	});
	let roots = $state<LibraryRoots>({ library_path: '/data', ingest_path: '/ingest' });
	let localLoading = $state(true);
	let refreshTimer: ReturnType<typeof setTimeout> | undefined;

	onMount(async () => {
		await Promise.all([loadSummary(), loadLibraryEvents()]);
		localLoading = false;
	});

	onDestroy(() => {
		if (refreshTimer) clearTimeout(refreshTimer);
	});

	async function loadSummary() {
		try {
			const response = await fetchLibrary('', 1, 0);
			librarySummary = response.summary;
			roots = response.roots;
		} catch {
			/* keep defaults */
		}
	}

	async function loadLibraryEvents() {
		try {
			recentChanges = await fetchLibraryEvents(12);
		} catch {
			recentChanges = [];
		}
	}

	function scheduleRefresh() {
		if (refreshTimer) clearTimeout(refreshTimer);
		refreshTimer = setTimeout(() => {
			void Promise.all([loadSummary(), loadLibraryEvents()]);
		}, 700);
	}

	// React to SSE library changes via the global store
	const _changeFlags = { skipFirst: true };
	$effect(() => {
		libraryState.changeCount;
		if (_changeFlags.skipFirst) {
			_changeFlags.skipFirst = false;
			return;
		}
		if (libraryState.latestChange) {
			recentChanges = [libraryState.latestChange, ...recentChanges].slice(0, 12);
		}
		scheduleRefresh();
	});

	function formatBytes(value: number): string {
		if (!value) return '0 B';
		const units = ['B', 'KB', 'MB', 'GB', 'TB'];
		let size = value;
		let unitIndex = 0;
		while (size >= 1024 && unitIndex < units.length - 1) {
			size /= 1024;
			unitIndex += 1;
		}
		return `${size.toFixed(size >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
	}

	function formatTimestamp(value: number | null | undefined): string {
		if (!value) return 'Unknown';
		return new Date(value * 1000).toLocaleString();
	}

	function statusTone(status: string): string {
		switch (status) {
			case 'COMPLETED': return 'completed';
			case 'FAILED': return 'failed';
			case 'PROCESSING': return 'processing';
			default: return '';
		}
	}

	const jobs = $derived(jobStore.jobs);
	const progress = progressStore;
	const loading = $derived(jobStore.loading || localLoading);

	const pending = $derived(jobs.filter((job) => job.status === 'PENDING').length);
	const processing = $derived(jobs.filter((job) => job.status === 'PROCESSING').length);
	const completed = $derived(jobs.filter((job) => job.status === 'COMPLETED').length);
	const failed = $derived(jobs.filter((job) => job.status === 'FAILED').length);
	const activeProgress = $derived(Object.values(progress));
</script>

<!-- Summary Stats -->
<section class="mb-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
	<div class="stat-card">
		<div class="section-label">Library Items</div>
		<div class="metric-value mt-2">{librarySummary.total_items}</div>
		<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{librarySummary.video_items} video · {librarySummary.audio_items} audio</div>
	</div>
	<div class="stat-card">
		<div class="section-label">Footprint</div>
		<div class="metric-value mt-2">{formatBytes(librarySummary.total_bytes)}</div>
		<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{librarySummary.other_items} subtitle assets</div>
	</div>
	<div class="stat-card">
		<div class="section-label">Queue</div>
		<div class="metric-value mt-2">{pending + processing}</div>
		<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{pending} pending · {processing} active</div>
	</div>
	<div class="stat-card">
		<div class="section-label">Processed</div>
		<div class="metric-value mt-2">{completed}</div>
		<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{failed} failed</div>
	</div>
</section>

<!-- Active Transcodes + Recent Changes -->
<section class="mb-6 grid gap-5 lg:grid-cols-2">
	<!-- Active Transcodes -->
	<div class="surface-card p-5">
		<div class="mb-4 flex items-center justify-between gap-3">
			<div>
				<p class="section-label mb-1">Active Transcodes</p>
				<p class="text-lg text-[color:var(--ink-strong)]">Live progress</p>
			</div>
			{#if activeProgress.length > 0}
				<span class="status-chip processing">{activeProgress.length} running</span>
			{/if}
		</div>
		{#if activeProgress.length > 0}
			<div class="space-y-3">
				{#each activeProgress as p (p.job_id)}
					{@const job = jobs.find((j) => j.id === p.job_id)}
					<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
						<div class="mb-2 flex items-center justify-between gap-3 text-sm">
							<div class="min-w-0">
								<div class="truncate font-medium text-[color:var(--ink-strong)]">{job?.file_path ?? `Job #${p.job_id}`}</div>
							</div>
							<div class="text-right text-[color:var(--accent-deep)]">
								<div class="text-base font-semibold">{p.percent != null ? `${p.percent.toFixed(1)}%` : '…'}</div>
								<div class="text-xs">{p.speed ?? ''}{#if p.fps} · {p.fps.toFixed(1)} fps{/if}</div>
							</div>
						</div>
						<div class="h-2 overflow-hidden rounded-full bg-[color:var(--paper-deep)]">
							<div class="h-full rounded-full bg-[linear-gradient(90deg,var(--accent),var(--accent-soft),var(--olive))] transition-all duration-300" style="width: {p.percent ?? 0}%"></div>
						</div>
					</div>
				{/each}
			</div>
		{:else}
			<div class="rounded-[1rem] border border-dashed border-[color:var(--line)] bg-[color:rgba(255,248,237,0.6)] px-5 py-8 text-center text-sm text-[color:var(--ink-muted)]">
				No active transcodes. Work submitted to <a href="/forge" class="underline">The Forge</a> will appear here.
			</div>
		{/if}
	</div>

	<!-- Recent Changes -->
	<div class="surface-card p-5">
		<div class="mb-4 flex items-center justify-between gap-3">
			<div>
				<p class="section-label mb-1">Library Feed</p>
				<p class="text-lg text-[color:var(--ink-strong)]">Recent changes</p>
			</div>
			<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--olive)]">{recentChanges.length} events</span>
		</div>
		{#if recentChanges.length === 0}
			<div class="rounded-[1rem] border border-dashed border-[color:var(--line)] bg-[color:rgba(255,248,237,0.6)] px-5 py-8 text-sm text-[color:var(--ink-muted)]">
				Watching for file changes. Creations, removals, and renames will appear here.
			</div>
		{:else}
			<div class="space-y-2 max-h-[24rem] overflow-y-auto">
				{#each recentChanges as change (change.path + change.occurred_at)}
					<div class="rounded-[0.875rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-3">
						<div class="mb-1 flex items-center justify-between gap-3 text-xs uppercase tracking-[0.16em]">
							<span class="font-semibold text-[color:var(--accent-deep)]">{change.change}</span>
							<span class="text-[color:var(--ink-muted)]">{formatTimestamp(change.occurred_at)}</span>
						</div>
						<div class="truncate font-mono text-[13px] text-[color:var(--ink-strong)]">{change.relative_path}</div>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</section>

<!-- Monitoring Paths -->
<section class="mb-6 grid gap-5 sm:grid-cols-2">
	<div class="surface-card p-5">
		<div class="mb-1 font-semibold uppercase tracking-[0.18em] text-[color:var(--olive)] text-xs">Library Root</div>
		<div class="break-all rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-3 font-mono text-[12px] text-[color:var(--ink-strong)]">{roots.library_path}</div>
	</div>
	<div class="surface-card p-5">
		<div class="mb-1 font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)] text-xs">Ingest Root</div>
		<div class="break-all rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-3 font-mono text-[12px] text-[color:var(--ink-strong)]">{roots.ingest_path}</div>
	</div>
</section>

<!-- Recent Jobs -->
<section class="surface-card p-5">
	<div class="mb-4 flex items-center justify-between gap-4">
		<div>
			<p class="section-label mb-1">Recent Jobs</p>
			<p class="text-lg text-[color:var(--ink-strong)]">Pipeline ledger</p>
		</div>
		<a href="/forge" class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-medium text-[color:var(--ink-strong)] no-underline hover:bg-[color:var(--paper-deep)]">View all →</a>
	</div>
	{#if loading}
		<p class="text-[color:var(--ink-muted)]">Loading…</p>
	{:else if jobs.length === 0}
		<div class="rounded-[1rem] border border-dashed border-[color:var(--line)] bg-[color:rgba(255,248,237,0.6)] py-10 text-center text-[color:var(--ink-muted)]">
			<p class="font-[family-name:var(--font-display)] text-xl text-[color:var(--ink-strong)]">No jobs yet</p>
			<p class="mt-2 text-sm">Drop media into the ingest path to start.</p>
		</div>
	{:else}
		<div class="overflow-hidden rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)]">
			<table class="w-full text-left text-sm">
				<thead class="border-b border-[color:var(--line)] bg-[color:rgba(234,223,201,0.6)] text-xs uppercase tracking-[0.18em] text-[color:var(--ink-muted)]">
					<tr>
						<th class="px-4 py-3">ID</th>
						<th class="px-4 py-3">File</th>
						<th class="px-4 py-3">Status</th>
						<th class="px-4 py-3">Created</th>
					</tr>
				</thead>
				<tbody>
					{#each jobs.slice(0, 10) as job (job.id)}
						<tr class="border-b border-[color:rgba(123,105,81,0.14)] last:border-b-0 hover:bg-[color:rgba(214,180,111,0.08)]">
							<td class="px-4 py-3 font-mono text-[color:var(--ink-muted)]">#{job.id}</td>
							<td class="max-w-sm px-4 py-3 truncate font-mono text-[13px] text-[color:var(--ink-strong)]">{job.file_path}</td>
							<td class="px-4 py-3"><span class="status-chip {statusTone(job.status)}">{job.status}</span></td>
							<td class="px-4 py-3 text-[color:var(--ink-muted)]">{new Date(job.created_at).toLocaleString()}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</section>
