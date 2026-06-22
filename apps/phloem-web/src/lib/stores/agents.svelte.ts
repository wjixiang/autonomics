import { listAgents, createAgent, deleteAgent } from '$lib/api/agents';
import { getAgentMessages } from '$lib/api/agents';
import type { AgentInfo } from '$lib/types/agent';
import type { ChatMessage } from '$lib/types/chat';

export function createAgentsStore(onHistoryLoaded: (messages: ChatMessage[]) => void) {
  let agents = $state<AgentInfo[]>([]);
  let selectedId = $state<string | null>(null);
  let loading = $state(false);
  let error = $state('');

  async function load() {
    loading = true;
    error = '';
    try {
      agents = await listAgents();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function select(id: string) {
    selectedId = id;
    // Load conversation history for the selected agent
    try {
      const messages = await getAgentMessages(id);
      const chatMessages = messageViewsToChatMessages(messages);
      onHistoryLoaded(chatMessages);
    } catch (e) {
      console.error('Failed to load agent messages:', e);
      onHistoryLoaded([]);
    }
  }

  async function create(identity?: string) {
    error = '';
    try {
      const agent = await createAgent(identity);
      agents.unshift(agent);
      await select(agent.id);
    } catch (e) {
      error = String(e);
    }
  }

  async function remove(id: string) {
    error = '';
    try {
      await deleteAgent(id);
      agents = agents.filter((a) => a.id !== id);
      if (selectedId === id) {
        selectedId = null;
        onHistoryLoaded([]);
      }
    } catch (e) {
      error = String(e);
    }
  }

  return {
    get agents() { return agents; },
    get selectedId() { return selectedId; },
    get loading() { return loading; },
    get error() { return error; },
    load,
    select,
    create,
    remove,
  };
}

function messageViewsToChatMessages(views: Array<{
  id: string;
  role: string;
  content: string;
  thinking?: string;
  tool_calls: Array<{ name: string; input: unknown; result?: { ok: boolean; content: string } }>;
}>): ChatMessage[] {
  return views.map((v) => ({
    id: v.id,
    role: v.role as 'user' | 'assistant',
    content: v.content,
    thinking: v.thinking,
    toolCalls: (v.tool_calls ?? []).map((tc) => ({
      name: tc.name,
      input: tc.input,
      result: tc.result ? { ok: tc.result.ok, content: tc.result.content } : undefined,
    })),
    isStreaming: false,
    timestamp: 0,
  }));
}
