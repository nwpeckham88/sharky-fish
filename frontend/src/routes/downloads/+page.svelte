<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import {
		deleteDownloadItem,
		fetchDownloadItems,
		fetchDownloadLinkedPaths,
		fetchQbittorrentStatus,
		type DownloadItem,
		type DownloadsLibraryMatch,
		type DownloadsSummary,
		type QbittorrentStatusResponse
	} from '$lib/api';
	import { formatBytes, formatTimestamp } from '$lib/status';

	type DownloadsFilter = 'all' | 'linked_import' | 'checksum_duplicate' | 'download_orphan' | 'possibly_duplicated';

	const filters: Array<{ value: DownloadsFilter; label: string }> = [
		{ value: 'all', label: 'All' },
		{ value: 'linked_import', label: 'Linked Imports' },
		{ value: 'checksum_duplicate', label: 'Checksum Duplicates' },
		{ value: 'download_orphan', label: 'Orphans' },
		{ value: 'possibly_duplicated', label: 'Possible Duplicates' }
	];

	let items = $state<DownloadItem[]>([]);
	let summary = $state<DownloadsSummary>({
		total_items: 0,
		total_bytes: 0,
		linked_import_count: 0,
		checksum_duplicate_count: 0,
		orphan_count: 0,
		possibly_duplicated_count: 0,
		hard_linked_count: 0
	});
	let loading = $state(true);
	let error = $state('');
	let query = $state('');
	let filter = $state<DownloadsFilter>('all');
	type MatchGroups = {
		linked_paths: DownloadsLibraryMatch[];
		checksum_paths: DownloadsLibraryMatch[];
		heuristic_paths: DownloadsLibraryMatch[];
		checksum_blake3: string;
	};

	let linkedPaths = $state<Record<string, MatchGroups>>({});
	let linkedLoading = $state<Record<string, boolean>>({});
	let deleteBusy = $state<Record<string, boolean>>({});
	let status = $state('');
	let queryTimer: ReturnType<typeof setTimeout> | undefined;
	let qbStatus = $state<QbittorrentStatusResponse | null>(null);
	let qbLoading = $state(false);

	onMount(async () => {
		await Promise.all([loadItems(), loadQbittorrentStatus()]);
	});

	async function loadQbittorrentStatus() {
		qbLoading = true;
		try {
			qbStatus = await fetchQbittorrentStatus();
		} catch {
			qbStatus = {
				enabled: true,
				connected: false,
				base_url: '',
				transfer: null,
				torrents: [],
				error: 'Failed to query qBittorrent status'
			};
		} finally {
			qbLoading = false;
		}
	}

	async function loadItems() {
		loading = true;
		error = '';
		try {
			const response = await fetchDownloadItems({
				query,
				classification: filter,
				limit: 250,
				offset: 0
			});
			items = response.items;
			summary = response.summary;
		} catch (loadError) {
			error = loadError instanceof Error ? loadError.message : 'Failed to load downloads audit';
			items = [];
		} finally {
			loading = false;
		}
	}

	function scheduleLoad() {
		if (queryTimer) clearTimeout(queryTimer);
		queryTimer = setTimeout(() => {
			void loadItems();
		}, 180);
	}

	async function loadLinkedPaths(item: DownloadItem) {
		const existing = linkedPaths[item.relative_path];
		if (existing) return existing;

		linkedLoading = { ...linkedLoading, [item.relative_path]: true };
		try {
			const response = await fetchDownloadLinkedPaths(item.relative_path);
			linkedPaths = { ...linkedPaths, [item.relative_path]: response };
			return response;
		} finally {
			linkedLoading = { ...linkedLoading, [item.relative_path]: false };
		}
	}

	function bestLibraryPathForMatch(match: MatchGroups): DownloadsLibraryMatch | null {
		return match.linked_paths[0] ?? match.checksum_paths[0] ?? match.heuristic_paths[0] ?? null;
	}

	function libraryHref(match: DownloadsLibraryMatch): string {
		const params = new URLSearchParams({ path: match.path });
		if (match.library_id) {
			params.set('library', match.library_id);
		}
		return `/library?${params.toString()}`;
	}

	function organizeHref(match: DownloadsLibraryMatch): string {
		const params = new URLSearchParams({ path: match.path, autopreview: '1' });
		if (match.library_id) {
			params.set('library', match.library_id);
		}
		return `/organize?${params.toString()}`;
	}

	async function openLinkedPaths(item: DownloadItem) {
		if (linkedPaths[item.relative_path]) {
			const next = { ...linkedPaths };
			delete next[item.relative_path];
			linkedPaths = next;
			return;
		}

		linkedLoading = { ...linkedLoading, [item.relative_path]: true };
		try {
			const response = await loadLinkedPaths(item);
			linkedPaths = { ...linkedPaths, [item.relative_path]: response };
		} catch (loadError) {
			status = loadError instanceof Error ? loadError.message : 'Failed to load linked library paths';
		}
	}

	async function openBestLibraryMatch(item: DownloadItem) {
		status = '';
		try {
			const response = await loadLinkedPaths(item);
			const targetMatch = bestLibraryPathForMatch(response);
			if (!targetMatch) {
				status = 'No library match is available for this download yet.';
				return;
			}
			await goto(libraryHref(targetMatch));
		} catch (loadError) {
			status = loadError instanceof Error ? loadError.message : 'Failed to resolve the best library match';
		}
	}

	async function openBestOrganizeMatch(item: DownloadItem) {
		status = '';
		try {
			const response = await loadLinkedPaths(item);
			const targetMatch = bestLibraryPathForMatch(response);
			if (!targetMatch) {
				status = 'No library match is available to organize yet.';
				return;
			}
			await goto(organizeHref(targetMatch));
		} catch (loadError) {
			status = loadError instanceof Error ? loadError.message : 'Failed to resolve the best organize target';
		}
	}

	async function removeItem(item: DownloadItem) {
		const confirmed = confirm(
			`Delete ${item.relative_path}? This action only removes the download-side path.`
		);
		if (!confirmed) return;

		deleteBusy = { ...deleteBusy, [item.relative_path]: true };
		status = '';
		try {
			const response = await deleteDownloadItem(item.relative_path);
			status = response.warning ?? (response.frees_space
				? 'Download deleted and disk space can be reclaimed.'
				: 'Download path deleted.');
			await loadItems();
		} catch (deleteError) {
			status = deleteError instanceof Error ? deleteError.message : 'Failed to delete download item';
		} finally {
			deleteBusy = { ...deleteBusy, [item.relative_path]: false };
		}
	}

	function classificationLabel(value: string): string {
		if (value === 'linked_import') return 'Linked Import';
		if (value === 'checksum_duplicate') return 'Checksum Duplicate';
		if (value === 'download_orphan') return 'Orphan';
		if (value === 'possibly_duplicated') return 'Possible Duplicate';
		return value;
	}

	function shortChecksum(value: string): string {
		return value.length > 16 ? `${value.slice(0, 16)}…` : value;
	}

	function bestMatchLabel(item: DownloadItem): string {
		if (item.classification === 'linked_import') return 'Open Linked Import';
		if (item.classification === 'checksum_duplicate') return 'Open Exact Duplicate';
		return 'Open Best Match';
	}

	function canOpenBestMatch(item: DownloadItem): boolean {
		return item.linked_library_paths_count > 0 || item.checksum_library_paths_count > 0 || item.duplicate_library_paths_count > 0;
	}
