<script lang="ts">
	import { createModelStore } from '$lib/stores/models.svelte';
	import { createProviderStore } from '$lib/stores/providers.svelte';
	import ModelList from './components/model-list.svelte';
	import ModelFormSheet from './components/model-form-sheet.svelte';
	import ProviderList from './components/provider-list.svelte';
	import ProviderFormSheet from './components/provider-form-sheet.svelte';

	const providers = createProviderStore();
	const models = createModelStore();

	$effect(() => {
		providers.load();
		providers.loadProviderTypes();
		models.load();
		models.loadProviders();
	});

	// When a provider is added via the preset flow, models may have been attached — refresh.
	async function refreshModels() {
		await models.load();
		await models.loadProviders();
	}
</script>

<div class="mx-auto max-w-3xl space-y-6 p-6">
	<h1 class="text-xl font-semibold">Settings</h1>

	<ProviderList store={providers} onAdd={providers.openAdd} />
	<ProviderFormSheet store={providers} onModelsChanged={refreshModels} />

	<ModelList store={models} onAdd={models.openAdd} />
	<ModelFormSheet store={models} />
</div>
