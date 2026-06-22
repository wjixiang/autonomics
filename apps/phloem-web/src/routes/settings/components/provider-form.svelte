<script lang="ts">
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Alert, AlertDescription } from '$lib/components/ui/alert/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';

	let {
		store,
		onModelsChanged
	}: {
		store: ReturnType<typeof import('$lib/stores/providers.svelte').createProviderStore>;
		onModelsChanged?: () => Promise<void> | void;
	} = $props();

	let selectedType = $derived(
		store.selectedTypeIndex >= 0 ? store.providerTypes[store.selectedTypeIndex] : null
	);
</script>

<div class="space-y-4">
	{#if store.formError}
		<Alert variant="destructive">
			<AlertDescription>{store.formError}</AlertDescription>
		</Alert>
	{/if}

	{#if store.isAdding}
		<div class="flex gap-1 rounded-lg border border-border p-0.5">
			<button
				type="button"
				class="flex-1 rounded-md px-3 py-1 text-xs font-medium transition-colors {store.presetMode
					? 'bg-primary text-primary-foreground'
					: 'text-muted-foreground hover:text-foreground'}"
				onclick={() => (store.presetMode = true)}
			>
				From Preset
			</button>
			<button
				type="button"
				class="flex-1 rounded-md px-3 py-1 text-xs font-medium transition-colors {!store.presetMode
					? 'bg-primary text-primary-foreground'
					: 'text-muted-foreground hover:text-foreground'}"
				onclick={() => (store.presetMode = false)}
			>
				Custom
			</button>
		</div>
	{/if}

	{#if store.isAdding && store.presetMode}
		<!-- ── Preset mode ──────────────────────────────────────────── -->
		<div class="space-y-3">
			<div class="space-y-1.5">
				<Label for="provider_type">Provider Type</Label>
				<select
					id="provider_type"
					class="flex h-8 w-full rounded-lg border border-input bg-transparent px-2.5 py-1 text-sm transition-colors focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:outline-none focus-visible:ring-3"
					bind:value={store.selectedTypeIndex}
					onchange={(e) => {
						const idx = parseInt(e.currentTarget.value);
						store.selectedTypeIndex = idx;
						store.selectedEndpointIndex = 0;
						store.checkedModels = new Set();
					}}
				>
					<option value={-1} disabled>Select a provider...</option>
					{#each store.providerTypes as pt, i}
						<option value={i}>{pt.display_name}</option>
					{/each}
				</select>
			</div>

			{#if selectedType}
				<div class="space-y-1.5">
					<Label for="provider_name">Provider Name *</Label>
					<Input id="provider_name" bind:value={store.form.name} placeholder="e.g. deepseek-prod" />
				</div>

				<div class="flex gap-3 text-xs text-muted-foreground">
					<Badge variant="outline">{selectedType.auth_method}</Badge>
					{#if selectedType.endpoint_presets.length > 0}
						<span>
							Endpoint:
							<select
								class="inline border-0 bg-transparent p-0 text-xs font-normal text-muted-foreground underline decoration-border underline-offset-2 focus-visible:ring-0"
								bind:value={store.selectedEndpointIndex}
							>
								{#each selectedType.endpoint_presets as ep, i}
									<option value={i}>{ep.label}</option>
								{/each}
							</select>
						</span>
					{:else}
						<span>Custom base URL required</span>
					{/if}
				</div>

				<div class="space-y-1.5">
					<Label for="preset_api_key">API Key</Label>
					<Input
						id="preset_api_key"
						type="password"
						bind:value={store.apiKey}
						placeholder="Enter API key"
					/>
				</div>

				{#if selectedType.endpoint_presets.length === 0}
					<div class="space-y-1.5">
						<Label for="preset_base_url">Base URL *</Label>
						<Input
							id="preset_base_url"
							placeholder="https://..."
							bind:value={store.form.base_url}
						/>
					</div>
				{/if}

				<div class="space-y-2">
					<p class="text-xs font-medium text-muted-foreground">Attach models</p>
					<div class="max-h-64 space-y-1 overflow-y-auto">
						{#each selectedType.models as preset (preset.model_name)}
							<label
								class="flex cursor-pointer items-start gap-2 rounded-md border px-3 py-2 transition-colors hover:bg-muted/50 {store.checkedModels.has(preset.model_name)
									? 'border-primary bg-primary/5'
									: 'border-border'}"
							>
								<input
									type="checkbox"
									checked={store.checkedModels.has(preset.model_name)}
									onchange={() => store.togglePresetModel(preset.model_name)}
									class="mt-0.5 rounded"
								/>
								<div class="min-w-0 flex-1">
									<div class="flex items-center gap-1.5">
										<span class="text-sm font-medium">{preset.model_name}</span>
									</div>
									<div
										class="mt-0.5 flex flex-wrap gap-x-2 gap-y-0.5 text-[10px] text-muted-foreground"
									>
										<span>{(preset.context_length / 1000).toFixed(0)}K ctx</span>
										<span>{(preset.max_output_tokens / 1000).toFixed(0)}K out</span>
										{#if preset.vision_ability}<span>👁 vision</span>{/if}
										{#if preset.supports_thinking}<span>💭 thinking</span>{/if}
										{#if preset.input_token_price > 0 || preset.output_token_price > 0}
											<span>${preset.input_token_price}/${preset.output_token_price}</span>
										{/if}
									</div>
								</div>
							</label>
						{/each}
					</div>
				</div>
			{/if}
		</div>
	{:else}
		<!-- ── Custom / Edit mode ────────────────────────────────────── -->
		<div class="space-y-3">
			<div class="space-y-1.5">
				<Label for="name">Provider Name *</Label>
				<Input id="name" bind:value={store.form.name} required />
			</div>

			<div class="space-y-1.5">
				<Label for="provider_type">Provider Type</Label>
				<select
					id="provider_type"
					bind:value={store.form.provider_type}
					class="flex h-8 w-full rounded-lg border border-input bg-transparent px-2.5 py-1 text-sm transition-colors focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:outline-none focus-visible:ring-3"
				>
					<option value="">—</option>
					{#each store.providerTypes as pt}
						<option value={pt.type_name}>{pt.display_name}</option>
					{/each}
				</select>
			</div>

			<div class="space-y-1.5">
				<Label for="base_url">Base URL *</Label>
				<Input id="base_url" bind:value={store.form.base_url} required />
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="auth_method">Auth Method</Label>
					<select
						id="auth_method"
						bind:value={store.form.auth_method}
						class="flex h-8 w-full rounded-lg border border-input bg-transparent px-2.5 py-1 text-sm transition-colors focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:outline-none focus-visible:ring-3"
					>
						<option value="Anthropic">Anthropic</option>
						<option value="Bearer">Bearer</option>
					</select>
				</div>
				<div class="space-y-1.5">
					<Label for="api_key">API Key</Label>
					<Input
						id="api_key"
						type="password"
						bind:value={store.apiKey}
						placeholder={store.editing ? 'Leave blank to keep current' : 'Enter API key'}
					/>
				</div>
			</div>
		</div>
	{/if}
</div>
