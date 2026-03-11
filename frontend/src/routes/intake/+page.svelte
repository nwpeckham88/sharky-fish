<script lang="ts">
	import { approveJob, rejectJob, type Job, type MediaProbe } from '$lib/api';
	import { getExecutionState, jobStore, getReviewState } from '$lib/stores.svelte';
	import { fileName } from '$lib/status';

	let actionBusy = $state<Record<number, 'approve' | 'reject' | null>>({});
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
		return job.decision?.arguments.join(' ') ?? 'No AI command generated yet.';
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

	async function runAction(jobId: number, action: 'approve' | 'reject') {
		actionBusy = { ...actionBusy, [jobId]: action };
		actionErrors = { ...actionErrors, [jobId]: '' };
		try {
			if (action === 'approve') {
				await approveJob(jobId);
			} else {
				await rejectJob(jobId);
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

	const jobs = $derived(jobStore.jobs);
	const loading = $derived(jobStore.loading);

	const reviewJobs = $derived(getReviewState().awaitingApproval);
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
		<div class="mt-4 grid gap-3 sm:grid-cols-3 lg:grid-cols-1 xl:grid-cols-3">
			<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4">
				<div class="text-xs uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">Awaiting Approval</div>
				<div class="mt-2 text-2xl font-semibold text-[color:var(--ink-strong)]">{reviewJobs.length}</div>
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
			<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)]">{reviewJobs.length}</span>
		</div>

		{#if reviewJobs.length === 0}
			<div class="surface-card border-dashed px-6 py-12 text-center">
				<p class="font-[family-name:var(--font-display)] text-xl text-[color:var(--ink-strong)]">No plans await approval</p>
				<p class="mt-2 text-sm text-[color:var(--ink-muted)]">Use the backlog page to create AI reviews for unprocessed files, then approve them here.</p>
			</div>
		{:else}
			<div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
				{#each reviewJobs as job (job.id)}
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
							<p class="text-xs text-[color:var(--ink-muted)]">{job.decision?.rationale ?? 'No AI rationale available.'}</p>
						</div>

						<details class="mb-4 rounded-lg border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.5)] px-3 py-2">
							<summary class="cursor-pointer text-xs font-semibold uppercase tracking-[0.14em] text-[color:var(--ink-muted)]">Generated FFmpeg plan</summary>
							<pre class="mt-2 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] text-[color:var(--ink-strong)]">{formatCommand(job)}</pre>
						</details>

						{#if actionErrors[job.id]}
							<p class="mb-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-3 py-2 text-xs text-[color:var(--danger)]">{actionErrors[job.id]}</p>
						{/if}

						<div class="flex gap-2">
							<button class="flex-1 rounded-lg bg-[color:var(--accent)] px-3 py-2 text-xs font-semibold text-white disabled:opacity-60" onclick={() => runAction(job.id, 'approve')} disabled={actionBusy[job.id] !== null && actionBusy[job.id] !== undefined}>
								{actionBusy[job.id] === 'approve' ? 'Approving…' : 'Approve'}
							</button>
							<button class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-muted)] disabled:opacity-60" onclick={() => runAction(job.id, 'reject')} disabled={actionBusy[job.id] !== null && actionBusy[job.id] !== undefined}>
								{actionBusy[job.id] === 'reject' ? 'Rejecting…' : 'Reject'}
							</button>
						</div>
					</div>
				{/each}
			</div>
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
