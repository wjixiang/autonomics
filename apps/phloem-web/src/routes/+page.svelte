<script lang="ts">
  import { onMount } from 'svelte';
  import { createChatStore } from '$lib/stores/chat.svelte';
  import { createAgentsStore } from '$lib/stores/agents.svelte';
  import { reconnectAgent, disconnect } from '$lib/stores/sse.svelte';
  import AgentSidebar from '$lib/components/AgentSidebar.svelte';
  import Markdown from '$lib/components/Markdown.svelte';

  const chat = createChatStore();
  const agents = createAgentsStore((history) => chat.loadHistory(history));

  let inputText = $state('');

  onMount(async () => {
    // Load agent list and connect SSE to default stream initially
    await agents.load();
    if (agents.agents.length > 0) {
      // Auto-select the first (most recent) agent
      await agents.select(agents.agents[0].id);
    }
    return () => {
      disconnect();
    };
  });

  // React when selected agent changes — reconnect SSE
  $effect(() => {
    const id = agents.selectedId;
    if (id) {
      chat.setAgentId(id);
      disconnect();
      reconnectAgent((eventType, eventData, agentId) => {
        chat.handleSseEvent(eventType, eventData, id);
      }, id);
    }
  });

  async function handleNew() {
    await agents.create();
  }

  async function handleSelect(id: string) {
    if (agents.selectedId === id) return;
    await agents.select(id);
  }

  async function handleDelete(id: string) {
    if (confirm('Delete this conversation? This cannot be undone.')) {
      await agents.remove(id);
    }
  }

  async function handleSend() {
    const text = inputText.trim();
    if (!text || chat.isStreaming) return;
    inputText = '';
    await chat.send(text);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }
</script>

<div class="flex h-full w-full">
  <!-- Sidebar -->
  <AgentSidebar
    agents={agents.agents}
    selectedId={agents.selectedId}
    loading={agents.loading}
    onSelect={handleSelect}
    onNew={handleNew}
    onDelete={handleDelete}
  />

  <!-- Chat area -->
  <div class="flex-1 flex flex-col min-w-0">
    {#if chat.error}
      <div class="mx-4 mt-4 p-3 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
        {chat.error}
      </div>
    {/if}

    <!-- Message area -->
    <div class="flex-1 overflow-y-auto px-4 pb-4">
      {#if agents.agents.length === 0}
        <div class="flex items-center justify-center h-full text-slate-400">
          <div class="text-center">
            <p class="text-lg mb-1">No conversations</p>
            <p class="text-sm">Click <strong>+</strong> in the sidebar to start one.</p>
          </div>
        </div>
      {:else if chat.messages.length === 0}
        <div class="flex items-center justify-center h-[60%] text-slate-400">
          <p>Send a message to start chatting.</p>
        </div>
      {:else}
        {#each chat.messages as msg (msg.id)}
          <div class="mb-4 {msg.role === 'user' ? 'text-right' : ''}">
            <div class="text-xs text-slate-400 mb-1">
              {msg.role === 'user' ? 'You' : msg.role === 'system' ? 'Context' : 'Agent'}
            </div>
            <div
              class="inline-block text-left max-w-[85%] p-3 rounded-lg break-words {msg.role ===
              'user'
                ? 'whitespace-pre-wrap bg-blue-50'
                : msg.role === 'system'
                  ? 'bg-slate-100 text-slate-500 text-xs italic'
                  : 'bg-slate-50'}"
            >
              {#if msg.thinking}
                <details class="mb-2 text-sm text-slate-500">
                  <summary>Thinking</summary>
                  <pre class="whitespace-pre-wrap mt-1">{msg.thinking}</pre>
                </details>
              {/if}
              {#if msg.toolCalls && msg.toolCalls.length > 0}
                <div class="mb-2">
                  {#each msg.toolCalls as tc}
                    <details
                      class="bg-yellow-50 border border-yellow-200 rounded-md p-2 mb-2 text-sm"
                    >
                      <summary>🔧 {tc.name}</summary>
                      <pre
                        class="whitespace-pre-wrap overflow-x-auto text-xs">{JSON.stringify(
                          tc.input,
                          null,
                          2,
                        )}</pre>
                      {#if tc.result}
                        <pre
                          class="whitespace-pre-wrap overflow-x-auto text-xs text-green-800">{tc
                            .result.content}</pre>
                      {/if}
                    </details>
                  {/each}
                </div>
              {/if}
              <div class="text-content leading-relaxed">
                {#if msg.role === 'assistant'}
                  <Markdown content={msg.content} />
                {:else}
                  {msg.content}
                {/if}
                {#if msg.isStreaming}
                  <span class="cursor">▌</span>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      {/if}
    </div>

    <!-- Input area -->
    <div class="flex gap-2 px-4 py-3 border-t border-slate-200 shrink-0">
      <textarea
        bind:value={inputText}
        onkeydown={handleKeydown}
        placeholder={agents.selectedId ? "Type a message... (Enter to send)" : "Select a conversation"}
        rows="3"
        disabled={!agents.selectedId}
        class="flex-1 resize-none p-3 border border-slate-300 rounded-lg font-[inherit] text-sm leading-normal focus:outline-none focus:border-blue-500 disabled:opacity-50"
      ></textarea>
      <button
        onclick={handleSend}
        disabled={chat.isStreaming || !inputText.trim() || !agents.selectedId}
        class="px-6 bg-blue-500 text-white border-none rounded-lg cursor-pointer text-sm disabled:opacity-50 disabled:cursor-not-allowed"
      >
        Send
      </button>
    </div>
  </div>
</div>

<style>
  .cursor {
    animation: blink 0.7s infinite;
    color: #3b82f6;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
</style>