</script>

<section class="mb-6 grid gap-4 lg:grid-cols-[minmax(0,1.1fr)_minmax(18rem,0.9fr)]">
	<div class="surface-card p-6">
		<p class="section-label">Downloads Audit</p>
		<h2 class="mt-2 text-3xl text-[color:var(--ink-strong)]">Review download-folder hygiene before deleting anything</h2>
		<p class="mt-3 text-sm leading-6 text-[color:var(--ink-muted)]">
			This page compares the ingest root against the indexed library by inode and BLAKE3 checksum so you can separate hard-linked imports from exact duplicate content.
		</p>
	</div>

	<div class="surface-card p-6">
		<p class="section-label">Safety Rules</p>
		<div class="mt-4 space-y-2 text-sm text-[color:var(--ink-muted)]">
			<p>Deleting a download path is always explicit and manual.</p>
			<p>When a file is hard-linked into the library, deleting the download removes one directory entry only and does not free space.</p>
			<p>Use linked-path inspection before cleanup when link counts are greater than 1.</p>
		</div>
	</div>
</section>

<section class="mb-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
	<div class="stat-card"><div class="section-label">Downloads</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{summary.total_items}</div></div>
	<div class="stat-card"><div class="section-label">Linked Imports</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{summary.linked_import_count}</div></div>
	<div class="stat-card"><div class="section-label">Checksum Duplicates</div><div class="mt-1 text-2xl font-semibold text-[color:var(--accent-deep)]">{summary.checksum_duplicate_count}</div></div>
	<div class="stat-card"><div class="section-label">Orphans</div><div class="mt-1 text-2xl font-semibold text-[color:var(--danger)]">{summary.orphan_count}</div></div>
	<div class="stat-card"><div class="section-label">Footprint</div><div class="mt-1 text-2xl font-semibold text-[color:var(--ink-strong)]">{formatBytes(summary.total_bytes)}</div><div class="mt-1 text-xs text-[color:var(--ink-muted)]">{summary.hard_linked_count} currently hard-linked</div></div>
