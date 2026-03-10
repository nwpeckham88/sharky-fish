<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { page } from '$app/state';
	import {
		fetchLibrary,
		fetchLibraryMetadata,
		fetchLibraryEvents,
		type LibraryEntry,
		type LibraryMetadata,
		type LibraryResponse,
		type LibraryRoots,
		type LibrarySummary,
		type LibraryChangeEvent
	} from '$lib/api';
	import { libraryState } from '$lib/stores.svelte';

	let library = $state<LibraryEntry[]>([]);
	let librarySummary = $state<LibrarySummary>({
		total_items: 0, total_bytes: 0, video_items: 0, audio_items: 0, other_items: 0
	});
	let roots = $state<LibraryRoots>({ library_path: '/data', ingest_path: '/ingest' });
	let selectedItem = $state<LibraryEntry | null>(null);
	let selectedMetadata = $state<LibraryMetadata | null>(null);
	let metadataLoading = $state(false);
	let metadataError = $state('');
	let libraryLoading = $state(true);
	let query = $state('');
	let offset = $state(0);
	let totalLibrary = $state(0);
	let recentChanges = $state<LibraryChangeEvent[]>([]);
	const pageSize = 40;
	let queryTimer: ReturnType<typeof setTimeout> | undefined;
	let refreshTimer: ReturnType<typeof setTimeout> | undefined;

	// Filter state
	let typeFilter = $state('all');

	onMount(async () => {
		const urlQuery = page.url.searchParams.get('q');
		if (urlQuery) query = urlQuery;
		await Promise.all([loadLibrary(), loadLibraryEvents()]);
	});

	onDestroy(() => {
		if (queryTimer) clearTimeout(queryTimer);
		if (refreshTimer) clearTimeout(refreshTimer);
	});

	// React to SSE library changes via the global store
	const _changeFlags = { skipFirst: true };
	$effect(() => {
		libraryState.changeCount;
		if (_changeFlags.skipFirst) {
			_changeFlags.skipFirst = false;
			return;
		}
		if (selectedItem && libraryState.latestChange?.relative_path === selectedItem.relative_path) {
			void loadMetadata(selectedItem);
		}
		scheduleLibraryRefresh();
	});

	async function loadLibrary() {
		libraryLoading = true;
		try {
			const response: LibraryResponse = await fetchLibrary(query, pageSize, offset);
			library = response.items;
			librarySummary = response.summary;
			roots = response.roots;
			totalLibrary = response.total_items;
			if (selectedItem) {
				const updated = response.items.find((i) => i.relative_path === selectedItem?.relative_path) ?? null;
				selectedItem = updated;
				if (!updated) { selectedMetadata = null; metadataError = ''; }
			}
		} catch {
			library = [];
			totalLibrary = 0;
		} finally {
			libraryLoading = false;
		}
	}

	async function loadLibraryEvents() {
		try { recentChanges = await fetchLibraryEvents(12); } catch { recentChanges = []; }
	}

	async function loadMetadata(item: LibraryEntry) {
		selectedItem = item;
		selectedMetadata = null;
		metadataError = '';
		metadataLoading = true;
		try {
			selectedMetadata = await fetchLibraryMetadata(item.relative_path);
		} catch (error) {
			metadataError = error instanceof Error ? error.message : 'Metadata fetch failed';
		} finally {
			metadataLoading = false;
		}
	}

	function scheduleLibraryLoad(resetOffset = false) {
		if (resetOffset) offset = 0;
		if (queryTimer) clearTimeout(queryTimer);
		queryTimer = setTimeout(() => { void loadLibrary(); }, 220);
	}

	function scheduleLibraryRefresh() {
		if (refreshTimer) clearTimeout(refreshTimer);
		refreshTimer = setTimeout(() => { void Promise.all([loadLibrary(), loadLibraryEvents()]); }, 700);
	}

	function handleSearchInput(event: Event) {
		query = (event.currentTarget as HTMLInputElement).value;
		scheduleLibraryLoad(true);
	}

	function formatBytes(value: number): string {
		if (!value) return '0 B';
		const units = ['B', 'KB', 'MB', 'GB', 'TB'];
		let size = value;
		let unitIndex = 0;
		while (size >= 1024 && unitIndex < units.length - 1) { size /= 1024; unitIndex += 1; }
		return `${size.toFixed(size >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
	}

	function formatTimestamp(value: number | null | undefined): string {
		if (!value) return 'Unknown';
		return new Date(value * 1000).toLocaleString();
	}

	function formatDuration(value: number | null | undefined): string {
		if (!value) return 'Unknown';
		const t = Math.round(value);
		const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
		if (h > 0) return `${h}h ${m}m ${s}s`;
		if (m > 0) return `${m}m ${s}s`;
		return `${s}s`;
	}

	function pageRangeLabel(): string {
		if (totalLibrary === 0) return '0-0';
		return `${offset + 1}-${Math.min(offset + pageSize, totalLibrary)}`;
	}

	function nextPage() {
		if (offset + pageSize >= totalLibrary) return;
		offset += pageSize;
		void loadLibrary();
	}

	function previousPage() {
		if (offset === 0) return;
		offset = Math.max(0, offset - pageSize);
		void loadLibrary();
	}

	const filteredLibrary = $derived(
		typeFilter === 'all' ? library : library.filter((i) => i.media_type === typeFilter)
	);
</script>

<!-- Summary Stats Row -->
<section class="mb-5 grid gap-3 sm:grid-cols-5">
	<div class="stat-card"><div class="section-label">Total</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.total_items}</div></div>
	<div class="stat-card"><div class="section-label">Footprint</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{formatBytes(librarySummary.total_bytes)}</div></div>
	<div class="stat-card"><div class="section-label">Video</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.video_items}</div></div>
	<div class="stat-card"><div class="section-label">Audio</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.audio_items}</div></div>
	<div class="stat-card"><div class="section-label">Other</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.other_items}</div></div>
</section>

<!-- Controls Bar -->
<section class="mb-5 flex flex-wrap items-center gap-3">
	<label class="flex flex-1 items-center gap-3 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5">
		<svg class="h-4 w-4 text-[color:var(--ink-muted)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
		<input class="min-w-0 flex-1 bg-transparent text-sm text-[color:var(--ink-strong)] outline-none placeholder:text-[color:var(--ink-muted)]" type="search" placeholder="Search paths, filenames, codecs…" value={query} oninput={handleSearchInput} />
	</label>
	<div class="flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1">
		{#each ['all', 'video', 'audio', 'other'] as t (t)}
			<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {typeFilter === t ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { typeFilter = t; }}>{t}</button>
		{/each}
	</div>
	<div class="rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5 text-sm text-[color:var(--ink-muted)]">
		{pageRangeLabel()} of {totalLibrary}
	</div>
</section>

<!-- Main Content -->
<section class="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(20rem,0.7fr)]">
	<!-- File Table -->
	<div class="overflow-hidden rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)]">
		<table class="w-full text-left text-sm">
			<thead class="border-b border-[color:var(--line)] bg-[color:rgba(234,223,201,0.6)] text-xs uppercase tracking-[0.18em] text-[color:var(--ink-muted)]">
				<tr>
					<th class="px-4 py-3">Path</th>
					<th class="px-4 py-3">Type</th>
					<th class="px-4 py-3">Size</th>
					<th class="px-4 py-3">Modified</th>
				</tr>
			</thead>
			<tbody>
				{#if libraryLoading}
					<tr><td colspan="4" class="px-4 py-14 text-center text-[color:var(--ink-muted)]">Scanning library…</td></tr>
				{:else if filteredLibrary.length === 0}
					<tr><td colspan="4" class="px-4 py-14 text-center text-[color:var(--ink-muted)]">No entries match the current filter.</td></tr>
				{:else}
					{#each filteredLibrary as item (item.relative_path)}
						<tr class="cursor-pointer border-b border-[color:rgba(123,105,81,0.14)] last:border-b-0 hover:bg-[color:rgba(214,180,111,0.08)] {selectedItem?.relative_path === item.relative_path ? 'bg-[color:rgba(214,180,111,0.12)]' : ''}" onclick={() => loadMetadata(item)}>
							<td class="px-4 py-3">
								<div class="font-medium text-[color:var(--ink-strong)]">{item.file_name}</div>
								<div class="mt-0.5 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</div>
							</td>
							<td class="px-4 py-3">
								<span class="status-chip {item.media_type === 'video' ? 'processing' : item.media_type === 'audio' ? 'completed' : ''}">{item.media_type}</span>
							</td>
							<td class="px-4 py-3 text-[color:var(--ink-strong)]">{formatBytes(item.size_bytes)}</td>
							<td class="px-4 py-3 text-[color:var(--ink-muted)]">{formatTimestamp(item.modified_at)}</td>
						</tr>
					{/each}
				{/if}
			</tbody>
		</table>
	</div>

	<!-- Metadata Panel -->
	<div class="space-y-4">
		<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-5">
			<p class="section-label mb-3">Metadata Inspector</p>
			{#if !selectedItem}
				<div class="rounded-lg border border-dashed border-[color:var(--line)] px-5 py-8 text-center text-sm text-[color:var(--ink-muted)]">
					Select a file to inspect its streams, codec, and container metadata.
				</div>
			{:else if metadataLoading}
				<div class="rounded-lg border border-[color:var(--line)] px-5 py-8 text-center text-sm text-[color:var(--ink-muted)]">
					Probing {selectedItem.file_name}…
				</div>
			{:else if metadataError}
				<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-5 py-4 text-sm text-[color:var(--danger)]">
					{metadataError}
				</div>
			{:else if selectedMetadata}
				<div class="space-y-3 text-sm">
					<div>
						<h4 class="text-lg text-[color:var(--ink-strong)]">{selectedItem.file_name}</h4>
						<p class="mt-0.5 break-all font-mono text-[11px] text-[color:var(--ink-muted)]">{selectedMetadata.relative_path}</p>
					</div>
					<div class="grid gap-2 sm:grid-cols-2">
						<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
							<div class="section-label">Container</div>
							<div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{selectedMetadata.format}</div>
						</div>
						<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
							<div class="section-label">Duration</div>
							<div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{formatDuration(selectedMetadata.duration_secs)}</div>
						</div>
						<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
							<div class="section-label">Video</div>
							<div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{selectedMetadata.video_codec ?? 'None'}</div>
							<div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">{selectedMetadata.width && selectedMetadata.height ? `${selectedMetadata.width}×${selectedMetadata.height}` : 'No frame size'}</div>
						</div>
						<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
							<div class="section-label">Audio</div>
							<div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{selectedMetadata.audio_codec ?? 'None'}</div>
							<div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">{selectedMetadata.audio_channels ? `${selectedMetadata.audio_channels} channels` : 'No channels'}</div>
						</div>
					</div>
					<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
						<div class="mb-2 flex items-center justify-between">
							<div class="section-label">Streams</div>
							<span class="status-chip {selectedMetadata.cached ? 'completed' : 'processing'}">{selectedMetadata.cached ? 'cached' : 'live probe'}</span>
						</div>
						<div class="space-y-1.5">
							{#each selectedMetadata.probe.streams as stream (stream.index)}
								<div class="rounded-lg bg-[color:rgba(244,236,223,0.7)] px-3 py-2.5">
									<div class="flex items-center justify-between">
										<span class="font-semibold text-[color:var(--ink-strong)]">#{stream.index} · {stream.codec_type}</span>
										<span class="font-mono text-[11px] text-[color:var(--ink-muted)]">{stream.codec_name}</span>
									</div>
									<div class="mt-1 text-xs text-[color:var(--ink-muted)]">
										{#if stream.width && stream.height}{stream.width}×{stream.height}{/if}
										{#if stream.channels} · {stream.channels}ch{/if}
										{#if stream.sample_rate} · {stream.sample_rate} Hz{/if}
										{#if stream.bit_rate} · {formatBytes(stream.bit_rate)}/s{/if}
									</div>
								</div>
							{/each}
						</div>
					</div>
				</div>
			{/if}
		</div>

		<!-- Recent Changes -->
		{#if recentChanges.length > 0}
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-5">
				<p class="section-label mb-3">Recent Changes</p>
				<div class="space-y-1.5 max-h-48 overflow-y-auto">
					{#each recentChanges.slice(0, 6) as change (change.path + change.occurred_at)}
						<div class="rounded-lg bg-[color:rgba(244,236,223,0.5)] px-3 py-2">
							<div class="flex items-center justify-between text-[11px] uppercase tracking-[0.12em]">
								<span class="font-semibold text-[color:var(--accent-deep)]">{change.change}</span>
								<span class="text-[color:var(--ink-muted)]">{formatTimestamp(change.occurred_at)}</span>
							</div>
							<div class="truncate font-mono text-[11px] text-[color:var(--ink-strong)]">{change.relative_path}</div>
						</div>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</section>

<!-- Pagination -->
<section class="mt-4 flex items-center justify-between gap-3">
	<div class="text-sm text-[color:var(--ink-muted)]">{librarySummary.video_items} video, {librarySummary.audio_items} audio, {librarySummary.other_items} subtitle assets</div>
	<div class="flex gap-2">
		<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] disabled:opacity-40" onclick={previousPage} disabled={offset === 0}>Previous</button>
		<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] disabled:opacity-40" onclick={nextPage} disabled={offset + pageSize >= totalLibrary}>Next</button>
	</div>
</section>
