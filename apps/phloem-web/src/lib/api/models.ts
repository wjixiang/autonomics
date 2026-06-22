import { apiFetch } from './client';
import type { ModelInfo, ModelRequest } from '$lib/types/model';

export async function listModels(): Promise<ModelInfo[]> {
	return apiFetch<ModelInfo[]>('/api/models');
}

export async function createModel(req: ModelRequest): Promise<ModelInfo> {
	return apiFetch<ModelInfo>('/api/models', {
		method: 'POST',
		body: JSON.stringify(req)
	});
}

export async function updateModel(name: string, req: ModelRequest): Promise<ModelInfo> {
	return apiFetch<ModelInfo>(`/api/models/${encodeURIComponent(name)}`, {
		method: 'PUT',
		body: JSON.stringify(req)
	});
}

export async function deleteModel(name: string): Promise<void> {
	return apiFetch<void>(`/api/models/${encodeURIComponent(name)}`, {
		method: 'DELETE'
	});
}