</section>

<section class="mb-5 surface-card p-5">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<div>
			<p class="section-label">qBittorrent</p>
			<h3 class="mt-1 text-lg font-semibold text-[color:var(--ink-strong)]">Transfer Monitor</h3>
			<p class="mt-1 text-xs text-[color:var(--ink-muted)]">Optional API feed for download/upload activity and current torrent paths.</p>
		</div>
		<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={loadQbittorrentStatus} disabled={qbLoading}>
			{qbLoading ? 'Refreshing…' : 'Refresh'}
		</button>
	</div>

	{#if !qbStatus}
		<div class="mt-3 text-sm text-[color:var(--ink-muted)]">No qBittorrent status loaded yet.</div>
	{:else if !qbStatus.enabled}
		<div class="mt-3 rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-2 text-sm text-[color:var(--ink-muted)]">qBittorrent monitoring is disabled in Settings → System Profile.</div>
	{:else if qbStatus.error || !qbStatus.connected}
		<div class="mt-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-sm text-[color:var(--danger)]">
			Unable to read qBittorrent API at <span class="font-mono">{qbStatus.base_url || '(not configured)'}</span>{qbStatus.error ? `: ${qbStatus.error}` : ''}
		</div>
	{:else}
		{#if qbStatus.transfer}
			<div class="mt-3 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
				<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Download Rate</div><div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{formatBytes(qbStatus.transfer.dl_info_speed)}/s</div></div>
				<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Upload Rate</div><div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{formatBytes(qbStatus.transfer.up_info_speed)}/s</div></div>
				<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Downloaded</div><div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{formatBytes(qbStatus.transfer.dl_info_data)}</div></div>
				<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Uploaded</div><div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{formatBytes(qbStatus.transfer.up_info_data)}</div><div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">{qbStatus.transfer.connection_status} · DHT {qbStatus.transfer.dht_nodes}</div></div>
			</div>
		{/if}
		<div class="mt-3 rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-3">
			<div class="section-label mb-2">Recent Torrents ({qbStatus.torrents.length})</div>
			{#if qbStatus.torrents.length === 0}
				<div class="text-xs text-[color:var(--ink-muted)]">No torrents returned.</div>
			{:else}
				<div class="space-y-2">
					{#each qbStatus.torrents.slice(0, 12) as torrent (torrent.hash)}
						<div class="rounded-lg border border-[color:var(--line)] bg-white/40 px-3 py-2">
							<div class="flex flex-wrap items-center justify-between gap-2">
								<div class="font-semibold text-[color:var(--ink-strong)]">{torrent.name}</div>
								<div class="text-xs text-[color:var(--ink-muted)]">{Math.round((torrent.progress ?? 0) * 100)}% · {torrent.state}</div>
							</div>
							<div class="mt-1 text-xs text-[color:var(--ink-muted)]">↓ {formatBytes(torrent.dlspeed)}/s · ↑ {formatBytes(torrent.upspeed)}/s · {formatBytes(torrent.total_size || torrent.size)}</div>
							<div class="mt-1 break-all font-mono text-[11px] text-[color:var(--ink-muted)]">{torrent.content_path || torrent.save_path}</div>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</section>

<section class="mb-5 flex flex-wrap items-center gap-3">
	<label class="flex flex-1 items-center gap-3 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5">
		<svg class="h-4 w-4 text-[color:var(--ink-muted)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
		<input
			class="min-w-0 flex-1 bg-transparent text-sm text-[color:var(--ink-strong)] outline-none placeholder:text-[color:var(--ink-muted)]"
			type="search"
			placeholder="Search downloads path or file name"
			bind:value={query}
			oninput={scheduleLoad}
		/>
	</label>
	<div class="flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1">
		{#each filters as option (option.value)}
			<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {filter === option.value ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={async () => { filter = option.value; await loadItems(); }}>
				{option.label}
			</button>
		{/each}
	</div>
</section>

{#if status}
	<div class="mb-5 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-4 py-3 text-sm text-[color:var(--accent-deep)]">{status}</div>
{/if}

{#if error}
	<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-3 text-sm text-[color:var(--danger)]">{error}</div>
{:else if loading}
	<div class="surface-card px-8 py-16 text-center text-[color:var(--ink-muted)]">Scanning downloads…</div>
{:else if items.length === 0}
	<div class="surface-card px-8 py-16 text-center text-[color:var(--ink-muted)]">No downloads matched this view.</div>
{:else}
	<div class="space-y-3">
		{#each items as item (item.relative_path)}
			<div class="surface-card p-5">
				<div class="flex flex-wrap items-start justify-between gap-3">
					<div class="min-w-0">
						<div class="flex flex-wrap items-center gap-2">
							<h3 class="text-base font-semibold text-[color:var(--ink-strong)]">{item.file_name}</h3>
							<span class="rounded-full border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-strong)]">{classificationLabel(item.classification)}</span>
							{#if item.filesystem.is_hard_linked}
								<span class="rounded-full border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--accent-deep)]">{item.filesystem.link_count} links</span>
							{/if}
						</div>
						<p class="mt-1 break-all font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</p>
					</div>
					<div class="flex flex-wrap gap-2">
						<button class="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-white disabled:opacity-50" onclick={() => openBestLibraryMatch(item)} disabled={linkedLoading[item.relative_path] || !canOpenBestMatch(item)}>
							{linkedLoading[item.relative_path] ? 'Resolving…' : bestMatchLabel(item)}
						</button>
						<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => openBestOrganizeMatch(item)} disabled={linkedLoading[item.relative_path] || !canOpenBestMatch(item)}>
							{linkedLoading[item.relative_path] ? 'Resolving…' : 'Open In Organize'}
						</button>
						<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => openLinkedPaths(item)} disabled={linkedLoading[item.relative_path]}>
							{linkedLoading[item.relative_path] ? 'Loading…' : linkedPaths[item.relative_path] ? 'Hide Library Matches' : 'Open Library Matches'}
						</button>
						<button class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-1.5 text-xs font-semibold text-[color:var(--danger)] disabled:opacity-50" onclick={() => removeItem(item)} disabled={deleteBusy[item.relative_path]}>
							{deleteBusy[item.relative_path] ? 'Deleting…' : 'Delete'}
						</button>
					</div>
				</div>

				<div class="mt-4 grid gap-2 sm:grid-cols-2 xl:grid-cols-5">
					<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Size</div><div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{formatBytes(item.size_bytes)}</div></div>
					<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Modified</div><div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{formatTimestamp(item.modified_at)}</div></div>
					<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Inode</div><div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{item.filesystem.inode}</div><div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">Device {item.filesystem.device_id}</div></div>
					<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Checksum</div><div class="mt-0.5 font-mono text-[11px] font-semibold text-[color:var(--ink-strong)]">{shortChecksum(item.checksum_blake3)}</div><div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">BLAKE3 content hash</div></div>
					<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5"><div class="section-label">Library Matches</div><div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{item.linked_library_paths_count} inode</div><div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">{item.checksum_library_paths_count} checksum · {item.duplicate_library_paths_count} name heuristics</div></div>
				</div>

				{#if item.classification === 'checksum_duplicate'}
					<div class="mt-3 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
						This download has the same checksum as indexed library content, even though it is not sharing the same inode.
					</div>
				{/if}

				{#if item.filesystem.is_hard_linked}
					<div class="mt-3 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
						Deleting this path does not free space unless it is the last remaining hard link.
					</div>
				{/if}

				{#if linkedPaths[item.relative_path]}
					<div class="mt-3 rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-3">
						<div class="section-label mb-2">Library Matches</div>
						<div class="rounded-lg border border-[color:var(--line)] bg-white/40 px-3 py-2 text-xs text-[color:var(--ink-muted)]">
							<div class="font-semibold text-[color:var(--ink-strong)]">BLAKE3</div>
							<div class="mt-1 break-all font-mono">{linkedPaths[item.relative_path].checksum_blake3}</div>
						</div>
						<div class="mt-3 grid gap-3 lg:grid-cols-3">
							<div>
								<div class="mb-2 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">Same Inode</div>
								{#if linkedPaths[item.relative_path].linked_paths.length === 0}
									<div class="text-xs text-[color:var(--ink-muted)]">No hard-link matches.</div>
								{:else}
									<div class="space-y-1.5">
										{#each linkedPaths[item.relative_path].linked_paths as match (match.path)}
											<a href={libraryHref(match)} class="block break-all font-mono text-[11px] text-[color:var(--accent-deep)] hover:underline">{match.path}</a>
										{/each}
									</div>
								{/if}
							</div>
							<div>
								<div class="mb-2 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">Same Checksum</div>
								{#if linkedPaths[item.relative_path].checksum_paths.length === 0}
									<div class="text-xs text-[color:var(--ink-muted)]">No checksum matches.</div>
								{:else}
									<div class="space-y-1.5">
										{#each linkedPaths[item.relative_path].checksum_paths as match (match.path)}
											<a href={libraryHref(match)} class="block break-all font-mono text-[11px] text-[color:var(--accent-deep)] hover:underline">{match.path}</a>
										{/each}
									</div>
								{/if}
							</div>
							<div>
								<div class="mb-2 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">Name Heuristics</div>
								{#if linkedPaths[item.relative_path].heuristic_paths.length === 0}
									<div class="text-xs text-[color:var(--ink-muted)]">No heuristic matches.</div>
								{:else}
									<div class="space-y-1.5">
										{#each linkedPaths[item.relative_path].heuristic_paths as match (match.path)}
											<a href={libraryHref(match)} class="block break-all font-mono text-[11px] text-[color:var(--accent-deep)] hover:underline">{match.path}</a>
										{/each}
									</div>
								{/if}
							</div>
						</div>
					</div>
				{/if}
			</div>
		{/each}
	</div>
{/if}