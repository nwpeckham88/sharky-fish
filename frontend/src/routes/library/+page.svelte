<script lang="ts">
	import { goto } from '$app/navigation';
	import { onDestroy, onMount } from 'svelte';
	import { page } from '$app/state';
	import {
		autoSelectLibraryInternetMetadataBulk,
		createBulkIntakeReviews,
		createIntakeReview,
		fetchConfig,
		fetchLibrary,
		fetchLibraryArtwork,
		saveConfig,
		triggerLibraryRescan,
		fetchLibraryMetadata,
		fetchLibraryInternetMetadata,
		searchLibraryInternetMetadata,
		fetchLibraryInternetMetadataBulk,
		saveSelectedLibraryInternetMetadata,
		fetchSelectedLibraryInternetMetadata,
		fetchRelatedLibraryInternetMetadataPaths,
		organizeLibraryFile,
		fetchLibraryEvents,
		fetchLibraries,
		updateBulkIntakeManagedStatus,
		updateIntakeManagedStatus,
		buildLibraryArtworkUrl,
		type LibrarySortBy,
		type LibrarySortDirection,
		type BacklogFilter,
		type LibraryEntry,
		type LibraryFolder,
		type LibraryArtwork,
		type LibraryMetadata,
		type InternetMetadataMatch,
		type InternetMetadataResponse,
		type LibraryResponse,
		type LibraryRoots,
		type LibrarySummary,
		type LibraryScanStatus,
		type LibraryChangeEvent,
		type LibraryMediaFilter,
		type LibraryManagedStatusFilter,
		type LibraryViewMode
	} from '$lib/api';
	import { jobStore, libraryState, managedItemStore, refreshManagedItemStore } from '$lib/stores.svelte';
	import { formatBytes, formatTimestamp, statusLabel, statusTone } from '$lib/status';

	type LibraryManagedFilter = LibraryManagedStatusFilter;
	type LibraryShapingView =
		| 'all'
		| 'unprocessed'
		| 'awaiting_approval'
		| 'failed'
		| 're_source'
		| 'missing_metadata'
		| 'organize_needed'
		| 'missing_sidecar'
		| 'processed';

	const mediaFilters: LibraryMediaFilter[] = ['all', 'video', 'audio', 'subtitle', 'other'];
	const pageSizeOptions = [40, 100, 200, 500];
	const sortOptions: Array<{ value: LibrarySortBy; label: string }> = [
		{ value: 'modified_at', label: 'Recently Modified' },
		{ value: 'size_bytes', label: 'File Size' },
		{ value: 'file_name', label: 'File Name' },
		{ value: 'relative_path', label: 'Library Path' },
		{ value: 'media_type', label: 'Media Type' },
		{ value: 'managed_status', label: 'Managed Status' }
	];
	const managedFilters: LibraryManagedFilter[] = [
		'all',
		'UNPROCESSED',
		'REVIEWED',
		'RE_SOURCE',
		'AWAITING_APPROVAL',
		'APPROVED',
		'PROCESSED',
		'FAILED',
		'KEPT_ORIGINAL',
		'MISSING_METADATA',
		'ORGANIZE_NEEDED',
		'NO_SIDECAR'
	];
	const shapingViews: LibraryShapingView[] = [
		'all',
		'unprocessed',
		'awaiting_approval',
		'failed',
		're_source',
		'missing_metadata',
		'organize_needed',
		'missing_sidecar',
		'processed'
	];

	const shapingViewFilters: Record<LibraryShapingView, { typeFilter: LibraryMediaFilter; managedStatusFilter: LibraryManagedFilter }> = {
		all: { typeFilter: 'all', managedStatusFilter: 'all' },
		unprocessed: { typeFilter: 'all', managedStatusFilter: 'UNPROCESSED' },
		awaiting_approval: { typeFilter: 'all', managedStatusFilter: 'AWAITING_APPROVAL' },
		failed: { typeFilter: 'all', managedStatusFilter: 'FAILED' },
		re_source: { typeFilter: 'all', managedStatusFilter: 'RE_SOURCE' },
		missing_metadata: { typeFilter: 'all', managedStatusFilter: 'MISSING_METADATA' },
		organize_needed: { typeFilter: 'all', managedStatusFilter: 'ORGANIZE_NEEDED' },
		missing_sidecar: { typeFilter: 'all', managedStatusFilter: 'NO_SIDECAR' },
		processed: { typeFilter: 'all', managedStatusFilter: 'PROCESSED' }
	};

	function parseMediaFilter(value: string | null): LibraryMediaFilter {
		if (value && mediaFilters.includes(value as LibraryMediaFilter)) {
			return value as LibraryMediaFilter;
		}
		return 'all';
	}

	function parseManagedFilter(value: string | null): LibraryManagedFilter {
		if (value && managedFilters.includes(value as LibraryManagedFilter)) {
			return value as LibraryManagedFilter;
		}
		return 'all';
	}

	function parseShapingView(value: string | null): LibraryShapingView | null {
		if (value && shapingViews.includes(value as LibraryShapingView)) {
			return value as LibraryShapingView;
		}
		return null;
	}

	function backlogFilterHref(filter: BacklogFilter): string {
		return filter === 'needs_attention' ? '/' : `/?filter=${filter}`;
	}

	function parsePageSize(value: string | null): number {
		const parsed = Number(value ?? '40');
		return pageSizeOptions.includes(parsed) ? parsed : 40;
	}

	function parseSortBy(value: string | null): LibrarySortBy {
		return sortOptions.some((option) => option.value === value)
			? (value as LibrarySortBy)
			: 'modified_at';
	}

	function parseSortDirection(value: string | null): LibrarySortDirection {
		return value === 'asc' ? 'asc' : 'desc';
	}

	function reviewOutcomePriority(item: LibraryEntry): number {
		switch (item.managed_status ?? 'UNPROCESSED') {
			case 'RE_SOURCE':
				return 5;
			case 'KEPT_ORIGINAL':
				return 4;
			case 'FAILED':
				return 3;
			case 'AWAITING_APPROVAL':
				return 2;
			case 'REVIEWED':
				return 1;
			default:
				return 0;
		}
	}

	function preferredOutcomeNote(items: LibraryEntry[]): LibraryEntry | null {
		const notedItems = items.filter((item) => !!item.review_note);
		if (notedItems.length === 0) return null;

		return [...notedItems].sort((left, right) => {
			const priorityDiff = reviewOutcomePriority(right) - reviewOutcomePriority(left);
			if (priorityDiff !== 0) return priorityDiff;

			const reviewDiff = (right.review_updated_at ?? 0) - (left.review_updated_at ?? 0);
			if (reviewDiff !== 0) return reviewDiff;

			const modifiedDiff = (right.modified_at ?? 0) - (left.modified_at ?? 0);
			if (modifiedDiff !== 0) return modifiedDiff;

			return left.file_name.localeCompare(right.file_name);
		})[0];
	}

	type ExpandedItemState = {
		loading: boolean;
		loaded: boolean;
		error: string;
		selectedMatch: InternetMetadataMatch | null;
		relatedPaths: string[];
		artwork: LibraryArtwork | null;
	};

	let library = $state<LibraryEntry[]>([]);
	let librarySummary = $state<LibrarySummary>({
		total_items: 0, total_bytes: 0, video_items: 0, audio_items: 0, other_items: 0
	});
	let roots = $state<LibraryRoots>({ library_path: '/data/media', ingest_path: '/data/downloads' });
	let selectedItem = $state<LibraryEntry | null>(null);
	let selectedMetadata = $state<LibraryMetadata | null>(null);
	let metadataLoading = $state(false);
	let metadataError = $state('');
	let internetMetadata = $state<InternetMetadataResponse | null>(null);
	let internetMetadataLoading = $state(false);
	let internetMetadataError = $state('');
	let manualMetadataQuery = $state('');
	let selectedInternetMatch = $state<InternetMetadataMatch | null>(null);
	let relatedPaths = $state<string[]>([]);
	let internetSaveLoading = $state(false);
	let internetSaveError = $state('');
	let internetSaveWarning = $state('');
	let organizePreview = $state<import('$lib/api').OrganizeLibraryResult | null>(null);
	let organizeLoading = $state(false);
	let organizeError = $state('');
	let organizeStatus = $state('');
	let bulkInternetLoading = $state(false);
	let bulkInternetStatus = $state('');
	let bulkActionLoading = $state(false);
	let bulkActionStatus = $state('');
	let bulkActionFailedPaths = $state<string[]>([]);
	let bulkInternetFailedPaths = $state<string[]>([]);
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
	let pageSize = $state(40);
	let sortBy = $state<LibrarySortBy>('modified_at');
	let sortDirection = $state<LibrarySortDirection>('desc');
	let advancedMode = $state(false);
	let libraryViewMode = $state<LibraryViewMode>('compact');
	let libraryViewModeSaving = $state(false);
	let libraryViewModeError = $state('');
	let requestedPath = $state<string | null>(null);
	let requestedShow = $state<string | null>(null);
	let requestedBulkMode = $state(false);
	let requestedSelectPage = $state(false);
	let queryTimer: ReturnType<typeof setTimeout> | undefined;
	let refreshTimer: ReturnType<typeof setTimeout> | undefined;

	// Library folder tabs
	let libraryFolders = $state<LibraryFolder[]>([]);
	let activeLibraryId = $state<string | null>(null);

	// Filter state
	let typeFilter = $state<LibraryMediaFilter>('all');
	let managedStatusFilter = $state<LibraryManagedFilter>('all');
	let rowActionBusy = $state<Record<string, string>>({});
	let rowActionError = $state('');

	// Bulk selection
	let selectedPaths = $state<Set<string>>(new Set());
	let bulkMode = $state(false);
	let expandedItems = $state<Set<string>>(new Set());
	let expandedItemDetails = $state<Record<string, ExpandedItemState>>({});
	let expandedShows = $state<Set<string>>(new Set());

	const shapingViewMeta = $derived([
		{
			key: 'unprocessed' as const,
			label: 'Unprocessed',
			count: managedItemStore.summary.unprocessed_count,
			description: 'No durable sharky-fish context yet'
		},
		{
			key: 'awaiting_approval' as const,
			label: 'Awaiting Approval',
			count: managedItemStore.summary.awaiting_approval_count,
			description: 'AI plans waiting for an operator decision'
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
			description: 'Items deferred until a better source is available'
		},
		{
			key: 'missing_metadata' as const,
			label: 'Needs Metadata',
			count: managedItemStore.summary.missing_metadata_count,
			description: 'Items without selected internet metadata'
		},
		{
			key: 'organize_needed' as const,
			label: 'Organize Needed',
			count: managedItemStore.summary.organize_needed_count,
			description: 'Metadata is selected, but placement still drifts from the canonical target'
		},
		{
			key: 'missing_sidecar' as const,
			label: 'Missing NFO',
			count: managedItemStore.summary.missing_sidecar_count,
			description: 'No Jellyfin NFO alongside the media file'
		},
		{
			key: 'processed' as const,
			label: 'Processed',
			count: managedItemStore.summary.processed_count,
			description: 'Shaped items that are already complete'
		}
	]);

	const activeShapingView = $derived.by(() => {
		if (typeFilter !== 'all') {
			return null;
		}
		for (const [key, filters] of Object.entries(shapingViewFilters)) {
			if (filters.typeFilter === typeFilter && filters.managedStatusFilter === managedStatusFilter) {
				return key as Exclude<LibraryShapingView, 'missing_metadata'>;
			}
		}
		return null;
	});

	onMount(async () => {
		const urlQuery = page.url.searchParams.get('q');
		if (urlQuery) query = urlQuery;
		const urlLib = page.url.searchParams.get('library');
		if (urlLib) activeLibraryId = urlLib;
		requestedPath = page.url.searchParams.get('path');
		requestedShow = page.url.searchParams.get('show');
		const urlOffset = Number(page.url.searchParams.get('offset') ?? '0');
		if (Number.isFinite(urlOffset) && urlOffset > 0) offset = urlOffset;
		const urlView = parseShapingView(page.url.searchParams.get('view'));
		const urlType = parseMediaFilter(page.url.searchParams.get('type'));
		const urlStatus = parseManagedFilter(page.url.searchParams.get('status'));
		pageSize = parsePageSize(page.url.searchParams.get('page_size'));
		sortBy = parseSortBy(page.url.searchParams.get('sort'));
		sortDirection = parseSortDirection(page.url.searchParams.get('dir'));
		advancedMode = page.url.searchParams.get('advanced') === '1';
		requestedBulkMode = page.url.searchParams.get('bulk') === '1';
		requestedSelectPage = page.url.searchParams.get('select') === 'page';
		if (urlView) {
			typeFilter = shapingViewFilters[urlView].typeFilter;
			managedStatusFilter = shapingViewFilters[urlView].managedStatusFilter;
		} else {
			typeFilter = urlType;
			managedStatusFilter = urlStatus;
		}
		try {
				const [config, libraries] = await Promise.all([fetchConfig(), fetchLibraries()]);
				libraryViewMode = config.library_view_mode ?? 'compact';
				libraryFolders = libraries;
		} catch { libraryFolders = []; }
		await Promise.all([refreshManagedItemStore(), loadLibrary(), loadLibraryEvents()]);
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

	$effect(() => {
		if (libraryViewMode !== 'expanded') {
			return;
		}

		for (const item of library) {
			const detail = expandedItemDetails[item.relative_path];
			if (!detail || (!detail.loading && !detail.loaded)) {
				void loadExpandedItemData(item);
			}
		}
	});

	async function loadLibrary() {
		libraryLoading = true;
		try {
			const response: LibraryResponse = await fetchLibrary({
				query,
				limit: pageSize,
				offset,
				libraryId: activeLibraryId ?? undefined,
				mediaType: typeFilter,
				managedStatus: managedStatusFilter,
				sortBy,
				sortDirection
			});
			library = response.items;
			if (requestedBulkMode) {
				bulkMode = true;
				if (requestedSelectPage) {
					selectedPaths = new Set(response.items.map((item) => item.relative_path));
				}
				requestedBulkMode = false;
				requestedSelectPage = false;
			}
			librarySummary = response.summary;
			roots = response.roots;
			scanStatus = response.scan;
			libraryState.scan = response.scan;
			totalLibrary = response.total_items;
			if (requestedShow && activeLibraryId) {
				expandedShows = new Set([...expandedShows, requestedShow]);
			}
			if (requestedPath) {
				const match = response.items.find((item) => item.relative_path === requestedPath) ?? null;
				if (match) {
					requestedShow = activeLibraryFolder?.media_type === 'tv'
						? stripLibraryPrefix(match.relative_path, activeLibraryFolder.path).split('/').filter(Boolean)[0] ?? requestedShow
						: requestedShow;
					if (requestedShow) {
						expandedShows = new Set([...expandedShows, requestedShow]);
					}
					void loadMetadata(match);
				}
				requestedPath = null;
			}
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
		manualMetadataQuery = '';
		selectedInternetMatch = null;
		relatedPaths = [];
		internetSaveError = '';
		internetSaveWarning = '';
		organizePreview = null;
		organizeError = '';
		organizeStatus = '';
		bulkActionFailedPaths = [];
		bulkInternetFailedPaths = [];
		metadataLoading = true;
		try {
			const [metadata, selected] = await Promise.all([
				fetchLibraryMetadata(item.relative_path),
				fetchSelectedLibraryInternetMetadata(item.relative_path).catch(() => null)
			]);
			selectedMetadata = metadata;
			selectedInternetMatch = selected?.selected ?? null;
			if (selected?.selected) {
				const related = await fetchRelatedLibraryInternetMetadataPaths(item.relative_path).catch(() => ({ paths: [] }));
				relatedPaths = related.paths;
			}
		} catch (error) {
			metadataError = error instanceof Error ? error.message : 'Metadata fetch failed';
		} finally {
			metadataLoading = false;
		}
	}

	async function loadInternetMetadata(item: LibraryEntry) {
		await runInternetLookup(item, null);
	}

	async function runInternetLookup(item: LibraryEntry, queryOverride: string | null) {
		internetMetadataLoading = true;
		internetMetadataError = '';
		internetSaveError = '';
		internetSaveWarning = '';
		organizePreview = null;
		organizeError = '';
		internetMetadata = null;
		try {
			internetMetadata = queryOverride && queryOverride.trim()
				? await searchLibraryInternetMetadata(item.relative_path, queryOverride.trim())
				: await fetchLibraryInternetMetadata(item.relative_path);
		} catch (error) {
			internetMetadataError = error instanceof Error ? error.message : 'Internet metadata lookup failed';
		} finally {
			internetMetadataLoading = false;
		}
	}

	async function runManualInternetLookup() {
		if (!selectedItem) return;
		await runInternetLookup(selectedItem, manualMetadataQuery);
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
		organizePreview = null;
		organizeError = '';
		organizeStatus = '';
		try {
			const saved = await saveSelectedLibraryInternetMetadata(selectedItem.relative_path, match);
			selectedInternetMatch = saved.selected;
			const related = await fetchRelatedLibraryInternetMetadataPaths(selectedItem.relative_path).catch(() => ({ paths: [] }));
			relatedPaths = related.paths;
			internetSaveWarning = saved.metadata_sidecar_warning ?? '';
			organizeStatus = saved.metadata_sidecar_warning
				? 'Selected metadata saved.'
				: saved.metadata_sidecar_written
					? 'Selected metadata saved and Jellyfin .nfo updated.'
					: 'Selected metadata saved.';
			void loadExpandedItemData(selectedItem, true);
		} catch (error) {
			internetSaveError = error instanceof Error ? error.message : 'Failed to save selected match';
		} finally {
			internetSaveLoading = false;
		}
	}

	async function previewCanonicalRename() {
		if (!selectedItem || !selectedInternetMatch) return;
		organizeLoading = true;
		organizeError = '';
		organizeStatus = '';
		try {
			organizePreview = await organizeLibraryFile({
				path: selectedItem.relative_path,
				library_id: selectedItem.library_id ?? undefined,
				selected: selectedInternetMatch,
				scope: 'movie_folder',
				apply: false
			});
		} catch (error) {
			organizeError = error instanceof Error ? error.message : 'Preview failed';
		} finally {
			organizeLoading = false;
		}
	}

	async function applyCanonicalRename(mergeExisting = false) {
		if (!selectedItem || !selectedInternetMatch) return;
		organizeLoading = true;
		organizeError = '';
		organizeStatus = '';
		try {
			const result = await organizeLibraryFile({
				path: selectedItem.relative_path,
				library_id: selectedItem.library_id ?? undefined,
				selected: selectedInternetMatch,
				scope: 'movie_folder',
				merge_existing: mergeExisting,
				apply: true
			});
			organizePreview = result;
			organizeStatus = result.changed ? 'Library item renamed to the canonical target.' : 'Library item already matches the canonical target.';
			await loadLibrary();
			const updated = library.find((item) => item.relative_path === result.target_relative_path) ?? null;
			if (updated) {
				await loadMetadata(updated);
			}
		} catch (error) {
			organizeError = error instanceof Error ? error.message : 'Apply failed';
		} finally {
			organizeLoading = false;
		}
	}

	async function runBulkInternetLookup(autoSelectTop = false) {
		if (selectedPaths.size === 0) return;
		bulkInternetLoading = true;
		bulkInternetStatus = '';
		bulkInternetFailedPaths = [];
		bulkActionStatus = '';
		internetSaveError = '';
		try {
			const paths = Array.from(selectedPaths);
			if (autoSelectTop) {
				const response = await autoSelectLibraryInternetMetadataBulk(paths);
				bulkInternetFailedPaths = response.failures.map((failure) => failure.path);
				bulkInternetStatus = response.failure_count === 0
					? `Auto-selected metadata for ${response.success_count} file(s).`
					: `Auto-selected metadata for ${response.success_count} file(s), ${response.failure_count} need follow-up.`;
				await Promise.all([loadLibrary(), refreshManagedItemStore()]);
				if (selectedItem) {
					const refreshed = await fetchSelectedLibraryInternetMetadata(selectedItem.relative_path).catch(() => null);
					selectedInternetMatch = refreshed?.selected ?? selectedInternetMatch;
				}
			} else {
				const response = await fetchLibraryInternetMetadataBulk(paths);
				const withMatches = response.items.filter((item) => item.result.matches.length > 0);
				const withWarnings = response.items.filter((item) => item.result.warnings.length > 0);
				const withoutMatches = response.items
					.filter((item) => item.result.matches.length === 0)
					.map((item) => item.path);

				bulkInternetFailedPaths = withoutMatches;
				bulkInternetStatus = `Looked up ${response.items.length} file(s), found matches for ${withMatches.length}, warnings on ${withWarnings.length}.`;
			}
		} catch (error) {
			bulkInternetStatus = error instanceof Error ? error.message : 'Bulk internet lookup failed';
		} finally {
			bulkInternetLoading = false;
		}
	}

	async function runBulkManagedStatus(status: 'REVIEWED' | 'KEPT_ORIGINAL') {
		if (selectedPaths.size === 0) return;
		bulkActionLoading = true;
		bulkActionStatus = '';
		bulkActionFailedPaths = [];
		rowActionError = '';
		const paths = Array.from(selectedPaths);
		try {
			const response = await updateBulkIntakeManagedStatus(paths, status);
			const failedPaths = response.failures.map((failure) => failure.path);
			bulkActionFailedPaths = failedPaths;
			selectedPaths = new Set(failedPaths);
			bulkActionStatus = response.failure_count === 0
				? `${response.success_count} item(s) marked ${status === 'REVIEWED' ? 'reviewed' : 'kept original'}.`
				: `${response.success_count} item(s) updated, ${response.failure_count} failed. Failed items remain selected.`;
			await Promise.all([loadLibrary(), refreshManagedItemStore()]);
		} catch (error) {
			bulkActionStatus = error instanceof Error ? error.message : 'Bulk status update failed';
		} finally {
			bulkActionLoading = false;
		}
	}

	async function runBulkCreateReview() {
		if (selectedPaths.size === 0) return;
		bulkActionLoading = true;
		bulkActionStatus = '';
		bulkActionFailedPaths = [];
		rowActionError = '';
		const paths = Array.from(selectedPaths);
		try {
			const response = await createBulkIntakeReviews(paths);
			const createdJobs = response.jobs;
			const failedPaths = response.failures.map((failure) => failure.path);
			bulkActionFailedPaths = failedPaths;
			selectedPaths = new Set(failedPaths);
			if (createdJobs.length > 0) {
				const existingIds = new Set(createdJobs.map((job) => job.id));
				jobStore.jobs = [...createdJobs, ...jobStore.jobs.filter((job) => !existingIds.has(job.id))];
			}
			bulkActionStatus = response.failure_count === 0
				? `${response.success_count} AI review job(s) created.`
				: `${response.success_count} AI review job(s) created, ${response.failure_count} failed. Failed items remain selected.`;
			await Promise.all([loadLibrary(), refreshManagedItemStore()]);
		} catch (error) {
			bulkActionStatus = error instanceof Error ? error.message : 'Bulk review creation failed';
		} finally {
			bulkActionLoading = false;
		}
	}

	async function createReview(item: LibraryEntry) {
		rowActionBusy = { ...rowActionBusy, [item.relative_path]: 'review' };
		rowActionError = '';
		try {
			const job = await createIntakeReview(item.relative_path);
			jobStore.jobs = [job, ...jobStore.jobs.filter((existing) => existing.id !== job.id)];
			await Promise.all([loadLibrary(), refreshManagedItemStore()]);
		} catch (error) {
			rowActionError = error instanceof Error ? error.message : 'Failed to create AI review';
		} finally {
			rowActionBusy = { ...rowActionBusy, [item.relative_path]: '' };
		}
	}

	async function updateManagedStatus(item: LibraryEntry, status: 'REVIEWED' | 'KEPT_ORIGINAL') {
		rowActionBusy = { ...rowActionBusy, [item.relative_path]: status };
		rowActionError = '';
		try {
			await updateIntakeManagedStatus(item.relative_path, status);
			await Promise.all([loadLibrary(), refreshManagedItemStore()]);
		} catch (error) {
			rowActionError = error instanceof Error ? error.message : 'Failed to update managed status';
		} finally {
			rowActionBusy = { ...rowActionBusy, [item.relative_path]: '' };
		}
	}

	function scheduleLibraryLoad(resetOffset = false) {
		if (resetOffset) offset = 0;
		if (queryTimer) clearTimeout(queryTimer);
		queryTimer = setTimeout(() => {
			void syncLibraryUrl();
			void loadLibrary();
		}, 220);
	}

	function scheduleLibraryRefresh() {
		if (refreshTimer) clearTimeout(refreshTimer);
		refreshTimer = setTimeout(() => { void Promise.all([loadLibrary(), loadLibraryEvents()]); }, 700);
	}

	function handleSearchInput(event: Event) {
		query = (event.currentTarget as HTMLInputElement).value;
		scheduleLibraryLoad(true);
	}

	function formatDuration(value: number | null | undefined): string {
		if (!value) return 'Unknown';
		const t = Math.round(value);
		const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
		if (h > 0) return `${h}h ${m}m ${s}s`;
		if (m > 0) return `${m}m ${s}s`;
		return `${s}s`;
	}

	function hardLinkSummary(linkCount: number): string {
		return `${linkCount} hard links share this inode`;
	}

	function metadataSelectionNeeded(item: LibraryEntry): boolean {
		const status = item.managed_status ?? 'UNPROCESSED';
		return !item.has_selected_metadata && status !== 'KEPT_ORIGINAL' && status !== 'PROCESSED';
	}

	function detailState(path: string): ExpandedItemState | null {
		return expandedItemDetails[path] ?? null;
	}

	function itemIsExpanded(path: string): boolean {
		return libraryViewMode === 'expanded' || expandedItems.has(path);
	}

	function itemArtworkSrc(detail: ExpandedItemState | null): string | null {
		if (detail?.artwork?.poster_path) {
			return buildLibraryArtworkUrl(detail.artwork.poster_path);
		}
		if (detail?.artwork?.backdrop_path) {
			return buildLibraryArtworkUrl(detail.artwork.backdrop_path);
		}
		return detail?.selectedMatch?.poster_url ?? null;
	}

	async function loadExpandedItemData(item: LibraryEntry, force = false) {
		const existing = expandedItemDetails[item.relative_path];
		if (existing?.loading || (existing?.loaded && !force)) {
			return;
		}

		expandedItemDetails = {
			...expandedItemDetails,
			[item.relative_path]: {
				loading: true,
				loaded: false,
				error: '',
				selectedMatch: existing?.selectedMatch ?? null,
				relatedPaths: existing?.relatedPaths ?? [],
				artwork: existing?.artwork ?? null
			}
		};

		try {
			const [selected, artwork] = await Promise.all([
				fetchSelectedLibraryInternetMetadata(item.relative_path).catch(() => null),
				fetchLibraryArtwork(item.relative_path, item.library_id).catch(() => ({ poster_path: null, backdrop_path: null }))
			]);
			const selectedMatch = selected?.selected ?? null;
			const relatedPaths = selectedMatch
				? (await fetchRelatedLibraryInternetMetadataPaths(item.relative_path).catch(() => ({ paths: [] }))).paths
				: [];

			expandedItemDetails = {
				...expandedItemDetails,
				[item.relative_path]: {
					loading: false,
					loaded: true,
					error: '',
					selectedMatch,
					relatedPaths,
					artwork
				}
			};
		} catch (error) {
			expandedItemDetails = {
				...expandedItemDetails,
				[item.relative_path]: {
					loading: false,
					loaded: true,
					error: error instanceof Error ? error.message : 'Failed to load inline details',
					selectedMatch: existing?.selectedMatch ?? null,
					relatedPaths: existing?.relatedPaths ?? [],
					artwork: existing?.artwork ?? null
				}
			};
		}
	}

	function toggleItemExpansion(item: LibraryEntry) {
		if (libraryViewMode === 'expanded') {
			return;
		}

		const next = new Set(expandedItems);
		if (next.has(item.relative_path)) {
			next.delete(item.relative_path);
		} else {
			next.add(item.relative_path);
			void loadExpandedItemData(item);
		}
		expandedItems = next;
	}

	function pageRangeLabel(): string {
		if (totalLibrary === 0) return '0-0';
		return `${offset + 1}-${Math.min(offset + pageSize, totalLibrary)}`;
	}

	function nextPage() {
		if (offset + pageSize >= totalLibrary) return;
		offset += pageSize;
		void syncLibraryUrl();
		void loadLibrary();
	}

	function previousPage() {
		if (offset === 0) return;
		offset = Math.max(0, offset - pageSize);
		void syncLibraryUrl();
		void loadLibrary();
	}

	const filteredLibrary = $derived(library);

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
		1 +
		(!activeLibraryId && libraryFolders.length > 0 ? 1 : 0) +
		1 +
		1 +
		1
	);

	function switchLibrary(id: string | null) {
		activeLibraryId = id;
		offset = 0;
		selectedItem = null;
		selectedMetadata = null;
		selectedPaths = new Set();
		expandedItems = new Set();
		expandedShows = new Set();
		void syncLibraryUrl();
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
		bulkActionFailedPaths = [];
		bulkInternetFailedPaths = [];
	}

	async function reloadLibrary(resetOffset = false) {
		if (resetOffset) offset = 0;
		selectedPaths = new Set();
		expandedItems = new Set();
		await syncLibraryUrl();
		await loadLibrary();
	}

	async function syncLibraryUrl(view: LibraryShapingView | null = activeShapingView) {
		const params = new URLSearchParams(page.url.searchParams);
		if (query.trim()) params.set('q', query.trim());
		else params.delete('q');

		if (activeLibraryId) params.set('library', activeLibraryId);
		else params.delete('library');

		if (offset > 0) params.set('offset', String(offset));
		else params.delete('offset');

		if (pageSize !== 40) params.set('page_size', String(pageSize));
		else params.delete('page_size');

		if (sortBy !== 'modified_at') params.set('sort', sortBy);
		else params.delete('sort');

		if (sortDirection !== 'desc') params.set('dir', sortDirection);
		else params.delete('dir');

		if (advancedMode) params.set('advanced', '1');
		else params.delete('advanced');

		if (view && view !== 'all') {
			params.set('view', view);
			params.delete('type');
			params.delete('status');
		} else {
			params.delete('view');
			if (typeFilter !== 'all') params.set('type', typeFilter);
			else params.delete('type');
			if (managedStatusFilter !== 'all') params.set('status', managedStatusFilter);
			else params.delete('status');
		}

		const next = params.toString();
		await goto(next ? `/library?${next}` : '/library', {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
	}

	async function applyShapingView(view: LibraryShapingView) {
		typeFilter = shapingViewFilters[view].typeFilter;
		managedStatusFilter = shapingViewFilters[view].managedStatusFilter;
		await reloadLibrary(true);
		await syncLibraryUrl(view);
	}

	function setTypeFilter(filter: LibraryMediaFilter) {
		typeFilter = filter;
		void reloadLibrary(true);
	}

	function setManagedStatusFilter(filter: LibraryManagedFilter) {
		managedStatusFilter = filter;
		void reloadLibrary(true);
	}

	function setPageSize(nextSize: number) {
		pageSize = nextSize;
		void reloadLibrary(true);
	}

	function setSortBy(nextSort: LibrarySortBy) {
		sortBy = nextSort;
		void reloadLibrary(true);
	}

	function toggleSortDirection() {
		sortDirection = sortDirection === 'desc' ? 'asc' : 'desc';
		void reloadLibrary(true);
	}

	function toggleAdvancedMode() {
		advancedMode = !advancedMode;
		void syncLibraryUrl();
	}

	async function setLibraryViewMode(mode: LibraryViewMode) {
		if (libraryViewMode === mode) {
			return;
		}

		libraryViewMode = mode;
		libraryViewModeError = '';
		libraryViewModeSaving = true;
		try {
			const config = await fetchConfig();
			config.library_view_mode = mode;
			await saveConfig(config);
		} catch (error) {
			libraryViewModeError =
				error instanceof Error ? error.message : 'Failed to save library view preference';
		} finally {
			libraryViewModeSaving = false;
		}
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

	function providerLabel(provider: string): string {
		return provider.toUpperCase();
	}

	function showDetailHref(show: string, item?: LibraryEntry): string {
		const params = new URLSearchParams();
		if (activeLibraryId) {
			params.set('library', activeLibraryId);
		}
		params.set('show', show);
		if (item) {
			params.set('path', item.relative_path);
		}
		return `/library/show?${params.toString()}`;
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

<section class="mb-5 grid gap-3 lg:grid-cols-3 xl:grid-cols-6">
	{#each shapingViewMeta as view (view.key)}
		{@const isActive = activeShapingView === view.key}
		<button class="rounded-[1rem] border px-4 py-4 text-left transition-colors {isActive ? 'border-[color:var(--accent)] bg-[color:rgba(164,79,45,0.08)]' : 'border-[color:var(--line)] bg-[color:var(--panel-strong)] hover:bg-[color:rgba(214,180,111,0.08)]'}" onclick={() => applyShapingView(view.key)}>
			<div class="section-label">{view.label}</div>
			<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{view.count}</div>
			<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{view.description}</div>
		</button>
	{/each}
</section>

<!-- Controls Bar -->
<section class="mb-5 flex flex-wrap items-center gap-3">
	<label class="flex flex-1 items-center gap-3 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5">
		<svg class="h-4 w-4 text-[color:var(--ink-muted)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
		<input class="min-w-0 flex-1 bg-transparent text-sm text-[color:var(--ink-strong)] outline-none placeholder:text-[color:var(--ink-muted)]" type="search" placeholder="Search paths, filenames, codecs…" value={query} oninput={handleSearchInput} />
	</label>
	<div class="flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1">
		{#each mediaFilters as t (t)}
			<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {typeFilter === t ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => setTypeFilter(t)}>{t}</button>
		{/each}
	</div>
	<div class="flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1">
		{#each [
			['all', 'All Statuses'],
			['UNPROCESSED', 'Unprocessed'],
			['REVIEWED', 'Reviewed'],
			['RE_SOURCE', 'Re-source'],
			['AWAITING_APPROVAL', 'Awaiting Approval'],
			['APPROVED', 'Approved'],
			['PROCESSED', 'Processed'],
			['FAILED', 'Failed'],
			['KEPT_ORIGINAL', 'Kept Original'],
			['MISSING_METADATA', 'Needs Metadata'],
			['ORGANIZE_NEEDED', 'Organize Needed'],
			['NO_SIDECAR', 'No NFO']
		] as [value, label] (value)}
			<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {managedStatusFilter === value ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => setManagedStatusFilter(value as LibraryManagedFilter)}>{label}</button>
		{/each}
	</div>
	<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {bulkMode ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { bulkMode = !bulkMode; if (!bulkMode) clearSelection(); }}>
		Select
	</button>
	<div class="flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1">
		<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {libraryViewMode === 'compact' ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => setLibraryViewMode('compact')} disabled={libraryViewModeSaving}>
			Compact
		</button>
		<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {libraryViewMode === 'expanded' ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => setLibraryViewMode('expanded')} disabled={libraryViewModeSaving}>
			Expanded
		</button>
	</div>
	<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {advancedMode ? 'bg-[color:var(--olive)] text-white border-[color:var(--olive)]' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={toggleAdvancedMode}>
		{advancedMode ? 'Advanced On' : 'Advanced Mode'}
	</button>
	<div class="rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5 text-sm text-[color:var(--ink-muted)]">
		{pageRangeLabel()} of {totalLibrary}
	</div>
</section>

{#if libraryViewModeSaving || libraryViewModeError}
	<section class="mb-4 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2.5 text-xs text-[color:var(--ink-muted)]">
		{#if libraryViewModeSaving}
			Saving library view preference…
		{:else if libraryViewModeError}
			<span class="text-[color:var(--danger)]">{libraryViewModeError}</span>
		{/if}
	</section>
{/if}

{#if advancedMode}
	<section class="mb-5 grid gap-3 rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
		<label class="flex flex-col gap-1 text-sm text-[color:var(--ink-muted)]">
			<span class="section-label">Rows Per Page</span>
			<select class="rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.75)] px-3 py-2 text-sm text-[color:var(--ink-strong)] outline-none" bind:value={pageSize} onchange={(event) => setPageSize(Number((event.currentTarget as HTMLSelectElement).value))}>
				{#each pageSizeOptions as option (option)}
					<option value={option}>{option}</option>
				{/each}
			</select>
		</label>
		<label class="flex flex-col gap-1 text-sm text-[color:var(--ink-muted)]">
			<span class="section-label">Sort Order</span>
			<select class="rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.75)] px-3 py-2 text-sm text-[color:var(--ink-strong)] outline-none" bind:value={sortBy} onchange={(event) => setSortBy((event.currentTarget as HTMLSelectElement).value as LibrarySortBy)}>
				{#each sortOptions as option (option.value)}
					<option value={option.value}>{option.label}</option>
				{/each}
			</select>
		</label>
		<div class="flex items-end gap-3">
			<button class="rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.75)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)]" onclick={toggleSortDirection}>
				{sortDirection === 'desc' ? 'Descending' : 'Ascending'}
			</button>
			<div class="text-xs text-[color:var(--ink-muted)]">
				Server-backed sort and filters. Use larger pages when you need to sweep a library quickly.
			</div>
		</div>
	</section>
{/if}

{#if rowActionError}
	<section class="mb-4 rounded-xl border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-3 text-sm text-[color:var(--danger)]">
		{rowActionError}
	</section>
{/if}

<!-- Bulk Actions Bar -->
{#if bulkMode && selectedPaths.size > 0}
	<section class="mb-4 flex items-center gap-3 rounded-xl border border-[color:var(--accent)]/30 bg-[color:var(--accent)]/5 px-4 py-2.5">
		<span class="text-sm font-semibold text-[color:var(--ink-strong)]">{selectedPaths.size} selected</span>
		<div class="flex gap-2">
			<button class="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-white disabled:opacity-50" onclick={runBulkCreateReview} disabled={bulkActionLoading || bulkInternetLoading}>{bulkActionLoading ? 'Working…' : 'Create Reviews'}</button>
			<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => runBulkManagedStatus('REVIEWED')} disabled={bulkActionLoading || bulkInternetLoading}>{bulkActionLoading ? 'Working…' : 'Mark Reviewed'}</button>
			<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => runBulkManagedStatus('KEPT_ORIGINAL')} disabled={bulkActionLoading || bulkInternetLoading}>{bulkActionLoading ? 'Working…' : 'Keep Original'}</button>
			<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => runBulkInternetLookup(false)} disabled={bulkInternetLoading}>{bulkInternetLoading ? 'Working…' : 'Bulk Lookup Metadata'}</button>
			<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => runBulkInternetLookup(true)} disabled={bulkInternetLoading}>{bulkInternetLoading ? 'Working…' : 'Auto-Select Top Match'}</button>
		</div>
		{#if bulkActionStatus}
			<span class="text-xs text-[color:var(--ink-muted)]">{bulkActionStatus}</span>
		{/if}
		{#if bulkInternetStatus}
			<span class="text-xs text-[color:var(--ink-muted)]">{bulkInternetStatus}</span>
		{/if}
		{#if bulkActionFailedPaths.length > 0}
			<div class="max-w-full text-xs text-[color:var(--danger)]">
				Failed action paths: {bulkActionFailedPaths.join(' • ')}
			</div>
		{/if}
		{#if bulkInternetFailedPaths.length > 0}
			<div class="max-w-full text-xs text-[color:var(--danger)]">
				Metadata lookup needs follow-up for: {bulkInternetFailedPaths.join(' • ')}
			</div>
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
				TV Shows By Managed Status
			</div>
			{#if libraryLoading}
				<div class="px-4 py-14 text-center text-[color:var(--ink-muted)]">Scanning library…</div>
			{:else if tvShowGroups.length === 0}
				<div class="px-4 py-14 text-center text-[color:var(--ink-muted)]">No show entries match the current filter.</div>
			{:else}
				<div class="max-h-[38rem] overflow-y-auto">
					{#each tvShowGroups as group (group.show)}
						{@const showStatuses = group.items.map((item) => item.managed_status ?? 'UNPROCESSED')}
						{@const showNeedsAttention = showStatuses.filter((status) => status === 'UNPROCESSED' || status === 'FAILED' || status === 'AWAITING_APPROVAL').length}
						{@const showMissingMetadata = group.items.filter((item) => !item.has_selected_metadata && (item.managed_status ?? 'UNPROCESSED') !== 'KEPT_ORIGINAL' && (item.managed_status ?? 'UNPROCESSED') !== 'PROCESSED').length}
						{@const showMissingNfo = group.items.filter((item) => !item.has_sidecar).length}
						{@const showOrganizeNeeded = group.items.filter((item) => item.organize_needed).length}
						{@const showReSourceCount = group.items.filter((item) => (item.managed_status ?? 'UNPROCESSED') === 'RE_SOURCE').length}
						{@const showKeptOriginalCount = group.items.filter((item) => (item.managed_status ?? 'UNPROCESSED') === 'KEPT_ORIGINAL').length}
						{@const showNotes = group.items.filter((item) => !!item.review_note)}
						{@const showPreviewNote = preferredOutcomeNote(group.items)}
						<div class="border-b border-[color:rgba(123,105,81,0.14)]">
							<button class="flex w-full items-center justify-between px-4 py-3 text-left hover:bg-[color:rgba(214,180,111,0.08)]" onclick={() => toggleShow(group.show)}>
								<div>
									<div class="font-semibold text-[color:var(--ink-strong)]">{group.show}</div>
									<div class="mt-1 flex flex-wrap items-center gap-2 text-xs text-[color:var(--ink-muted)]">
										<span>{group.items.length} episode file(s)</span>
										{#if showNeedsAttention > 0}
											<span class="status-chip failed">{showNeedsAttention} need attention</span>
										{/if}
										{#if showMissingMetadata > 0}
											<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">{showMissingMetadata} need metadata</span>
										{/if}
										{#if showMissingNfo > 0}
											<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">{showMissingNfo} missing nfo</span>
										{/if}
										{#if showOrganizeNeeded > 0}
											<span class="rounded-full border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--accent-deep)]">{showOrganizeNeeded} organize needed</span>
										{/if}
										{#if showReSourceCount > 0}
											<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">{showReSourceCount} re-source</span>
										{/if}
										{#if showKeptOriginalCount > 0}
											<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">{showKeptOriginalCount} kept original</span>
										{/if}
										{#if showNotes.length > 0}
											<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">{showNotes.length} noted</span>
										{/if}
									</div>
									{#if showPreviewNote}
										<div class="mt-2 max-w-3xl rounded-lg border border-[color:rgba(106,142,72,0.2)] bg-[color:rgba(106,142,72,0.08)] px-3 py-2 text-xs text-[color:var(--olive)]">
											<span class="font-semibold">Outcome highlight:</span>
											 {showPreviewNote.file_name}: {showPreviewNote.review_note}
											{#if showPreviewNote.review_updated_at}
												<div class="mt-1 text-[11px] text-[color:var(--ink-muted)]">Reviewed {formatTimestamp(showPreviewNote.review_updated_at)}</div>
											{/if}
										</div>
									{/if}
								</div>
								<div class="flex items-center gap-3">
									<a href={showDetailHref(group.show, group.items[0])} class="rounded-md border border-[color:var(--line)] px-2.5 py-1 text-[10px] font-semibold text-[color:var(--ink-strong)] no-underline" onclick={(event) => event.stopPropagation()}>Open show page</a>
									<span class="text-[color:var(--ink-muted)]">{expandedShows.has(group.show) ? '▾' : '▸'}</span>
								</div>
							</button>
							{#if expandedShows.has(group.show)}
								<div class="bg-[color:rgba(244,236,223,0.5)]">
									{#each group.items as item (item.relative_path)}
										{@const detail = detailState(item.relative_path)}
										{@const artworkSrc = itemArtworkSrc(detail)}
										<div class="border-t border-[color:rgba(123,105,81,0.1)] first:border-t-0">
											<div class="flex items-start justify-between gap-3 px-8 py-3 text-left text-sm hover:bg-[color:rgba(214,180,111,0.08)] {selectedItem?.relative_path === item.relative_path ? 'bg-[color:rgba(214,180,111,0.12)]' : ''}">
												<button class="min-w-0 flex-1 text-left" onclick={() => loadMetadata(item)}>
													<div class="flex flex-wrap items-center gap-2">
														<div class="font-medium text-[color:var(--ink-strong)]">{item.file_name}</div>
														<span class="status-chip {statusTone(item.managed_status ?? 'UNPROCESSED')}">{statusLabel(item.managed_status ?? 'UNPROCESSED')}</span>
														{#if metadataSelectionNeeded(item)}
															<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">needs metadata</span>
														{:else if item.has_selected_metadata}
															<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">metadata selected</span>
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
													<div class="mt-0.5 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</div>
												</button>
												{#if libraryViewMode === 'compact'}
													<button class="shrink-0 rounded-md border border-[color:var(--line)] px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.1em] text-[color:var(--ink-strong)]" onclick={() => toggleItemExpansion(item)}>
														{itemIsExpanded(item.relative_path) ? 'Collapse' : 'Expand'}
													</button>
												{/if}
											</div>
											{#if itemIsExpanded(item.relative_path)}
												<div class="px-8 pb-4">
													<div class="grid gap-4 rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)] p-4 lg:grid-cols-[10rem_minmax(0,1fr)]">
														<div class="overflow-hidden rounded-xl border border-[color:var(--line)] bg-[color:rgba(234,223,201,0.5)]">
															{#if artworkSrc}
																<img src={artworkSrc} alt={`${item.file_name} artwork`} class="h-44 w-full object-cover" />
															{:else}
																<div class="flex h-44 items-center justify-center px-4 text-center text-xs font-semibold uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">No artwork yet</div>
															{/if}
														</div>
														<div>
															{#if detail?.loading}
																<div class="rounded-lg border border-[color:var(--line)] px-4 py-6 text-sm text-[color:var(--ink-muted)]">Loading inline details…</div>
															{:else if detail?.error}
																<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-3 text-sm text-[color:var(--danger)]">{detail.error}</div>
															{:else}
																<div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
																	<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
																		<div class="section-label">Selected Metadata</div>
																		{#if detail?.selectedMatch}
																			<div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{detail.selectedMatch.title}{detail.selectedMatch.year ? ` (${detail.selectedMatch.year})` : ''}</div>
																			<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{detail.selectedMatch.provider.toUpperCase()}</div>
																		{:else}
																			<div class="mt-1 text-sm text-[color:var(--ink-muted)]">No provider match selected yet.</div>
																		{/if}
																	</div>
																	<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
																		<div class="section-label">Filesystem</div>
																		<div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{formatBytes(item.size_bytes)}</div>
																		<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Modified {formatTimestamp(item.modified_at)}</div>
																		<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{item.filesystem.is_hard_linked ? hardLinkSummary(item.filesystem.link_count) : 'Single directory entry'}</div>
																	</div>
																	<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
																		<div class="section-label">Placement</div>
																		<div class="mt-1 break-all font-mono text-[11px] text-[color:var(--ink-strong)]">{item.relative_path}</div>
																		{#if item.organize_target_path}
																			<div class="mt-2 break-all font-mono text-[11px] text-[color:var(--ink-strong)]">Target: {item.organize_target_path}</div>
																		{/if}
																	</div>
																	<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
																		<div class="section-label">Library Facts</div>
																		<div class="mt-1 text-sm font-semibold text-[color:var(--ink-strong)]">{item.has_sidecar ? 'Jellyfin NFO present' : 'Missing Jellyfin NFO'}</div>
																		<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{artworkSrc ? (detail?.artwork?.poster_path || detail?.artwork?.backdrop_path ? 'Local artwork found beside media' : 'Using remote artwork fallback') : 'No artwork available'}</div>
																		{#if detail?.relatedPaths.length}
																			<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{detail.relatedPaths.length} related path{detail.relatedPaths.length === 1 ? '' : 's'} share this selection</div>
																		{/if}
																	</div>
																</div>
																{#if detail?.selectedMatch?.overview}
																	<p class="mt-3 text-sm leading-6 text-[color:var(--ink-muted)]">{detail.selectedMatch.overview}</p>
																{/if}
															{/if}
														</div>
													</div>
												</div>
											{/if}
										</div>
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
						<th class="px-4 py-3">Managed</th>
						{#if !activeLibraryId && libraryFolders.length > 0}
							<th class="px-4 py-3">Library</th>
						{/if}
						<th class="px-4 py-3">Size</th>
						<th class="px-4 py-3">Modified</th>
						<th class="px-4 py-3">Actions</th>
					</tr>
				</thead>
				<tbody>
					{#if libraryLoading}
						<tr><td colspan={visibleColumnCount} class="px-4 py-14 text-center text-[color:var(--ink-muted)]">Scanning library…</td></tr>
					{:else if filteredLibrary.length === 0}
						<tr><td colspan={visibleColumnCount} class="px-4 py-14 text-center text-[color:var(--ink-muted)]">No entries match the current filter.</td></tr>
					{:else}
						{#each filteredLibrary as item (item.relative_path)}
							{@const detail = detailState(item.relative_path)}
							{@const artworkSrc = itemArtworkSrc(detail)}
							<tr class="cursor-pointer border-b border-[color:rgba(123,105,81,0.14)] hover:bg-[color:rgba(214,180,111,0.08)] {selectedItem?.relative_path === item.relative_path ? 'bg-[color:rgba(214,180,111,0.12)]' : ''} {selectedPaths.has(item.relative_path) ? 'bg-[color:rgba(214,180,111,0.08)]' : ''}" onclick={() => { if (bulkMode) { toggleSelect(item.relative_path); } else { loadMetadata(item); } }}>
								{#if bulkMode}
									<td class="w-10 px-3 py-3" onclick={(e) => { e.stopPropagation(); toggleSelect(item.relative_path); }}>
										<input type="checkbox" checked={selectedPaths.has(item.relative_path)} class="accent-[color:var(--accent)]" />
									</td>
								{/if}
								<td class="px-4 py-3 align-top">
									<div class="flex items-start justify-between gap-3">
										<div class="min-w-0">
											<div class="font-medium text-[color:var(--ink-strong)]">{item.file_name}</div>
											<div class="mt-0.5 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{item.relative_path}</div>
										</div>
										{#if libraryViewMode === 'compact'}
											<button class="shrink-0 rounded-md border border-[color:var(--line)] px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.1em] text-[color:var(--ink-strong)]" onclick={(event) => { event.stopPropagation(); toggleItemExpansion(item); }}>
												{itemIsExpanded(item.relative_path) ? 'Collapse' : 'Expand'}
											</button>
										{/if}
									</div>
								</td>
								<td class="px-4 py-3 align-top">
									<span class="status-chip {item.media_type === 'video' ? 'processing' : item.media_type === 'audio' ? 'completed' : ''}">{item.media_type}</span>
								</td>
								<td class="px-4 py-3 align-top">
									<div class="flex max-w-[18rem] flex-wrap items-center gap-2">
										<span class="status-chip {statusTone(item.managed_status ?? 'UNPROCESSED')}">{statusLabel(item.managed_status ?? 'UNPROCESSED')}</span>
										{#if metadataSelectionNeeded(item)}
											<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">needs metadata</span>
										{:else if item.has_selected_metadata}
											<span class="rounded-full border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">metadata selected</span>
										{/if}
										{#if item.organize_needed}
											<span class="rounded-full border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--accent-deep)]">organize needed</span>
										{/if}
										{#if item.has_sidecar}
											<span class="rounded-full border border-[color:var(--line)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-muted)]">nfo</span>
										{:else}
											<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">missing nfo</span>
										{/if}
									</div>
								</td>
								{#if !activeLibraryId && libraryFolders.length > 0}
									<td class="px-4 py-3 align-top">
										{#if item.library_id}
											{@const lib = libraryFolders.find((l) => l.id === item.library_id)}
											{#if lib}
												<span class="rounded-full bg-[color:rgba(214,180,111,0.15)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--ink-strong)]">{lib.name}</span>
											{/if}
										{:else}
											<span class="text-xs text-[color:var(--ink-muted)]">—</span>
										{/if}
									</td>
								{/if}
								<td class="px-4 py-3 align-top text-[color:var(--ink-strong)]">{formatBytes(item.size_bytes)}</td>
								<td class="px-4 py-3 align-top text-[color:var(--ink-muted)]">{formatTimestamp(item.modified_at)}</td>
								<td class="px-4 py-3 align-top" onclick={(e) => e.stopPropagation()}>
									<div class="flex flex-wrap gap-2">
										{#if (item.managed_status ?? 'UNPROCESSED') === 'UNPROCESSED'}
											<button class="rounded-md bg-[color:var(--accent)] px-2.5 py-1.5 text-[10px] font-semibold text-white disabled:opacity-50" onclick={() => createReview(item)} disabled={!!rowActionBusy[item.relative_path]}>
												{rowActionBusy[item.relative_path] === 'review' ? 'Building…' : 'Create Review'}
											</button>
											<button class="rounded-md border border-[color:var(--line)] px-2.5 py-1.5 text-[10px] font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => updateManagedStatus(item, 'REVIEWED')} disabled={!!rowActionBusy[item.relative_path]}>
												{rowActionBusy[item.relative_path] === 'REVIEWED' ? 'Saving…' : 'Mark Reviewed'}
											</button>
											<button class="rounded-md border border-[color:var(--line)] px-2.5 py-1.5 text-[10px] font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => updateManagedStatus(item, 'KEPT_ORIGINAL')} disabled={!!rowActionBusy[item.relative_path]}>
												{rowActionBusy[item.relative_path] === 'KEPT_ORIGINAL' ? 'Saving…' : 'Keep Original'}
											</button>
										{:else}
											<button class="rounded-md border border-[color:var(--line)] px-2.5 py-1.5 text-[10px] font-semibold text-[color:var(--ink-strong)]" onclick={() => loadMetadata(item)}>
												Inspect
											</button>
										{/if}
										{#if item.library_id}
											<a href={`/organize?library=${encodeURIComponent(item.library_id)}&path=${encodeURIComponent(item.relative_path)}`} class="rounded-md border border-[color:var(--line)] px-2.5 py-1.5 text-[10px] font-semibold text-[color:var(--ink-strong)] no-underline">
												Organize
											</a>
										{/if}
									</div>
								</td>
							</tr>
							{#if itemIsExpanded(item.relative_path)}
								<tr class="border-b border-[color:rgba(123,105,81,0.14)] bg-[color:rgba(244,236,223,0.42)] last:border-b-0">
									<td colspan={visibleColumnCount} class="px-4 pb-4 pt-1">
										<div class="grid gap-4 rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)] p-4 lg:grid-cols-[10rem_minmax(0,1fr)]">
											<div class="overflow-hidden rounded-xl border border-[color:var(--line)] bg-[color:rgba(234,223,201,0.5)]">
												{#if artworkSrc}
													<img src={artworkSrc} alt={`${item.file_name} artwork`} class="h-48 w-full object-cover" />
												{:else}
													<div class="flex h-48 items-center justify-center px-4 text-center text-xs font-semibold uppercase tracking-[0.12em] text-[color:var(--ink-muted)]">No artwork yet</div>
												{/if}
											</div>
											<div>
												{#if detail?.loading}
													<div class="rounded-lg border border-[color:var(--line)] px-4 py-6 text-sm text-[color:var(--ink-muted)]">Loading inline details…</div>
												{:else if detail?.error}
													<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-3 text-sm text-[color:var(--danger)]">{detail.error}</div>
												{:else}
													<div class="flex flex-wrap items-center gap-2">
														<span class="status-chip {statusTone(item.managed_status ?? 'UNPROCESSED')}">{statusLabel(item.managed_status ?? 'UNPROCESSED')}</span>
														{#if detail?.selectedMatch}
															<span class="rounded-full bg-[color:rgba(106,142,72,0.1)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--olive)]">{detail.selectedMatch.provider}</span>
														{/if}
														{#if metadataSelectionNeeded(item)}
															<span class="rounded-full border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.1em] text-[color:var(--danger)]">awaiting metadata selection</span>
														{/if}
													</div>
													<div class="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
														<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
															<div class="section-label">Selected Metadata</div>
															{#if detail?.selectedMatch}
																<div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{detail.selectedMatch.title}{detail.selectedMatch.year ? ` (${detail.selectedMatch.year})` : ''}</div>
																<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{detail.selectedMatch.provider.toUpperCase()} {detail.selectedMatch.media_kind ? `· ${detail.selectedMatch.media_kind}` : ''}</div>
															{:else}
																<div class="mt-1 text-sm text-[color:var(--ink-muted)]">No provider match has been selected for this item yet.</div>
															{/if}
														</div>
														<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
															<div class="section-label">Filesystem</div>
															<div class="mt-1 font-semibold text-[color:var(--ink-strong)]">{formatBytes(item.size_bytes)}</div>
															<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Modified {formatTimestamp(item.modified_at)}</div>
															<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{item.filesystem.is_hard_linked ? hardLinkSummary(item.filesystem.link_count) : 'Single directory entry'}</div>
														</div>
														<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
															<div class="section-label">Placement</div>
															<div class="mt-1 text-xs text-[color:var(--ink-muted)]">Current</div>
															<div class="break-all font-mono text-[11px] text-[color:var(--ink-strong)]">{item.relative_path}</div>
															{#if item.organize_target_path}
																<div class="mt-2 text-xs text-[color:var(--ink-muted)]">Target</div>
																<div class="break-all font-mono text-[11px] text-[color:var(--ink-strong)]">{item.organize_target_path}</div>
															{/if}
														</div>
														<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
															<div class="section-label">Library Facts</div>
															<div class="mt-1 text-sm font-semibold text-[color:var(--ink-strong)]">{libraryFolders.find((library) => library.id === item.library_id)?.name ?? 'Unassigned'}</div>
															<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{item.has_sidecar ? 'Jellyfin NFO present' : 'Missing Jellyfin NFO'}</div>
															<div class="mt-1 text-xs text-[color:var(--ink-muted)]">{artworkSrc ? (detail?.artwork?.poster_path || detail?.artwork?.backdrop_path ? 'Local artwork found beside media' : 'Using remote artwork fallback') : 'No artwork available'}</div>
														</div>
													</div>
													{#if detail?.selectedMatch?.overview}
														<p class="mt-3 text-sm leading-6 text-[color:var(--ink-muted)]">{detail.selectedMatch.overview}</p>
													{/if}
													{#if detail?.relatedPaths.length}
														<div class="mt-3 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
															<div class="font-semibold uppercase tracking-[0.12em]">Related Library Paths</div>
															{#each detail.relatedPaths as path (path)}
																<div class="mt-1 break-all font-mono text-[11px] text-[color:var(--ink-strong)]">{path}</div>
															{/each}
														</div>
													{/if}
													{#if item.review_note}
														<div class="mt-3 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">
															{#if item.review_updated_at}
																<div class="mb-1 text-[11px] text-[color:var(--ink-muted)]">Reviewed {formatTimestamp(item.review_updated_at)}</div>
															{/if}
															{item.review_note}
														</div>
													{/if}
												{/if}
											</div>
										</div>
									</td>
								</tr>
							{/if}
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
									<div class="flex flex-wrap gap-2">
										{#if selectedItem.library_id}
											<a href={`/organize?library=${encodeURIComponent(selectedItem.library_id)}&path=${encodeURIComponent(selectedItem.relative_path)}`} class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] no-underline">
												Open organize
											</a>
										{/if}
										<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => loadInternetMetadata(selectedItem!)} disabled={internetMetadataLoading}>
											{internetMetadataLoading ? 'Looking up…' : 'Lookup IMDb/TVDB'}
										</button>
									</div>
							</div>
						<p class="mt-0.5 break-all font-mono text-[11px] text-[color:var(--ink-muted)]">{selectedMetadata.relative_path}</p>
						{#if selectedMetadata.filesystem.is_hard_linked}
							<div class="mt-2 inline-flex rounded-full border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--accent-deep)]">
								{hardLinkSummary(selectedMetadata.filesystem.link_count)}
							</div>
						{/if}
						{#if selectedItem.organize_needed && selectedItem.organize_target_path}
							<div class="mt-3 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs">
								<div class="font-semibold uppercase tracking-[0.12em] text-[color:var(--accent-deep)]">Canonical Target</div>
								<div class="mt-1 break-all font-mono text-[color:var(--ink-strong)]">{selectedItem.organize_target_path}</div>
								<div class="mt-1 text-[color:var(--ink-muted)]">This file has metadata selected, but it is not yet placed at the canonical target.</div>
							</div>
						{/if}
						{#if selectedItem.review_note}
							<div class="mt-3 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">
								<div class="font-semibold uppercase tracking-[0.12em]">Review Outcome</div>
								{#if selectedItem.review_updated_at}
									<div class="mt-1 text-[11px] text-[color:var(--ink-muted)]">Reviewed {formatTimestamp(selectedItem.review_updated_at)}</div>
								{/if}
								<div class="mt-1">{selectedItem.review_note}</div>
							</div>
						{/if}
						{#if selectedMetadata.filesystem.is_hard_linked}
							<div class="mt-3 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
								This item is hard-linked. Organize-only changes keep the same inode and preserve the shared storage relationship.
							</div>
						{/if}
						<div class="mt-3 flex flex-wrap gap-2">
							<input
								class="min-w-[16rem] flex-1 rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
								type="search"
								placeholder="Override metadata search query"
								bind:value={manualMetadataQuery}
								onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void runManualInternetLookup(); } }}
							/>
							<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={runManualInternetLookup} disabled={internetMetadataLoading || !manualMetadataQuery.trim()}>
								Search Override
							</button>
						</div>
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
							<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
								<div class="section-label">Inode</div>
								<div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{selectedMetadata.filesystem.inode || 'Unknown'}</div>
								<div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">Device {selectedMetadata.filesystem.device_id || 0}</div>
							</div>
							<div class="rounded-lg border border-[color:var(--line)] px-3 py-2.5">
								<div class="section-label">Link Count</div>
								<div class="mt-0.5 font-semibold text-[color:var(--ink-strong)]">{selectedMetadata.filesystem.link_count}</div>
								<div class="mt-0.5 text-xs text-[color:var(--ink-muted)]">{selectedMetadata.filesystem.is_hard_linked ? 'Shared storage is currently active' : 'Single directory entry'}</div>
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
						{#if internetMetadata?.search_candidates?.length}
							<div class="mb-2 rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.45)] px-3 py-2 text-[11px] text-[color:var(--ink-muted)]">
								Search candidates: <span class="font-semibold text-[color:var(--ink-strong)]">{internetMetadata.search_candidates.join(' -> ')}</span>
							</div>
						{/if}
						{#if selectedInternetMatch}
							<div class="mb-2 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">
								Selected: <span class="font-semibold">{selectedInternetMatch.title}{selectedInternetMatch.year ? ` (${selectedInternetMatch.year})` : ''}</span> via {selectedInternetMatch.provider}
							</div>
						{/if}
						{#if relatedPaths.length > 0}
							<div class="mb-2 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
								<div class="mb-1 font-semibold uppercase tracking-[0.12em]">Related Library Paths</div>
								{#each relatedPaths as path (path)}
									<div class="font-mono text-[11px] text-[color:var(--ink-strong)]">{path}</div>
								{/each}
							</div>
						{/if}
						{#if internetSaveError}
							<div class="mb-2 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{internetSaveError}</div>
						{/if}
						{#if internetSaveWarning}
							<div class="mb-2 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">{internetSaveWarning}</div>
						{/if}
						{#if internetMetadataError}
							<div class="rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{internetMetadataError}</div>
						{:else if internetMetadataLoading}
							<div class="text-xs text-[color:var(--ink-muted)]">Querying provider…</div>
						{:else if internetMetadata}
							{#if internetMetadata.providers.length > 0}
								<div class="mb-2 grid gap-2 sm:grid-cols-2">
									{#each internetMetadata.providers as provider (provider.provider)}
										<div class="rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-2 text-[11px] text-[color:var(--ink-muted)]">
											<div class="flex items-center justify-between gap-2">
												<span class="font-semibold uppercase tracking-[0.12em] text-[color:var(--ink-strong)]">{providerLabel(provider.provider)}</span>
												<span>{provider.match_count} match{provider.match_count === 1 ? '' : 'es'}</span>
											</div>
											{#if provider.warning}
												<div class="mt-1 text-[color:var(--danger)]">{provider.warning}</div>
											{:else if provider.match_count === 0}
												<div class="mt-1">No matches returned.</div>
											{/if}
										</div>
									{/each}
								</div>
							{/if}
							{#if internetMetadata.provider_used}
								<div class="mb-2 text-[11px] text-[color:var(--ink-muted)]">Searched: <span class="font-semibold uppercase tracking-[0.08em] text-[color:var(--ink-strong)]">{internetMetadata.provider_used}</span></div>
							{/if}
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

						{#if selectedInternetMatch && selectedItem.library_id}
							<div class="mt-3 rounded-lg border border-[color:var(--line)] px-3 py-2.5">
								<div class="mb-2 flex items-center justify-between gap-2">
									<div class="section-label">Canonical Rename</div>
									<span class="text-[11px] text-[color:var(--ink-muted)]">Library: {libraryFolders.find((library) => library.id === selectedItem?.library_id)?.name ?? selectedItem?.library_id}</span>
								</div>
								<div class="flex flex-wrap gap-2">
									<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={previewCanonicalRename} disabled={organizeLoading}>
										{organizeLoading ? 'Working…' : 'Preview Canonical Rename'}
									</button>
									<button class="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-white disabled:opacity-50" onclick={() => applyCanonicalRename(false)} disabled={organizeLoading || organizePreview?.target_exists}>
										Apply Rename
									</button>
									{#if organizePreview?.target_exists}
										<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-50" onclick={() => applyCanonicalRename(true)} disabled={organizeLoading}>
											Merge Into Existing Folder
										</button>
									{/if}
								</div>
								{#if organizePreview}
									<div class="mt-3 rounded-lg bg-[color:rgba(244,236,223,0.55)] px-3 py-2 text-xs">
										<div class="text-[color:var(--ink-muted)]">Current</div>
										<div class="break-all font-mono text-[color:var(--ink-strong)]">{organizePreview.current_relative_path}</div>
										<div class="mt-2 text-[color:var(--ink-muted)]">Target</div>
										<div class="break-all font-mono text-[color:var(--ink-strong)]">{organizePreview.target_relative_path}</div>
										{#if organizePreview.hard_link_warning}
											<div class="mt-2 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-2.5 py-2 text-[color:var(--accent-deep)]">
												{organizePreview.hard_link_warning}
											</div>
										{/if}
										{#if organizePreview.target_exists}
											<div class="mt-2 text-[color:var(--danger)]">A file already exists at the canonical target. This usually means the same movie already exists in another folder.</div>
										{/if}
									</div>
								{/if}
								{#if organizeError}
									<div class="mt-2 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{organizeError}</div>
								{/if}
								{#if organizeStatus}
									<div class="mt-2 rounded-lg border border-[color:rgba(106,142,72,0.25)] bg-[color:rgba(106,142,72,0.1)] px-3 py-2 text-xs text-[color:var(--olive)]">{organizeStatus}</div>
								{/if}
							</div>
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
