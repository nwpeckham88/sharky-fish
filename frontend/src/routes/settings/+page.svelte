<script lang="ts">
	import { onMount } from 'svelte';
	import {
		fetchConfig,
		saveConfig,
		testLlmConnection,
		testQbittorrentConnection,
		improveSystemPrompt,
		fetchLibraries,
		addLibrary,
		updateLibrary,
		removeLibrary,
		type AppConfig,
		type LibraryFolder
	} from '$lib/api';

	let activeTab = $state<'libraries' | 'artwork' | 'standards' | 'prompt' | 'system'>('libraries');
	let config = $state<AppConfig | null>(null);
	let loading = $state(true);
	let saving = $state(false);
	let testingLlm = $state(false);
	let testingQbittorrent = $state(false);
	let improvingPrompt = $state(false);
	let llmTestResult = $state<{ ok: boolean; message: string } | null>(null);
	let qbittorrentTestResult = $state<{ ok: boolean; message: string } | null>(null);
	let toast = $state<{ msg: string; ok: boolean } | null>(null);
	let promptIdea = $state('');
	let improvedPromptDraft = $state('');
	let promptImproveError = $state('');
	let promptImproveMode = $state<'replace' | 'append_policy'>('replace');
	let promptDraftMode = $state<'replace' | 'append_policy'>('replace');

	// Library folder management
	let libraries = $state<LibraryFolder[]>([]);
	let newLib = $state<LibraryFolder>({ id: '', name: '', path: '', media_type: 'movie' });
	let editingLibraryId = $state<string | null>(null);
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

	async function runLlmTest() {
		if (!config) return;
		testingLlm = true;
		llmTestResult = null;
		try {
			const result = await testLlmConnection(config.llm);
			llmTestResult = { ok: result.ok, message: result.message };
		} catch (error) {
			llmTestResult = {
				ok: false,
				message: error instanceof Error ? error.message : 'Failed to test LLM connection'
			};
		} finally {
			testingLlm = false;
		}
	}

	async function runQbittorrentTest() {
		if (!config || !canTestQbittorrentConnection()) return;
		testingQbittorrent = true;
		qbittorrentTestResult = null;
		try {
			const result = await testQbittorrentConnection(config.qbittorrent);
			qbittorrentTestResult = { ok: result.ok, message: result.message };
		} catch (error) {
			qbittorrentTestResult = {
				ok: false,
				message: error instanceof Error ? error.message : 'Failed to test qBittorrent connection'
			};
		} finally {
			testingQbittorrent = false;
		}
	}

	function canTestQbittorrentConnection(): boolean {
		if (!config) return false;
		const baseUrl = config.qbittorrent.base_url.trim();
		const username = (config.qbittorrent.username ?? '').trim();
		const password = (config.qbittorrent.password ?? '').trim();
		return baseUrl.length > 0 && username.length > 0 && password.length > 0;
	}

	function resetToGeminiDefaults() {
		if (!config) return;
		config.llm.provider = 'google';
		config.llm.base_url = 'https://generativelanguage.googleapis.com/v1beta';
		config.llm.model = 'gemini-3.1-flash-lite-preview';
		llmTestResult = null;
	}

	function applyRecommendedStandards() {
		if (!config) return;
		config.golden_standards.video.codec = 'h265';
		config.golden_standards.audio.codec = 'opus';
		config.golden_standards.audio.max_channels = '5.1';
		config.golden_standards.audio.keep_multiple_tracks = true;
		config.golden_standards.audio.create_stereo_downmix = true;
	}

	function buildRecommendedPrompt(playbackContext: string): string {
		const normalizedContext = playbackContext.trim() || 'No player notes supplied yet. Favor broadly compatible outputs while still preferring modern codecs when device support is likely.';
		return `You are Sharky Fish's FFmpeg planning engine. Your only job is to generate highly optimized, syntactically valid FFmpeg argument arrays based on library policy, source media characteristics, and playback compatibility.

Return only strict JSON in the shape {"arguments": [...], "requires_two_pass": bool, "rationale": "..."}. Do not output markdown, prose, analysis, or the ffmpeg binary name.

Planning rules:
- Favor H.265 video unless the source, container, or playback compatibility notes clearly justify another choice.
- Favor Opus audio by default.
- Preserve a surround track up to 5.1 when useful and keep multiple retained tracks when they add value.
- Ensure a stereo-compatible track exists for every deliverable, creating a downmix when the source lacks one.
- Keep subtitle handling aligned with the configured subtitle policy.
- Preserve high-value source characteristics such as HDR, strong detail, and worthwhile 4K masters when they remain compatible with the playback environment.
- Optimize for the playback context below without inventing hardware that was not described.

Playback context:
${normalizedContext}`;
	}

	function loadRecommendedPrompt() {
		if (!config) return;
		config.system_prompt = buildRecommendedPrompt(config.playback_context);
		improvedPromptDraft = '';
		promptImproveError = '';
	}

	async function runPromptImprover() {
		if (!config) return;
		if (!promptIdea.trim()) {
			promptImproveError = 'Enter a rough idea first.';
			return;
		}

		improvingPrompt = true;
		promptImproveError = '';
		improvedPromptDraft = '';
		try {
			const result = await improveSystemPrompt({
				llm: config.llm,
				concept: promptIdea.trim(),
				current_prompt: config.system_prompt,
				playback_context: config.playback_context,
				golden_standards: config.golden_standards,
				mode: promptImproveMode
			});
			promptDraftMode = promptImproveMode;
			improvedPromptDraft = result.prompt;
		} catch (error) {
			promptImproveError = error instanceof Error ? error.message : 'Failed to improve prompt';
		} finally {
			improvingPrompt = false;
		}
	}

	function applyImprovedPrompt() {
		if (!config || !improvedPromptDraft.trim()) return;
		if (promptDraftMode === 'append_policy') {
			const existing = config.system_prompt.trimEnd();
			const separator = existing ? '\n\nAdditional Policy:\n' : '';
			config.system_prompt = `${existing}${separator}${improvedPromptDraft}`.trim();
			toaster('Improved policy appended to the prompt editor', true);
		} else {
			config.system_prompt = improvedPromptDraft;
			toaster('Improved prompt loaded into the editor', true);
		}
	}

	const CODECS = [
		{ value: 'h264', label: 'H.264 (libx264 / nvenc)' },
		{ value: 'h265', label: 'H.265 (libx265 / nvenc)' },
		{ value: 'av1',  label: 'AV1 (libaom / svt-av1)' },
		{ value: 'vp9',  label: 'VP9' },
	];

	const AUDIO_CODECS = [
		{ value: 'opus', label: 'Opus' },
		{ value: 'aac', label: 'AAC' },
		{ value: 'ac3', label: 'AC-3' },
		{ value: 'eac3', label: 'E-AC-3' },
		{ value: 'copy', label: 'Copy Source Codec' }
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
		{ value: 'google', label: 'Google AI API (Gemini)' },
		{ value: 'ollama', label: 'Ollama (local)' },
		{ value: 'openai', label: 'OpenAI API' },
	];

	const GEMINI_DEFAULT_MODEL = 'gemini-3.1-flash-lite-preview';

	const METADATA_PROVIDERS = [
		{ value: 'tmdb', label: 'TMDb (Jellyfin default for movies and shows)' },
		{ value: 'omdb', label: 'OMDb (IMDb-backed)' },
		{ value: 'tvdb', label: 'TVDB' }
	] as const;

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
			if (editingLibraryId) {
				const updated = await updateLibrary(editingLibraryId, newLib);
				libraries = libraries.map((library) => (library.id === editingLibraryId ? updated : library));
				toaster('Library updated', true);
			} else {
				const added = await addLibrary(newLib);
				libraries = [...libraries, added];
				toaster('Library added', true);
			}
			newLib = { id: '', name: '', path: '', media_type: 'movie' };
			editingLibraryId = null;
		} catch (e) {
			libraryError = e instanceof Error ? e.message : 'Failed to add library';
		} finally {
			addingLibrary = false;
		}
	}

	function startEditLibrary(library: LibraryFolder) {
		editingLibraryId = library.id;
		newLib = { ...library };
		libraryError = '';
	}

	function cancelEditLibrary() {
		editingLibraryId = null;
		newLib = { id: '', name: '', path: '', media_type: 'movie' };
		libraryError = '';
	}

	async function handleRemoveLibrary(id: string) {
		try {
			await removeLibrary(id);
			libraries = libraries.filter((l) => l.id !== id);
			if (editingLibraryId === id) {
				cancelEditLibrary();
			}
			toaster('Library removed', true);
		} catch (e) {
			toaster(e instanceof Error ? e.message : 'Failed to remove library', false);
		}
	}

	function toaster(message: string, ok: boolean) {
		toast = { msg: message, ok };
		setTimeout(() => { toast = null; }, 3000);
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
	{#each [['libraries', 'Libraries'], ['artwork', 'Artwork'], ['standards', 'Golden Standards'], ['prompt', 'Prompt Playground'], ['system', 'System Profile']] as [key, label] (key)}
		<button class="rounded-lg px-4 py-2 text-sm font-semibold transition-colors {activeTab === key ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { activeTab = key as typeof activeTab; }}>{label}</button>
	{/each}
</div>

{#if activeTab === 'libraries'}
	<section class="surface-card p-6">
		<h2 class="mb-4 text-xl text-[color:var(--ink-strong)]">Library Folders</h2>
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">Map folders inside your managed media root to named libraries. Following the standard TRaSH layout, Sharky Fish defaults to <span class="font-mono text-[color:var(--ink-strong)]">/data/media</span> for libraries and expects folders here such as Movies and TV.</p>

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
						<div class="flex gap-2">
							<button onclick={() => startEditLibrary(lib)} class="rounded-lg border border-[color:var(--line)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)] hover:bg-[color:rgba(214,180,111,0.08)]" title="Edit library">Edit</button>
							<button onclick={() => handleRemoveLibrary(lib.id)} class="rounded-lg border border-[color:var(--line)] px-3 py-1.5 text-xs font-semibold text-[color:var(--danger)] hover:bg-[color:rgba(138,75,67,0.08)]" title="Remove library">Remove</button>
						</div>
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
			<h3 class="section-label mb-4">{editingLibraryId ? 'Edit Library Folder' : 'Add Library Folder'}</h3>
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
			<div class="mt-4 flex justify-end gap-2">
				{#if editingLibraryId}
					<button onclick={cancelEditLibrary} class="rounded-lg border border-[color:var(--line)] px-5 py-2.5 text-sm font-semibold text-[color:var(--ink-strong)]">Cancel</button>
				{/if}
				<button onclick={handleAddLibrary} disabled={addingLibrary} class="rounded-lg bg-[color:var(--accent)] px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50">{addingLibrary ? (editingLibraryId ? 'Saving…' : 'Adding…') : (editingLibraryId ? 'Save Library' : 'Add Library')}</button>
			</div>
		</div>
	</section>

{:else if activeTab === 'artwork'}
	<section class="surface-card p-6">
		<h2 class="mb-4 text-xl text-[color:var(--ink-strong)]">Artwork Downloads</h2>
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">
			Configure how many images of each type Sharky Fish downloads per content category, following the same model as Jellyfin's image settings. Set a value to <span class="font-mono text-[color:var(--ink-strong)]">0</span> to skip that image type entirely.
		</p>

		<!-- Legend -->
		<div class="mb-6 grid grid-cols-2 gap-2 rounded-xl border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] p-4 sm:grid-cols-3 lg:grid-cols-5 xl:grid-cols-6">
			{#each [
				['Primary', 'Main poster / showcase image'],
				['Backdrop', 'Full-width fanart backgrounds'],
				['Logo', 'Transparent clearlogo / title art'],
				['Banner', 'Wide horizontal banner (800×150)'],
				['Thumb', 'Landscape thumbnail (16:9)'],
				['Disc', 'Optical disc cover art'],
				['Art', 'ClearArt — transparent character/scene art'],
				['Screenshot', 'Episode or scene screenshot'],
				['Box', 'Box cover art (music / games)'],
				['Box Rear', 'Rear box art'],
				['Menu', 'Bonus-menu art'],
			] as [name, desc]}
				<div class="flex flex-col gap-0.5">
					<span class="text-xs font-semibold text-[color:var(--ink-strong)]">{name}</span>
					<span class="text-[10px] leading-4 text-[color:var(--ink-muted)]">{desc}</span>
				</div>
			{/each}
		</div>

		<div class="grid gap-5 md:grid-cols-2">

			<!-- Movies -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<div class="mb-4 flex items-center gap-3">
					<div class="flex h-8 w-8 items-center justify-center rounded-lg bg-[color:var(--accent)]/15">
						<svg class="h-4 w-4 text-[color:var(--accent-deep)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><path d="M7 2v20M17 2v20M2 12h20M2 7h5M2 17h5M17 17h5M17 7h5"/></svg>
					</div>
					<h3 class="section-label">Movies</h3>
				</div>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
					{#each [
						['primary',    'Primary'],
						['backdrop',   'Backdrop'],
						['logo',       'Logo'],
						['banner',     'Banner'],
						['thumb',      'Thumb'],
						['disc',       'Disc'],
						['art',        'Art'],
						['box_art',    'Box'],
						['box_rear',   'Box Rear'],
						['menu',       'Menu'],
					] as [field, label]}
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">{label}</span>
							<input
								type="number"
								min="0"
								max="10"
								bind:value={config.artwork_download.movies[field as keyof typeof config.artwork_download.movies]}
								class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
							/>
						</label>
					{/each}
				</div>
			</div>

			<!-- TV Series -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<div class="mb-4 flex items-center gap-3">
					<div class="flex h-8 w-8 items-center justify-center rounded-lg bg-[color:var(--accent)]/15">
						<svg class="h-4 w-4 text-[color:var(--accent-deep)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="2" y="7" width="20" height="15" rx="2" ry="2"/><polyline points="17 2 12 7 7 2"/></svg>
					</div>
					<h3 class="section-label">TV Series</h3>
				</div>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
					{#each [
						['primary',    'Primary'],
						['backdrop',   'Backdrop'],
						['logo',       'Logo'],
						['banner',     'Banner'],
						['thumb',      'Thumb'],
						['art',        'Art'],
					] as [field, label]}
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">{label}</span>
							<input
								type="number"
								min="0"
								max="10"
								bind:value={config.artwork_download.series[field as keyof typeof config.artwork_download.series]}
								class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
							/>
						</label>
					{/each}
				</div>
			</div>

			<!-- TV Seasons -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<div class="mb-4 flex items-center gap-3">
					<div class="flex h-8 w-8 items-center justify-center rounded-lg bg-[color:var(--accent)]/15">
						<svg class="h-4 w-4 text-[color:var(--accent-deep)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="2" y="7" width="20" height="15" rx="2" ry="2"/><line x1="8" y1="7" x2="8" y2="22"/></svg>
					</div>
					<h3 class="section-label">TV Seasons</h3>
				</div>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
					{#each [
						['primary',    'Primary'],
						['backdrop',   'Backdrop'],
						['banner',     'Banner'],
						['thumb',      'Thumb'],
					] as [field, label]}
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">{label}</span>
							<input
								type="number"
								min="0"
								max="10"
								bind:value={config.artwork_download.seasons[field as keyof typeof config.artwork_download.seasons]}
								class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
							/>
						</label>
					{/each}
				</div>
			</div>

			<!-- TV Episodes -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<div class="mb-4 flex items-center gap-3">
					<div class="flex h-8 w-8 items-center justify-center rounded-lg bg-[color:var(--accent)]/15">
						<svg class="h-4 w-4 text-[color:var(--accent-deep)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><polygon points="5 3 19 12 5 21 5 3"/></svg>
					</div>
					<h3 class="section-label">TV Episodes</h3>
				</div>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
					{#each [
						['primary',    'Primary'],
						['backdrop',   'Backdrop'],
						['thumb',      'Thumb'],
						['screenshot', 'Screenshot'],
					] as [field, label]}
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">{label}</span>
							<input
								type="number"
								min="0"
								max="10"
								bind:value={config.artwork_download.episodes[field as keyof typeof config.artwork_download.episodes]}
								class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
							/>
						</label>
					{/each}
				</div>
			</div>

		</div>

		<div class="mt-5 flex items-center justify-between gap-3">
			<p class="text-xs text-[color:var(--ink-muted)]">
				These counts are passed to internet metadata providers (TMDb, TVDB) when building image download queues. Requires a metadata refresh to take effect on existing library items.
			</p>
			<button onclick={save} disabled={saving} class="shrink-0 rounded-lg bg-[color:var(--accent)] px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50">{saving ? 'Saving…' : 'Save Artwork Settings'}</button>
		</div>
	</section>

