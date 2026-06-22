<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
		SheetFooter
	} from '$lib/components/ui/sheet/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import ProviderForm from './provider-form.svelte';

	let {
		store,
		onModelsChanged
	}: {
		store: ReturnType<typeof import('$lib/stores/providers.svelte').createProviderStore>;
		onModelsChanged?: () => Promise<void> | void;
	} = $props();
</script>

<Sheet bind:open={store.sheetOpen}>
	<SheetContent>
		<SheetHeader>
			<SheetTitle>{store.isAdding ? 'Add Provider' : 'Edit Provider'}</SheetTitle>
			<SheetDescription>
				{store.isAdding
					? 'Add a provider from a preset (with models) or configure manually.'
					: `Editing ${store.editing?.name}`}
			</SheetDescription>
		</SheetHeader>

		<div class="py-4">
			<ProviderForm {store} {onModelsChanged} />
		</div>

		<SheetFooter>
			<Button variant="outline" onclick={store.cancel}>Cancel</Button>
			{#if store.isAdding && store.presetMode}
				<Button onclick={() => store.saveFromPresets(onModelsChanged)}>
					Create{store.checkedModels.size > 0 ? ` + ${store.checkedModels.size} models` : ''}
				</Button>
			{:else}
				<Button onclick={store.save}>
					{store.isAdding ? 'Create' : 'Save'}
				</Button>
			{/if}
		</SheetFooter>
	</SheetContent>
</Sheet>
