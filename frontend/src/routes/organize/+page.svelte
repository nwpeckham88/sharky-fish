<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import {
		fetchLibrary,
		fetchLibraries,
		fetchLibraryInternetMetadata,
		fetchSelectedLibraryInternetMetadata,
		saveSelectedLibraryInternetMetadata,
		organizeLibraryFile,
		type LibraryEntry,
		type LibraryFolder,
		type InternetMetadataMatch,
		type InternetMetadataResponse,
		type OrganizeLibraryResult
	} from '$lib/api';

	let libraries = $state<LibraryFolder[]>([]);
	let activeLibraryId = $state<string | null>(null);
	let items = $state<LibraryEntry[]>([]);
	let loading = $state(false);
	let selected = $state<LibraryEntry | null>(null);
	let metadataResults = $state<InternetMetadataResponse | null>(null);
	let metadataLoading = $state(false);
	let metadataError = $state('');
	let chosenMatch = $state<InternetMetadataMatch | null>(null);
	let season = $state<number | null>(null);
	let episode = $state<number | null>(null);
	let idMode = $state<'none' | 'imdb' | 'tvdb'>('none');
	let writeNfo = $state(true);
	let previewResult = $state<OrganizeLibraryResult | null>(null);
	let actionLoading = $state(false);
	let actionError = $state('');
	let status = $state('');
	let requestedPath = $state<string | null>(null);

	onMount(async () => {
		libraries = await fetchLibraries().catch(() => []);
		const urlLibrary = page.url.searchParams.get('library');
		requestedPath = page.url.searchParams.get('path');
		if (libraries.length > 0) {
			activeLibraryId = libraries.find((library) => library.id === urlLibrary)?.id ?? libraries[0].id;
			await loadLibraryItems();
		}
	});

	async function loadLibraryItems() {
		if (!activeLibraryId) return;
		loading = true;
		try {
			const response = await fetchLibrary('', 400, 0, activeLibraryId);
			items = response.items;
			if (requestedPath) {
				const match = response.items.find((item) => item.relative_path === requestedPath) ?? null;
				if (match) {
					selectItem(match);
					await loadSavedSelection(match.relative_path);
				}
				requestedPath = null;
			}
		} finally {
			loading = false;
		}
	}

	function selectItem(item: LibraryEntry) {
		selected = item;
		metadataResults = null;
		metadataError = '';
		chosenMatch = null;
		previewResult = null;
		actionError = '';
		status = '';
		season = null;
		episode = null;
		idMode = item.library_id === activeLibraryId && activeLibrary()?.media_type === 'tv' ? 'tvdb' : 'none';
		writeNfo = true;
	}

	async function loadSavedSelection(path: string) {
		actionError = '';
		try {
			const selected = await fetchSelectedLibraryInternetMetadata(path);
			chosenMatch = selected?.selected ?? null;
			if (selected?.selected) {
				status = 'Loaded saved metadata selection.';
			}
		} catch (error) {
			actionError = error instanceof Error ? error.message : 'Failed to load saved selection';
		}
	}

	async function lookupMetadata() {
		if (!selected) return;
		metadataLoading = true;
		metadataError = '';
		try {
			metadataResults = await fetchLibraryInternetMetadata(selected.relative_path);
		} catch (error) {
			metadataError = error instanceof Error ? error.message : 'Failed metadata lookup';
		} finally {
			metadataLoading = false;
		}
	}

	async function chooseMatch(match: InternetMetadataMatch) {
		if (!selected) return;
		actionLoading = true;
		actionError = '';
		try {
			await saveSelectedLibraryInternetMetadata(selected.relative_path, match);
			chosenMatch = match;
			status = 'Selected metadata saved.';
		} catch (error) {
			actionError = error instanceof Error ? error.message : 'Failed to save match';
		} finally {
			actionLoading = false;
		}
	}

	async function previewRename() {
		if (!selected || !activeLibraryId || !chosenMatch) return;
		actionLoading = true;
		actionError = '';
		try {
			previewResult = await organizeLibraryFile({
				path: selected.relative_path,
				library_id: activeLibraryId,
				selected: chosenMatch,
				season: season ?? undefined,
				episode: episode ?? undefined,
				id_mode: idMode,
				write_nfo: writeNfo,
				apply: false
			});
		} catch (error) {
			actionError = error instanceof Error ? error.message : 'Preview failed';
		} finally {
			actionLoading = false;
		}
	}

	async function applyRename() {
		if (!selected || !activeLibraryId || !chosenMatch) return;
		actionLoading = true;
		actionError = '';
		try {
			const result = await organizeLibraryFile({
				path: selected.relative_path,
				library_id: activeLibraryId,
				selected: chosenMatch,
				season: season ?? undefined,
				episode: episode ?? undefined,
				id_mode: idMode,
				write_nfo: writeNfo,
				apply: true
			});
			previewResult = result;
			status = result.changed ? 'File renamed and organized.' : 'File already follows naming conventions.';
			if (result.metadata_sidecar_written) {
				status += ' Jellyfin .nfo sidecar written.';
			}
			await loadLibraryItems();
			if (selected) {
				selected = items.find((i) => i.relative_path === result.target_relative_path) ?? null;
			}
		} catch (error) {
			actionError = error instanceof Error ? error.message : 'Apply failed';
		} finally {
			actionLoading = false;
		}
	}

	function activeLibrary(): LibraryFolder | null {
		return libraries.find((l) => l.id === activeLibraryId) ?? null;
	}