{:else if activeTab === 'standards'}
	<section class="surface-card p-6">
		<div class="mb-4 flex flex-wrap items-center justify-between gap-3">
			<h2 class="text-xl text-[color:var(--ink-strong)]">Golden Standards</h2>
			<button onclick={applyRecommendedStandards} class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)]">Apply H.265 + Opus Defaults</button>
		</div>
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
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Target Codec</span>
						<select bind:value={config.golden_standards.audio.codec} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
							{#each AUDIO_CODECS as { value, label }}
								<option {value}>{label}</option>
							{/each}
						</select>
					</label>
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
					<div class="grid gap-3">
						<label class="flex items-center gap-2.5 cursor-pointer rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-3">
							<input type="checkbox" bind:checked={config.golden_standards.audio.keep_multiple_tracks} class="rounded border-[color:var(--line)] accent-[color:var(--accent)]" />
							<div>
								<span class="block text-sm font-semibold text-[color:var(--ink-strong)]">Keep Multiple Audio Tracks</span>
								<span class="block text-xs text-[color:var(--ink-muted)]">Allow both surround and alternate retained tracks when they serve the library policy.</span>
							</div>
						</label>
						<label class="flex items-center gap-2.5 cursor-pointer rounded-lg border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-3">
							<input type="checkbox" bind:checked={config.golden_standards.audio.create_stereo_downmix} class="rounded border-[color:var(--line)] accent-[color:var(--accent)]" />
							<div>
								<span class="block text-sm font-semibold text-[color:var(--ink-strong)]">Ensure Stereo Compatibility Track</span>
								<span class="block text-xs text-[color:var(--ink-muted)]">Create or keep a stereo track so every file remains playable on simpler clients.</span>
							</div>
						</label>
					</div>
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
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">Define the system prompt sent to the LLM. Keep player details in Playback Context unless you truly need a dedicated device-management feature later.</p>

		<div class="mb-5 grid gap-4 rounded-[1rem] border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] p-4 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-end">
			<div>
				<div class="section-label">Recommended Prompt</div>
				<p class="mt-2 text-sm text-[color:var(--ink-muted)]">Start from a stricter house prompt tuned for H.265 video, Opus audio, 5.1 plus stereo compatibility, and the playback notes from your System Profile.</p>
			</div>
			<button onclick={loadRecommendedPrompt} class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)]">Load Recommended Prompt</button>
		</div>

		<div class="mb-5 rounded-[1rem] border border-[color:var(--line)] p-5">
			<div class="section-label mb-3">Improve Prompt With AI</div>
			<p class="mb-3 text-sm text-[color:var(--ink-muted)]">Enter a rough idea like “Keep visually stunning or highly rated films in 4K HDR where it makes sense.” The model will expand it into a more precise saved prompt using your current standards and playback notes.</p>
			<div class="mb-3 flex flex-wrap gap-1.5 rounded-xl border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-1 w-fit">
				<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {promptImproveMode === 'replace' ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { promptImproveMode = 'replace'; }}>
					Replace Full Prompt
				</button>
				<button class="rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.12em] transition-colors {promptImproveMode === 'append_policy' ? 'bg-[color:var(--accent)] text-white' : 'text-[color:var(--ink-muted)] hover:text-[color:var(--ink-strong)]'}" onclick={() => { promptImproveMode = 'append_policy'; }}>
					Append Policy Section
				</button>
			</div>
			<p class="mb-3 text-xs text-[color:var(--ink-muted)]">
				{promptImproveMode === 'replace'
					? 'Replace mode asks the model for a complete new system prompt.'
					: 'Append mode asks the model for a standalone policy block that can be appended to your current prompt.'}
			</p>
			<textarea bind:value={promptIdea} rows="4" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-3 text-sm text-[color:var(--ink-strong)]" placeholder="I want all the good looking or very highly rated movies in 4K HDR. Animated movies in 4K HDR where available."></textarea>
			<div class="mt-3 flex flex-wrap items-center gap-2">
				<button onclick={runPromptImprover} disabled={improvingPrompt} class="rounded-lg bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-50">{improvingPrompt ? 'Improving…' : 'Improve Prompt With AI'}</button>
				{#if improvedPromptDraft}
					<button onclick={applyImprovedPrompt} class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)]">{promptDraftMode === 'append_policy' ? 'Append Improved Policy' : 'Use Improved Prompt'}</button>
				{/if}
			</div>
			{#if promptImproveError}
				<div class="mt-3 rounded-lg border border-[color:rgba(138,75,67,0.22)] bg-[color:rgba(138,75,67,0.08)] px-4 py-2.5 text-sm text-[color:var(--danger)]">{promptImproveError}</div>
			{/if}
			{#if improvedPromptDraft}
				<div class="mt-4">
					<div class="mb-2 text-xs font-semibold uppercase tracking-[0.16em] text-[color:var(--ink-muted)]">AI Draft · {promptDraftMode === 'append_policy' ? 'Append Policy Section' : 'Replace Full Prompt'}</div>
					<textarea readonly value={improvedPromptDraft} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--paper-deep)] p-4 font-mono text-sm leading-relaxed text-[color:var(--ink-strong)]" rows="10"></textarea>
				</div>
			{/if}
		</div>

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
					<p class="text-sm text-[color:var(--ink-muted)]">Prompt dry-run is planned for a future update. For now, use Backlog and Review to evaluate live AI decisions with real files.</p>
				</div>
				<div class="mt-3 flex gap-2">
					<button disabled class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-muted)] disabled:cursor-not-allowed disabled:opacity-60">Select File…</button>
					<button disabled class="rounded-lg bg-[color:var(--accent)] px-4 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60">Dry Run</button>
				</div>
				<p class="mt-2 text-xs text-[color:var(--ink-muted)]">Coming soon: run planner simulations against a chosen library item without queueing work.</p>
			</div>
		</div>

		<div class="mt-5 flex justify-end">
			<button onclick={save} disabled={saving} class="rounded-lg bg-[color:var(--accent)] px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50">{saving ? 'Saving…' : 'Save Prompt'}</button>
		</div>
	</section>

