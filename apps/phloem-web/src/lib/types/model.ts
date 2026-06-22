export type AuthMethod = 'Anthropic' | 'Bearer';

// ── Provider instances (master: connection config lives here) ──────────

export interface ProviderResponse {
	id: string;
	name: string;
	provider_type: string;
	base_url: string;
	api_key_masked: boolean;
	auth_method: AuthMethod;
}

export interface ProviderRequest {
	name: string;
	provider_type: string;
	base_url: string;
	api_key?: string;
	auth_method: AuthMethod;
}

// ── Models (reference a provider by id) ────────────────────────────────

export interface ModelInfo {
	model_name: string;
	provider_id: string;
	/** Resolved provider instance display name (server-side join). */
	provider_name: string;
	context_length: number;
	max_output_tokens: number;
	vision_ability: boolean;
	supports_function_calling: boolean;
	supports_streaming: boolean;
	supports_thinking: boolean;
	input_token_price: number;
	output_token_price: number;
}

export interface ModelRequest {
	model_name: string;
	provider_id: string;
	context_length: number;
	max_output_tokens: number;
	vision_ability: boolean;
	supports_function_calling: boolean;
	supports_streaming: boolean;
	supports_thinking: boolean;
	input_token_price: number;
	output_token_price: number;
}

// ── Provider type presets (from GET /api/provider-types) ───────────────

export interface ProviderTypeMeta {
	type_name: string;
	display_name: string;
	auth_method: AuthMethod;
	endpoint_presets: EndpointPreset[];
	models: ModelPreset[];
}

export interface EndpointPreset {
	label: string;
	url: string;
}

export interface ModelPreset {
	model_name: string;
	provider_name: string;
	context_length: number;
	max_output_tokens: number;
	vision_ability: boolean;
	supports_function_calling: boolean;
	supports_streaming: boolean;
	supports_thinking: boolean;
	input_token_price: number;
	output_token_price: number;
}
