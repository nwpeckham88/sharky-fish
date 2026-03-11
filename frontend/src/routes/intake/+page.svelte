<script lang="ts">
	import { approveJob, rejectJob, type Job, type MediaProbe } from '$lib/api';
	import { jobStore, progressStore } from '$lib/stores.svelte';

	let actionBusy = $state<Record<number, 'approve' | 'reject' | null>>({});
	let actionErrors = $state<Record<number, string>>({});

	function statusTone(status: string): string {
		switch (status) {
			case 'APPROVED': return 'processing';
			case 'COMPLETED': return 'completed';
			case 'FAILED': return 'failed';
			case 'REJECTED': return 'failed';
			case 'PROCESSING': return 'processing';
			default: return '';
		}
	}

	function fileName(path: string): string {
		return path.split('/').pop() ?? path;
	}

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
	const progress = progressStore;
	const loading = $derived(jobStore.loading);

	const reviewJobs = $derived(jobs.filter((j) => j.status === 'AWAITING_APPROVAL'));
	const approvedJobs = $derived(jobs.filter((j) => j.status === 'APPROVED'));
	const processingJobs = $derived(jobs.filter((j) => j.status === 'PROCESSING'));
	const recentCompleted = $derived(
		jobs.filter((j) => j.status === 'COMPLETED' || j.status === 'FAILED' || j.status === 'REJECTED').slice(0, 8)
	);
</script>

<div class="mb-5">
	<p class="text-sm leading-6 text-[color:var(--ink-muted)]">
		New files land here with an AI-generated transcode plan, source analysis, and rationale. Manual approval and rejection stay available as operator overrides.
	</p>
</div>

{#if loading}
	<div class="surface-card px-8 py-16 text-center text-[color:var(--ink-muted)]">Loading intake queue…</div>
{:else}
	<!-- Pending Triage -->
	<section class="mb-6">
		<div class="mb-3 flex items-center gap-3">
			<h2 class="text-lg text-[color:var(--ink-strong)]">AI Review</h2>
			<span class="rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--accent-deep)]">{reviewJobs.length}</span>
		</div>

		{#if reviewJobs.length === 0}
			<div class="surface-card border-dashed px-6 py-12 text-center">
				<p class="font-[family-name:var(--font-display)] text-xl text-[color:var(--ink-strong)]">No AI plans awaiting review</p>
				<p class="mt-2 text-sm text-[color:var(--ink-muted)]">Drop media files into the ingest directory. Once probed and scored, they will appear here with an AI-generated plan.</p>
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
								{actionBusy[job.id] === 'approve' ? 'Approving…' : 'Approve AI plan'}
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

	{#if approvedJobs.length > 0}
		<section class="mb-6">
			<div class="mb-3 flex items-center gap-3">
				<h2 class="text-lg text-[color:var(--ink-strong)]">Ready For Forge</h2>
				<span class="status-chip processing">{approvedJobs.length} queued</span>
			</div>
			<div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
				{#each approvedJobs as job (job.id)}
					<div class="triage-card">
						<div class="mb-2 flex items-center justify-between gap-2">
							<span class="status-chip processing">Approved</span>
							<span class="text-xs text-[color:var(--ink-muted)]">#{job.id}</span>
						</div>
						<h3 class="mb-1 truncate text-sm font-semibold text-[color:var(--ink-strong)]">{fileName(job.file_path)}</h3>
						<p class="mb-3 text-xs text-[color:var(--ink-muted)]">{job.decision?.rationale ?? 'Queued with the AI-generated plan.'}</p>
						<button class="rounded-lg border border-[color:var(--line)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-muted)] disabled:opacity-60" onclick={() => runAction(job.id, 'reject')} disabled={actionBusy[job.id] !== null && actionBusy[job.id] !== undefined}>
							{actionBusy[job.id] === 'reject' ? 'Rejecting…' : 'Remove from queue'}
						</button>
					</div>
				{/each}
			</div>
		</section>
	{/if}

	<!-- Currently Processing -->
	{#if processingJobs.length > 0}
		<section class="mb-6">
			<div class="mb-3 flex items-center gap-3">
				<h2 class="text-lg text-[color:var(--ink-strong)]">Processing</h2>
				<span class="status-chip processing">{processingJobs.length} active</span>
			</div>
			<div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
				{#each processingJobs as job (job.id)}
					{@const p = progress[job.id]}
					<div class="triage-card">
						<div class="mb-2 flex items-center justify-between gap-2">
							<span class="status-chip processing">Processing</span>
							<span class="text-xs text-[color:var(--ink-muted)]">#{job.id}</span>
						</div>
						<h3 class="mb-1 truncate text-sm font-semibold text-[color:var(--ink-strong)]">{fileName(job.file_path)}</h3>
						{#if p}
							<div class="mt-3 flex items-center justify-between text-xs text-[color:var(--ink-muted)]">
								<span>{p.percent != null ? `${p.percent.toFixed(1)}%` : '…'}</span>
								<span>{p.speed ?? ''}{#if p.fps} · {p.fps.toFixed(1)} fps{/if}</span>
							</div>
							<div class="mt-2 h-1.5 overflow-hidden rounded-full bg-[color:var(--paper-deep)]">
								<div class="h-full rounded-full bg-[linear-gradient(90deg,var(--accent),var(--accent-soft),var(--olive))] transition-all duration-300" style="width: {p.percent ?? 0}%"></div>
							</div>
						{:else}
							<p class="mt-3 text-xs text-[color:var(--ink-muted)]">Awaiting progress data from FFmpeg…</p>
						{/if}
					</div>
				{/each}
			</div>
		</section>
	{/if}

	<!-- Recently Completed -->
	{#if recentCompleted.length > 0}
		<section>
			<div class="mb-3 flex items-center gap-3">
				<h2 class="text-lg text-[color:var(--ink-strong)]">Recently Completed</h2>
			</div>
			<div class="overflow-hidden rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)]">
				<table class="w-full text-left text-sm">
					<thead class="border-b border-[color:var(--line)] bg-[color:rgba(234,223,201,0.6)] text-xs uppercase tracking-[0.18em] text-[color:var(--ink-muted)]">
						<tr>
							<th class="px-4 py-3">File</th>
							<th class="px-4 py-3">Status</th>
							<th class="px-4 py-3">Created</th>
						</tr>
					</thead>
					<tbody>
						{#each recentCompleted as job (job.id)}
							<tr class="border-b border-[color:rgba(123,105,81,0.14)] last:border-b-0">
								<td class="max-w-xs truncate px-4 py-3 font-mono text-[13px] text-[color:var(--ink-strong)]">{fileName(job.file_path)}</td>
								<td class="px-4 py-3"><span class="status-chip {statusTone(job.status)}">{job.status}</span></td>
								<td class="px-4 py-3 text-[color:var(--ink-muted)]">{new Date(job.created_at).toLocaleString()}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</section>
	{/if}
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
