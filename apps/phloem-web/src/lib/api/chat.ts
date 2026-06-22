import { apiFetch } from './client';

export interface SendResponse {
	agent_id: string;
	status: string;
}

export async function sendChatMessage(content: string): Promise<SendResponse> {
	return apiFetch<SendResponse>('/api/chat/send', {
		method: 'POST',
		body: JSON.stringify({ content })
	});
}
