<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onDestroy, onMount } from 'svelte';
	import {
		createBulkIntakeReviews,
		createIntakeReview,
		fetchBacklogItems,
		fetchLibrary,
		fetchLibraryEvents,
		updateBulkIntakeManagedStatus,
		updateIntakeManagedStatus,
		type BacklogFilter,
		type IntakeManagedItem,
		type LibraryChangeEvent,
		type LibraryRoots,
		type LibrarySummary
	} from '$lib/api';
	import {
		getExecutionState,
		jobStore,
		libraryState,
		managedItemStore,
		refreshManagedItemStore,
		getReviewState
	} from '$lib/stores.svelte';
	import { fileName, formatBytes, formatTimestamp, statusLabel, statusTone } from '$lib/status';

	const backlogFilters: BacklogFilter[] = [
		'all',
		'needs_attention',
		'unprocessed',
		'failed',
		'awaiting_approval',
		'approved',
		'reviewed',
		're_source',
		'missing_metadata',
		'missing_sidecar',
		'organize_needed'
	];

	function parseBacklogFilter(value: string | null): BacklogFilter {
		if (value && backlogFilters.includes(value as BacklogFilter)) {
			return value as BacklogFilter;
		}
		return 'needs_attention';
	}

	let recentChanges = $state<LibraryChangeEvent[]>([]);
	let backlogItems = $state<IntakeManagedItem[]>([]);
	let librarySummary = $state<LibrarySummary>({
		total_items: 0,
		total_bytes: 0,
		video_items: 0,
		audio_items: 0,
		other_items: 0
	});
	let roots = $state<LibraryRoots>({ library_path: '/data', ingest_path: '/ingest' });
	let localLoading = $state(true);
	let backlogError = $state('');
	let activeBacklogFilter = $state<BacklogFilter>(parseBacklogFilter(page.url.searchParams.get('filter')));
	let showOutsideLibrary = $state(page.url.searchParams.get('outside') === '1');
	let reviewBusy = $state<Record<string, boolean>>({});
	let statusBusy = $state<Record<string, boolean>>({});
	let refreshTimer: ReturnType<typeof setTimeout> | undefined;

	const reviewJobs = $derived(getReviewState().awaitingApproval);
	const reviewItemCount = $derived(getReviewState().counts.awaitingApprovalItems);
	const executionCounts = $derived(getExecutionState().counts);
	const failedJobs = $derived(getExecutionState().failed);

	onMount(async () => {
		await Promise.all([
			refreshManagedItemStore(),
			loadBacklog(activeBacklogFilter),
			loadSummary(),
			loadLibraryEvents()
		]);
		localLoading = false;
	});

	onDestroy(() => {
		if (refreshTimer) clearTimeout(refreshTimer);
	});

	async function loadBacklog(filter: BacklogFilter = activeBacklogFilter) {
		try {
			backlogItems = await fetchBacklogItems(filter, 200);
			backlogError = '';
		} catch (error) {
			backlogError = error instanceof Error ? error.message : 'Failed to load backlog';
			backlogItems = [];
		}
	}

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
			recentChanges = await fetchLibraryEvents(8);
		} catch {
			recentChanges = [];
		}
	}

	function scheduleRefresh() {
		if (refreshTimer) clearTimeout(refreshTimer);
		refreshTimer = setTimeout(() => {
			void Promise.all([
				refreshManagedItemStore(),
				loadBacklog(activeBacklogFilter),
				loadSummary(),
				loadLibraryEvents()
			]);
		}, 700);
	}

	function backlogHref(filter: BacklogFilter): string {
		const params = new URLSearchParams(page.url.searchParams);
		if (filter === 'needs_attention') {
			params.delete('filter');
		} else {
			params.set('filter', filter);
		}
		if (showOutsideLibrary) {
			params.set('outside', '1');
		} else {
			params.delete('outside');
		}
		const query = params.toString();
		return query ? `/?${query}` : '/';
	}

	async function syncBacklogFilter(filter: BacklogFilter) {
		await goto(backlogHref(filter), { replaceState: true, noScroll: true, keepFocus: true });
	}

	const _changeFlags = { skipFirst: true };
	$effect(() => {
		libraryState.changeCount;
		if (_changeFlags.skipFirst) {
			_changeFlags.skipFirst = false;
			return;
		}
		if (libraryState.latestChange) {
			recentChanges = [libraryState.latestChange, ...recentChanges].slice(0, 8);
		}
		scheduleRefresh();
	});

	$effect(() => {
		const filterFromUrl = parseBacklogFilter(page.url.searchParams.get('filter'));
		showOutsideLibrary = page.url.searchParams.get('outside') === '1';
		if (filterFromUrl === activeBacklogFilter) {
			return;
		}
		activeBacklogFilter = filterFromUrl;
		localLoading = true;
		void loadBacklog(filterFromUrl).finally(() => {
			localLoading = false;
		});
	});

	async function createReview(item: IntakeManagedItem) {
		reviewBusy = { ...reviewBusy, [item.relative_path]: true };
		backlogError = '';
		try {
			if (item.group_kind === 'tv_show') {
				const response = await createBulkIntakeReviews(item.member_paths);
				jobStore.jobs = [
					...response.jobs,
					...jobStore.jobs.filter(
						(existing) => !response.jobs.some((created) => created.id === existing.id)
					)
				];
				if (response.failure_count > 0) {
					const firstFailure = response.failures[0];
					backlogError = firstFailure
						? `Created ${response.success_count} review job(s); ${response.failure_count} failed. ${firstFailure.path}: ${firstFailure.error}`
						: `Created ${response.success_count} review job(s); ${response.failure_count} failed.`;
				}
			} else {
				const job = await createIntakeReview(item.relative_path);
				jobStore.jobs = [job, ...jobStore.jobs.filter((existing) => existing.id !== job.id)];
			}
			await Promise.all([refreshManagedItemStore(), loadBacklog(activeBacklogFilter)]);
		} catch (error) {
			backlogError = error instanceof Error ? error.message : 'Failed to create AI review';
		} finally {
			reviewBusy = { ...reviewBusy, [item.relative_path]: false };
		}
	}

	async function updateManagedStatus(item: IntakeManagedItem, status: 'REVIEWED' | 'KEPT_ORIGINAL') {
		statusBusy = { ...statusBusy, [item.relative_path]: true };
		backlogError = '';
		try {
			if (item.group_kind === 'tv_show') {
				const response = await updateBulkIntakeManagedStatus(item.member_paths, status);
				if (response.failure_count > 0) {
					const firstFailure = response.failures[0];
					backlogError = firstFailure
						? `Updated ${response.success_count} path(s); ${response.failure_count} failed. ${firstFailure.path}: ${firstFailure.error}`
						: `Updated ${response.success_count} path(s); ${response.failure_count} failed.`;
				}
			} else {
				await updateIntakeManagedStatus(item.relative_path, status);
			}
			await Promise.all([refreshManagedItemStore(), loadBacklog(activeBacklogFilter)]);
		} catch (error) {
			backlogError = error instanceof Error ? error.message : 'Failed to update managed status';
		} finally {
			statusBusy = { ...statusBusy, [item.relative_path]: false };
		}
	}

	function setBacklogFilter(filter: BacklogFilter) {
		if (filter === activeBacklogFilter) {
			return;
		}
		activeBacklogFilter = filter;
		localLoading = true;
		void Promise.all([loadBacklog(filter), syncBacklogFilter(filter)]).finally(() => {
			localLoading = false;
		});
	}

	function toggleOutsideLibrary() {
		showOutsideLibrary = !showOutsideLibrary;
		void syncBacklogFilter(activeBacklogFilter);
	}

	function backlogDisplayItems(items: IntakeManagedItem[]): IntakeManagedItem[] {
		if (showOutsideLibrary) {
			return items;
		}
		return items.filter((item) => !!item.library_id);
	}

	function showDetailHref(item: IntakeManagedItem): string {
		const params = new URLSearchParams();
		if (item.library_id) {
			params.set('library', item.library_id);
		}
		if (item.group_label) {
			params.set('show', item.group_label);
		}
		params.set('path', item.relative_path);
		if (!item.library_id || showOutsideLibrary) {
			params.set('outside', '1');
		}
		return `/library/show?${params.toString()}`;
	}

	function libraryHref(item: IntakeManagedItem): string {
		if (item.group_kind === 'tv_show') {
			return showDetailHref(item);
		}
		const params = new URLSearchParams();
		if (item.library_id) {
			params.set('library', item.library_id);
		}
		params.set('path', item.relative_path);
		if (!item.library_id || showOutsideLibrary) {
			params.set('outside', '1');
		}
		return `/library?${params.toString()}`;
	}

	function organizeHref(item: IntakeManagedItem): string {
		const libraryId = item.library_id;
		if (item.group_kind === 'tv_show') {
			return showDetailHref(item);
		}
		if (!libraryId) {
			return libraryHref(item);
		}
		const params = new URLSearchParams({ path: item.relative_path });
		params.set('library', libraryId);
		return `/organize?${params.toString()}`;
	}

	const backlogFilterMeta = $derived([
		{
			key: 'needs_attention' as const,
			label: 'Needs Attention',
			count: managedItemStore.summary.needs_attention_count,
			description: 'Open issues across status, metadata, or Jellyfin NFO files'
		},
		{
			key: 'unprocessed' as const,
			label: 'Unprocessed',
			count: managedItemStore.summary.unprocessed_count,
			description: 'No durable sharky-fish context yet'
		},
		{
			key: 'failed' as const,
			label: 'Failed',
			count: managedItemStore.summary.failed_count,
			description: 'Execution exceptions needing follow-up'
		},
		{
			key: 're_source' as const,
			label: 'Re-source',
			count: managedItemStore.summary.re_source_count,
			description: 'Items intentionally deferred for a better source instead of another transcode'
		},
		{
			key: 'missing_metadata' as const,
			label: 'Needs Metadata',
			count: managedItemStore.summary.missing_metadata_count,
			description: 'Items without selected internet metadata'
		},
		{
			key: 'missing_sidecar' as const,
			label: 'Missing NFO',
			count: managedItemStore.summary.missing_sidecar_count,
			description: 'No Jellyfin NFO alongside the media file'
		},
		{
			key: 'organize_needed' as const,
			label: 'Organize Needed',
			count: managedItemStore.summary.organize_needed_count,
			description: 'Metadata is selected, but placement still drifts from the canonical target'
		},
		{
			key: 'awaiting_approval' as const,
			label: 'Awaiting Approval',
			count: managedItemStore.summary.awaiting_approval_count,
			description: 'AI plans waiting for an operator decision'
		}
	]);

	const activeBacklogMeta = $derived(
		backlogFilterMeta.find((filter) => filter.key === activeBacklogFilter) ?? backlogFilterMeta[0]
	);
	const visibleBacklogItems = $derived(backlogDisplayItems(backlogItems));
