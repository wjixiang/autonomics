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
	import ModelForm from './model-form.svelte';

	let {
		store
	}: {
		store: ReturnType<typeof import('$lib/stores/models.svelte').createModelStore>;
	} = $props();
</script>

<Sheet bind:open={store.sheetOpen}>
	<SheetContent>
		<SheetHeader>
			<SheetTitle>{store.isAdding ? 'Add Model' : 'Edit Model'}</SheetTitle>
			<SheetDescription>
				{store.isAdding
					? 'Attach a model to an existing provider.'
					: `Editing ${store.editing?.model_name}`}
			</SheetDescription>
		</SheetHeader>

		<div class="py-4">
			<ModelForm {store} />
		</div>

		<SheetFooter>
			<Button variant="outline" onclick={store.cancel}>Cancel</Button>
			<Button onclick={store.save}>
				{store.isAdding ? 'Create' : 'Save'}
			</Button>
		</SheetFooter>
	</SheetContent>
</Sheet>
