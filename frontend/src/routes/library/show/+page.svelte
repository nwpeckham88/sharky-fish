<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import {
		fetchLibraries,
		fetchLibrary,
		fetchLibraryInternetMetadata,
		searchLibraryInternetMetadata,
		fetchSelectedLibraryInternetMetadata,
		saveSelectedLibraryInternetMetadata,
		organizeLibraryFile,
		type InternetMetadataMatch,
		type InternetMetadataResponse,
		type LibraryEntry,
		type LibraryFolder,
		type OrganizeLibraryResult
	} from '$lib/api';
	import { formatBytes, formatTimestamp, statusLabel, statusTone } from '$lib/status';

	let libraries = $state<LibraryFolder[]>([]);
	let activeLibraryId = $state<string | null>(null);
	let showName = $state('');
	let requestedPath = $state<string | null>(null);
	let episodes = $state<LibraryEntry[]>([]);
	let loading = $state(true);
	let error = $state('');
	let metadataResults = $state<InternetMetadataResponse | null>(null);
	let metadataLoading = $state(false);
	let metadataError = $state('');
	let selectedMatch = $state<InternetMetadataMatch | null>(null);
	let manualQuery = $state('');
	let saveLoading = $state(false);
	let saveStatus = $state('');
	let saveWarnings = $state<string[]>([]);
	let batchLoading = $state(false);
	let batchError = $state('');
	let batchStatus = $state('');
	let previewResults = $state<Array<{ item: LibraryEntry; result: OrganizeLibraryResult | null; error: string | null }>>([]);
	let idMode = $state<'none' | 'tvdb'>('none');
	let writeNfo = $state(true);

	function activeLibrary(): LibraryFolder | null {
		return libraries.find((library) => library.id === activeLibraryId) ?? null;
	}

	function stripLibraryPrefix(relativePath: string, libraryPath: string): string {
		const normalized = relativePath.replaceAll('\\', '/');
		const prefix = libraryPath.endsWith('/') ? libraryPath : `${libraryPath}/`;
		if (normalized === libraryPath) return '';
		if (normalized.startsWith(prefix)) return normalized.slice(prefix.length);
		return normalized;
	}

	function showNameForItem(item: LibraryEntry, library: LibraryFolder | null): string {
		const normalized = item.relative_path.replaceAll('\\', '/');
		const stripped = library ? stripLibraryPrefix(normalized, library.path) : normalized;
		return stripped.split('/').filter(Boolean)[0] ?? 'Unknown Show';
	}

	function matchesSelected(match: InternetMetadataMatch): boolean {
		if (!selectedMatch) return false;
		return (
			selectedMatch.provider === match.provider &&
			selectedMatch.title === match.title &&
			selectedMatch.year === match.year &&
			selectedMatch.imdb_id === match.imdb_id &&
			selectedMatch.tvdb_id === match.tvdb_id
		);
	}

	function seasonKeyForItem(item: LibraryEntry): string {
		const normalized = item.relative_path.replaceAll('\\', '/');
		const seasonFolder = normalized.match(/\/Season[ _-]?(\d{1,2})\//i);
		if (seasonFolder) {
			return `Season ${seasonFolder[1].padStart(2, '0')}`;
		}
		const episodePattern = normalized.match(/S(\d{2})E\d{2}/i);
		if (episodePattern) {
			return `Season ${episodePattern[1]}`;
		}
		return 'Unknown Season';
	}

	async function loadShow() {
		loading = true;
		error = '';
		metadataResults = null;
		metadataError = '';
		previewResults = [];
		batchStatus = '';
		batchError = '';
		try {
			const response = await fetchLibrary({
				limit: 500,
				offset: 0,
				libraryId: activeLibraryId ?? undefined,
				mediaType: 'video'
			});
			const library = activeLibrary();
			episodes = response.items
				.filter((item) => item.library_id === activeLibraryId || !activeLibraryId)
				.filter((item) => showNameForItem(item, library) === showName)
				.sort((left, right) => left.relative_path.localeCompare(right.relative_path));

			if (episodes.length === 0) {
				error = 'No episodes found for this show.';
				selectedMatch = null;
				return;
			}

			const seedPath = requestedPath ?? episodes[0].relative_path;
			const selected = await fetchSelectedLibraryInternetMetadata(seedPath).catch(() => null);
			selectedMatch = selected?.selected ?? null;
			requestedPath = null;
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to load TV show';
			episodes = [];
		} finally {
			loading = false;
		}
	}

	async function lookupMetadata(queryOverride: string | null = null) {
		if (episodes.length === 0) return;
		metadataLoading = true;
		metadataError = '';
		try {
			metadataResults = queryOverride && queryOverride.trim()
				? await searchLibraryInternetMetadata(episodes[0].relative_path, queryOverride.trim())
				: await fetchLibraryInternetMetadata(episodes[0].relative_path);
		} catch (err) {
			metadataError = err instanceof Error ? err.message : 'Metadata lookup failed';
		} finally {
			metadataLoading = false;
		}
	}

	async function chooseMatch(match: InternetMetadataMatch) {
		if (episodes.length === 0) return;
		saveLoading = true;
		saveStatus = '';
		saveWarnings = [];
		metadataError = '';
		try {
			const warnings: string[] = [];
			for (const episode of episodes) {
				const response = await saveSelectedLibraryInternetMetadata(episode.relative_path, match);
				if (response.metadata_sidecar_warning) {
					warnings.push(`${episode.file_name}: ${response.metadata_sidecar_warning}`);
				}
			}
			selectedMatch = match;
			saveWarnings = warnings;
			saveStatus = warnings.length > 0
				? `Saved show metadata across ${episodes.length} episode file${episodes.length === 1 ? '' : 's'}, but ${warnings.length} file${warnings.length === 1 ? '' : 's'} still need season and episode numbers in the filename before Jellyfin .nfo files can be written.`
				: `Saved show metadata across ${episodes.length} episode file${episodes.length === 1 ? '' : 's'} and updated Jellyfin .nfo files.`;
			await loadShow();
		} catch (err) {
			metadataError = err instanceof Error ? err.message : 'Failed to save selected metadata';
		} finally {
			saveLoading = false;
		}
	}

	async function runBatchOrganize(apply: boolean) {
		if (!selectedMatch || episodes.length === 0) return;
		batchLoading = true;
		batchError = '';
		batchStatus = '';
		previewResults = [];
		const nextResults: Array<{ item: LibraryEntry; result: OrganizeLibraryResult | null; error: string | null }> = [];

		for (const item of episodes) {
			try {
				const result = await organizeLibraryFile({
					path: item.relative_path,
					library_id: item.library_id ?? activeLibraryId ?? undefined,
					selected: selectedMatch,
					id_mode: idMode,
					write_nfo: writeNfo,
					apply
				});
				nextResults.push({ item, result, error: null });
			} catch (err) {
				nextResults.push({
					item,
					result: null,
					error: err instanceof Error ? err.message : 'Organize failed'
				});
			}
		}

		previewResults = nextResults;
		const failures = nextResults.filter((entry) => entry.error);
		if (failures.length > 0) {
			batchError = `${failures.length} episode file${failures.length === 1 ? '' : 's'} need manual follow-up.`;
		}
		const changed = nextResults.filter((entry) => entry.result?.changed).length;
		const nfoWritten = nextResults.filter((entry) => entry.result?.metadata_sidecar_written).length;
		batchStatus = apply
			? `Applied organization to ${nextResults.length - failures.length} episode file${nextResults.length - failures.length === 1 ? '' : 's'}${nfoWritten > 0 ? ` and wrote ${nfoWritten} .nfo file${nfoWritten === 1 ? '' : 's'}` : ''}.`
			: `Previewed ${nextResults.length} episode file${nextResults.length === 1 ? '' : 's'}${changed > 0 ? `, ${changed} would move` : ''}.`;

		if (apply) {
			await loadShow();
		}
		batchLoading = false;
	}

	onMount(async () => {
		libraries = await fetchLibraries().catch(() => []);
		activeLibraryId = page.url.searchParams.get('library');
		showName = page.url.searchParams.get('show') ?? '';
		requestedPath = page.url.searchParams.get('path');
		if (!showName && requestedPath) {
			const fromPath = requestedPath.split('/').filter(Boolean);
			showName = activeLibrary()
				? stripLibraryPrefix(requestedPath, activeLibrary()!.path).split('/').filter(Boolean)[0] ?? ''
				: fromPath[0] ?? '';
		}
		if (!activeLibraryId) {
			activeLibraryId = libraries.find((library) => library.media_type === 'tv')?.id ?? null;
		}
		if (!showName) {
			error = 'Show name is required.';
			loading = false;
			return;
		}
		await loadShow();
	});

		const seasonSummaries = $derived.by(() => {
			const groups = new Map<string, LibraryEntry[]>();
			for (const item of episodes) {
				const key = seasonKeyForItem(item);
				const existing = groups.get(key) ?? [];
				existing.push(item);
				groups.set(key, existing);
			}

			return Array.from(groups.entries())
				.map(([season, items]) => ({
					season,
					count: items.length,
					totalBytes: items.reduce((sum, item) => sum + item.size_bytes, 0),
					needsMetadata: items.filter((item) => !item.has_selected_metadata && (item.managed_status ?? 'UNPROCESSED') !== 'KEPT_ORIGINAL' && (item.managed_status ?? 'UNPROCESSED') !== 'PROCESSED').length,
					missingNfo: items.filter((item) => !item.has_sidecar).length,
					organizeNeeded: items.filter((item) => item.organize_needed).length,
					attentionCount: items.filter((item) => ['UNPROCESSED', 'FAILED', 'AWAITING_APPROVAL'].includes(item.managed_status ?? 'UNPROCESSED')).length,
					reSourceCount: items.filter((item) => (item.managed_status ?? 'UNPROCESSED') === 'RE_SOURCE').length,
					keptOriginalCount: items.filter((item) => (item.managed_status ?? 'UNPROCESSED') === 'KEPT_ORIGINAL').length,
					notedCount: items.filter((item) => !!item.review_note).length
				}))
				.sort((left, right) => left.season.localeCompare(right.season));
		});

		const outcomeSummary = $derived.by(() => {
			const notedEpisodes = [...episodes.filter((item) => !!item.review_note)].sort((left, right) => {
				const reviewDiff = (right.review_updated_at ?? 0) - (left.review_updated_at ?? 0);
				if (reviewDiff !== 0) return reviewDiff;

				const modifiedDiff = (right.modified_at ?? 0) - (left.modified_at ?? 0);
				if (modifiedDiff !== 0) return modifiedDiff;

				return left.file_name.localeCompare(right.file_name);
			});
			return {
				notedCount: notedEpisodes.length,
				reSourceCount: episodes.filter((item) => (item.managed_status ?? 'UNPROCESSED') === 'RE_SOURCE').length,
				keptOriginalCount: episodes.filter((item) => (item.managed_status ?? 'UNPROCESSED') === 'KEPT_ORIGINAL').length,
				recentNotes: notedEpisodes.slice(0, 4).map((item) => ({
					fileName: item.file_name,
					note: item.review_note ?? '',
					reviewedAt: item.review_updated_at
				}))
			};
		});
</script>

<section class="mb-6 flex flex-wrap items-start justify-between gap-4">
	<div>
		<p class="section-label">TV Show Workspace</p>
		<h2 class="mt-2 text-3xl text-[color:var(--ink-strong)]">{showName}</h2>
		<p class="mt-3 max-w-3xl text-sm leading-6 text-[color:var(--ink-muted)]">
			Use one show-level metadata choice, then preview or apply rename and organize across the season files. Drop to the per-episode organize screen when one file needs special handling.
		</p>
	</div>
	<div class="flex flex-wrap gap-2">
		<a href={activeLibraryId ? `/library?library=${encodeURIComponent(activeLibraryId)}&show=${encodeURIComponent(showName)}` : '/library'} class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] no-underline">Back to library</a>
		<a href="/organize" class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] no-underline">Generic organize</a>
	</div>
