<script lang="ts">
	import { onMount } from 'svelte';
	import { fetchConfig, saveConfig, type AppConfig } from '$lib/api';

	let activeTab = $state<'standards' | 'prompt' | 'system'>('standards');
	let config = $state<AppConfig | null>(null);
	let loading = $state(true);
	let saving = $state(false);
	let toast = $state<{ msg: string; ok: boolean } | null>(null);

	onMount(async () => {
		try {
			config = await fetchConfig();
		} catch (e) {
			toast = { msg: 'Failed to load configuration', ok: false };
		} finally {
			loading = false;
		}
	});

	async function save() {
		if (!config) return;
		saving = true;
		toast = null;
		try {
			config = await saveConfig(config);
			toast = { msg: 'Configuration saved', ok: true };
		} catch (e) {
			toast = { msg: 'Failed to save configuration', ok: false };
		} finally {
			saving = false;
			setTimeout(() => { toast = null; }, 3000);
		}
	}

	const CODECS = [
		{ value: 'h264', label: 'H.264 (libx264 / nvenc)' },
		{ value: 'h265', label: 'H.265 (libx265 / nvenc)' },
		{ value: 'av1',  label: 'AV1 (libaom / svt-av1)' },
		{ value: 'vp9',  label: 'VP9' },
	];

	const RESOLUTIONS = [
		{ value: 'none',  label: 'No limit' },
		{ value: '4k',    label: '3840×2160 (4K)' },
		{ value: '1080p', label: '1920×1080 (1080p)' },
		{ value: '720p',  label: '1280×720 (720p)' },
	];

	const CHANNELS = [
		{ value: 'none',   label: 'No limit' },
		{ value: '7.1',    label: '7.1' },
		{ value: '5.1',    label: '5.1' },
		{ value: 'stereo', label: 'Stereo' },
	];

	const LLM_PROVIDERS = [
		{ value: 'ollama', label: 'Ollama (local)' },
		{ value: 'openai', label: 'OpenAI API' },
	];
</script>

<div class="mb-5">
	<p class="text-sm leading-6 text-[color:var(--ink-muted)]">
		Define encoding rules, tune the LLM system prompt, and review the host hardware profile.
	</p>
</div>

