<script lang="ts">
	import '../app.css';
	import favicon from '$lib/assets/favicon.svg';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { onDestroy, onMount } from 'svelte';
	import { fetchJobs, fetchHealth } from '$lib/api';
	import { createEventSource } from '$lib/sse';
	import {
		jobStore,
		healthStore,
		handleSseEvent,
		managedItemStore,
		refreshManagedItemStore
	} from '$lib/stores.svelte';
	import { statusLabel } from '$lib/status';

	let { children } = $props();
	let collapsed = $state(false);
	let searchQuery = $state('');
	let searchEl: HTMLInputElement | undefined;
	let es: EventSource | undefined;
	let healthTimer: ReturnType<typeof setInterval> | undefined;

	const navItems = [
		{ href: '/', label: 'Backlog', icon: 'dashboard', badge: 'backlog' },
		{ href: '/intake', label: 'Review', icon: 'intake', badge: 'review' },
		{ href: '/library', label: 'Library', icon: 'library' },
		{ href: '/downloads', label: 'Downloads', icon: 'downloads' },
		{ href: '/forge', label: 'Execution', icon: 'forge' },
		{ href: '/settings', label: 'Settings', icon: 'settings' }
	] as const;

	function isActive(href: string): boolean {
		if (href === '/') return page.url.pathname === '/';
		return page.url.pathname.startsWith(href);
	}

	onMount(async () => {
		try {
			jobStore.jobs = await fetchJobs(200);
		} catch {
			jobStore.jobs = [];
		} finally {
			jobStore.loading = false;
		}

		void refreshManagedItemStore();

		es = createEventSource(handleSseEvent, () => {
			healthStore.connected = false;
		});

		checkHealth();
		healthTimer = setInterval(checkHealth, 15000);

		document.addEventListener('keydown', handleGlobalKeydown);
	});

	onDestroy(() => {
		es?.close();
		if (healthTimer) clearInterval(healthTimer);
		if (typeof document !== 'undefined') {
			document.removeEventListener('keydown', handleGlobalKeydown);
		}
	});

	async function checkHealth() {
		healthStore.connected = await fetchHealth();
	}

	function handleGlobalKeydown(e: KeyboardEvent) {
		if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
			e.preventDefault();
			searchEl?.focus();
		}
	}

	function handleSearchKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter' && searchQuery.trim()) {
			goto(`/library?q=${encodeURIComponent(searchQuery.trim())}`);
			searchQuery = '';
			searchEl?.blur();
		}
		if (e.key === 'Escape') {
			searchQuery = '';
			searchEl?.blur();
		}
	}

	function pageTitle(pathname: string): string {
		if (pathname === '/') return 'Backlog';
		if (pathname.startsWith('/intake')) return 'Review';
		if (pathname.startsWith('/library')) return 'Library';
		if (pathname.startsWith('/downloads')) return 'Downloads';
		if (pathname.startsWith('/organize')) return 'Organize';
		if (pathname.startsWith('/forge')) return 'Execution';
		if (pathname.startsWith('/settings')) return 'Settings';
		return 'sharky-fish';
	}

	function pageSubtitle(pathname: string): string {
		if (pathname === '/') return 'Shape the library backlog, not the queue.';
		if (pathname.startsWith('/intake')) return 'Approve or reject AI-generated plans before they enter execution.';
		if (pathname.startsWith('/library')) return 'Audit managed state, sidecars, metadata, and organization.';
		if (pathname.startsWith('/downloads')) return 'Audit linked imports, orphans, and download-folder cleanup risk.';
		if (pathname.startsWith('/organize')) return 'Rename files into the canonical library structure.';
		if (pathname.startsWith('/forge')) return 'Monitor approved, running, completed, and failed work.';
		if (pathname.startsWith('/settings')) return 'Configure standards, prompts, and system policy.';
		return 'Library shaping workspace';
	}

	const backlogBadge = $derived(
		managedItemStore.summary.needs_attention_count
	);
	const reviewBadge = $derived(managedItemStore.summary.awaiting_approval_count);

	function navBadge(item: (typeof navItems)[number]): number | null {
		if ('badge' in item && item.badge === 'backlog') return backlogBadge;
		if ('badge' in item && item.badge === 'review') return reviewBadge;
		return null;
	}

	$effect(() => {
		jobStore.jobs.length;
		void refreshManagedItemStore();
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>sharky-fish</title>
</svelte:head>

<div class="shell">
	<nav class="nav-rail" class:collapsed>
		<div class="nav-brand">
			<div class="nav-logo">
				<svg class="h-6 w-6" viewBox="0 0 24 24" fill="currentColor">
					<path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z"/>
				</svg>
			</div>
			{#if !collapsed}
				<div class="nav-brand-text">
					<span class="font-[family-name:var(--font-display)] text-lg tracking-[0.02em] text-[color:var(--ink-strong)]">sharky-fish</span>
				</div>
			{/if}
		</div>

		<div class="nav-items">
			{#each navItems as item (item.href)}
				<a href={item.href} class="nav-link" class:active={isActive(item.href)}>
					<span class="nav-icon">
						{#if item.icon === 'dashboard'}
							<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>
						{:else if item.icon === 'intake'}
							<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M12 3v12m0 0l-4-4m4 4l4-4"/><path d="M4 17v2a2 2 0 002 2h12a2 2 0 002-2v-2"/></svg>
						{:else if item.icon === 'library'}
							<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 19.5A2.5 2.5 0 016.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/></svg>
						{:else if item.icon === 'downloads'}
							<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M12 3v10"/><path d="m8 9 4 4 4-4"/><path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"/></svg>
						{:else if item.icon === 'forge'}
							<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z"/></svg>
						{:else if item.icon === 'settings'}
							<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>
						{/if}
					</span>
					{#if !collapsed}
						<span class="nav-label">{item.label}</span>
						{@const badge = navBadge(item)}
						{#if badge !== null && badge > 0}
							<span class="nav-badge">{badge}</span>
						{/if}
					{/if}
				</a>
			{/each}
		</div>

		<button class="nav-collapse-btn" onclick={() => collapsed = !collapsed}>
			<svg class="h-4 w-4 transition-transform" class:rotate-180={collapsed} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 18l-6-6 6-6"/></svg>
			{#if !collapsed}<span class="nav-label text-xs">Collapse</span>{/if}
		</button>
	</nav>

	<div class="main-area">
		<header class="top-bar">
			<div class="flex items-center gap-3">
				<div>
					<h1 class="font-[family-name:var(--font-display)] text-xl tracking-[0.02em] text-[color:var(--ink-strong)]">{pageTitle(page.url.pathname)}</h1>
					<p class="mt-1 text-sm text-[color:var(--ink-muted)]">{pageSubtitle(page.url.pathname)}</p>
				</div>
			</div>
			<div class="flex items-center gap-3">
				<label class="hidden items-center gap-2 rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-4 py-2 text-sm text-[color:var(--ink-muted)] sm:flex">
					<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
					<input bind:this={searchEl} bind:value={searchQuery} onkeydown={handleSearchKeydown} class="w-32 bg-transparent text-sm text-[color:var(--ink-strong)] outline-none placeholder:text-[color:var(--ink-muted)]" placeholder="Search…" />
					<kbd class="rounded border border-[color:var(--line)] bg-[color:var(--paper)] px-1.5 py-0.5 font-mono text-[10px]">⌘K</kbd>
				</label>
				<div class="flex items-center gap-2 rounded-full border border-[color:var(--line)] bg-[color:var(--panel-strong)] px-3 py-2" title={healthStore.connected ? 'Backend connected' : 'Backend disconnected'}>
					<span class="llm-pulse" class:disconnected={!healthStore.connected}></span>
					<span class="text-xs font-semibold uppercase tracking-[0.18em] text-[color:var(--ink-muted)]">{healthStore.connected ? 'Online' : 'Offline'}</span>
					{#if reviewBadge > 0}
						<span class="text-xs text-[color:var(--ink-muted)]">· {reviewBadge} {statusLabel('AWAITING_APPROVAL')}</span>
					{/if}
				</div>
			</div>
		</header>

		<main class="main-content">
			{@render children()}
		</main>
	</div>
</div>
