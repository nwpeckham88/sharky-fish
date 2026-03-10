<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { page } from '$app/state';
	import {
		fetchLibrary,
		triggerLibraryRescan,
		fetchLibraryMetadata,
		fetchLibraryInternetMetadata,
		fetchLibraryInternetMetadataBulk,
		saveSelectedLibraryInternetMetadata,
		fetchSelectedLibraryInternetMetadata,
		fetchLibraryEvents,
		fetchLibraries,
		type LibraryEntry,
		type LibraryFolder,
		type LibraryMetadata,
		type InternetMetadataMatch,
		type InternetMetadataResponse,
		type LibraryResponse,
		type LibraryRoots,
		type LibrarySummary,
		type LibraryScanStatus,
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
	let internetMetadata = $state<InternetMetadataResponse | null>(null);
	let internetMetadataLoading = $state(false);
	let internetMetadataError = $state('');
	let selectedInternetMatch = $state<InternetMetadataMatch | null>(null);
	let internetSaveLoading = $state(false);
	let internetSaveError = $state('');
	let bulkInternetLoading = $state(false);
	let bulkInternetStatus = $state('');
	let libraryLoading = $state(true);
	let query = $state('');
	let offset = $state(0);
	let totalLibrary = $state(0);
	let recentChanges = $state<LibraryChangeEvent[]>([]);
	let scanStatus = $state<LibraryScanStatus>({
		status: 'idle',
		scanned_items: 0,
		total_items: 0,
		started_at: null,
		completed_at: null,
		last_scan_at: null,
		last_error: null
	});
	let rescanLoading = $state(false);
	let rescanError = $state('');
	const pageSize = 40;
	let queryTimer: ReturnType<typeof setTimeout> | undefined;
	let refreshTimer: ReturnType<typeof setTimeout> | undefined;

	// Library folder tabs
	let libraryFolders = $state<LibraryFolder[]>([]);
	let activeLibraryId = $state<string | null>(null);

	// Filter state
	let typeFilter = $state('all');

	// Bulk selection
	let selectedPaths = $state<Set<string>>(new Set());
	let bulkMode = $state(false);
	let expandedShows = $state<Set<string>>(new Set());

	onMount(async () => {
		const urlQuery = page.url.searchParams.get('q');
		if (urlQuery) query = urlQuery;
		const urlLib = page.url.searchParams.get('library');
		if (urlLib) activeLibraryId = urlLib;
		try {
			libraryFolders = await fetchLibraries();
		} catch { libraryFolders = []; }
		await Promise.all([loadLibrary(), loadLibraryEvents()]);
	});

	onDestroy(() => {
		if (queryTimer) clearTimeout(queryTimer);
		if (refreshTimer) clearTimeout(refreshTimer);
	});

	// React to SSE library changes via the global store
	const _changeFlags = { skipFirstChange: true };
	$effect(() => {
		libraryState.scan;
		scanStatus = { ...libraryState.scan };
	});

	$effect(() => {
		libraryState.changeCount;
		if (_changeFlags.skipFirstChange) {
			_changeFlags.skipFirstChange = false;
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
			const response: LibraryResponse = await fetchLibrary(query, pageSize, offset, activeLibraryId ?? undefined);
			library = response.items;
			librarySummary = response.summary;
			roots = response.roots;
			scanStatus = response.scan;
			libraryState.scan = response.scan;
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

	async function runRescan() {
		rescanLoading = true;
		rescanError = '';
		try {
			await triggerLibraryRescan();
		} catch (error) {
			rescanError = error instanceof Error ? error.message : 'Failed to trigger rescan';
		} finally {
			rescanLoading = false;
		}
	}

	function scanProgressPercent(): number {
		if (!scanStatus.total_items) return 0;
		return Math.min(100, Math.round((scanStatus.scanned_items / scanStatus.total_items) * 100));
	}

	async function loadLibraryEvents() {
		try { recentChanges = await fetchLibraryEvents(12); } catch { recentChanges = []; }
	}

	async function loadMetadata(item: LibraryEntry) {
		selectedItem = item;
		selectedMetadata = null;
		metadataError = '';
		internetMetadata = null;
		internetMetadataError = '';
		selectedInternetMatch = null;
		internetSaveError = '';
		metadataLoading = true;
		try {
			const [metadata, selected] = await Promise.all([
				fetchLibraryMetadata(item.relative_path),
				fetchSelectedLibraryInternetMetadata(item.relative_path).catch(() => null)
			]);
			selectedMetadata = metadata;
			selectedInternetMatch = selected?.selected ?? null;
		} catch (error) {
			metadataError = error instanceof Error ? error.message : 'Metadata fetch failed';
		} finally {
			metadataLoading = false;
		}
	}

	async function loadInternetMetadata(item: LibraryEntry) {
		internetMetadataLoading = true;
		internetMetadataError = '';
		internetSaveError = '';
		internetMetadata = null;
		try {
			internetMetadata = await fetchLibraryInternetMetadata(item.relative_path);
		} catch (error) {
			internetMetadataError = error instanceof Error ? error.message : 'Internet metadata lookup failed';
		} finally {
			internetMetadataLoading = false;
		}
	}

	function matchesSelected(match: InternetMetadataMatch): boolean {
		if (!selectedInternetMatch) return false;
		return (
			selectedInternetMatch.provider === match.provider &&
			selectedInternetMatch.title === match.title &&
			selectedInternetMatch.year === match.year &&
			selectedInternetMatch.imdb_id === match.imdb_id &&
			selectedInternetMatch.tvdb_id === match.tvdb_id
		);
	}

	async function chooseInternetMatch(match: InternetMetadataMatch) {
		if (!selectedItem) return;
		internetSaveLoading = true;
		internetSaveError = '';
		try {
			const saved = await saveSelectedLibraryInternetMetadata(selectedItem.relative_path, match);
			selectedInternetMatch = saved.selected;
		} catch (error) {
			internetSaveError = error instanceof Error ? error.message : 'Failed to save selected match';
		} finally {
			internetSaveLoading = false;
		}
	}

	async function runBulkInternetLookup(autoSelectTop = false) {
		if (selectedPaths.size === 0) return;
		bulkInternetLoading = true;
		bulkInternetStatus = '';
		internetSaveError = '';
		try {
			const paths = Array.from(selectedPaths);
			const response = await fetchLibraryInternetMetadataBulk(paths);
			const withMatches = response.items.filter((item) => item.result.matches.length > 0);
			const withWarnings = response.items.filter((item) => item.result.warnings.length > 0);

			let autoSelected = 0;
			if (autoSelectTop) {
				for (const item of withMatches) {
					const first = item.result.matches[0];
					if (!first) continue;
					await saveSelectedLibraryInternetMetadata(item.path, first);
					autoSelected += 1;
				}
				if (selectedItem) {
					const refreshed = await fetchSelectedLibraryInternetMetadata(selectedItem.relative_path).catch(() => null);
					selectedInternetMatch = refreshed?.selected ?? selectedInternetMatch;
				}
			}

			bulkInternetStatus = autoSelectTop
				? `Looked up ${response.items.length} file(s), found matches for ${withMatches.length}, auto-selected ${autoSelected}.`
				: `Looked up ${response.items.length} file(s), found matches for ${withMatches.length}, warnings on ${withWarnings.length}.`;
		} catch (error) {
			bulkInternetStatus = error instanceof Error ? error.message : 'Bulk internet lookup failed';
		} finally {
			bulkInternetLoading = false;
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
		typeFilter === 'all'
			? library
			: typeFilter === 'other'
				? library.filter((i) => i.media_type !== 'video' && i.media_type !== 'audio')
				: library.filter((i) => i.media_type === typeFilter)
	);

	const activeLibraryFolder = $derived(
		activeLibraryId ? libraryFolders.find((l) => l.id === activeLibraryId) ?? null : null
	);

	const tvShowGroups = $derived.by(() => {
		if (!activeLibraryFolder || activeLibraryFolder.media_type !== 'tv') {
			return [] as Array<{ show: string; items: LibraryEntry[] }>;
		}

		const groups = new Map<string, LibraryEntry[]>();
		for (const item of filteredLibrary) {
			const stripped = stripLibraryPrefix(item.relative_path, activeLibraryFolder.path);
			const parts = stripped.split('/').filter(Boolean);
			const show = parts[0] ?? 'Unsorted';
			const arr = groups.get(show) ?? [];
			arr.push(item);
			groups.set(show, arr);
		}

		return Array.from(groups.entries())
			.map(([show, items]) => ({ show, items: items.sort((a, b) => a.relative_path.localeCompare(b.relative_path)) }))
			.sort((a, b) => a.show.localeCompare(b.show));
	});

	const visibleColumnCount = $derived(
		(bulkMode ? 1 : 0) +
		1 +
		1 +
		(!activeLibraryId && libraryFolders.length > 0 ? 1 : 0) +
		1 +
		1
	);

	function switchLibrary(id: string | null) {
		activeLibraryId = id;
		offset = 0;
		selectedItem = null;
		selectedMetadata = null;
		selectedPaths = new Set();
		expandedShows = new Set();
		void loadLibrary();
	}

	function toggleSelect(path: string) {
		const next = new Set(selectedPaths);
		if (next.has(path)) { next.delete(path); } else { next.add(path); }
		selectedPaths = next;
	}

	function toggleSelectAll() {
		if (selectedPaths.size === filteredLibrary.length) {
			selectedPaths = new Set();
		} else {
			selectedPaths = new Set(filteredLibrary.map((i) => i.relative_path));
		}
	}

	function clearSelection() {
		selectedPaths = new Set();
		bulkMode = false;
	}

	function stripLibraryPrefix(relativePath: string, libraryPath: string): string {
		const normalized = relativePath.replaceAll('\\', '/');
		const prefix = libraryPath.endsWith('/') ? libraryPath : `${libraryPath}/`;
		if (normalized === libraryPath) return '';
		if (normalized.startsWith(prefix)) return normalized.slice(prefix.length);
		return normalized;
	}

	function toggleShow(show: string) {
		const next = new Set(expandedShows);
		if (next.has(show)) next.delete(show);
		else next.add(show);
		expandedShows = next;
	}

	function activeLibraryName(): string {
		if (!activeLibraryId) return 'All Libraries';
		return libraryFolders.find((l) => l.id === activeLibraryId)?.name ?? 'Unknown';
	}
</script>

<!-- Library Folder Tabs -->
{#if libraryFolders.length > 0}
	<section class="mb-5 flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1 w-fit">
		<button class="rounded-lg px-4 py-2 text-sm font-semibold transition-colors {activeLibraryId === null ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => switchLibrary(null)}>All</button>
		{#each libraryFolders as folder (folder.id)}
			<button class="flex items-center gap-1.5 rounded-lg px-4 py-2 text-sm font-semibold transition-colors {activeLibraryId === folder.id ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => switchLibrary(folder.id)}>
				{#if folder.media_type === 'movie'}
					<svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><path d="M7 2v20M17 2v20M2 12h20M2 7h5M2 17h5M17 17h5M17 7h5"/></svg>
				{:else}
					<svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="7" width="20" height="15" rx="2" ry="2"/><polyline points="17 2 12 7 7 2"/></svg>
				{/if}
				{folder.name}
			</button>
		{/each}
	</section>
{/if}

<!-- Summary Stats Row -->
<section class="mb-5 grid gap-3 sm:grid-cols-5">
	<div class="stat-card"><div class="section-label">Total</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.total_items}</div></div>
	<div class="stat-card"><div class="section-label">Footprint</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{formatBytes(librarySummary.total_bytes)}</div></div>
	<div class="stat-card"><div class="section-label">Video</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.video_items}</div></div>
	<div class="stat-card"><div class="section-label">Audio</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.audio_items}</div></div>
	<div class="stat-card"><div class="section-label">Other</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.other_items}</div></div>
</section>

<section class="mb-5 rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div>
			<p class="section-label">Library Index</p>
			<p class="mt-1 text-sm text-[color:var(--ink-muted)]">
				Status: <span class="font-semibold text-[color:var(--ink-strong)]">{scanStatus.status}</span>
				{#if scanStatus.status === 'running'}
					· {scanStatus.scanned_items}/{scanStatus.total_items} ({scanProgressPercent()}%)
				{/if}
				{#if scanStatus.last_scan_at}
					· Last scan {formatTimestamp(scanStatus.last_scan_at)}
				{/if}
			</p>
			{#if scanStatus.last_error}
				<p class="mt-1 text-xs text-[color:var(--danger)]">{scanStatus.last_error}</p>
			{/if}
		</div>
		<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] text-[color:var(--ink-strong)] disabled:opacity-50" onclick={runRescan} disabled={rescanLoading || scanStatus.status === 'running'}>
			{rescanLoading ? 'Starting…' : scanStatus.status === 'running' ? 'Rescan Running' : 'Rescan Now'}
		</button>
	</div>
	{#if scanStatus.status === 'running'}
		<div class="mt-3 h-2 overflow-hidden rounded-full bg-[color:rgba(123,105,81,0.16)]">
			<div class="h-full bg-[color:var(--accent)] transition-[width] duration-300" style={`width: ${scanProgressPercent()}%`}></div>
		</div>
	{/if}
	{#if rescanError}
		<p class="mt-2 text-xs text-[color:var(--danger)]">{rescanError}</p>
	{/if}
</section>

<!-- Controls Bar -->
<section class="mb-5 flex flex-wrap items-center gap-3">
	<label class="flex flex-1 items-center gap-3 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5">
		<svg class="h-4 w-4 text-[color:var(--ink-muted)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
		<input class="min-w-0 flex-1 bg-transparent text-sm text-[color:var(--ink-strong)] outline-none placeholder:text-[color:var(--ink-muted)]" type="search" placeholder="Search paths, filenames, codecs…" value={query} oninput={handleSearchInput} />
	</label>
	<div class="flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1">
		{#each ['all', 'video', 'audio', 'subtitle', 'other'] as t (t)}
			<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {typeFilter === t ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { typeFilter = t; }}>{t}</button>
		{/each}
	</div>
	<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {bulkMode ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { bulkMode = !bulkMode; if (!bulkMode) clearSelection(); }}>
		Select
	</button>
	<div class="rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5 text-sm text-[color:var(--ink-muted)]">
		{pageRangeLabel()} of {totalLibrary}
	</div>
</section>

<!-- Bulk Actions Bar -->
{#if bulkMode && selectedPaths.size > 0}
	<section class="mb-4 flex items-center gap-3 rounded-xl border border-[color:var(--accent)]/30 bg-[color:var(--accent)]/5 px-4 py-2.5">
		<span class="text-sm font-semibold text-[color:var(--ink-strong)]">{selectedPaths.size} selected</span>
		<div class="flex gap-2">
			<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => runBulkInternetLookup(false)} disabled={bulkInternetLoading}>{bulkInternetLoading ? 'Working…' : 'Bulk Lookup Metadata'}</button>
			<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => runBulkInternetLookup(true)} disabled={bulkInternetLoading}>{bulkInternetLoading ? 'Working…' : 'Auto-Select Top Match'}</button>
		</div>
		{#if bulkInternetStatus}
			<span class="text-xs text-[color:var(--ink-muted)]">{bulkInternetStatus}</span>
		{/if}
		<button class="ml-auto text-xs font-semibold text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]" onclick={clearSelection}>Clear</button>
	</section>
{/if}

<!-- Main Content -->
<section class="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(20rem,0.7fr)]">
	<!-- File Table -->
	<div class="overflow-hidden rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)]">
		{#if activeLibraryFolder?.media_type === 'tv'}
			<div class="border-b border-[color:var(--line)] bg-[color:rgba(234,223,201,0.6)] px-4 py-3 text-xs uppercase tracking-[0.18em] text-[color:var(--ink-muted)]">
				TV Shows (Expandable)
			</div>
			{#if libraryLoading}
				<div class="px-4 py-14 text-center text-[color:var(--ink-muted)]">Scanning library…</div>
			{:else if tvShowGroups.length === 0}
				<div class="px-4 py-14 text-center text-[color:var(--ink-muted)]">No show entries match the current filter.</div>
			{:else}
				<div class="max-h-[38rem] overflow-y-auto">
					{#each tvShowGroups as group (group.show)}
						<div class="border-b border-[color:rgba(123,105,81,0.14)]">
							<button class="flex w-full items-center justify-between px-4 py-3 text-left hover:bg-[color:rgba(214,180,111,0.08)]" onclick={() => toggleShow(group.show)}>
								<div>
									<div class="font-semibold text-[color:var(--ink-strong)]">{group.show}</div>
									<div class="text-xs text-[color:var(--ink-muted)]">{group.items.length} episode file(s)</div>
								</div>
								<span class="text-[color:var(--ink-muted)]">{expandedShows.has(group.show) ? '▾' : '▸'}</span>
							</button>
							{#if expandedShows.has(group.show)}
								<div class="bg-[color:rgba(244,236,223,0.5)]">
									{#each group.items as item (item.relative_path)}
										<button class="block w-full px-8 py-2.5 text-left text-sm hover:bg-[color:rgba(214,180,111,0.08)] {selectedItem?.relative_path === item.relative_path ? 'bg-[color:rgba(214,180,111,0.12)]' : ''}" onclick={() => loadMetadata(item)}>
											<div class="font-medium text-[color:var(--ink-strong)]">{item.file_name}</div>
											<div class="mt-0.5 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</div>
										</button>
									{/each}
								</div>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		{:else}
			<table class="w-full text-left text-sm">
				<thead class="border-b border-[color:var(--line)] bg-[color:rgba(234,223,201,0.6)] text-xs uppercase tracking-[0.18em] text-[color:var(--ink-muted)]">
					<tr>
						{#if bulkMode}
							<th class="w-10 px-3 py-3">
								<input type="checkbox" checked={filteredLibrary.length > 0 && selectedPaths.size === filteredLibrary.length} onchange={toggleSelectAll} class="accent-[color:var(--accent)]" />
							</th>
						{/if}
						<th class="px-4 py-3">Path</th>
						<th class="px-4 py-3">Type</th>
						{#if !activeLibraryId && libraryFolders.length > 0}
							<th class="px-4 py-3">Library</th>
						{/if}
						<th class="px-4 py-3">Size</th>
						<th class="px-4 py-3">Modified</th>
					</tr>
				</thead>
				<tbody>
					{#if libraryLoading}
						<tr><td colspan={visibleColumnCount} class="px-4 py-14 text-center text-[color:var(--ink-muted)]">Scanning library…</td></tr>
					{:else if filteredLibrary.length === 0}
						<tr><td colspan={visibleColumnCount} class="px-4 py-14 text-center text-[color:var(--ink-muted)]">No entries match the current filter.</td></tr>
					{:else}
						{#each filteredLibrary as item (item.relative_path)}
							<tr class="cursor-pointer border-b border-[color:rgba(123,105,81,0.14)] last:border-b-0 hover:bg-[color:rgba(214,180,111,0.08)] {selectedItem?.relative_path === item.relative_path ? 'bg-[color:rgba(214,180,111,0.12)]' : ''} {selectedPaths.has(item.relative_path) ? 'bg-[color:rgba(214,180,111,0.08)]' : ''}" onclick={() => { if (bulkMode) { toggleSelect(item.relative_path); } else { loadMetadata(item); } }}>
								{#if bulkMode}
									<td class="w-10 px-3 py-3" onclick={(e) => { e.stopPropagation(); toggleSelect(item.relative_path); }}>
										<input type="checkbox" checked={selectedPaths.has(item.relative_path)} class="accent-[color:var(--accent)]" />
									</td>
								{/if}
								<td class="px-4 py-3">
									<div class="font-medium text-[color:var(--ink-strong)]">{item.file_name}</div>
									<div class="mt-0.5 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</div>
								</td>
								<td class="px-4 py-3">
									<span class="status-chip {item.media_type === 'video' ? 'processing' : item.media_type === 'audio' ? 'completed' : ''}">{item.media_type}</span>
								</td>
								{#if !activeLibraryId && libraryFolders.length > 0}
									<td class="px-4 py-3">
										{#if item.library_id}
											{@const lib = libraryFolders.find((l) => l.id === item.library_id)}
											{#if lib}
												<span class="rounded-full bg-[color:rgba(214,180,111,0.15)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-strong)]">{lib.name}</span>
											{/if}
										{:else}
											<span class="text-[color:var(--ink-muted)] text-xs">—</span>
										{/if}
									</td>
								{/if}
								<td class="px-4 py-3 text-[color:var(--ink-strong)]">{formatBytes(item.size_bytes)}</td>
								<td class="px-4 py-3 text-[color:var(--ink-muted)]">{formatTimestamp(item.modified_at)}</td>
							</tr>
						{/each}
					{/if}
				</tbody>
			</table>
		{/if}
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
							<div class="flex flex-wrap items-center justify-between gap-2">
								<h4 class="text-lg text-[color:var(--ink-strong)]">{selectedItem.file_name}</h4>
								<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => loadInternetMetadata(selectedItem!)} disabled={internetMetadataLoading}>
									{internetMetadataLoading ? 'Looking up…' : 'Lookup IMDb/TVDB'}
								</button>
							</div>
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

					<!-- Subtitle Summary -->
					{#if selectedMetadata.subtitle_count > 0}
						<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
							<div class="section-label mb-1.5">Subtitles · {selectedMetadata.subtitle_count} track{selectedMetadata.subtitle_count !== 1 ? 's' : ''}</div>
							<div class="space-y-1">
								{#each selectedMetadata.probe.streams.filter(s => s.codec_type === 'subtitle') as sub (sub.index)}
									<div class="flex items-center gap-2 text-xs">
										<span class="font-mono text-[color:var(--ink-muted)]">#{sub.index}</span>
										<span class="font-semibold text-[color:var(--ink-strong)]">{sub.language ?? 'und'}</span>
										<span class="text-[color:var(--ink-muted)]">{sub.codec_name}</span>
										{#if sub.disposition?.forced}
											<span class="rounded-full bg-[color:rgba(164,79,45,0.12)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--accent-deep)]">forced</span>
										{/if}
										{#if sub.disposition?.hearing_impaired}
											<span class="rounded-full bg-[color:rgba(106,142,72,0.12)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--olive)]">SDH</span>
										{/if}
										{#if sub.disposition?.default}
											<span class="rounded-full bg-[color:rgba(214,180,111,0.2)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">default</span>
										{/if}
										{#if sub.title}
											<span class="truncate text-[color:var(--ink-muted)]">{sub.title}</span>
										{/if}
									</div>
								{/each}
							</div>
							{#if selectedMetadata.subtitle_languages.length > 0}
								<div class="mt-1.5 text-[11px] text-[color:var(--ink-muted)]">Languages: {selectedMetadata.subtitle_languages.join(', ')}</div>
							{/if}
						</div>
					{:else}
						<div class="rounded-lg border border-dashed border-[color:var(--line)] px-3 py-2.5 text-xs text-[color:var(--ink-muted)]">No subtitle streams</div>
					{/if}
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
										<div class="flex items-center gap-1.5">
											{#if stream.language}<span class="rounded-full bg-[color:rgba(214,180,111,0.15)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-strong)]">{stream.language}</span>{/if}
											<span class="font-mono text-[11px] text-[color:var(--ink-muted)]">{stream.codec_name}</span>
										</div>
									</div>
									<div class="mt-1 flex items-center gap-1.5 text-xs text-[color:var(--ink-muted)]">
										{#if stream.width && stream.height}{stream.width}×{stream.height}{/if}
										{#if stream.channels} · {stream.channels}ch{/if}
										{#if stream.sample_rate} · {stream.sample_rate} Hz{/if}
										{#if stream.bit_rate} · {formatBytes(stream.bit_rate)}/s{/if}
										{#if stream.disposition?.forced}<span class="rounded-full bg-[color:rgba(164,79,45,0.12)] px-1.5 py-0.5 text-[10px] font-bold text-[color:var(--accent-deep)]">forced</span>{/if}
										{#if stream.disposition?.hearing_impaired}<span class="rounded-full bg-[color:rgba(106,142,72,0.12)] px-1.5 py-0.5 text-[10px] font-bold text-[color:var(--olive)]">SDH</span>{/if}
										{#if stream.title}<span class="truncate">{stream.title}</span>{/if}
									</div>
								</div>
							{/each}
						</div>
					</div>

					<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
						<div class="mb-2 flex items-center justify-between">
							<div class="section-label">Internet Metadata</div>
						</div>
						{#if selectedInternetMatch}
							<div class="mb-2 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">
								Selected: <span class="font-semibold">{selectedInternetMatch.title}{selectedInternetMatch.year ? ` (${selectedInternetMatch.year})` : ''}</span> via {selectedInternetMatch.provider}
							</div>
						{/if}
						{#if internetSaveError}
							<div class="mb-2 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{internetSaveError}</div>
						{/if}
						{#if internetMetadataError}
							<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{internetMetadataError}</div>
						{:else if internetMetadataLoading}
							<div class="text-xs text-[color:var(--ink-muted)]">Querying providers…</div>
						{:else if internetMetadata}
							{#if internetMetadata.matches.length === 0}
								<div class="text-xs text-[color:var(--ink-muted)]">No matches found for "{internetMetadata.query}".</div>
							{:else}
								<div class="space-y-2">
									{#each internetMetadata.matches as match, index (match.provider + '-' + index)}
										<div class="rounded-lg bg-[color:rgba(244,236,223,0.7)] px-3 py-2">
											<div class="flex flex-wrap items-center gap-2">
												<span class="font-semibold text-[color:var(--ink-strong)]">{match.title}{match.year ? ` (${match.year})` : ''}</span>
												<span class="rounded-full bg-[color:rgba(214,180,111,0.2)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-strong)]">{match.provider}</span>
												{#if match.media_kind}<span class="text-[10px] uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">{match.media_kind}</span>{/if}
												{#if match.rating}<span class="text-xs text-[color:var(--ink-muted)]">IMDb {match.rating.toFixed(1)}</span>{/if}
											</div>
											{#if match.overview}
												<p class="mt-1 text-xs text-[color:var(--ink-muted)]">{match.overview}</p>
											{/if}
											<div class="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-[color:var(--ink-muted)]">
												{#if match.imdb_id}<span class="font-mono">{match.imdb_id}</span>{/if}
												{#if match.tvdb_id}<span class="font-mono">tvdb:{match.tvdb_id}</span>{/if}
												{#if match.genres.length > 0}<span>{match.genres.join(', ')}</span>{/if}
												<button class="rounded-md border border-[color:var(--line)] px-2 py-1 text-[10px] font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => chooseInternetMatch(match)} disabled={internetSaveLoading || matchesSelected(match)}>
													{matchesSelected(match) ? 'Selected' : internetSaveLoading ? 'Saving…' : 'Use this match'}
												</button>
												{#if match.source_url}
													<a href={match.source_url} target="_blank" rel="noreferrer" class="font-semibold text-[color:var(--accent-deep)] hover:underline">Open</a>
												{/if}
											</div>
										</div>
									{/each}
								</div>
							{/if}
							{#if internetMetadata.warnings.length > 0}
								<div class="mt-2 text-[11px] text-[color:var(--ink-muted)]">{internetMetadata.warnings.join(' • ')}</div>
							{/if}
						{:else}
							<div class="text-xs text-[color:var(--ink-muted)]">Lookup with OMDb (IMDb-backed) and TVDB using your configured API keys.</div>
						{/if}
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
	<div class="text-sm text-[color:var(--ink-muted)]">{activeLibraryName()} · {librarySummary.video_items} video, {librarySummary.audio_items} audio, {librarySummary.other_items} subtitle assets</div>
	<div class="flex gap-2">
		<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] disabled:opacity-40" onclick={previousPage} disabled={offset === 0}>Previous</button>
		<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] disabled:opacity-40" onclick={nextPage} disabled={offset + pageSize >= totalLibrary}>Next</button>
	</div>
</section>