</script>

<section class="mb-6 grid gap-4 md:grid-cols-[minmax(0,1.3fr)_minmax(18rem,0.7fr)]">
	<div class="surface-card p-6">
		<div class="flex flex-wrap items-start justify-between gap-4">
			<div>
				<p class="section-label">Backlog Workspace</p>
				<h2 class="mt-2 text-3xl text-[color:var(--ink-strong)]">What needs shaping now</h2>
				<p class="mt-3 max-w-2xl text-sm leading-6 text-[color:var(--ink-muted)]">
					Use this page to clear the unmanaged library backlog, send the right files to review, and keep execution as a downstream concern.
				</p>
			</div>
			<div class="grid min-w-[15rem] gap-3 sm:grid-cols-2">
				<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
					<div class="section-label">Library</div>
					<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{librarySummary.total_items}</div>
					<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{formatBytes(librarySummary.total_bytes)} indexed</div>
				</div>
				<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
					<div class="section-label">Needs Attention</div>
					<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{managedItemStore.summary.needs_attention_count}</div>
					<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{managedItemStore.summary.unprocessed_count} unprocessed · {managedItemStore.summary.failed_count} failed</div>
				</div>
			</div>
		</div>
		<div class="mt-5 flex flex-wrap gap-2">
			<a href="/intake" class="rounded-full bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white no-underline">Review {reviewItemCount} item{reviewItemCount === 1 ? '' : 's'}</a>
			<a href="/library" class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] no-underline">Audit library state</a>
			<a href="/forge" class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] no-underline">Open execution</a>
		</div>
	</div>

	<div class="surface-card p-6">
		<p class="section-label">Signals</p>
		<div class="mt-4 grid gap-3">
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="flex items-center justify-between gap-3">
					<div>
						<div class="text-sm font-semibold text-[color:var(--ink-strong)]">Awaiting Approval</div>
						<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Plans queued for operator decision</div>
					</div>
					<span class="status-chip processing">{reviewItemCount}</span>
				</div>
			</div>
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="flex items-center justify-between gap-3">
					<div>
						<div class="text-sm font-semibold text-[color:var(--ink-strong)]">Approved</div>
						<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Ready to enter execution</div>
					</div>
					<span class="status-chip processing">{executionCounts.approved}</span>
				</div>
			</div>
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="flex items-center justify-between gap-3">
					<div>
						<div class="text-sm font-semibold text-[color:var(--ink-strong)]">Processing</div>
						<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Currently active execution jobs</div>
					</div>
					<span class="status-chip processing">{executionCounts.processing}</span>
				</div>
			</div>
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="flex items-center justify-between gap-3">
					<div>
						<div class="text-sm font-semibold text-[color:var(--ink-strong)]">Failed</div>
						<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Execution exceptions that need follow-up</div>
					</div>
					<span class="status-chip failed">{executionCounts.failed}</span>
				</div>
			</div>
		</div>
	</div>
