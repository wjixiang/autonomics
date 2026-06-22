import { listModels, createModel, updateModel, deleteModel } from '$lib/api/models';
import { listProviders } from '$lib/api/providers';
import type { ModelInfo, ModelRequest, ProviderResponse } from '$lib/types/model';

const DEFAULT_FORM: ModelRequest = {
	model_name: '',
	provider_id: '',
	context_length: 200000,
	max_output_tokens: 8192,
	vision_ability: false,
	supports_function_calling: true,
	supports_streaming: true,
	supports_thinking: false,
	input_token_price: 0,
	output_token_price: 0
};

/** Model management state — each page creates its own instance. */
export function createModelStore() {
	let models = $state<ModelInfo[]>([]);
	let providers = $state<ProviderResponse[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let editing = $state<ModelInfo | null>(null);
	let isAdding = $state(false);
	let showAdvanced = $state(false);
	let formError = $state<string | null>(null);
	let form = $state<ModelRequest>({ ...DEFAULT_FORM });
	let sheetOpen = $state(false);

	async function loadProviders() {
		try {
			providers = await listProviders();
		} catch (e) {
			console.error('Failed to load providers', e);
		}
	}

	async function load() {
		try {
			models = await listModels();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	function openAdd() {
		form = { ...DEFAULT_FORM, provider_id: providers[0]?.id ?? '' };
		editing = null;
		isAdding = true;
		showAdvanced = false;
		formError = null;
		sheetOpen = true;
	}

	function openEdit(model: ModelInfo) {
		form = {
			model_name: model.model_name,
			provider_id: model.provider_id,
			context_length: model.context_length,
			max_output_tokens: model.max_output_tokens,
			vision_ability: model.vision_ability,
			supports_function_calling: model.supports_function_calling,
			supports_streaming: model.supports_streaming,
			supports_thinking: model.supports_thinking,
			input_token_price: model.input_token_price,
			output_token_price: model.output_token_price
		};
		editing = model;
		isAdding = false;
		showAdvanced = false;
		formError = null;
		sheetOpen = true;
	}

	function cancel() {
		sheetOpen = false;
	}

	async function save() {
		formError = null;
		if (!form.provider_id) {
			formError = 'Select a provider';
			return;
		}
		try {
			if (isAdding) {
				await createModel(form);
			} else if (editing) {
				await updateModel(editing.model_name, form);
			}
			sheetOpen = false;
			await load();
		} catch (e) {
			formError = e instanceof Error ? e.message : String(e);
		}
	}

	async function remove(model: ModelInfo) {
		try {
			await deleteModel(model.model_name);
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
	}

	function clearError() {
		error = null;
	}

	return {
		get models() {
			return models;
		},
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
		get showAdvanced() {
			return showAdvanced;
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
		set showAdvanced(v: boolean) {
			showAdvanced = v;
		},
		load,
		loadProviders,
		openAdd,
		openEdit,
		cancel,
		save,
		remove,
		clearError
	};
}
