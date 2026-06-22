import {
	listProviders,
	createProvider,
	updateProvider,
	deleteProvider,
	listProviderTypes
} from '$lib/api/providers';
import { createModel } from '$lib/api/models';
import type {
	AuthMethod,
	ProviderRequest,
	ProviderResponse,
	ProviderTypeMeta
} from '$lib/types/model';

const DEFAULT_FORM: ProviderRequest = {
	name: '',
	provider_type: '',
	base_url: '',
	auth_method: 'Anthropic'
};

/** Provider management state — each page creates its own instance. */
export function createProviderStore() {
	let providers = $state<ProviderResponse[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let editing = $state<ProviderResponse | null>(null);
	let isAdding = $state(false);
	let formError = $state<string | null>(null);
	let form = $state<ProviderRequest>({ ...DEFAULT_FORM });
	let sheetOpen = $state(false);

	// Preset mode: pick a provider type + endpoint + key, optionally attach models.
	let presetMode = $state(true);
	let providerTypes = $state<ProviderTypeMeta[]>([]);
	let selectedTypeIndex = $state(-1);
	let selectedEndpointIndex = $state(0);
	let checkedModels = $state<Set<string>>(new Set());
	let apiKey = $state('');

	async function loadProviderTypes() {
		try {
			providerTypes = await listProviderTypes();
		} catch (e) {
			console.error('Failed to load provider types', e);
		}
	}

	async function load() {
		try {
			providers = await listProviders();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	function openAdd() {
		form = { ...DEFAULT_FORM };
		editing = null;
		isAdding = true;
		formError = null;
		presetMode = true;
		selectedTypeIndex = -1;
		selectedEndpointIndex = 0;
		checkedModels = new Set();
		apiKey = '';
		sheetOpen = true;
	}

	function openEdit(provider: ProviderResponse) {
		const { api_key_masked, ...rest } = provider;
		form = { ...rest };
		editing = provider;
		isAdding = false;
		formError = null;
		presetMode = false;
		sheetOpen = true;
	}

	function cancel() {
		sheetOpen = false;
	}

	/** Custom mode: create or update a single provider. */
	async function save() {
		formError = null;
		if (!form.name.trim()) {
			formError = 'Name is required';
			return;
		}
		if (!form.base_url.trim()) {
			formError = 'Base URL is required';
			return;
		}
		try {
			if (isAdding) {
				await createProvider({ ...form, api_key: apiKey || undefined });
			} else if (editing) {
				await updateProvider(editing.id, form);
			}
			sheetOpen = false;
			await load();
		} catch (e) {
			formError = e instanceof Error ? e.message : String(e);
		}
	}

	/** Preset mode: create a provider, then attach each checked preset model. */
	async function saveFromPresets(onModelsChanged?: () => Promise<void> | void) {
		formError = null;
		const meta = providerTypes[selectedTypeIndex];
		if (!meta) {
			formError = 'Select a provider type';
			return;
		}

		const base_url =
			meta.endpoint_presets.length > 0
				? (meta.endpoint_presets[selectedEndpointIndex]?.url ?? '')
				: form.base_url;

		if (!form.name.trim()) {
			formError = 'Provider name is required';
			return;
		}
		if (!base_url.trim()) {
			formError = 'Base URL is required';
			return;
		}
		if (!apiKey.trim()) {
			formError = 'API key is required';
			return;
		}

		try {
			// 1. Create the provider instance.
			const created = await createProvider({
				name: form.name,
				provider_type: meta.type_name,
				base_url,
				api_key: apiKey,
				auth_method: meta.auth_method
			});

			// 2. Attach each checked preset model, referencing the new provider.
			for (const name of checkedModels) {
				const preset = meta.models.find((m) => m.model_name === name);
				if (!preset) continue;
				await createModel({
					model_name: preset.model_name,
					provider_id: created.id,
					context_length: preset.context_length,
					max_output_tokens: preset.max_output_tokens,
					vision_ability: preset.vision_ability,
					supports_function_calling: preset.supports_function_calling,
					supports_streaming: preset.supports_streaming,
					supports_thinking: preset.supports_thinking,
					input_token_price: preset.input_token_price,
					output_token_price: preset.output_token_price
				});
			}

			sheetOpen = false;
			await load();
			await onModelsChanged?.();
		} catch (e) {
			formError = e instanceof Error ? e.message : String(e);
		}
	}

	async function remove(provider: ProviderResponse) {
		try {
			await deleteProvider(provider.id);
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
	}

	function clearError() {
		error = null;
	}

	function togglePresetModel(name: string) {
		const next = new Set(checkedModels);
		if (next.has(name)) {
			next.delete(name);
		} else {
			next.add(name);
		}
		checkedModels = next;
	}

	return {
		get providers() {
			return providers;
		},
		get loading() {
			return loading;
		},
		get error() {
			return error;
		},
		get editing() {
			return editing;
		},
		get isAdding() {
			return isAdding;
		},
		get formError() {
			return formError;
		},
		get form() {
			return form;
		},
		get sheetOpen() {
			return sheetOpen;
		},
		set sheetOpen(v: boolean) {
			sheetOpen = v;
		},
		// Preset-mode state
		get presetMode() {
			return presetMode;
		},
		set presetMode(v: boolean) {
			presetMode = v;
		},
		get providerTypes() {
			return providerTypes;
		},
		get selectedTypeIndex() {
			return selectedTypeIndex;
		},
		set selectedTypeIndex(v: number) {
			selectedTypeIndex = v;
		},
		get selectedEndpointIndex() {
			return selectedEndpointIndex;
		},
		set selectedEndpointIndex(v: number) {
			selectedEndpointIndex = v;
		},
		get checkedModels() {
			return checkedModels;
		},
		set checkedModels(v: Set<string>) {
			checkedModels = v;
		},
		get apiKey() {
			return apiKey;
		},
		set apiKey(v: string) {
			apiKey = v;
		},
		load,
		loadProviderTypes,
		openAdd,
		openEdit,
		cancel,
		save,
		saveFromPresets,
		remove,
		clearError,
		togglePresetModel
	};
}

export type { AuthMethod };
