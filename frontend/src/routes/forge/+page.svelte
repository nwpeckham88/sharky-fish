<script lang="ts">
	import { fetchJob, type Task } from '$lib/api';
	import { getExecutionState, jobStore, progressStore } from '$lib/stores.svelte';
	import { fileName, statusTone } from '$lib/status';

	let selectedJobId = $state<number | null>(null);
	let selectedTasks = $state<Task[]>([]);
	let tasksLoading = $state(false);
	let statusFilter = $state('all');

	async function loadTasks(jobId: number) {
		selectedJobId = jobId;
		selectedTasks = [];
		tasksLoading = true;
		try {
			const result = await fetchJob(jobId);
			selectedTasks = result.tasks;
		} catch {
			selectedTasks = [];
		} finally {
			tasksLoading = false;
		}
	}

	const jobs = $derived(jobStore.jobs);
	const loading = $derived(jobStore.loading);
	const progress = progressStore;
	const executionState = $derived(getExecutionState());

	const activeProgress = $derived(Object.values(progress));
	const filteredJobs = $derived(
		statusFilter === 'all' ? executionState.jobs : executionState.jobs.filter((job) => job.status === statusFilter)
	);
	const jobCounts = $derived(executionState.counts);
	const failedJobs = $derived(executionState.failed.slice(0, 5));
</script>

<div class="mb-5">
	<p class="text-sm leading-6 text-[color:var(--ink-muted)]">
		Execution is the operational view: approved work, active transcodes, failures, and task pipeline detail.
	</p>
</div>

<!-- Stats -->
<section class="mb-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
	<div class="stat-card"><div class="section-label">Approved</div><div class="mt-1 text-2xl font-semibold text-[color:var(--accent)]">{jobCounts.approved}</div></div>
	<div class="stat-card"><div class="section-label">Processing</div><div class="mt-1 text-2xl font-semibold text-[color:var(--accent)]">{jobCounts.processing}</div></div>
	<div class="stat-card"><div class="section-label">Completed</div><div class="mt-1 text-2xl font-semibold text-[color:var(--olive)]">{jobCounts.completed}</div></div>
	<div class="stat-card"><div class="section-label">Failed</div><div class="mt-1 text-2xl font-semibold text-[color:var(--danger)]">{jobCounts.failed}</div></div>
</section>