<!-- Toast -->
{#if toast}
	<div class="mb-4 rounded-lg px-4 py-2.5 text-sm font-semibold {toast.ok ? 'bg-[color:var(--olive)]/15 text-[color:var(--olive)]' : 'bg-red-500/15 text-red-400'}">
		{toast.msg}
	</div>
{/if}

{#if loading}
	<div class="surface-card p-12 text-center text-sm text-[color:var(--ink-muted)]">Loading configuration…</div>
{:else if config}

<!-- Tab Bar -->
<div class="mb-5 flex gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1 w-fit">
	{#each [['standards', 'Golden Standards'], ['prompt', 'Prompt Playground'], ['system', 'System Profile']] as [key, label] (key)}
		<button class="rounded-lg px-4 py-2 text-sm font-semibold transition-colors {activeTab === key ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { activeTab = key as typeof activeTab; }}>{label}</button>
	{/each}
</div>

{#if activeTab === 'standards'}
	<section class="surface-card p-6">
		<h2 class="mb-4 text-xl text-[color:var(--ink-strong)]">Golden Standards</h2>
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">Define the target encoding profile that the LLM must respect. Files deviating from these standards will be flagged during library audits.</p>

		<div class="grid gap-5 md:grid-cols-2">
			<!-- Video Rules -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">Video</h3>
				<div class="space-y-4">
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Default Codec</span>
						<select bind:value={config.golden_standards.video.codec} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
							{#each CODECS as { value, label }}
								<option {value}>{label}</option>
							{/each}
						</select>
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Max Bitrate (Mbps)</span>
						<input type="number" bind:value={config.golden_standards.video.max_bitrate_mbps} min="1" step="1" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Resolution Ceiling</span>
						<select bind:value={config.golden_standards.video.resolution_ceiling} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
							{#each RESOLUTIONS as { value, label }}
								<option {value}>{label}</option>
							{/each}
						</select>
					</label>
				</div>
			</div>

			<!-- Audio Rules -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">Audio</h3>
				<div class="space-y-4">
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Target Integrated Loudness (LUFS)</span>
						<input type="range" min="-31" max="-14" bind:value={config.golden_standards.audio.target_lufs} class="w-full" />
						<div class="mt-1 flex justify-between text-xs text-[color:var(--ink-muted)]"><span>-31 (cinematic)</span><span>{config.golden_standards.audio.target_lufs} LUFS</span><span>-14 (desktop)</span></div>
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Target True Peak (dBTP)</span>
						<input type="number" bind:value={config.golden_standards.audio.target_true_peak} step="0.1" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Max Channels</span>
						<select bind:value={config.golden_standards.audio.max_channels} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
							{#each CHANNELS as { value, label }}
								<option {value}>{label}</option>
							{/each}
						</select>
					</label>
				</div>
			</div>
		</div>

		<div class="mt-5 flex justify-end">
			<button onclick={save} disabled={saving} class="rounded-lg bg-[color:var(--accent)] px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50">{saving ? 'Saving…' : 'Save Standards'}</button>
		</div>
	</section>

{:else if activeTab === 'prompt'}
	<section class="surface-card p-6">
		<h2 class="mb-4 text-xl text-[color:var(--ink-strong)]">Prompt Playground</h2>
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">Define the system prompt sent to the LLM. Test it with sample media before applying to your library.</p>

		<div class="grid gap-5 xl:grid-cols-2">
			<!-- System Prompt -->
			<label class="block">
				<span class="mb-2 block text-xs font-semibold uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">System Prompt</span>
				<textarea bind:value={config.system_prompt} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-4 font-mono text-sm text-[color:var(--ink-strong)] leading-relaxed" rows="16" placeholder="You are an expert media processing engine…"></textarea>
			</label>

			<!-- Dry Run Result -->
			<div>
				<span class="mb-2 block text-xs font-semibold uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">Dry Run Output</span>
				<div class="rounded-lg border border-[color:var(--line)] bg-[color:var(--paper-deep)] p-4 min-h-[16rem]">
					<p class="text-sm text-[color:var(--ink-muted)]">Select a file from the library and click "Dry Run" to test the prompt against an actual media file without executing any transcoding.</p>
				</div>
				<div class="mt-3 flex gap-2">
					<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)]">Select File…</button>
					<button class="rounded-lg bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white">Dry Run</button>
				</div>
			</div>
		</div>

		<div class="mt-5 flex justify-end">
			<button onclick={save} disabled={saving} class="rounded-lg bg-[color:var(--accent)] px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50">{saving ? 'Saving…' : 'Save Prompt'}</button>
		</div>
	</section>

{:else if activeTab === 'system'}
	<section class="surface-card p-6">
		<h2 class="mb-4 text-xl text-[color:var(--ink-strong)]">System Profile</h2>
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">Configure the LLM connection, tune performance parameters, and review storage paths.</p>

		<div class="grid gap-5 md:grid-cols-2">
			<!-- LLM Configuration -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">LLM Endpoint</h3>
				<div class="space-y-4">
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Provider</span>
						<select bind:value={config.llm.provider} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
							{#each LLM_PROVIDERS as { value, label }}
								<option {value}>{label}</option>
							{/each}
						</select>
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Base URL</span>
						<input type="text" bind:value={config.llm.base_url} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Model</span>
						<input type="text" bind:value={config.llm.model} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">API Key {config.llm.provider === 'ollama' ? '(not required for Ollama)' : ''}</span>
						<input type="password" value={config.llm.api_key ?? ''} oninput={(e) => { if (config) config.llm.api_key = (e.target as HTMLInputElement).value || null; }} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
				</div>
			</div>

			<!-- Performance -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">Performance</h3>
				<div class="space-y-4">
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Max I/O Concurrency</span>
						<input type="number" bind:value={config.max_io_concurrency} min="1" max="32" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" />
						<span class="mt-1 block text-xs text-[color:var(--ink-muted)]">Simultaneous ffprobe / read operations on the storage array</span>
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Metadata Prewarm Limit</span>
						<input type="number" bind:value={config.metadata_prewarm_limit} min="0" max="5000" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" />
						<span class="mt-1 block text-xs text-[color:var(--ink-muted)]">Number of recent items to probe on startup</span>
					</label>
				</div>
			</div>

			<!-- Storage (read-only, set via container volumes) -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">Storage <span class="text-xs font-normal text-[color:var(--ink-muted)]">(set via container volumes)</span></h3>
				<div class="space-y-2 text-sm">
					<div class="flex justify-between"><span class="text-[color:var(--ink-muted)]">Library</span><span class="font-mono text-[color:var(--ink-strong)]">{config.data_path}</span></div>
					<div class="flex justify-between"><span class="text-[color:var(--ink-muted)]">Ingest</span><span class="font-mono text-[color:var(--ink-strong)]">{config.ingest_path}</span></div>
					<div class="flex justify-between"><span class="text-[color:var(--ink-muted)]">Config</span><span class="font-mono text-[color:var(--ink-strong)]">{config.config_path}</span></div>
				</div>
			</div>

			<!-- Server (read-only) -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">Server <span class="text-xs font-normal text-[color:var(--ink-muted)]">(requires restart)</span></h3>
				<div class="space-y-2 text-sm">
					<div class="flex justify-between"><span class="text-[color:var(--ink-muted)]">HTTP Port</span><span class="font-mono text-[color:var(--ink-strong)]">{config.port}</span></div>
				</div>
			</div>
		</div>

		<div class="mt-5 flex justify-end">
			<button onclick={save} disabled={saving} class="rounded-lg bg-[color:var(--accent)] px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50">{saving ? 'Saving…' : 'Save Settings'}</button>
		</div>
	</section>
{/if}

{/if}
