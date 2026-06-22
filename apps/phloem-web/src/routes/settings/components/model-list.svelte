<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import ModelRow from './model-row.svelte';

	let {
		store,
		onAdd
	}: {
		store: ReturnType<typeof import('$lib/stores/models.svelte').createModelStore>;
		onAdd: () => void;
	} = $props();
</script>

<div class="rounded-lg border border-border bg-card">
	<!-- Header -->
	<div class="flex items-center justify-between px-5 py-3 border-b border-border">
		<h2 class="text-sm font-medium text-muted-foreground">Model Pool</h2>
		{#if !store.sheetOpen}
			<Button size="xs" onclick={onAdd}>+ Add Model</Button>
		{/if}
	</div>

	<!-- Body -->
	{#if store.loading}
		<div class="space-y-3 p-5">
			<Skeleton class="h-12 w-full" />
			<Skeleton class="h-12 w-full" />
			<Skeleton class="h-12 w-full" />
		</div>
	{:else if store.error && store.models.length === 0}
		<p class="px-5 py-4 text-sm text-destructive">{store.error}</p>
	{:else if !store.sheetOpen && store.models.length === 0}
		<p class="px-5 py-8 text-sm text-muted-foreground text-center">
			No models configured. Click "Add Model" to get started.
		</p>
	{:else}
		<div class="divide-y divide-border">
			{#each store.models as model (model.model_name)}
				<ModelRow {model} onEdit={store.openEdit} onDelete={store.remove} />
			{/each}
		</div>
	{/if}
</div>
