<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import ProviderRow from './provider-row.svelte';

	let {
		store,
		onAdd
	}: {
		store: ReturnType<typeof import('$lib/stores/providers.svelte').createProviderStore>;
		onAdd: () => void;
	} = $props();
</script>

<div class="rounded-lg border border-border bg-card">
	<div class="flex items-center justify-between border-b border-border px-5 py-3">
		<h2 class="text-sm font-medium text-muted-foreground">Providers</h2>
		{#if !store.sheetOpen}
			<Button size="xs" onclick={onAdd}>+ Add Provider</Button>
		{/if}
	</div>

	{#if store.loading}
		<div class="space-y-3 p-5">
			<Skeleton class="h-12 w-full" />
			<Skeleton class="h-12 w-full" />
		</div>
	{:else if store.error && store.providers.length === 0}
		<p class="px-5 py-4 text-sm text-destructive">{store.error}</p>
	{:else if !store.sheetOpen && store.providers.length === 0}
		<p class="px-5 py-8 text-center text-sm text-muted-foreground">
			No providers configured. Click "Add Provider" to get started.
		</p>
	{:else}
		<div class="divide-y divide-border">
			{#each store.providers as provider (provider.id)}
				<ProviderRow {provider} onEdit={store.openEdit} onDelete={store.remove} />
			{/each}
		</div>
	{/if}
</div>
