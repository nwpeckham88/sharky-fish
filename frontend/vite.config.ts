import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	build: {
		rollupOptions: {
			output: {
				manualChunks(id) {
					if (id.includes('/node_modules/svelte/')) {
						return 'svelte';
					}
				}
			}
		}
	},
	server: {
		proxy: {
			'/api': 'http://localhost:3000'
		}
	}
});