</section>

<section class="mb-4 grid gap-3 lg:grid-cols-3 xl:grid-cols-7">
	{#each backlogFilterMeta as filter (filter.key)}
		<button class="rounded-[1rem] border px-4 py-4 text-left transition-colors {activeBacklogFilter === filter.key ? 'border-[color:var(--accent)] bg-[color:rgba(164,79,45,0.08)]' : 'border-[color:var(--line)] bg-[color:var(--panel-strong)] hover:bg-[color:rgba(214,180,111,0.08)]'}" onclick={() => setBacklogFilter(filter.key)}>
			<div class="section-label">{filter.label}</div>
			<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{filter.count}</div>
			<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{filter.description}</div>
		</button>
	{/each}
</section>

<section class="mb-6 grid gap-5 xl:grid-cols-[minmax(0,1.35fr)_minmax(20rem,0.65fr)]">
	<div class="surface-card p-5">
		<div class="mb-4 flex items-center justify-between gap-3">
			<div>
				<p class="section-label">Needs Attention</p>
				<p class="text-lg text-[color:var(--ink-strong)]">{activeBacklogMeta.label}</p>
				<p class="mt-1 text-sm text-[color:var(--ink-muted)]">{activeBacklogMeta.description}</p>
			</div>
			<div class="flex items-center gap-2">
				<label class="flex items-center gap-2 rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.14em] text-[color:var(--ink-muted)]">
					<input type="checkbox" checked={showOutsideLibrary} onchange={toggleOutsideLibrary} class="accent-[color:var(--accent)]" />
					Show outside library
				</label>
				<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)]">{visibleBacklogItems.length}</span>
			</div>
		</div>

		{#if localLoading}
			<div class="rounded-[1rem] border border-dashed border-[color:var(--line)] px-5 py-10 text-center text-sm text-[color:var(--ink-muted)]">Loading backlog…</div>
		{:else if backlogError}
			<div class="rounded-[1rem] border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-3 text-sm text-[color:var(--danger)]">{backlogError}</div>
		{:else if visibleBacklogItems.length === 0}
			<div class="rounded-[1rem] border border-dashed border-[color:var(--line)] px-5 py-10 text-center text-sm text-[color:var(--ink-muted)]">
				No items match this backlog filter right now. Use <a href="/library" class="underline">Library</a> to audit shaped items and <a href="/intake" class="underline">Review</a> to approve new plans.
			</div>
		{:else}
			<div class="space-y-3">
				{#each visibleBacklogItems.slice(0, 8) as item (item.group_key ?? item.relative_path)}
					<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
						<div class="flex flex-wrap items-start justify-between gap-3">
							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-center gap-2">
									<span class="status-chip {statusTone(item.managed_status)}">{statusLabel(item.managed_status)}</span>
									<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">{item.group_kind === 'tv_show' ? 'tv show' : item.media_type}</span>
									{#if item.group_source === 'path'}
										<span class="rounded-full border border-[color:rgba(214,180,111,0.35)] bg-[color:rgba(214,180,111,0.12)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--accent-deep)]">path grouped</span>
									{/if}
									{#if item.library_id}
										<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">{item.library_id}</span>
									{:else}
										<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">outside library</span>
									{/if}
									{#if item.has_sidecar}
										<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">nfo</span>
									{:else}
										<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">missing nfo</span>
									{/if}
									{#if item.missing_metadata}
										<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">needs metadata</span>
									{:else if item.selected_metadata}
										<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">metadata selected</span>
									{/if}
									{#if item.organize_needed}
										<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">organize needed</span>
									{/if}
									{#if item.managed_status === 'REVIEWED'}
										<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">reviewed</span>
									{:else if item.managed_status === 'APPROVED'}
										<span class="rounded-full border border-[color:rgba(214,180,111,0.35)] bg-[color:rgba(214,180,111,0.12)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--accent-deep)]">approved</span>
									{:else if item.managed_status === 'KEPT_ORIGINAL'}
										<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">kept original</span>
									{/if}
									{#if item.group_kind === 'tv_show'}
										<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">{item.member_count} episode{item.member_count === 1 ? '' : 's'}</span>
									{/if}
								</div>
								<h3 class="mt-3 truncate text-base font-semibold text-[color:var(--ink-strong)]">{item.group_label ?? item.file_name}</h3>
								<p class="mt-1 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.group_kind === 'tv_show' ? item.group_source === 'path' ? 'Show-level backlog item inferred from path' : 'Show-level backlog item' : item.relative_path}</p>
								<p class="mt-3 text-sm text-[color:var(--ink-muted)]">
									{#if item.group_kind === 'tv_show' && item.group_source === 'path' && item.managed_status === 'UNPROCESSED'}
										This show was grouped from its path pattern because it is not matched to a TV library or selected metadata yet.
									{:else if item.group_kind === 'tv_show' && item.managed_status === 'UNPROCESSED'}
										No durable sharky-fish context exists for this show yet. Create one review bundle for the full show or mark the show as intentionally left alone.
									{:else if item.managed_status === 'UNPROCESSED'}
										No durable sharky-fish context exists for this file yet. Choose whether to review it with AI or mark it as intentionally left alone.
									{:else if item.managed_status === 'FAILED'}
										This {item.group_kind === 'tv_show' ? 'show' : 'item'} has a failed execution history. Inspect the plan and task pipeline before resubmitting work.
									{:else if item.managed_status === 'AWAITING_APPROVAL'}
										This {item.group_kind === 'tv_show' ? 'show already has AI plans' : 'item already has an AI plan'} and is waiting for an operator decision.
									{:else}
										This {item.group_kind === 'tv_show' ? 'show' : 'item'} still needs library shaping follow-up based on metadata or persistence state.
									{/if}
								</p>
								{#if item.review_note}
									<div class="mt-3 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">
										{#if item.review_updated_at}
											<div class="mb-1 text-[11px] text-[color:var(--ink-muted)]">Reviewed {formatTimestamp(item.review_updated_at)}</div>
										{/if}
										{item.review_note}
									</div>
								{/if}
							</div>
							<div class="min-w-[10rem] text-right text-xs text-[color:var(--ink-muted)]">
								<div>{formatBytes(item.size_bytes)}</div>
								<div class="mt-1">{formatTimestamp(item.modified_at)}</div>
							</div>
						</div>
						<div class="mt-4 flex flex-wrap gap-2">
							{#if item.managed_status !== 'AWAITING_APPROVAL' && item.managed_status !== 'APPROVED' && item.managed_status !== 'PROCESSED'}
								<button class="rounded-lg bg-[color:var(--accent)] px-3 py-2 text-xs font-semibold text-white disabled:opacity-60" onclick={() => createReview(item)} disabled={!!reviewBusy[item.relative_path]}>
									{reviewBusy[item.relative_path] ? 'Building AI review…' : 'Create AI review'}
								</button>
							{/if}
							{#if item.managed_status === 'UNPROCESSED' || item.managed_status === 'REVIEWED'}
								<button class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-60" onclick={() => updateManagedStatus(item, 'REVIEWED')} disabled={!!statusBusy[item.relative_path] || !!reviewBusy[item.relative_path]}>
									{statusBusy[item.relative_path] ? 'Saving…' : 'Mark reviewed'}
								</button>
								<button class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-60" onclick={() => updateManagedStatus(item, 'KEPT_ORIGINAL')} disabled={!!statusBusy[item.relative_path] || !!reviewBusy[item.relative_path]}>
									Keep original
								</button>
							{/if}
							{#if item.managed_status === 'AWAITING_APPROVAL'}
								<a href="/intake" class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] no-underline">Open review</a>
							{:else if item.managed_status === 'FAILED' || item.last_decision}
								<a href="/forge" class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] no-underline">Open execution</a>
							{/if}
							<a href={organizeHref(item)} class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] no-underline">{item.group_kind === 'tv_show' ? 'Open show' : 'Open organize'}</a>
							<a href={libraryHref(item)} class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] no-underline">Open in library</a>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</div>

	<div class="space-y-5">
		<div class="surface-card p-5">
			<div class="mb-4 flex items-center justify-between gap-3">
				<div>
					<p class="section-label">Review Queue</p>
					<p class="text-lg text-[color:var(--ink-strong)]">Awaiting operator approval</p>
				</div>
				<a href="/intake" class="text-sm font-semibold text-[color:var(--accent-deep)] no-underline hover:underline">Open review</a>
			</div>
			{#if reviewItemCount === 0}
				<p class="text-sm text-[color:var(--ink-muted)]">No AI plans are waiting for approval.</p>
			{:else}
				<div class="space-y-2">
					{#each reviewJobs.slice(0, 4) as job (job.id)}
						<div class="rounded-[0.875rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-3">
							<div class="flex items-center justify-between gap-3">
								<div class="min-w-0">
									<div class="truncate font-semibold text-[color:var(--ink-strong)]">{fileName(job.file_path)}</div>
									<div class="mt-1 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{job.file_path}</div>
								</div>
								<span class="status-chip processing">Awaiting Approval</span>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</div>

		<div class="surface-card p-5">
			<div class="mb-4 flex items-center justify-between gap-3">
				<div>
					<p class="section-label">Execution Alerts</p>
					<p class="text-lg text-[color:var(--ink-strong)]">Failures and recent library activity</p>
				</div>
				<a href="/forge" class="text-sm font-semibold text-[color:var(--accent-deep)] no-underline hover:underline">Open execution</a>
			</div>
			{#if failedJobs.length > 0}
				<div class="mb-4 space-y-2">
					{#each failedJobs.slice(0, 3) as job (job.id)}
						<div class="rounded-[0.875rem] border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-3">
							<div class="flex items-center justify-between gap-3">
								<div class="min-w-0">
									<div class="truncate font-semibold text-[color:var(--ink-strong)]">{fileName(job.file_path)}</div>
									<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Job #{job.id} failed on {new Date(job.created_at).toLocaleString()}</div>
								</div>
								<span class="status-chip failed">Failed</span>
							</div>
						</div>
					{/each}
				</div>
			{/if}
			<div class="space-y-2">
				{#if recentChanges.length === 0}
					<p class="text-sm text-[color:var(--ink-muted)]">No recent library change events.</p>
				{:else}
					{#each recentChanges as change (change.path + change.occurred_at)}
						<div class="rounded-[0.875rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-3">
							<div class="mb-1 flex items-center justify-between gap-3 text-xs uppercase tracking-[0.16em]">
								<span class="font-semibold text-[color:var(--accent-deep)]">{change.change}</span>
								<span class="text-[color:var(--ink-muted)]">{formatTimestamp(change.occurred_at)}</span>
							</div>
							<div class="truncate font-mono text-[12px] text-[color:var(--ink-strong)]">{change.relative_path}</div>
						</div>
					{/each}
				{/if}
			</div>
		</div>
	</div>
</section>

<section class="grid gap-5 sm:grid-cols-2">
	<div class="surface-card p-5">
		<div class="mb-1 font-semibold uppercase tracking-[0.18em] text-[color:var(--olive)] text-xs">Library Root</div>
		<div class="break-all rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-3 font-mono text-[12px] text-[color:var(--ink-strong)]">{roots.library_path}</div>
	</div>
	<div class="surface-card p-5">
		<div class="mb-1 font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)] text-xs">Ingest Root</div>
		<div class="break-all rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-3 font-mono text-[12px] text-[color:var(--ink-strong)]">{roots.ingest_path}</div>
	</div>
</section>
