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
		fetchLibraries,
		triggerLibraryRescan,
		updateBulkIntakeManagedStatus,
		updateIntakeManagedStatus,
		type BacklogFilter,
		type IntakeManagedItem,
		type LibraryChangeEvent,
		type LibraryFolder,
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
	let libraries = $state<LibraryFolder[]>([]);
	let librarySummary = $state<LibrarySummary>({
		total_items: 0,
		total_bytes: 0,
		video_items: 0,
		audio_items: 0,
		other_items: 0
	});
	let roots = $state<LibraryRoots>({ library_path: '/data/media', ingest_path: '/data/downloads' });
	let localLoading = $state(true);
	let backlogError = $state('');
	let activeBacklogFilter = $state<BacklogFilter>(parseBacklogFilter(page.url.searchParams.get('filter')));
	let showOutsideLibrary = $state(page.url.searchParams.get('outside') === '1');
	let reviewBusy = $state<Record<string, boolean>>({});
	let statusBusy = $state<Record<string, boolean>>({});
	let rescanLoading = $state(false);
	let rescanError = $state('');
	let shortcutBusy = $state<'review_batch' | null>(null);
	let shortcutStatus = $state('');
	let shortcutError = $state('');
	let refreshTimer: ReturnType<typeof setTimeout> | undefined;

	const reviewJobs = $derived(getReviewState().awaitingApproval);
	const reviewItemCount = $derived(getReviewState().counts.awaitingApprovalItems);
	const executionCounts = $derived(getExecutionState().counts);
	const failedJobs = $derived(getExecutionState().failed);
	const scanStatus = $derived(libraryState.scan);
	const hasLibraries = $derived(libraries.length > 0);
	const scanRunning = $derived(scanStatus.status === 'running');
	const showFirstRunChecklist = $derived(!hasLibraries || scanRunning || librarySummary.total_items === 0);
	const currentQueuePressure = $derived(
		reviewItemCount + executionCounts.approved + executionCounts.processing
	);
	const suggestedReviewBatchSize = $derived.by(() => {
		let size = 25;

		if (librarySummary.total_items >= 100000) {
			size = 200;
		} else if (librarySummary.total_items >= 50000) {
			size = 150;
		} else if (librarySummary.total_items >= 20000) {
			size = 100;
		} else if (librarySummary.total_items >= 5000) {
			size = 50;
		}

		if (currentQueuePressure >= 200) {
			size = Math.min(size, 10);
		} else if (currentQueuePressure >= 100) {
			size = Math.min(size, 20);
		} else if (currentQueuePressure >= 40) {
			size = Math.min(size, 35);
		} else if (currentQueuePressure >= 15) {
			size = Math.min(size, 50);
		}

		return Math.max(10, Math.min(size, 200));
	});

	onMount(async () => {
		await Promise.all([
			loadLibraries(),
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

	async function loadLibraries() {
		try {
			libraries = await fetchLibraries();
		} catch {
			libraries = [];
		}
	}

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

	async function runRescan() {
		rescanLoading = true;
		rescanError = '';
		shortcutError = '';
		try {
			await triggerLibraryRescan();
		} catch (error) {
			rescanError = error instanceof Error ? error.message : 'Failed to trigger rescan';
		} finally {
			rescanLoading = false;
		}
	}

	async function launchFirstReviewBatch() {
		shortcutBusy = 'review_batch';
		shortcutStatus = '';
		shortcutError = '';
		try {
			const fetchLimit = Math.min(Math.max(suggestedReviewBatchSize * 3, 200), 500);
			const unprocessed = await fetchBacklogItems('unprocessed', fetchLimit);
			const paths = Array.from(
				new Set(
					unprocessed
						.flatMap((item) => item.group_kind === 'tv_show' ? item.member_paths : [item.relative_path])
						.filter((path) => path.trim().length > 0)
				)
			).slice(0, suggestedReviewBatchSize);

			if (paths.length === 0) {
				shortcutStatus = 'No unprocessed items are available for an initial review batch.';
				return;
			}

			const response = await createBulkIntakeReviews(paths);
			if (response.jobs.length > 0) {
				const existingIds = new Set(response.jobs.map((job) => job.id));
				jobStore.jobs = [...response.jobs, ...jobStore.jobs.filter((job) => !existingIds.has(job.id))];
			}

			await Promise.all([
				refreshManagedItemStore(),
				loadBacklog(activeBacklogFilter)
			]);

			shortcutStatus = response.failure_count === 0
				? `Created ${response.success_count} review job(s) using the suggested batch size of ${suggestedReviewBatchSize} and opened Review.`
				: `Created ${response.success_count} review job(s) from the suggested batch size of ${suggestedReviewBatchSize}; ${response.failure_count} need follow-up. Opening Review.`;

			await goto('/intake');
		} catch (error) {
			shortcutError = error instanceof Error ? error.message : 'Failed to create the first review batch';
		} finally {
			shortcutBusy = null;
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
	const prioritizedQueues = $derived.by(() => {
		const candidates = [
			{
				label: 'Needs Metadata',
				count: managedItemStore.summary.missing_metadata_count,
				detail: 'Bulk-select matches before organizing or reviewing.',
				href: '/library?view=missing_metadata&bulk=1&select=page'
			},
			{
				label: 'Organize Needed',
				count: managedItemStore.summary.organize_needed_count,
				detail: 'Metadata is selected, but target paths still drift.',
				href: '/library?view=organize_needed'
			},
			{
				label: 'Unprocessed',
				count: managedItemStore.summary.unprocessed_count,
				detail: 'No durable sharky-fish decisions exist yet.',
				href: '/?filter=unprocessed'
			},
			{
				label: 'Awaiting Approval',
				count: managedItemStore.summary.awaiting_approval_count,
				detail: 'AI plans are waiting for a human decision.',
				href: '/intake'
			},
			{
				label: 'Missing NFO',
				count: managedItemStore.summary.missing_sidecar_count,
				detail: 'Sidecars are missing alongside otherwise-managed files.',
				href: '/library?view=missing_sidecar'
			},
			{
				label: 'Failed',
				count: managedItemStore.summary.failed_count,
				detail: 'Execution exceptions need follow-up before more queueing.',
				href: '/forge'
			}
		];

		return candidates.filter((item) => item.count > 0).sort((left, right) => right.count - left.count).slice(0, 3);
	});
	const guidanceFocus = $derived.by(() => {
		if (!hasLibraries) {
			return {
				title: 'Register libraries first',
				detail: 'Create your managed Movies and TV libraries under /data/media before the first scan.',
				href: '/settings',
				cta: 'Open settings'
			};
		}

		if (scanRunning) {
			const progressTotal = scanStatus.total_items > 0 ? ` of ${scanStatus.total_items}` : '';
			return {
				title: 'Initial indexing is in progress',
				detail: `Scanning ${scanStatus.scanned_items}${progressTotal} items. Let the first pass finish before doing deep backlog cleanup.`,
				href: '/library',
				cta: 'Open library'
			};
		}

		if (librarySummary.total_items === 0) {
			return {
				title: 'Run the first full scan',
				detail: 'Sharky Fish can only prioritize a huge library after it indexes the files under /data/media.',
				action: 'rescan' as const,
				cta: 'Start first scan'
			};
		}

		const topQueue = prioritizedQueues[0];
		if (topQueue) {
			return {
				title: `Work the ${topQueue.label.toLowerCase()} queue`,
				detail: `${topQueue.count} items currently need this kind of attention. ${topQueue.detail}`,
				href: topQueue.href,
				cta: `Open ${topQueue.label}`
			};
		}

		if (executionCounts.approved + executionCounts.processing > 0) {
			return {
				title: 'Execution is already moving',
				detail: `${executionCounts.approved} approved and ${executionCounts.processing} active jobs are downstream from backlog shaping.`,
				href: '/forge',
				cta: 'Open execution'
			};
		}

		return {
			title: 'Library is in a stable state',
			detail: 'No major backlog categories are spiking right now. Use Library for spot checks or Downloads for ingress cleanup.',
			href: '/library',
			cta: 'Open library'
		};
	});
	const workflowSteps = $derived([
		{
			title: 'Map libraries in /data/media',
			complete: hasLibraries,
			detail: hasLibraries ? `${libraries.length} librar${libraries.length === 1 ? 'y is' : 'ies are'} configured.` : 'Create Movies and TV libraries before scanning.'
		},
		{
			title: 'Run the first library scan',
			complete: librarySummary.total_items > 0,
			detail: scanRunning ? `Scanning ${scanStatus.scanned_items} items now.` : librarySummary.total_items > 0 ? `${librarySummary.total_items} items indexed so far.` : 'Build the initial index so queues can be prioritized automatically.'
		},
		{
			title: 'Work the biggest shaping queue',
			complete: managedItemStore.summary.needs_attention_count === 0,
			detail: managedItemStore.summary.needs_attention_count === 0 ? 'No shaping backlog is currently spiking.' : `${managedItemStore.summary.needs_attention_count} items still need backlog shaping.`
		},
		{
			title: 'Approve and monitor execution',
			complete: reviewItemCount === 0 && executionCounts.approved + executionCounts.processing === 0,
			detail: reviewItemCount > 0 ? `${reviewItemCount} items are waiting in Review.` : executionCounts.approved + executionCounts.processing > 0 ? `${executionCounts.approved + executionCounts.processing} jobs are downstream in execution.` : 'Review and execution are both clear.'
		}
	]);
	const shortcutActions = $derived.by(() => {
		const actions: Array<{
			title: string;
			detail: string;
			href?: string;
			action?: 'review_batch';
			cta: string;
		}> = [];

		if (managedItemStore.summary.missing_metadata_count > 0) {
			actions.push({
				title: 'Bulk metadata selection',
				detail: 'Open Library already filtered to missing metadata, with the current page preselected for bulk actions.',
				href: '/library?view=missing_metadata&bulk=1&select=page',
				cta: 'Open bulk metadata'
			});
		}

		if (managedItemStore.summary.unprocessed_count > 0) {
			actions.push({
				title: 'Launch the first review batch',
				detail: `Create a suggested batch of about ${suggestedReviewBatchSize} review jobs from the unprocessed queue. The batch grows with library size and shrinks when Review or Execution is already busy.`,
				action: 'review_batch',
				cta: `Start ${suggestedReviewBatchSize}-item batch`
			});
		}

		if (managedItemStore.summary.organize_needed_count > 0) {
			actions.push({
				title: 'Open organize-needed queue',
				detail: 'Jump into the library items that already have metadata but still need canonical placement.',
				href: '/library?view=organize_needed',
				cta: 'Open organize queue'
			});
		}

		return actions.slice(0, 3);
	});
</script>

{#if showFirstRunChecklist}
	<section class="sticky top-4 z-10 mb-6 rounded-[1rem] border border-[color:var(--accent)]/25 bg-[linear-gradient(135deg,rgba(164,79,45,0.12),rgba(214,180,111,0.08))] p-5 shadow-[0_12px_40px_rgba(69,53,32,0.08)] backdrop-blur">
		<div class="flex flex-wrap items-start justify-between gap-4">
			<div>
				<p class="section-label">First-Run Checklist</p>
				<h2 class="mt-1 text-xl text-[color:var(--ink-strong)]">Complete setup before you start shaping a huge library</h2>
				<p class="mt-2 max-w-3xl text-sm leading-6 text-[color:var(--ink-muted)]">This checklist stays pinned until libraries are configured and the first scan has finished, so operators always have a clear next move on a fresh instance.</p>
			</div>
			<div class="flex flex-wrap gap-2">
				<a href="/settings" class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] no-underline">Configure libraries</a>
				<button onclick={runRescan} disabled={rescanLoading || scanRunning || !hasLibraries} class="rounded-full bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-50">
					{rescanLoading ? 'Starting scan…' : scanRunning ? 'Scan running…' : 'Run first scan'}
				</button>
			</div>
		</div>

		<div class="mt-4 grid gap-3 lg:grid-cols-4">
			{#each workflowSteps as step, index (step.title)}
				<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)] p-4">
					<div class="flex items-start justify-between gap-3">
						<div>
							<div class="text-xs font-semibold uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">Step {index + 1}</div>
							<div class="mt-2 text-sm font-semibold text-[color:var(--ink-strong)]">{step.title}</div>
						</div>
						<span class={`status-chip ${step.complete ? 'completed' : 'processing'}`}>{step.complete ? 'Done' : 'Pending'}</span>
					</div>
					<p class="mt-2 text-sm text-[color:var(--ink-muted)]">{step.detail}</p>
				</div>
			{/each}
		</div>

		{#if scanRunning}
			<div class="mt-4 h-2.5 overflow-hidden rounded-full bg-[color:rgba(123,105,81,0.16)]">
				<div class="h-full rounded-full bg-[linear-gradient(90deg,var(--accent),var(--accent-soft),var(--olive))] transition-all duration-300" style={`width: ${scanStatus.total_items > 0 ? Math.min(100, (scanStatus.scanned_items / scanStatus.total_items) * 100) : 10}%`}></div>
			</div>
		{/if}
		{#if rescanError}
			<div class="mt-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-sm text-[color:var(--danger)]">{rescanError}</div>
		{/if}
	</section>
{/if}

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

	<section class="mb-6 grid gap-5 xl:grid-cols-[minmax(0,1.15fr)_minmax(20rem,0.85fr)]">
		<div class="surface-card p-6">
			<div class="mb-4 flex flex-wrap items-start justify-between gap-3">
				<div>
					<p class="section-label">Guided Workflow</p>
					<h3 class="mt-1 text-xl text-[color:var(--ink-strong)]">Make the first pass predictable</h3>
					<p class="mt-2 max-w-2xl text-sm leading-6 text-[color:var(--ink-muted)]">For a huge library, the fastest path is: register libraries, run one full scan, work the biggest queues first, then let Review and Execution stay downstream.</p>
				</div>
				{#if 'action' in guidanceFocus && guidanceFocus.action === 'rescan'}
					<button onclick={runRescan} disabled={rescanLoading || scanRunning} class="rounded-full bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-50">
						{rescanLoading ? 'Starting scan…' : scanRunning ? 'Scan running…' : guidanceFocus.cta}
					</button>
				{:else}
					<a href={guidanceFocus.href} class="rounded-full bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white no-underline">{guidanceFocus.cta}</a>
				{/if}
			</div>

			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] p-4">
				<div class="text-sm font-semibold text-[color:var(--ink-strong)]">{guidanceFocus.title}</div>
				<p class="mt-2 text-sm leading-6 text-[color:var(--ink-muted)]">{guidanceFocus.detail}</p>
				{#if scanRunning}
					<div class="mt-3 h-2.5 overflow-hidden rounded-full bg-[color:var(--paper-deep)]">
						<div class="h-full rounded-full bg-[linear-gradient(90deg,var(--accent),var(--accent-soft),var(--olive))] transition-all duration-300" style={`width: ${scanStatus.total_items > 0 ? Math.min(100, (scanStatus.scanned_items / scanStatus.total_items) * 100) : 10}%`}></div>
					</div>
				{/if}
				{#if rescanError}
					<div class="mt-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-sm text-[color:var(--danger)]">{rescanError}</div>
				{/if}
			</div>

			<div class="mt-4 grid gap-3 md:grid-cols-2">
				{#each workflowSteps as step, index (step.title)}
					<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
						<div class="flex items-start justify-between gap-3">
							<div>
								<div class="text-xs font-semibold uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">Step {index + 1}</div>
								<div class="mt-2 text-sm font-semibold text-[color:var(--ink-strong)]">{step.title}</div>
							</div>
							<span class={`status-chip ${step.complete ? 'completed' : 'processing'}`}>{step.complete ? 'Ready' : 'Next'}</span>
						</div>
						<p class="mt-2 text-sm text-[color:var(--ink-muted)]">{step.detail}</p>
					</div>
				{/each}
			</div>
		</div>

		<div class="surface-card p-6">
			<div class="mb-4 flex items-center justify-between gap-3">
				<div>
					<p class="section-label">Recommended Shortcuts</p>
					<p class="text-lg text-[color:var(--ink-strong)]">Do the next useful bulk action</p>
				</div>
			</div>
			{#if shortcutError}
				<div class="mb-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-sm text-[color:var(--danger)]">{shortcutError}</div>
			{/if}
			{#if shortcutStatus}
				<div class="mb-3 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-sm text-[color:var(--olive)]">{shortcutStatus}</div>
			{/if}
			{#if shortcutActions.length === 0}
				<p class="text-sm text-[color:var(--ink-muted)]">No immediate bulk shortcuts are needed right now. Work from the queue cards below as conditions change.</p>
			{:else}
				<div class="space-y-3">
					{#each shortcutActions as shortcut (shortcut.title)}
						<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
							<div class="flex flex-wrap items-start justify-between gap-3">
								<div>
									<div class="text-sm font-semibold text-[color:var(--ink-strong)]">{shortcut.title}</div>
									<div class="mt-1 text-xs leading-5 text-[color:var(--ink-muted)]">{shortcut.detail}</div>
								</div>
								{#if shortcut.action === 'review_batch'}
									<button onclick={launchFirstReviewBatch} disabled={shortcutBusy === 'review_batch'} class="rounded-full bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-50">
										{shortcutBusy === 'review_batch' ? 'Creating batch…' : shortcut.cta}
									</button>
								{:else if shortcut.href}
									<a href={shortcut.href} class="rounded-full bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white no-underline">{shortcut.cta}</a>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</div>

		<div class="surface-card p-6">
			<div class="mb-4 flex items-center justify-between gap-3">
				<div>
					<p class="section-label">Auto Prioritized</p>
					<p class="text-lg text-[color:var(--ink-strong)]">Start with the biggest queues</p>
				</div>
				<a href="/library" class="text-sm font-semibold text-[color:var(--accent-deep)] no-underline hover:underline">Open library</a>
			</div>
			{#if prioritizedQueues.length === 0}
				<p class="text-sm text-[color:var(--ink-muted)]">No shaping queues are currently dominating the library. Use Review, Execution, or Downloads for spot checks.</p>
			{:else}
				<div class="space-y-3">
					{#each prioritizedQueues as queue (queue.label)}
						<a href={queue.href} class="block rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4 no-underline transition-colors hover:bg-[color:rgba(214,180,111,0.08)]">
							<div class="flex items-center justify-between gap-3">
								<div>
									<div class="text-sm font-semibold text-[color:var(--ink-strong)]">{queue.label}</div>
									<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{queue.detail}</div>
								</div>
								<div class="text-right">
									<div class="text-2xl font-semibold text-[color:var(--ink-strong)]">{queue.count}</div>
									<div class="text-[11px] uppercase tracking-[0.14em] text-[color:var(--ink-muted)]">items</div>
								</div>
							</div>
						</a>
					{/each}
				</div>
			{/if}
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
