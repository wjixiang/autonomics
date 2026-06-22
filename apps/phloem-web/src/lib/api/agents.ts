import { apiFetch } from './client';
import type { AgentInfo, MessageView } from '$lib/types/agent';

export async function listAgents(): Promise<AgentInfo[]> {
  return apiFetch<AgentInfo[]>('/api/agents');
}

export async function createAgent(identity?: string): Promise<AgentInfo> {
  return apiFetch<AgentInfo>('/api/agents', {
    method: 'POST',
    body: JSON.stringify({ identity }),
  });
}

export async function deleteAgent(id: string): Promise<void> {
  await apiFetch<void>(`/api/agents/${id}`, { method: 'DELETE' });
}

export async function getAgentMessages(id: string): Promise<MessageView[]> {
  return apiFetch<MessageView[]>(`/api/agents/${id}/messages`);
}

export async function sendToAgent(id: string, content: string): Promise<{ agent_id: string; status: string }> {
  return apiFetch<{ agent_id: string; status: string }>(`/api/agents/${id}/send`, {
    method: 'POST',
    body: JSON.stringify({ content }),
  });
}
