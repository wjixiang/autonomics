<script lang="ts">
	import type { ProviderResponse } from '$lib/types/model';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';

	let {
		provider,
		onEdit,
		onDelete
	}: {
		provider: ProviderResponse;
		onEdit: (provider: ProviderResponse) => void;
		onDelete: (provider: ProviderResponse) => void;
	} = $props();

	function truncateUrl(url: string, max = 48): string {
		return url.length > max ? url.slice(0, max) + '...' : url;
	}
</script>

<div class="flex items-center justify-between px-5 py-3 transition-colors hover:bg-muted/50">
	<div class="min-w-0 flex-1">
		<div class="flex items-center gap-2">
			<span class="text-sm font-medium">{provider.name}</span>
			<Badge variant="secondary">{provider.provider_type}</Badge>
			<Badge variant="outline">{provider.auth_method}</Badge>
			{#if provider.api_key_masked}
				<Badge variant="outline">🔑 key set</Badge>
			{/if}
		</div>
		<p class="mt-0.5 truncate text-xs text-muted-foreground">
			{truncateUrl(provider.base_url)}
		</p>
	</div>
	<div class="ml-4 flex items-center gap-2">
		<Button variant="outline" size="xs" onclick={() => onEdit(provider)}>Edit</Button>
		<Button
			variant="destructive"
			size="xs"
			onclick={() => {
				if (
					confirm(
						`Remove provider "${provider.name}"? Models referencing it must be removed first.`
					)
				) {
					onDelete(provider);
				}
			}}
		>Delete</Button
		>
	</div>
</div>
