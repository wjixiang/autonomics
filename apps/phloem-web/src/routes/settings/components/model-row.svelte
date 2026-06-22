<script lang="ts">
	import type { ModelInfo } from '$lib/types/model';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';

	let {
		model,
		onEdit,
		onDelete
	}: {
		model: ModelInfo;
		onEdit: (model: ModelInfo) => void;
		onDelete: (model: ModelInfo) => void;
	} = $props();
</script>

<div class="flex items-center justify-between px-5 py-3 transition-colors hover:bg-muted/50">
	<div class="min-w-0 flex-1">
		<div class="flex items-center gap-2">
			<span class="text-sm font-medium">{model.model_name}</span>
			<Badge variant="secondary">{model.provider_name || '--'}</Badge>
		</div>
		<p class="mt-0.5 text-xs text-muted-foreground">
			{(model.context_length / 1000).toFixed(0)}K ctx · {(model.max_output_tokens / 1000).toFixed(0)}K out
		</p>
	</div>
	<div class="ml-4 flex items-center gap-2">
		<Button variant="outline" size="xs" onclick={() => onEdit(model)}>Edit</Button>
		<Button
			variant="destructive"
			size="xs"
			onclick={() => {
				if (confirm(`Remove model "${model.model_name}" from the pool?`)) {
					onDelete(model);
				}
			}}
		>Delete</Button
		>
	</div>
</div>
