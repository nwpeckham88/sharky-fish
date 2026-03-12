<script lang="ts">
	import {
		approveJobMode,
		approveJobGroupMode,
		markJobGroupKeepOriginal,
		markJobGroupReSource,
		markJobKeepOriginal,
		markJobReSource,
		rejectJob,
		rejectJobGroup,
		type Job,
		type MediaProbe,
		type ReviewExecutionMode,
		type ReviewProposal
	} from '$lib/api';
	import { getExecutionState, jobStore, getReviewState } from '$lib/stores.svelte';
	import { fileName } from '$lib/status';

	type ReviewAction =
		| ReviewExecutionMode
		| 'keep_original'
		| 're_source'
		| 'reject'
		| 'group:full_plan'
		| 'group:organize_only'
		| 'group:process_only'
		| 'keep_original_group'
		| 're_source_group'
		| 'reject_group'
		| null;

	let actionBusy = $state<Record<number, ReviewAction>>({});
	let actionErrors = $state<Record<number, string>>({});

	function formatDuration(seconds?: number): string {
		if (!seconds || seconds <= 0) return 'Unknown duration';
		const totalSeconds = Math.round(seconds);
		const hours = Math.floor(totalSeconds / 3600);
		const minutes = Math.floor((totalSeconds % 3600) / 60);
		const remainingSeconds = totalSeconds % 60;
		if (hours > 0) return `${hours}h ${minutes}m`;
		if (minutes > 0) return `${minutes}m ${remainingSeconds}s`;
		return `${remainingSeconds}s`;
	}

	function summarizeProbe(probe: MediaProbe | null): string[] {
		if (!probe) return [];
		const video = probe.streams.find((stream) => stream.codec_type === 'video');
		const audio = probe.streams.find((stream) => stream.codec_type === 'audio');
		const subtitles = probe.streams.filter((stream) => stream.codec_type === 'subtitle');
		const parts = [probe.format.toUpperCase(), formatDuration(probe.duration_secs)];
		if (video?.codec_name) parts.push(video.codec_name.toUpperCase());
		if (video?.width && video?.height) parts.push(`${video.width}x${video.height}`);
		if (audio?.codec_name) parts.push(audio.codec_name.toUpperCase());
		if (audio?.channels) parts.push(`${audio.channels} ch`);
		if (subtitles.length > 0) parts.push(`${subtitles.length} subs`);
		return parts;
	}

	function formatCommand(job: Job): string {
		return (
			job.proposal?.processing?.arguments.join(' ') ??
			job.decision?.arguments.join(' ') ??
			'No processing command generated for this review.'
		);
	}

	function hardLinkWarning(job: Job): string | null {
		if (!job.filesystem?.is_hard_linked) return null;
		return 'Processing this item will create a new file and break the current hard-link relationship.';
	}

	function proposalWarnings(job: Job): string[] {
		return job.proposal?.warnings ?? [];
	}

	function allowedModes(job: Job): ReviewExecutionMode[] {
		const modes = job.proposal?.allowed_modes;
		return modes && modes.length > 0 ? modes : ['process_only'];
	}

	function commonGroupModes(group: Job[]): ReviewExecutionMode[] {
		const [first, ...rest] = group;
		if (!first) return [];
		return allowedModes(first).filter((mode) => rest.every((job) => allowedModes(job).includes(mode)));
	}

	function recommendationMode(proposal: ReviewProposal | null): ReviewExecutionMode | null {
		if (!proposal) return null;
		if (proposal.recommendation === 'full_plan') return 'full_plan';
		if (proposal.recommendation === 'organize') return 'organize_only';
		if (proposal.recommendation === 'process') return 'process_only';
		return null;
	}

	function recommendationLabel(proposal: ReviewProposal | null): string {
		if (!proposal) return 'none';
		if (proposal.recommendation === 're_source') return 're-source';
		return proposal.recommendation.replaceAll('_', ' ');
	}

	function canMarkReSource(job: Job): boolean {
		return job.proposal?.recommendation === 're_source';
	}

	function canMarkGroupReSource(group: Job[]): boolean {
		return group.length > 0 && group.every((job) => canMarkReSource(job));
	}

	function modeLabel(mode: ReviewExecutionMode): string {
		if (mode === 'full_plan') return 'Approve Full Plan';
		if (mode === 'organize_only') return 'Approve Organize Only';
		return 'Approve Process Only';
	}

	function busyLabel(mode: ReviewExecutionMode): string {
		if (mode === 'full_plan') return 'Approving Full Plan…';
		if (mode === 'organize_only') return 'Approving Organize Only…';
		return 'Approving Process Only…';
	}

	function groupBusyLabel(mode: ReviewExecutionMode): string {
		if (mode === 'full_plan') return 'Approving Bundle…';
		if (mode === 'organize_only') return 'Organizing Bundle…';
		return 'Approving Processing…';
	}

	function formatScope(scope: string | null | undefined): string {
		if (!scope) return 'file';
		if (scope === 'movie_folder') return 'movie folder';
		return scope.replaceAll('_', ' ');
	}

	function formatBytes(bytes: number): string {
		if (!bytes || bytes <= 0) return '0 B';
		const units = ['B', 'KB', 'MB', 'GB', 'TB'];
		let value = bytes;
		let index = 0;
		while (value >= 1024 && index < units.length - 1) {
			value /= 1024;
			index += 1;
		}
		return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
	}

	async function runAction(jobId: number, action: ReviewExecutionMode | 'reject' | 're_source' | 'keep_original') {
		actionBusy = { ...actionBusy, [jobId]: action };
		actionErrors = { ...actionErrors, [jobId]: '' };
		try {
			if (action === 'reject') {
				await rejectJob(jobId);
			} else if (action === 'keep_original') {
				await markJobKeepOriginal(jobId);
			} else if (action === 're_source') {
				await markJobReSource(jobId);
			} else {
				await approveJobMode(jobId, action);
			}
		} catch (error) {
			actionErrors = {
				...actionErrors,
				[jobId]: error instanceof Error ? error.message : 'Action failed'
			};
		} finally {
			actionBusy = { ...actionBusy, [jobId]: null };
		}
	}

	async function runGroupAction(
		group: Job[],
		action: `group:${ReviewExecutionMode}` | 'reject_group' | 're_source_group' | 'keep_original_group'
	) {
		if (group.length === 0) return;
		const lead = group[0];
		const nextBusy = { ...actionBusy };
		const nextErrors = { ...actionErrors };
		for (const job of group) {
			nextBusy[job.id] = action;
			nextErrors[job.id] = '';
		}
		actionBusy = nextBusy;
		actionErrors = nextErrors;

		try {
			if (action === 'reject_group') {
				await rejectJobGroup(lead.id);
			} else if (action === 'keep_original_group') {
				await markJobGroupKeepOriginal(lead.id);
			} else if (action === 're_source_group') {
				await markJobGroupReSource(lead.id);
			} else {
				await approveJobGroupMode(lead.id, action.replace('group:', '') as ReviewExecutionMode);
			}
		} catch (error) {
			const message = error instanceof Error ? error.message : 'Group action failed';
			actionErrors = Object.fromEntries(group.map((job) => [job.id, message]));
		} finally {
			actionBusy = {
				...actionBusy,
				...Object.fromEntries(group.map((job) => [job.id, null]))
			};
		}
	}

	function leadJob(group: Job[]): Job {
		return group[0];
	}

	function groupBusy(group: Job[]): boolean {
		return group.some((job) => actionBusy[job.id] !== null && actionBusy[job.id] !== undefined);
	}

	function groupError(group: Job[]): string {
		for (const job of group) {
			if (actionErrors[job.id]) return actionErrors[job.id];
		}
		return '';
	}

	function actionMatches(jobId: number, action: ReviewAction): boolean {
		return actionBusy[jobId] === action;
	}

	const jobs = $derived(jobStore.jobs);
	const loading = $derived(jobStore.loading);

	const reviewState = $derived(getReviewState());
	const reviewJobs = $derived(reviewState.awaitingApproval);
	const showGroups = $derived(reviewState.showGroups);
	const standaloneReviewJobs = $derived(reviewState.standaloneReviewJobs);
	const reviewItemCount = $derived(reviewState.counts.awaitingApprovalItems);
	const executionCounts = $derived(getExecutionState().counts);