{:else if activeTab === 'system'}
	<section class="surface-card p-6">
		<h2 class="mb-4 text-xl text-[color:var(--ink-strong)]">System Profile</h2>
		<p class="mb-6 text-sm text-[color:var(--ink-muted)]">Configure the LLM connection, tune AI automation policy, and review system settings. New installs default to Google AI API with Gemini 3.1 Flash-Lite Preview.</p>

		<div class="grid gap-5 md:grid-cols-2">
			<!-- Automation -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5 md:col-span-2">
				<h3 class="section-label mb-4">Automation Policy</h3>
				<div class="grid gap-4 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,0.8fr)] lg:items-start">
					<div>
						<label class="flex items-start gap-3 cursor-pointer">
							<input type="checkbox" bind:checked={config.auto_approve_ai_jobs} class="mt-1 rounded border-[color:var(--line)] accent-[color:var(--accent)]" />
							<div>
								<span class="block text-sm font-semibold text-[color:var(--ink-strong)]">Auto-approve AI jobs</span>
								<span class="mt-1 block text-xs leading-5 text-[color:var(--ink-muted)]">When enabled, new AI-generated transcode plans move directly into the ready queue. Operators can still reject them from Intake until processing begins.</span>
							</div>
						</label>
					</div>
					<div class="rounded-xl border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.55)] px-4 py-3 text-xs leading-5 text-[color:var(--ink-muted)]">
						<p class="font-semibold uppercase tracking-[0.14em] text-[color:var(--accent-deep)]">Current behavior</p>
						<p class="mt-2">{config.auto_approve_ai_jobs ? 'AI decisions go straight to The Forge queue as APPROVED items.' : 'AI decisions pause in Intake as AWAITING_APPROVAL until an operator approves them.'}</p>
					</div>
				</div>
			</div>

			<div class="rounded-[1rem] border border-[color:var(--line)] p-5 md:col-span-2">
				<h3 class="section-label mb-4">Library View Defaults</h3>
				<div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,0.9fr)] lg:items-start">
					<div>
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Default Library Presentation</span>
							<select bind:value={config.library_view_mode} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
								<option value="compact">Compact rows</option>
								<option value="expanded">Expanded cards</option>
							</select>
						</label>
						<p class="mt-2 text-xs leading-5 text-[color:var(--ink-muted)]">Compact rows keep large libraries scannable and let you expand items inline on demand. Expanded cards trade density for richer per-item detail.</p>
					</div>
					<div class="rounded-xl border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.55)] px-4 py-3 text-xs leading-5 text-[color:var(--ink-muted)]">
						<p class="font-semibold uppercase tracking-[0.14em] text-[color:var(--accent-deep)]">Current behavior</p>
						<p class="mt-2">{config.library_view_mode === 'expanded' ? 'Library opens in expanded cards with artwork and richer metadata emphasis.' : 'Library opens in compact rows designed for sweeping large movie and show lists quickly.'}</p>
					</div>
				</div>
			</div>

			<div class="rounded-[1rem] border border-[color:var(--line)] p-5 md:col-span-2">
				<h3 class="section-label mb-4">Playback Context</h3>
				<p class="mb-3 text-sm text-[color:var(--ink-muted)]">This is the simpler alternative to a full player-endpoint feature for now. List the clients and constraints the AI should optimize for, such as Onn 4K Pro, Nvidia Shield, AVR support, 4K HDR, Dolby Vision, lossless-audio limits, or subtitle quirks.</p>
				<textarea bind:value={config.playback_context} rows="5" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] p-3 text-sm text-[color:var(--ink-strong)]" placeholder="Living room: Nvidia Shield Pro into a 4K HDR TV and AVR, prefers direct play, can handle 4K HDR10 and 5.1. Bedroom: Onn 4K Pro on a stereo TV, needs a stereo-compatible track on everything."></textarea>
				<p class="mt-2 text-xs text-[color:var(--ink-muted)]">This context is sent to the LLM with media planning requests and also feeds the prompt improver.</p>
			</div>

			<!-- LLM Configuration -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">LLM Endpoint</h3>
				<div class="space-y-4">
					<div class="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-[color:var(--line)] bg-[color:rgba(244,236,223,0.45)] px-3 py-2.5 text-xs text-[color:var(--ink-muted)]">
						<span>Recommended default: <span class="font-mono text-[color:var(--ink-strong)]">{GEMINI_DEFAULT_MODEL}</span></span>
						<button onclick={resetToGeminiDefaults} class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-1.5 text-xs font-semibold text-[color:var(--ink-strong)]">Reset to Gemini Default</button>
					</div>
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
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">API Key {config.llm.provider === 'ollama' ? '(not required for Ollama)' : config.llm.provider === 'google' ? '(Google AI Studio key)' : ''}</span>
						<input type="password" value={config.llm.api_key ?? ''} oninput={(e) => { if (config) config.llm.api_key = (e.target as HTMLInputElement).value || null; }} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					{#if config.llm.provider === 'google'}
						<p class="text-xs text-[color:var(--ink-muted)]">Recommended model: <span class="font-mono text-[color:var(--ink-strong)]">{GEMINI_DEFAULT_MODEL}</span></p>
					{/if}
					<div class="flex flex-wrap items-center gap-2 pt-1">
						<button onclick={runLlmTest} disabled={testingLlm} class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] disabled:opacity-50">
							{testingLlm ? 'Testing…' : 'Test LLM Connection'}
						</button>
						{#if llmTestResult}
							<span class="rounded-lg px-3 py-2 text-xs font-semibold {llmTestResult.ok ? 'bg-[color:var(--olive)]/15 text-[color:var(--olive)]' : 'bg-red-500/15 text-red-400'}">{llmTestResult.message}</span>
						{/if}
					</div>
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
					<label class="flex items-start gap-3">
						<input type="checkbox" bind:checked={config.scan_compute_checksums} class="mt-1 rounded border-[color:var(--line)] accent-[color:var(--accent)]" />
						<span>
							<span class="block text-sm font-semibold text-[color:var(--ink-strong)]">Compute checksums during scan</span>
							<span class="block text-xs text-[color:var(--ink-muted)]">Hashes every library file with BLAKE3 to enable duplicate detection in the Downloads view. Can add several minutes to scans on large NAS libraries. Off by default.</span>
						</span>
					</label>
				</div>
			</div>

			<!-- Internet Metadata Providers -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">Internet Metadata</h3>
				<p class="mb-4 text-xs leading-5 text-[color:var(--ink-muted)]">
					Sharky Fish is tuned for Jellyfin. TMDb should usually be your primary provider for movie and TV libraries, with OMDb and TVDB acting as fallback sources when you want broader coverage.
				</p>
				<div class="space-y-4">
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">TMDb API Key <span class="font-normal">(recommended)</span></span>
						<input type="password" value={config.internet_metadata.tmdb_api_key ?? ''} oninput={(e) => { if (config) config.internet_metadata.tmdb_api_key = (e.target as HTMLInputElement).value || null; }} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">OMDb API Key <span class="font-normal">(IMDb-backed)</span></span>
						<input type="password" value={config.internet_metadata.omdb_api_key ?? ''} oninput={(e) => { if (config) config.internet_metadata.omdb_api_key = (e.target as HTMLInputElement).value || null; }} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">TVDB API Key</span>
						<input type="password" value={config.internet_metadata.tvdb_api_key ?? ''} oninput={(e) => { if (config) config.internet_metadata.tvdb_api_key = (e.target as HTMLInputElement).value || null; }} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">TVDB PIN <span class="font-normal">(optional)</span></span>
						<input type="password" value={config.internet_metadata.tvdb_pin ?? ''} oninput={(e) => { if (config) config.internet_metadata.tvdb_pin = (e.target as HTMLInputElement).value || null; }} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Default Search Provider</span>
						<select bind:value={config.internet_metadata.default_provider} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]">
							{#each METADATA_PROVIDERS as { value, label }}
								<option {value}>{label}</option>
							{/each}
						</select>
						<span class="mt-1 block text-xs text-[color:var(--ink-muted)]">Sharky Fish queries configured providers in order, starting with this one and falling back when a search comes back empty.</span>
					</label>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">User Agent</span>
						<input type="text" bind:value={config.internet_metadata.user_agent} class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
					</label>
					<p class="text-xs text-[color:var(--ink-muted)]">Recommended Jellyfin setup: configure TMDb first for movies and shows, then add OMDb or TVDB if you want fallback IDs and alternate match coverage.</p>
				</div>
			</div>

			<!-- qBittorrent API -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5 md:col-span-2">
				<h3 class="section-label mb-4">qBittorrent API (Optional)</h3>
				<div class="space-y-4">
					<label class="flex items-start gap-3">
						<input type="checkbox" bind:checked={config.qbittorrent.enabled} class="mt-1 rounded border-[color:var(--line)] accent-[color:var(--accent)]" />
						<span>
							<span class="block text-sm font-semibold text-[color:var(--ink-strong)]">Enable qBittorrent monitoring</span>
							<span class="block text-xs text-[color:var(--ink-muted)]">When enabled, Sharky Fish polls the qBittorrent WebUI API for transfer rates, active torrents, and download paths.</span>
						</span>
					</label>
					<div class="grid gap-4 md:grid-cols-2">
						<label class="block md:col-span-2">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Base URL</span>
							<input type="text" bind:value={config.qbittorrent.base_url} placeholder="http://qbittorrent:8080" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
						</label>
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Username</span>
							<input
								type="text"
								value={config.qbittorrent.username ?? ''}
								oninput={(e) => { if (config) config.qbittorrent.username = (e.target as HTMLInputElement).value || null; }}
								class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
							/>
						</label>
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Password</span>
							<input
								type="password"
								value={config.qbittorrent.password ?? ''}
								oninput={(e) => { if (config) config.qbittorrent.password = (e.target as HTMLInputElement).value || null; }}
								class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]"
							/>
						</label>
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Timeout (seconds)</span>
							<input type="number" bind:value={config.qbittorrent.request_timeout_secs} min="2" max="60" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" />
						</label>
						<label class="block">
							<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Max Torrents</span>
							<input type="number" bind:value={config.qbittorrent.max_torrents} min="1" max="500" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 text-sm text-[color:var(--ink-strong)]" />
						</label>
					</div>
					<div class="flex flex-wrap items-center gap-2 pt-1">
						<button onclick={runQbittorrentTest} disabled={testingQbittorrent || !canTestQbittorrentConnection()} class="rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm font-semibold text-[color:var(--ink-strong)] disabled:opacity-50">
							{testingQbittorrent ? 'Testing…' : 'Test qBittorrent Connection'}
						</button>
						{#if qbittorrentTestResult}
							<span class="rounded-lg px-3 py-2 text-xs font-semibold {qbittorrentTestResult.ok ? 'bg-[color:var(--olive)]/15 text-[color:var(--olive)]' : 'bg-red-500/15 text-red-400'}">{qbittorrentTestResult.message}</span>
						{/if}
					</div>
					{#if !canTestQbittorrentConnection()}
						<p class="text-xs text-[color:var(--ink-muted)]">Enter base URL, username, and password to enable connection testing.</p>
					{/if}
					<p class="text-xs text-[color:var(--ink-muted)]">Use the same in-container downloads path mapping across qBittorrent, the *arr stack, and Sharky Fish.</p>
				</div>
			</div>

			<!-- Storage -->
			<div class="rounded-[1rem] border border-[color:var(--line)] p-5">
				<h3 class="section-label mb-4">Storage</h3>
				<div class="space-y-4">
					<div class="space-y-2 text-sm">
						<div class="flex justify-between"><span class="text-[color:var(--ink-muted)]">Library root</span><span class="font-mono text-[color:var(--ink-strong)]">{config.data_path}</span></div>
						<div class="flex justify-between"><span class="text-[color:var(--ink-muted)]">Config</span><span class="font-mono text-[color:var(--ink-strong)]">{config.config_path}</span></div>
					</div>
					<label class="block">
						<span class="mb-1 block text-xs font-semibold text-[color:var(--ink-muted)]">Downloads folder <span class="font-normal">(path inside container)</span></span>
						<input type="text" bind:value={config.ingest_path} placeholder="/data/downloads" class="w-full rounded-lg border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2 font-mono text-sm text-[color:var(--ink-strong)]" />
						<span class="mt-1 block text-xs text-[color:var(--ink-muted)]">The folder your download client writes completed files to. Following the TRaSH-style default layout, use <span class="font-mono text-[color:var(--ink-strong)]">/data/downloads</span> for qBittorrent ingress and <span class="font-mono text-[color:var(--ink-strong)]">/data/media</span> for your managed libraries. Requires a container restart to take effect.</span>
					</label>
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