</script>

<section class="mb-5">
	<p class="text-sm text-[color:var(--ink-muted)]">
		Convert messy downloaded names into Jellyfin-friendly movie and TV naming. Workflow: pick a file, lookup metadata, choose match, preview rename, then apply.
	</p>
</section>

<section class="mb-5 flex flex-wrap items-center gap-3">
	<label for="organize-library" class="text-xs font-semibold uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">Library</label>
	<select
		id="organize-library"
		class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
		bind:value={activeLibraryId}
		onchange={() => loadLibraryItems()}
	>
		{#each libraries as lib (lib.id)}
			<option value={lib.id}>{lib.name} ({lib.media_type})</option>
		{/each}
	</select>
</section>

<section class="grid gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(22rem,0.9fr)]">
	<div class="overflow-hidden rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)]">
		<div class="border-b border-[color:var(--line)] px-4 py-3 text-sm font-semibold text-[color:var(--ink-strong)]">Library Files</div>
		<div class="max-h-[34rem] overflow-y-auto">
			{#if loading}
				<div class="px-4 py-8 text-sm text-[color:var(--ink-muted)]">Loading…</div>
			{:else if items.length === 0}
				<div class="px-4 py-8 text-sm text-[color:var(--ink-muted)]">No files found.</div>
			{:else}
				{#each items as item (item.relative_path)}
					<button class="block w-full border-b border-[color:rgba(123,105,81,0.14)] px-4 py-3 text-left hover:bg-[color:rgba(214,180,111,0.08)] {selected?.relative_path === item.relative_path ? 'bg-[color:rgba(214,180,111,0.12)]' : ''}" onclick={() => selectItem(item)}>
						<div class="font-semibold text-[color:var(--ink-strong)]">{item.file_name}</div>
						<div class="mt-1 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</div>
					</button>
				{/each}
			{/if}
		</div>
	</div>

	<div class="space-y-4">
		<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-5">
			<p class="section-label mb-3">Organization Workflow</p>
			{#if !selected}
				<div class="rounded-lg border border-dashed border-[color:var(--line)] px-4 py-6 text-sm text-[color:var(--ink-muted)]">Select a file to begin.</div>
			{:else}
				<div class="space-y-3">
					<div>
						<div class="font-semibold text-[color:var(--ink-strong)]">{selected.file_name}</div>
						<div class="mt-1 break-all font-mono text-[11px] text-[color:var(--ink-muted)]">{selected.relative_path}</div>
					</div>

					<div class="flex flex-wrap gap-2">
						<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={lookupMetadata} disabled={metadataLoading}>
							{metadataLoading ? 'Looking up…' : 'Lookup Metadata'}
						</button>
					</div>

					{#if metadataError}
						<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{metadataError}</div>
					{/if}

					{#if metadataResults}
						{#if metadataResults.provider_used}
							<div class="text-[11px] text-[color:var(--ink-muted)]">Searched: <span class="font-semibold uppercase tracking-[0.08em] text-[color:var(--ink-strong)]">{metadataResults.provider_used}</span></div>
						{/if}
						<div class="space-y-2">
							{#if metadataResults.matches.length === 0}
								<div class="text-xs text-[color:var(--ink-muted)]">No matches found.</div>
							{:else}
								{#each metadataResults.matches as match, idx (`${match.provider}-${idx}`)}
									<div class="rounded-lg bg-[color:rgba(244,236,223,0.7)] px-3 py-2">
										<div class="font-semibold text-[color:var(--ink-strong)]">{match.title}{match.year ? ` (${match.year})` : ''}</div>
										<div class="mt-1 flex items-center gap-2">
											<span class="text-[11px] uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">{match.provider}</span>
											<button class="rounded-md border border-[color:var(--line)] px-2 py-1 text-[10px] font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => chooseMatch(match)} disabled={actionLoading}>
												{chosenMatch && chosenMatch.title === match.title && chosenMatch.provider === match.provider ? 'Selected' : 'Use'}
											</button>
										</div>
									</div>
								{/each}
							{/if}
						</div>
					{/if}

					{#if activeLibrary()?.media_type === 'tv'}
						<div class="grid gap-2 sm:grid-cols-2">
							<label class="text-xs text-[color:var(--ink-muted)]">Season
								<input type="number" min="1" bind:value={season} class="mt-1 w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-2 py-1.5 text-sm text-[color:var(--ink-strong)]" />
							</label>
							<label class="text-xs text-[color:var(--ink-muted)]">Episode
								<input type="number" min="1" bind:value={episode} class="mt-1 w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-2 py-1.5 text-sm text-[color:var(--ink-strong)]" />
							</label>
						</div>
					{/if}

						<div class="grid gap-3 rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-3 sm:grid-cols-2">
							<label class="text-xs text-[color:var(--ink-muted)]">Name Suffix
								<select bind:value={idMode} class="mt-1 w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-2 py-1.5 text-sm text-[color:var(--ink-strong)]">
									<option value="none">No external ID</option>
									{#if activeLibrary()?.media_type === 'tv'}
										<option value="tvdb">Add TVDB ID</option>
									{:else}
										<option value="imdb">Add IMDb ID</option>
									{/if}
								</select>
							</label>
							<label class="flex items-center gap-2 text-xs text-[color:var(--ink-muted)]">
								<input type="checkbox" bind:checked={writeNfo} class="accent-[color:var(--accent)]" />
								Write Jellyfin .nfo next to media
							</label>
						</div>

					<div class="flex flex-wrap gap-2">
						<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={previewRename} disabled={!chosenMatch || actionLoading}>Preview Rename</button>
						<button class="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-white disabled:opacity-50" onclick={applyRename} disabled={!chosenMatch || actionLoading}>Apply Rename</button>
					</div>

					{#if previewResult}
						<div class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs">
							<div class="text-[color:var(--ink-muted)]">Current</div>
							<div class="font-mono text-[color:var(--ink-strong)]">{previewResult.current_relative_path}</div>
							<div class="mt-2 text-[color:var(--ink-muted)]">Target (Jellyfin naming)</div>
							<div class="font-mono text-[color:var(--ink-strong)]">{previewResult.target_relative_path}</div>
							{#if previewResult.metadata_sidecar_path}
								<div class="mt-2 text-[color:var(--ink-muted)]">Metadata Sidecar</div>
								<div class="font-mono text-[color:var(--ink-strong)]">{previewResult.metadata_sidecar_path}</div>
							{/if}
						</div>
					{/if}

					{#if actionError}
						<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{actionError}</div>
					{/if}
					{#if status}
						<div class="rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">{status}</div>
					{/if}
				</div>
			{/if}
		</div>
	</div>
</section>
