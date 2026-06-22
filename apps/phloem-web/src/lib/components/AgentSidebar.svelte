<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import MessageSquare from '@lucide/svelte/icons/message-square';
  import Trash2 from '@lucide/svelte/icons/trash-2';

  interface Props {
    agents: Array<{ id: string; identity: string; status: string; last_active_ts: number | undefined }>;
    selectedId: string | null;
    loading: boolean;
    onSelect: (id: string) => void;
    onNew: () => void;
    onDelete: (id: string) => void;
  }

  let { agents, selectedId, loading, onSelect, onNew, onDelete }: Props = $props();

  function formatTime(ts: number | undefined): string {
    if (!ts) return '';
    const d = new Date(ts);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffMin = Math.floor(diffMs / 60000);
    if (diffMin < 1) return 'just now';
    if (diffMin < 60) return `${diffMin}m ago`;
    const diffHr = Math.floor(diffMin / 60);
    if (diffHr < 24) return `${diffHr}h ago`;
    const diffDay = Math.floor(diffHr / 24);
    if (diffDay < 7) return `${diffDay}d ago`;
    return d.toLocaleDateString();
  }

  function truncate(s: string, len: number): string {
    return s.length > len ? s.slice(0, len) + '…' : s;
  }

  function handleDeleteClick(e: MouseEvent, id: string) {
    e.stopPropagation();
    onDelete(id);
  }
</script>

<aside class="flex h-full w-64 flex-col border-r border-border bg-muted/30">
  <!-- Header -->
  <div class="flex items-center justify-between px-3 py-3 border-b border-border">
    <a href="/" class="text-lg font-semibold tracking-tight text-foreground">phloem</a>
    <button
      onclick={onNew}
      class="inline-flex items-center justify-center rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
      title="New conversation"
    >
      <Plus class="size-4" />
    </button>
  </div>

  <!-- Agent list -->
  <div class="flex-1 overflow-y-auto px-2 py-2">
    {#if loading}
      <div class="space-y-2 px-2">
        {#each Array(3) as _}
          <div class="h-12 rounded-lg bg-muted animate-pulse"></div>
        {/each}
      </div>
    {:else if agents.length === 0}
      <div class="flex flex-col items-center justify-center h-48 text-muted-foreground text-sm">
        <MessageSquare class="size-8 mb-2 opacity-50" />
        <p>No conversations yet</p>
      </div>
    {:else}
      <div class="space-y-1">
        {#each agents as agent (agent.id)}
          <button
            class="group w-full text-left rounded-lg px-3 py-2.5 transition-colors cursor-pointer {selectedId === agent.id
              ? 'bg-accent text-accent-foreground'
              : 'hover:bg-muted/50 text-foreground/80'}"
            onclick={() => onSelect(agent.id)}
          >
            <div class="flex items-start justify-between gap-2">
              <div class="flex-1 min-w-0">
                <p class="text-sm font-medium truncate">
                  {truncate(agent.identity.replace('You are ', '').replace('.', ''), 30) || 'New conversation'}
                </p>
                <p class="text-xs text-muted-foreground mt-0.5">
                  {formatTime(agent.last_active_ts)}
                </p>
              </div>
              <span
                role="button"
                tabindex="0"
                class="opacity-0 group-hover:opacity-100 inline-flex items-center justify-center rounded p-1 text-muted-foreground hover:text-destructive transition-all cursor-pointer"
                onclick={(e) => handleDeleteClick(e, agent.id)}
                title="Delete conversation"
              >
                <Trash2 class="size-3.5" />
              </span>
            </div>
            {#if agent.status === 'RUNNING'}
              <div class="mt-1.5 flex items-center gap-1.5">
                <span class="inline-block size-1.5 rounded-full bg-green-500 animate-pulse"></span>
                <span class="text-xs text-muted-foreground">Running</span>
              </div>
            {/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Footer: settings link -->
  <div class="border-t border-border px-3 py-2">
    <a
      href="/settings"
      class="flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
    >
      <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg>
      Settings
    </a>
  </div>
</aside>
