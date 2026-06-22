import { apiFetch } from './client';
import type { ProviderRequest, ProviderResponse, ProviderTypeMeta } from '$lib/types/model';

export async function listProviderTypes(): Promise<ProviderTypeMeta[]> {
	return apiFetch<ProviderTypeMeta[]>('/api/provider-types');
}

export async function listProviders(): Promise<ProviderResponse[]> {
	return apiFetch<ProviderResponse[]>('/api/providers');
}

export async function createProvider(req: ProviderRequest): Promise<ProviderResponse> {
	return apiFetch<ProviderResponse>('/api/providers', {
		method: 'POST',
		body: JSON.stringify(req)
	});
}

export async function updateProvider(id: string, req: ProviderRequest): Promise<ProviderResponse> {
	return apiFetch<ProviderResponse>(`/api/providers/${encodeURIComponent(id)}`, {
		method: 'PUT',
		body: JSON.stringify(req)
	});
}

export async function deleteProvider(id: string): Promise<void> {
	return apiFetch<void>(`/api/providers/${encodeURIComponent(id)}`, {
		method: 'DELETE'
	});
}