</script>

<section class="mb-6 grid gap-4 lg:grid-cols-[minmax(0,1.15fr)_minmax(18rem,0.85fr)]">
	<div class="surface-card p-6">
		<p class="section-label">Review Queue</p>
		<h2 class="mt-2 text-3xl text-[color:var(--ink-strong)]">Approve plans before they execute</h2>
		<p class="mt-3 text-sm leading-6 text-[color:var(--ink-muted)]">
			This page is only for AI-generated plans that are waiting on a human decision. Backlog shaping happens on the root page. Active and completed execution belongs in Execution.
		</p>
		<div class="mt-5 flex flex-wrap gap-2">
			<a href="/" class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] no-underline">Backlog</a>
			<a href="/forge" class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] no-underline">Execution</a>
		</div>
	</div>

	<div class="surface-card p-6">
		<p class="section-label">Queue Split</p>
		<div class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-1 xl:grid-cols-4">
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="text-xs uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">TV Shows Awaiting Approval</div>
				<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{showGroups.length}</div>
			</div>
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="text-xs uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">Awaiting Approval</div>
				<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{reviewItemCount}</div>
			</div>
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="text-xs uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">Approved / Processing</div>
				<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{executionCounts.approved + executionCounts.processing}</div>
			</div>
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="text-xs uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">Failed</div>
				<div class="mt-2 text-2xl font-semibold text-[color:var(--danger)]">{executionCounts.failed}</div>
			</div>
		</div>
	</div>
