<script lang="ts">
  import { browser } from '$app/environment';
  import { renderMarkdown } from '$lib/markdown';

  let { content }: { content: string } = $props();

  let html = $state('');

  // Re-render on every content change (including streaming deltas). The first
  // call warms the Shiki singleton; subsequent calls are fast.
  $effect(() => {
    const c = content;
    if (!browser || !c) {
      html = '';
      return;
    }
    let cancelled = false;
    renderMarkdown(c).then((h) => {
      if (!cancelled) html = h;
    });
    return () => {
      cancelled = true;
    };
  });
</script>

<div class="markdown-body">{@html html}</div>