</section>

{#if error}
	<div class="rounded-[1rem] border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-3 text-sm text-[color:var(--danger)]">{error}</div>
{:else if loading}
	<div class="rounded-[1rem] border border-dashed border-[color:var(--line)] px-5 py-12 text-center text-sm text-[color:var(--ink-muted)]">Loading TV show…</div>
{:else}
	<section class="mb-5 grid gap-4 lg:grid-cols-[minmax(0,1.05fr)_minmax(22rem,0.95fr)]">
		<div class="surface-card p-5">
			<div class="mb-5">
				<p class="section-label mb-3">Season Summary</p>
				<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
					{#each seasonSummaries as season (season.season)}
						<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.6)] p-4">
							<div class="flex items-start justify-between gap-3">
								<div>
									<div class="font-semibold text-[color:var(--ink-strong)]">{season.season}</div>
									<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{season.count} episode file{season.count === 1 ? '' : 's'} · {formatBytes(season.totalBytes)}</div>
								</div>
								{#if season.attentionCount > 0}
									<span class="status-chip failed">{season.attentionCount} attention</span>
								{/if}
							</div>
							<div class="mt-3 flex flex-wrap gap-2">
								{#if season.needsMetadata > 0}
									<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">{season.needsMetadata} need metadata</span>
								{/if}
								{#if season.missingNfo > 0}
									<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">{season.missingNfo} missing nfo</span>
								{/if}
								{#if season.organizeNeeded > 0}
									<span class="rounded-full border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--accent-deep)]">{season.organizeNeeded} organize needed</span>
								{/if}
								{#if season.reSourceCount > 0}
									<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">{season.reSourceCount} re-source</span>
								{/if}
								{#if season.keptOriginalCount > 0}
									<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">{season.keptOriginalCount} kept original</span>
								{/if}
								{#if season.notedCount > 0}
									<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">{season.notedCount} noted</span>
								{/if}
								{#if season.needsMetadata === 0 && season.missingNfo === 0 && season.organizeNeeded === 0}
									<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">consistent</span>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			</div>

			{#if outcomeSummary.notedCount > 0}
				<div class="mb-5 rounded-[1rem] border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] p-4">
					<div class="flex flex-wrap items-start justify-between gap-3">
						<div>
							<p class="section-label">Review Outcomes</p>
							<p class="mt-1 text-sm text-[color:var(--ink-strong)]">{outcomeSummary.notedCount} episode file{outcomeSummary.notedCount === 1 ? '' : 's'} have an operator outcome note.</p>
						</div>
						<div class="flex flex-wrap gap-2 text-[10px] font-bold uppercase tracking-[0.1em]">
							{#if outcomeSummary.reSourceCount > 0}
								<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-white/50 px-2 py-0.5 text-[color:var(--olive)]">{outcomeSummary.reSourceCount} re-source</span>
							{/if}
							{#if outcomeSummary.keptOriginalCount > 0}
								<span class="rounded-full border border-[color:var(--line)] bg-white/50 px-2 py-0.5 text-[color:var(--ink-muted)]">{outcomeSummary.keptOriginalCount} kept original</span>
							{/if}
						</div>
					</div>
					<div class="mt-3 space-y-2">
						{#each outcomeSummary.recentNotes as item (`${item.fileName}-${item.note}`)}
							<div class="rounded-lg border border-[color:rgba(106,142,72,0.2)] bg-white/40 px-3 py-2 text-xs">
								<div class="font-semibold text-[color:var(--ink-strong)]">{item.fileName}</div>
								{#if item.reviewedAt}
									<div class="mt-1 text-[11px] text-[color:var(--ink-muted)]">Reviewed {formatTimestamp(item.reviewedAt)}</div>
								{/if}
								<div class="mt-1 text-[color:var(--olive)]">{item.note}</div>
							</div>
						{/each}
					</div>
				</div>
			{/if}

			<div class="mb-4 flex flex-wrap items-center justify-between gap-3">
				<div>
					<p class="section-label">Episode Files</p>
					<p class="text-lg text-[color:var(--ink-strong)]">{episodes.length} episode file{episodes.length === 1 ? '' : 's'}</p>
				</div>
				<div class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)]">
					{formatBytes(episodes.reduce((sum, item) => sum + item.size_bytes, 0))}
				</div>
			</div>
			<div class="space-y-2">
				{#each episodes as item (item.relative_path)}
					<div class="rounded-[0.875rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.6)] px-4 py-3">
						<div class="flex flex-wrap items-start justify-between gap-3">
							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-center gap-2">
									<div class="font-semibold text-[color:var(--ink-strong)]">{item.file_name}</div>
									<span class="status-chip {statusTone(item.managed_status ?? 'UNPROCESSED')}">{statusLabel(item.managed_status ?? 'UNPROCESSED')}</span>
									{#if item.has_selected_metadata}
										<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">metadata selected</span>
									{:else}
										<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">needs metadata</span>
									{/if}
									{#if item.has_sidecar}
										<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">nfo</span>
									{:else}
										<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">missing nfo</span>
									{/if}
									{#if item.organize_needed}
										<span class="rounded-full border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--accent-deep)]">organize needed</span>
									{/if}
								</div>
								<div class="mt-1 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</div>
								{#if item.review_note}
									<div class="mt-2 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">
										{#if item.review_updated_at}
											<div class="mb-1 text-[11px] text-[color:var(--ink-muted)]">Reviewed {formatTimestamp(item.review_updated_at)}</div>
										{/if}
										{item.review_note}
									</div>
								{/if}
							</div>
							<div class="text-right text-xs text-[color:var(--ink-muted)]">
								<div>{formatBytes(item.size_bytes)}</div>
								<div class="mt-1">{formatTimestamp(item.modified_at)}</div>
							</div>
						</div>
						<div class="mt-3 flex flex-wrap gap-2">
							<a href={`/organize?library=${encodeURIComponent(item.library_id ?? activeLibraryId ?? '')}&path=${encodeURIComponent(item.relative_path)}`} class="rounded-md border border-[color:var(--line)] px-2.5 py-1.5 text-[10px] font-semibold text-[color:var(--ink-strong)] no-underline">Open episode organize</a>
						</div>
					</div>
				{/each}
			</div>
		</div>

		<div class="space-y-4">
			<div class="surface-card p-5">
				<p class="section-label mb-3">Show Metadata</p>
				<div class="flex flex-wrap gap-2">
					<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => lookupMetadata(null)} disabled={metadataLoading}>
						{metadataLoading ? 'Looking up…' : 'Lookup Show Metadata'}
					</button>
				</div>
				<div class="mt-3 flex flex-wrap gap-2">
					<input class="min-w-[16rem] flex-1 rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" type="search" placeholder="Override show search query" bind:value={manualQuery} onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void lookupMetadata(manualQuery); } }} />
					<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => lookupMetadata(manualQuery)} disabled={metadataLoading || !manualQuery.trim()}>Search Override</button>
				</div>

				{#if selectedMatch}
					<div class="mt-3 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">
						Selected for show: <span class="font-semibold">{selectedMatch.title}{selectedMatch.year ? ` (${selectedMatch.year})` : ''}</span>
					</div>
				{/if}
				{#if saveStatus}
					<div class="mt-3 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">{saveStatus}</div>
				{/if}
				{#if saveWarnings.length > 0}
					<div class="mt-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">
						<div class="mb-1 font-semibold uppercase tracking-[0.12em]">NFO Warnings</div>
						{#each saveWarnings.slice(0, 6) as warning (warning)}
							<div>{warning}</div>
						{/each}
						{#if saveWarnings.length > 6}
							<div class="mt-1 text-[color:var(--ink-muted)]">+{saveWarnings.length - 6} more</div>
						{/if}
					</div>
				{/if}
				{#if metadataError}
					<div class="mt-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{metadataError}</div>
				{/if}

				{#if metadataResults}
					<div class="mt-3 space-y-2">
						{#each metadataResults.matches as match, index (`${match.provider}-${index}`)}
							<div class="rounded-lg bg-[color:rgba(244,236,223,0.7)] px-3 py-2">
								<div class="flex flex-wrap items-center gap-2">
									<span class="font-semibold text-[color:var(--ink-strong)]">{match.title}{match.year ? ` (${match.year})` : ''}</span>
									<span class="rounded-full bg-[color:rgba(214,180,111,0.2)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-strong)]">{match.provider}</span>
									{#if match.tvdb_id}<span class="font-mono text-[11px] text-[color:var(--ink-muted)]">tvdb:{match.tvdb_id}</span>{/if}
								</div>
								{#if match.overview}
									<p class="mt-1 text-xs text-[color:var(--ink-muted)]">{match.overview}</p>
								{/if}
								<div class="mt-2 flex flex-wrap items-center gap-2">
									<button class="rounded-md border border-[color:var(--line)] px-2 py-1 text-[10px] font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => chooseMatch(match)} disabled={saveLoading || matchesSelected(match)}>
										{matchesSelected(match) ? 'Selected' : saveLoading ? 'Saving…' : 'Use for show'}
									</button>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</div>

			<div class="surface-card p-5">
				<p class="section-label mb-3">Batch Organize</p>
				<div class="grid gap-3 rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-3">
					<label class="text-xs text-[color:var(--ink-muted)]">Name Suffix
						<select bind:value={idMode} class="mt-1 w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-2 py-1.5 text-sm text-[color:var(--ink-strong)]">
							<option value="none">No external ID</option>
							<option value="tvdb">Add TVDB ID</option>
						</select>
					</label>
					<label class="flex items-center gap-2 text-xs text-[color:var(--ink-muted)]">
						<input type="checkbox" bind:checked={writeNfo} class="accent-[color:var(--accent)]" />
						Write Jellyfin .nfo next to every episode file
					</label>
				</div>

				<div class="mt-3 flex flex-wrap gap-2">
					<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => runBatchOrganize(false)} disabled={!selectedMatch || batchLoading}>Preview Show Rename</button>
					<button class="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-white disabled:opacity-50" onclick={() => runBatchOrganize(true)} disabled={!selectedMatch || batchLoading}>{batchLoading ? 'Working…' : 'Apply To Show'}</button>
				</div>

				{#if batchError}
					<div class="mt-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{batchError}</div>
				{/if}
				{#if batchStatus}
					<div class="mt-3 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">{batchStatus}</div>
				{/if}

				{#if previewResults.length > 0}
					<div class="mt-3 space-y-2">
						{#each previewResults as entry (entry.item.relative_path)}
							<div class="rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.5)] px-3 py-2 text-xs">
								<div class="font-semibold text-[color:var(--ink-strong)]">{entry.item.file_name}</div>
								<div class="mt-1 font-mono text-[color:var(--ink-muted)]">{entry.item.relative_path}</div>
								{#if entry.result}
									<div class="mt-2 text-[color:var(--ink-muted)]">Target</div>
									<div class="font-mono text-[color:var(--ink-strong)]">{entry.result.target_relative_path}</div>
									{#if entry.result.metadata_sidecar_path}
										<div class="mt-1 font-mono text-[color:var(--ink-muted)]">{entry.result.metadata_sidecar_path}</div>
									{/if}
								{:else if entry.error}
									<div class="mt-2 text-[color:var(--danger)]">{entry.error}</div>
								{/if}
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</div>
	</section>
{/if}