</section>

{#if loading}
	<div class="surface-card px-8 py-16 text-center text-[color:var(--ink-muted)]">Loading review queue…</div>
{:else}
	<section class="mb-6">
		<div class="mb-3 flex items-center gap-3">
			<h2 class="text-lg text-[color:var(--ink-strong)]">Awaiting Approval</h2>
			<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)]">{reviewItemCount}</span>
		</div>

		{#if reviewItemCount === 0}
			<div class="surface-card border-dashed px-6 py-12 text-center">
				<p class="font-[family-name:var(--font-display)] text-xl text-[color:var(--ink-strong)]">No plans await approval</p>
				<p class="mt-2 text-sm text-[color:var(--ink-muted)]">Use the backlog page to create AI reviews for unprocessed files, then approve them here.</p>
			</div>
		{:else}
			{#if showGroups.length > 0}
				<div class="mb-6">
					<div class="mb-3 flex items-center gap-3">
						<h3 class="text-base text-[color:var(--ink-strong)]">TV Show Bundles</h3>
						<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)]">{showGroups.length}</span>
					</div>
					<div class="grid gap-4 xl:grid-cols-2">
						{#each showGroups as group (leadJob(group).group_key ?? leadJob(group).id)}
							{@const lead = leadJob(group)}
							<div class="triage-card">
								<div class="mb-3 flex items-center justify-between gap-2">
									<div>
										<span class="rounded-full bg-[color:rgba(164,79,45,0.1)] px-2.5 py-1 text-[11px] font-bold uppercase tracking-[0.16em] text-[color:var(--accent-deep)]">TV Show</span>
										<h3 class="mt-3 text-lg font-semibold text-[color:var(--ink-strong)]">{lead.group_label ?? fileName(lead.file_path)}</h3>
									</div>
									<span class="text-xs text-[color:var(--ink-muted)]">{group.length} episode plan{group.length === 1 ? '' : 's'}</span>
								</div>
								<p class="mb-4 text-sm text-[color:var(--ink-muted)]">Approve or reject the pending AI plans for this entire show in one step.</p>

								<div class="mb-3 rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.5)] p-3">
									<div class="section-label mb-2">Episodes in Bundle</div>
									<div class="space-y-2">
										{#each group.slice(0, 6) as job (job.id)}
											<div class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2">
												<div class="flex items-center justify-between gap-3">
													<div class="min-w-0">
														<div class="truncate text-sm font-semibold text-[color:var(--ink-strong)]">{fileName(job.file_path)}</div>
														<div class="mt-1 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{job.file_path}</div>
													</div>
													<div class="flex flex-wrap justify-end gap-1.5">
														{#each summarizeProbe(job.probe) as detail (detail)}
															<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel)] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-[color:var(--ink-strong)]">{detail}</span>
														{/each}
													</div>
												</div>
											</div>
										{/each}
										{#if group.length > 6}
											<p class="text-xs text-[color:var(--ink-muted)]">+{group.length - 6} more episode plan{group.length - 6 === 1 ? '' : 's'} in this show.</p>
										{/if}
									</div>
								</div>

								<div class="mb-4 rounded-lg border-l-[3px] border-[color:var(--accent-soft)] bg-[color:rgba(214,180,111,0.08)] px-3 py-2">
									<p class="mb-1 text-[11px] font-semibold uppercase tracking-[0.16em] text-[color:var(--accent-deep)]">Representative AI rationale</p>
									<p class="text-xs text-[color:var(--ink-muted)]">{lead.proposal?.processing?.rationale ?? lead.decision?.rationale ?? 'No AI rationale available.'}</p>
								</div>

								{#if lead.proposal}
									<div class="mb-4 rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.5)] p-3 text-xs text-[color:var(--ink-muted)]">
										<div class="section-label mb-2">Representative Review Proposal</div>
										<p>Scope: <span class="font-semibold text-[color:var(--ink-strong)]">{formatScope(lead.proposal.organization.scope)}</span></p>
										<p class="mt-2">Recommendation: <span class="font-semibold text-[color:var(--ink-strong)]">{recommendationLabel(lead.proposal)}</span></p>
										{#if lead.proposal.organization.organize_needed}
											<p class="mt-2">Organize target: <span class="font-mono text-[11px] text-[color:var(--ink-strong)]">{lead.proposal.organization.target_relative_path}</span></p>
										{/if}
										{#if lead.proposal.recommendation_reason}
											<p class="mt-2 text-[color:var(--accent-deep)]">{lead.proposal.recommendation_reason}</p>
										{/if}
									</div>
								{/if}

								{#each proposalWarnings(lead) as warning (warning)}
									<div class="mb-3 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
										{warning}
									</div>
								{/each}
								{#if proposalWarnings(lead).length === 0 && hardLinkWarning(lead)}
									<div class="mb-4 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
										{hardLinkWarning(lead)}
									</div>
								{/if}

								<details class="mb-4 rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.5)] px-3 py-2">
									<summary class="cursor-pointer text-xs font-semibold uppercase tracking-[0.14em] text-[color:var(--ink-muted)]">Representative Processing Plan</summary>
									<pre class="mt-2 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] text-[color:var(--ink-strong)]">{formatCommand(lead)}</pre>
								</details>

								{#if groupError(group)}
									<p class="mb-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{groupError(group)}</p>
								{/if}

								<div class="flex flex-wrap gap-2">
									<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-60" onclick={() => runGroupAction(group, 'keep_original_group')} disabled={groupBusy(group)}>
										{actionMatches(lead.id, 'keep_original_group') ? 'Keeping Original…' : 'Keep Original'}
									</button>
									{#if canMarkGroupReSource(group)}
										<button class="rounded-lg bg-[color:var(--olive)] px-3 py-2 text-xs font-semibold text-white disabled:opacity-60" onclick={() => runGroupAction(group, 're_source_group')} disabled={groupBusy(group)}>
											{actionMatches(lead.id, 're_source_group') ? 'Marking Re-source…' : 'Mark Bundle Re-source'}
										</button>
									{/if}
									{#each commonGroupModes(group) as mode (mode)}
										<button
											class={`rounded-lg px-3 py-2 text-xs font-semibold text-white disabled:opacity-60 ${recommendationMode(lead.proposal) === mode ? 'bg-[color:var(--accent)]' : 'bg-[color:var(--accent-deep)]'}`}
											onclick={() => runGroupAction(group, `group:${mode}`)}
											disabled={groupBusy(group)}
										>
											{actionMatches(lead.id, `group:${mode}`) ? groupBusyLabel(mode) : modeLabel(mode)}
										</button>
									{/each}
									<button class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-muted)] disabled:opacity-60" onclick={() => runGroupAction(group, 'reject_group')} disabled={groupBusy(group)}>
										{actionBusy[lead.id] === 'reject_group' ? 'Rejecting Plan…' : 'Reject Plan'}
									</button>
								</div>
							</div>
						{/each}
					</div>
				</div>
			{/if}

			{#if standaloneReviewJobs.length > 0}
				<div>
					<div class="mb-3 flex items-center gap-3">
						<h3 class="text-base text-[color:var(--ink-strong)]">Individual File Plans</h3>
						<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)]">{standaloneReviewJobs.length}</span>
					</div>
					<div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
						{#each standaloneReviewJobs as job (job.id)}
							<div class="triage-card">
								<div class="mb-3 flex items-center justify-between gap-2">
									<span class="rounded-full bg-[color:rgba(164,79,45,0.1)] px-2.5 py-1 text-[11px] font-bold uppercase tracking-[0.16em] text-[color:var(--accent-deep)]">AI Plan</span>
									<span class="text-xs text-[color:var(--ink-muted)]">#{job.id}</span>
								</div>
								<h3 class="mb-1 truncate text-sm font-semibold text-[color:var(--ink-strong)]">{fileName(job.file_path)}</h3>
								<p class="mb-4 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{job.file_path}</p>

								<div class="mb-3 rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.5)] p-3">
									<div class="section-label mb-2">Source Analysis</div>
									{#if job.probe}
										<div class="flex flex-wrap gap-2">
											{#each summarizeProbe(job.probe) as detail (detail)}
												<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.08em] text-[color:var(--ink-strong)]">{detail}</span>
											{/each}
										</div>
									{:else}
										<p class="text-xs text-[color:var(--ink-muted)]">Probe data is not available yet.</p>
									{/if}
								</div>

								<div class="mb-4 rounded-lg border-l-[3px] border-[color:var(--accent-soft)] bg-[color:rgba(214,180,111,0.08)] px-3 py-2">
									<p class="mb-1 text-[11px] font-semibold uppercase tracking-[0.16em] text-[color:var(--accent-deep)]">AI rationale</p>
									<p class="text-xs text-[color:var(--ink-muted)]">{job.proposal?.processing?.rationale ?? job.decision?.rationale ?? 'No AI rationale available.'}</p>
								</div>

								{#if job.proposal}
									<div class="mb-4 rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.5)] p-3 text-xs text-[color:var(--ink-muted)]">
										<div class="section-label mb-2">Review Proposal</div>
										<p>Scope: <span class="font-semibold text-[color:var(--ink-strong)]">{formatScope(job.proposal.organization.scope)}</span></p>
										<p class="mt-2">Recommendation: <span class="font-semibold text-[color:var(--ink-strong)]">{recommendationLabel(job.proposal)}</span></p>
										{#if job.proposal.organization.organize_needed}
											<p class="mt-2">Organize target: <span class="font-mono text-[11px] text-[color:var(--ink-strong)]">{job.proposal.organization.target_relative_path}</span></p>
										{/if}
										{#if job.proposal.recommendation_reason}
											<p class="mt-2 text-[color:var(--accent-deep)]">{job.proposal.recommendation_reason}</p>
										{/if}
										<p class="mt-2">Allowed modes: <span class="font-semibold text-[color:var(--ink-strong)]">{job.proposal.allowed_modes.map((mode) => mode.replaceAll('_', ' ')).join(', ')}</span></p>
									</div>
								{/if}

								{#each proposalWarnings(job) as warning (warning)}
									<div class="mb-3 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
										{warning}
									</div>
								{/each}
								{#if proposalWarnings(job).length === 0 && hardLinkWarning(job)}
									<div class="mb-4 rounded-lg border border-[color:rgba(164,79,45,0.22)] bg-[color:rgba(164,79,45,0.08)] px-3 py-2 text-xs text-[color:var(--accent-deep)]">
										{hardLinkWarning(job)}
									</div>
								{/if}

								<details class="mb-4 rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.5)] px-3 py-2">
									<summary class="cursor-pointer text-xs font-semibold uppercase tracking-[0.14em] text-[color:var(--ink-muted)]">Generated Processing Plan</summary>
									<pre class="mt-2 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] text-[color:var(--ink-strong)]">{formatCommand(job)}</pre>
								</details>

								{#if actionErrors[job.id]}
									<p class="mb-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{actionErrors[job.id]}</p>
								{/if}

								<div class="flex flex-wrap gap-2">
									<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)] disabled:opacity-60" onclick={() => runAction(job.id, 'keep_original')} disabled={actionBusy[job.id] !== null && actionBusy[job.id] !== undefined}>
										{actionMatches(job.id, 'keep_original') ? 'Keeping Original…' : 'Keep Original'}
									</button>
									{#if canMarkReSource(job)}
										<button class="rounded-lg bg-[color:var(--olive)] px-3 py-2 text-xs font-semibold text-white disabled:opacity-60" onclick={() => runAction(job.id, 're_source')} disabled={actionBusy[job.id] !== null && actionBusy[job.id] !== undefined}>
											{actionMatches(job.id, 're_source') ? 'Marking Re-source…' : 'Mark Re-source'}
										</button>
									{/if}
									{#each allowedModes(job) as mode (mode)}
										<button
											class={`rounded-lg px-3 py-2 text-xs font-semibold text-white disabled:opacity-60 ${recommendationMode(job.proposal) === mode ? 'bg-[color:var(--accent)]' : 'bg-[color:var(--accent-deep)]'}`}
											onclick={() => runAction(job.id, mode)}
											disabled={actionBusy[job.id] !== null && actionBusy[job.id] !== undefined}
										>
											{actionMatches(job.id, mode) ? busyLabel(mode) : modeLabel(mode)}
										</button>
									{/each}
									<button class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-muted)] disabled:opacity-60" onclick={() => runAction(job.id, 'reject')} disabled={actionBusy[job.id] !== null && actionBusy[job.id] !== undefined}>
										{actionBusy[job.id] === 'reject' ? 'Rejecting Plan…' : 'Reject Plan'}
									</button>
								</div>
							</div>
						{/each}
					</div>
				</div>
			{/if}
		{/if}
	</section>

{/if}

<style>
	.triage-card {
		border: 1px solid var(--line);
		background: var(--panel);
		backdrop-filter: blur(14px);
		border-radius: 1rem;
		padding: 1.25rem;
		transition: box-shadow 0.15s ease;
	}
	.triage-card:hover {
		box-shadow: 0 12px 40px rgba(101, 73, 44, 0.1);
	}
</style>
