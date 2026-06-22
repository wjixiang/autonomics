<script lang="ts">
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Alert, AlertDescription } from '$lib/components/ui/alert/index.js';
	import { Collapsible, CollapsibleTrigger, CollapsibleContent } from '$lib/components/ui/collapsible/index.js';
	import { ChevronDown, ChevronRight } from '@lucide/svelte/icons';

	let {
		store
	}: {
		store: ReturnType<typeof import('$lib/stores/models.svelte').createModelStore>;
	} = $props();
</script>

<div class="space-y-4">
	{#if store.formError}
		<Alert variant="destructive">
			<AlertDescription>{store.formError}</AlertDescription>
		</Alert>
	{/if}

	{#if store.providers.length === 0}
		<Alert>
			<AlertDescription>
				No providers configured. Add a provider first, then attach models to it.
			</AlertDescription>
		</Alert>
	{/if}

	<div class="grid grid-cols-2 gap-3">
		<div class="space-y-1.5">
			<Label for="model_name">Model Name *</Label>
			<Input id="model_name" bind:value={store.form.model_name} required />
		</div>
		<div class="space-y-1.5">
			<Label for="provider_id">Provider *</Label>
			<select
				id="provider_id"
				bind:value={store.form.provider_id}
				class="flex h-8 w-full rounded-lg border border-input bg-transparent px-2.5 py-1 text-sm transition-colors focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:outline-none focus-visible:ring-3"
			>
				<option value="" disabled>Select a provider...</option>
				{#each store.providers as p (p.id)}
					<option value={p.id}>{p.name} ({p.provider_type})</option>
				{/each}
			</select>
		</div>
	</div>

	<!-- Advanced toggle -->
	<Collapsible bind:open={store.showAdvanced}>
		<CollapsibleTrigger>
			<button
				type="button"
				class="inline-flex items-center gap-1 px-0 text-xs text-muted-foreground hover:text-foreground"
			>
				{#if store.showAdvanced}
					<ChevronDown class="size-3" />
				{:else}
					<ChevronRight class="size-3" />
				{/if}
				{store.showAdvanced ? 'Hide advanced' : 'Show advanced'}
			</button>
		</CollapsibleTrigger>

		<CollapsibleContent class="space-y-3 border-l-2 border-border pl-3 pt-2">
			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="context_length">Context Length</Label>
					<Input id="context_length" type="number" bind:value={store.form.context_length} />
				</div>
				<div class="space-y-1.5">
					<Label for="max_output_tokens">Max Output Tokens</Label>
					<Input id="max_output_tokens" type="number" bind:value={store.form.max_output_tokens} />
				</div>
			</div>

			<div class="flex flex-wrap gap-x-4 gap-y-2">
				<label class="flex items-center gap-1.5 text-xs">
					<input type="checkbox" bind:checked={store.form.vision_ability} class="rounded" />
					Vision
				</label>
				<label class="flex items-center gap-1.5 text-xs">
					<input
						type="checkbox"
						bind:checked={store.form.supports_function_calling}
						class="rounded"
					/>
					Function Calling
				</label>
				<label class="flex items-center gap-1.5 text-xs">
					<input type="checkbox" bind:checked={store.form.supports_streaming} class="rounded" />
					Streaming
				</label>
				<label class="flex items-center gap-1.5 text-xs">
					<input type="checkbox" bind:checked={store.form.supports_thinking} class="rounded" />
					Thinking
				</label>
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="input_price">Input Price (per 1M tokens)</Label>
					<Input
						id="input_price"
						type="number"
						step="0.001"
						bind:value={store.form.input_token_price}
					/>
				</div>
				<div class="space-y-1.5">
					<Label for="output_price">Output Price (per 1M tokens)</Label>
					<Input
						id="output_price"
						type="number"
						step="0.001"
						bind:value={store.form.output_token_price}
					/>
				</div>
			</div>
		</CollapsibleContent>
	</Collapsible>
</div>
