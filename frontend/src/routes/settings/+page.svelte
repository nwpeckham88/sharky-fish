<script lang="ts">
	import { onMount } from 'svelte';
	import {
		fetchConfig,
		saveConfig,
		fetchLibraries,
		addLibrary,
		removeLibrary,
		type AppConfig,
		type LibraryFolder
	} from '$lib/api';

	let activeTab = $state<'libraries' | 'standards' | 'prompt' | 'system'>('libraries');
	let config = $state<AppConfig | null>(null);
	let loading = $state(true);
	let saving = $state(false);
	let toast = $state<{ msg: string; ok: boolean } | null>(null);

	// Library folder management
	let libraries = $state<LibraryFolder[]>([]);
	let newLib = $state<LibraryFolder>({ id: '', name: '', path: '', media_type: 'movie' });
	let addingLibrary = $state(false);
	let libraryError = $state('');

	onMount(async () => {
		try {
			const [cfg, libs] = await Promise.all([fetchConfig(), fetchLibraries()]);
			config = cfg;
			libraries = libs;
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

	const SUBTITLE_MODES = [
		{ value: 'keep_all',        label: 'Keep all subtitles' },
		{ value: 'keep_preferred',  label: 'Keep preferred languages only' },
		{ value: 'keep_forced_only', label: 'Keep forced subs in preferred languages' },
		{ value: 'remove_all',     label: 'Remove all subtitles' },
	];

	const COMMON_LANGUAGES = [
		{ code: 'eng', label: 'English' },
		{ code: 'spa', label: 'Spanish' },
		{ code: 'fra', label: 'French' },
		{ code: 'deu', label: 'German' },
		{ code: 'ita', label: 'Italian' },
		{ code: 'por', label: 'Portuguese' },
		{ code: 'rus', label: 'Russian' },
		{ code: 'jpn', label: 'Japanese' },
		{ code: 'kor', label: 'Korean' },
		{ code: 'zho', label: 'Chinese' },
		{ code: 'ara', label: 'Arabic' },
		{ code: 'hin', label: 'Hindi' },
		{ code: 'nld', label: 'Dutch' },
		{ code: 'pol', label: 'Polish' },
		{ code: 'swe', label: 'Swedish' },
		{ code: 'nor', label: 'Norwegian' },
		{ code: 'dan', label: 'Danish' },
		{ code: 'fin', label: 'Finnish' },
	];

	let customLangInput = $state('');

	function toggleLanguage(code: string) {
		if (!config) return;
		const idx = config.golden_standards.subtitle.preferred_languages.indexOf(code);
		if (idx >= 0) {
			config.golden_standards.subtitle.preferred_languages.splice(idx, 1);
			config.golden_standards.subtitle.preferred_languages = [...config.golden_standards.subtitle.preferred_languages];
		} else {
			config.golden_standards.subtitle.preferred_languages = [...config.golden_standards.subtitle.preferred_languages, code];
		}
	}

	function addCustomLanguage() {
		if (!config) return;
		const code = customLangInput.trim().toLowerCase();
		if (code.length >= 2 && code.length <= 3 && !config.golden_standards.subtitle.preferred_languages.includes(code)) {
			config.golden_standards.subtitle.preferred_languages = [...config.golden_standards.subtitle.preferred_languages, code];
			customLangInput = '';
		}
	}

	const LLM_PROVIDERS = [
		{ value: 'ollama', label: 'Ollama (local)' },
		{ value: 'openai', label: 'OpenAI API' },
	];

	function generateLibraryId(name: string): string {
		return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
	}

	function handleLibNameInput(e: Event) {
		const value = (e.currentTarget as HTMLInputElement).value;
		newLib.name = value;
		if (!newLib.id || newLib.id === generateLibraryId(newLib.name.slice(0, -1) || '')) {
			newLib.id = generateLibraryId(value);
		}
	}

	async function handleAddLibrary() {
		libraryError = '';
		if (!newLib.name.trim() || !newLib.path.trim()) {
			libraryError = 'Name and path are required';
			return;
		}
		if (!newLib.id.trim()) {
			newLib.id = generateLibraryId(newLib.name);
		}
		addingLibrary = true;
		try {
			const added = await addLibrary(newLib);
			libraries = [...libraries, added];
			newLib = { id: '', name: '', path: '', media_type: 'movie' };
			toast = { msg: 'Library added', ok: true };
			setTimeout(() => { toast = null; }, 3000);
		} catch (e) {
			libraryError = e instanceof Error ? e.message : 'Failed to add library';
		} finally {
			addingLibrary = false;
		}
	}

	async function handleRemoveLibrary(id: string) {
		try {
			await removeLibrary(id);
			libraries = libraries.filter((l) => l.id !== id);
			toast = { msg: 'Library removed', ok: true };
			setTimeout(() => { toast = null; }, 3000);
		} catch (e) {
			toast = { msg: e instanceof Error ? e.message : 'Failed to remove library', ok: false };
			setTimeout(() => { toast = null; }, 3000);
		}
	}
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
	{#each [['libraries', 'Libraries'], ['standards', 'Golden Standards'], ['prompt', 'Prompt Playground'], ['system', 'System Profile']] as [key, label] (key)}
		<button class="rounded-lg px-4 py-2 text-sm font-semibold transition-colors {activeTab === key ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { activeTab = key as typeof activeTab; }}>{label}</button>
	{/each}
</div>

{#if activeTab === 'libraries'}
	<section class="surface-card p-6">
		<h2 class="mb-4 text-xl text-[color:var(--ink-strong)]">Library Folders</h2>
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">Map folders inside your media volume to named libraries. Similar to Jellyfin, bind-mount your entire media directory and register sub-folders here as Movies, TV Shows, etc.</p>

		<!-- Existing libraries -->
		{#if libraries.length > 0}
			<div class="mb-6 space-y-2">
				{#each libraries as lib (lib.id)}
					<div class="flex items-center justify-between rounded-xl border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.6)] px-4 py-3">
						<div class="flex items-center gap-4">
							<div class="flex h-9 w-9 items-center justify-center rounded-lg bg-[color:var(--accent)]/15">
								{#if lib.media_type === 'movie'}
									<svg class="h-5 w-5 text-[color:var(--accent-deep)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><path d="M7 2v20M17 2v20M2 12h20M2 7h5M2 17h5M17 17h5M17 7h5"/></svg>
								{:else}
									<svg class="h-5 w-5 text-[color:var(--accent-deep)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="2" y="7" width="20" height="15" rx="2" ry="2"/><polyline points="17 2 12 7 7 2"/></svg>
								{/if}
							</div>
							<div>
								<div class="font-semibold text-[color:var(--ink-strong)]">{lib.name}</div>
								<div class="flex items-center gap-2 text-xs text-[color:var(--ink-muted)]">
									<span class="font-mono">{lib.path}</span>
									<span class="rounded-full bg-[color:rgba(214,180,111,0.2)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-[color:var(--ink-strong)]">{lib.media_type}</span>
								</div>
							</div>
						</div>
						<button onclick={() => handleRemoveLibrary(lib.id)} class="rounded-lg border border-[color:var(--line)] px-3 py-1.5 text-xs font-semibold text-[color:var(--danger)] hover:bg-[color:rgba(138,75,67,0.08)]" title="Remove library">Remove</button>
					</div>
				{/each}
			</div>
		{:else}
			<div class="mb-6 rounded-xl border border-dashed border-[color:var(--line)] px-5 py-8 text-center text-sm text-[color:var(--ink-muted)]">
				No library folders configured. Add one below to get started.
			</div>
		{/if}

		<!-- Add New Library -->
		<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
			<h3 class="section-label mb-4">Add Library Folder</h3>
			{#if libraryError}
				<div class="mb-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-2.5 text-sm text-[color:var(--danger)]">{libraryError}</div>
			{/if}
			<div class="grid gap-4 sm:grid-cols-2">
				<label class="block">
					<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Name</span>
					<input type="text" value={newLib.name} oninput={handleLibNameInput} placeholder="e.g. Movies" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" />
				</label>
				<label class="block">
					<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Path <span class="font-normal">(relative to data volume)</span></span>
					<input type="text" bind:value={newLib.path} placeholder="e.g. movies" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
				</label>
				<label class="block">
					<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Content Type</span>
					<select bind:value={newLib.media_type} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
						<option value="movie">Movies</option>
						<option value="tv">TV Shows</option>
					</select>
				</label>
				<label class="block">
					<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">ID <span class="font-normal">(auto-generated)</span></span>
					<input type="text" bind:value={newLib.id} placeholder="auto" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-muted)]" />
				</label>
			</div>
			<div class="mt-4 flex justify-end">
				<button onclick={handleAddLibrary} disabled={addingLibrary} class="rounded-lg bg-[color:var(--accent)] px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50">{addingLibrary ? 'Adding…' : 'Add Library'}</button>
			</div>
		</div>
	</section>

{:else if activeTab === 'standards'}
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

		<!-- Subtitle Rules (full-width below the 2-col grid) -->
		<div class="mt-5 rounded-[1rem] border border-[color:var(--line)] p-5">
			<h3 class="section-label mb-4">Subtitles</h3>
			<div class="space-y-5">
				<label class="block">
					<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Subtitle Mode</span>
					<select bind:value={config.golden_standards.subtitle.mode} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
						{#each SUBTITLE_MODES as { value, label }}
							<option {value}>{label}</option>
						{/each}
					</select>
					<span class="mt-1 block text-xs text-[color:var(--ink-muted)]">
						{#if config.golden_standards.subtitle.mode === 'keep_all'}
							All subtitle streams will be copied to the output file.
						{:else if config.golden_standards.subtitle.mode === 'remove_all'}
							All subtitle streams will be stripped during transcoding.
						{:else if config.golden_standards.subtitle.mode === 'keep_preferred'}
							Only subtitles in your preferred languages will be kept.
						{:else if config.golden_standards.subtitle.mode === 'keep_forced_only'}
							Only forced subtitles in your preferred languages will be kept (e.g., foreign dialogue translations).
						{/if}
					</span>
				</label>

				{#if config.golden_standards.subtitle.mode !== 'remove_all' && config.golden_standards.subtitle.mode !== 'keep_all'}
					<div>
						<span class="mb-2 block text-xs font-semibold text-[color:var(--ink-muted)]">Preferred Languages</span>
						<div class="flex flex-wrap gap-1.5 mb-3">
							{#each COMMON_LANGUAGES as { code, label } (code)}
								<button
									class="rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors border {config.golden_standards.subtitle.preferred_languages.includes(code) ? 'bg-[color:var(--accent)] text-white border-[color:var(--accent)]' : 'border-[color:var(--line)] text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)] hover:border-[color:var(--ink-muted)]'}"
									onclick={() => toggleLanguage(code)}
								>{label}</button>
							{/each}
						</div>
						<div class="flex gap-2 items-center">
							<input
								type="text"
								bind:value={customLangInput}
								placeholder="Custom ISO 639 code (e.g., tha)"
								maxlength="3"
								class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)] font-mono w-56"
								onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addCustomLanguage(); } }}
							/>
							<button class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-xs font-semibold text-[color:var(--ink-strong)]" onclick={addCustomLanguage}>Add</button>
						</div>
						{#if config.golden_standards.subtitle.preferred_languages.length > 0}
							<div class="mt-2 text-xs text-[color:var(--ink-muted)]">
								Selected: <span class="font-mono font-semibold text-[color:var(--ink-strong)]">{config.golden_standards.subtitle.preferred_languages.join(', ')}</span>
							</div>
						{/if}
					</div>

					<div class="flex flex-col gap-3 sm:flex-row sm:gap-6">
						<label class="flex items-center gap-2.5 cursor-pointer">
							<input type="checkbox" bind:checked={config.golden_standards.subtitle.keep_forced} class="rounded border-[color:var(--line)] accent-[color:var(--accent)]" />
							<div>
								<span class="block text-sm font-semibold text-[color:var(--ink-strong)]">Keep Forced Subtitles</span>
								<span class="block text-xs text-[color:var(--ink-muted)]">Always keep forced tracks in preferred languages (foreign dialogue translations)</span>
							</div>
						</label>
						<label class="flex items-center gap-2.5 cursor-pointer">
							<input type="checkbox" bind:checked={config.golden_standards.subtitle.keep_sdh} class="rounded border-[color:var(--line)] accent-[color:var(--accent)]" />
							<div>
								<span class="block text-sm font-semibold text-[color:var(--ink-strong)]">Keep SDH Subtitles</span>
								<span class="block text-xs text-[color:var(--ink-muted)]">Keep subtitles for the deaf and hard of hearing</span>
							</div>
						</label>
					</div>
				{/if}
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