{#if failedJobs.length > 0}
	<section class="mb-5 surface-card p-5">
		<div class="mb-4 flex items-center justify-between gap-3">
			<div>
				<p class="section-label">Failure Surface</p>
				<p class="text-lg text-[color:var(--ink-strong)]">Execution exceptions needing follow-up</p>
			</div>
			<span class="status-chip failed">{failedJobs.length} recent</span>
		</div>
		<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
			{#each failedJobs as job (job.id)}
				<div class="rounded-[1rem] border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] p-4">
					<div class="flex items-center justify-between gap-2">
						<span class="status-chip failed">Failed</span>
						<span class="text-xs text-[color:var(--ink-muted)]">#{job.id}</span>
					</div>
					<div class="mt-3 truncate font-semibold text-[color:var(--ink-strong)]">{fileName(job.file_path)}</div>
					<div class="mt-1 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{job.file_path}</div>
					<div class="mt-3 text-xs text-[color:var(--ink-muted)]">Created {new Date(job.created_at).toLocaleString()}</div>
				</div>
			{/each}
		</div>
	</section>
{/if}

<!-- Active Transcodes -->
{#if activeProgress.length > 0}
	<section class="mb-5">
		<h2 class="mb-3 text-lg text-[color:var(--ink-strong)]">Active Workers</h2>
		<div class="grid gap-4 md:grid-cols-2">
			{#each activeProgress as p (p.job_id)}
				{@const job = jobs.find((j) => j.id === p.job_id)}
				<div class="surface-card p-5">
					<div class="mb-3 flex items-center justify-between gap-3">
						<div class="min-w-0">
							<div class="truncate text-sm font-semibold text-[color:var(--ink-strong)]">{job ? fileName(job.file_path) : `Job #${p.job_id}`}</div>
							{#if job}<div class="mt-0.5 truncate font-mono text-[11px] text-[color:var(--ink-muted)]">{job.file_path}</div>{/if}
						</div>
						<div class="text-right">
							<div class="text-lg font-semibold text-[color:var(--accent-deep)]">{p.percent != null ? `${p.percent.toFixed(1)}%` : '…'}</div>
						</div>
					</div>

					<!-- Two-segment progress bar -->
					<div class="h-2.5 overflow-hidden rounded-full bg-[color:var(--paper-deep)]">
						<div class="h-full rounded-full bg-[linear-gradient(90deg,var(--accent),var(--accent-soft),var(--olive))] transition-all duration-300" style="width: {p.percent ?? 0}%"></div>
					</div>

					<div class="mt-2 flex items-center justify-between text-xs text-[color:var(--ink-muted)]">
						<span>{p.speed ?? 'Measuring…'}{#if p.fps} · {p.fps.toFixed(1)} fps{/if}</span>
						{#if p.frame}<span>Frame {p.frame}</span>{/if}
					</div>
				</div>
			{/each}
		</div>
	</section>
{/if}

<!-- Job Queue + Task Detail -->
<section class="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(20rem,0.7fr)]">
	<!-- Job list -->
	<div>
		<div class="mb-3 flex items-center justify-between gap-3">
			<h2 class="text-lg text-[color:var(--ink-strong)]">Execution Queue</h2>
			<div class="flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1">
				{#each ['all', 'APPROVED', 'PROCESSING', 'COMPLETED', 'FAILED'] as s (s)}
					<button class="rounded-lg px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.1em] transition-colors {statusFilter === s ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { statusFilter = s; }}>{s === 'all' ? 'All' : s === 'APPROVED' ? 'Approved' : s === 'PROCESSING' ? 'Live' : s === 'COMPLETED' ? 'Done' : 'Failed'}</button>
				{/each}
			</div>
		</div>

		{#if loading}
			<div class="surface-card px-6 py-14 text-center text-[color:var(--ink-muted)]">Loading queue…</div>
		{:else if filteredJobs.length === 0}
			<div class="surface-card border-dashed px-6 py-14 text-center">
				<p class="font-[family-name:var(--font-display)] text-xl text-[color:var(--ink-strong)]">No jobs</p>
				<p class="mt-2 text-sm text-[color:var(--ink-muted)]">{statusFilter === 'all' ? 'No jobs in the pipeline yet.' : `No ${statusFilter.toLowerCase()} jobs.`}</p>
			</div>
		{:else}
			<div class="overflow-hidden rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(255,248,237,0.72)]">
				<table class="w-full text-left text-sm">
					<thead class="border-b border-[color:var(--line)] bg-[color:rgba(234,223,201,0.6)] text-xs uppercase tracking-[0.18em] text-[color:var(--ink-muted)]">
						<tr>
							<th class="px-4 py-3">ID</th>
							<th class="px-4 py-3">File</th>
							<th class="px-4 py-3">Status</th>
							<th class="px-4 py-3">Created</th>
						</tr>
					</thead>
					<tbody>
						{#each filteredJobs as job (job.id)}
							{@const isSelected = selectedJobId === job.id}
							<tr class="cursor-pointer border-b border-[color:rgba(123,105,81,0.14)] last:border-b-0 hover:bg-[color:rgba(214,180,111,0.08)] {isSelected ? 'bg-[color:rgba(214,180,111,0.12)]' : ''}" onclick={() => loadTasks(job.id)}>
								<td class="px-4 py-3 font-mono text-[color:var(--ink-muted)]">#{job.id}</td>
								<td class="max-w-xs truncate px-4 py-3 font-mono text-[13px] text-[color:var(--ink-strong)]">{fileName(job.file_path)}</td>
								<td class="px-4 py-3"><span class="status-chip {statusTone(job.status)}">{job.status}</span></td>
								<td class="px-4 py-3 text-[color:var(--ink-muted)]">{new Date(job.created_at).toLocaleString()}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>

	<!-- Task Pipeline Detail -->
	<div class="rounded-[1rem] border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-5">
		<p class="section-label mb-3">Task Pipeline</p>
		{#if !selectedJobId}
			<div class="rounded-lg border border-dashed border-[color:var(--line)] px-5 py-8 text-center text-sm text-[color:var(--ink-muted)]">
				Select a job to view its multi-stage task pipeline.
			</div>
		{:else if tasksLoading}
			<div class="rounded-lg border border-[color:var(--line)] px-5 py-8 text-center text-sm text-[color:var(--ink-muted)]">
				Loading tasks for Job #{selectedJobId}…
			</div>
		{:else if selectedTasks.length === 0}
			<div class="rounded-lg border border-[color:var(--line)] px-5 py-6 text-sm text-[color:var(--ink-muted)]">
				No tasks found for Job #{selectedJobId}.
			</div>
		{:else}
			<div class="space-y-2">
				{#each selectedTasks as task (task.id)}
					<div class="rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.5)] p-4">
						<div class="flex items-center justify-between gap-3">
							<div class="flex items-center gap-2">
								<span class="flex h-6 w-6 items-center justify-center rounded-full bg-[color:var(--paper-deep)] text-xs font-bold text-[color:var(--ink-muted)]">{task.step_order}</span>
								<span class="text-sm font-semibold text-[color:var(--ink-strong)]">{task.task_type}</span>
							</div>
							<span class="status-chip {statusTone(task.status)}">{task.status}</span>
						</div>
						{#if task.payload}
							<details class="mt-2">
								<summary class="cursor-pointer text-xs text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]">View payload</summary>
								<pre class="mt-2 max-h-32 overflow-auto rounded-lg bg-[color:var(--paper-deep)] p-3 font-mono text-[11px] text-[color:var(--ink-strong)]">{task.payload}</pre>
							</details>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	</div>
</section>
