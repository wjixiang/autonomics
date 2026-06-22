import { browser } from '$app/environment';
import DOMPurify from 'dompurify';
import { Marked } from 'marked';
import { getSingletonHighlighter, type Highlighter } from 'shiki';

const THEME = 'github-dark';

// Common languages pre-loaded into the singleton; unknown langs fall back to
// an escaped <pre><code> block (see renderer.code below).
const LANGS = [
	'bash',
	'rust',
	'typescript',
	'tsx',
	'javascript',
	'jsx',
	'python',
	'json',
	'toml',
	'yaml',
	'html',
	'css',
	'sql',
	'markdown',
	'svelte',
	'go'
] as const;

let hlPromise: Promise<Highlighter> | null = null;

/** Lazily create (and cache) the singleton Shiki highlighter. Browser-only. */
export function getHighlighter(): Promise<Highlighter> {
	if (!browser) return Promise.reject(new Error('shiki requires a browser'));
	if (!hlPromise) {
		hlPromise = getSingletonHighlighter({ themes: [THEME], langs: [...LANGS] });
	}
	return hlPromise;
}

// The renderer closes over the loaded highlighter; `renderMarkdown` guarantees
// it is set before `marked.parse` runs.
let activeHighlighter: Highlighter | null = null;

function escapeHtml(s: string): string {
	return s
		.replace(/&/g, '&amp;')
		.replace(/</g, '&lt;')
		.replace(/>/g, '&gt;');
}

const marked = new Marked({ gfm: true, breaks: true });

marked.use({
	renderer: {
		code({ text, lang }: { text: string; lang?: string }): string {
			const language = (lang ?? '').trim().split(/\s+/)[0];
			const loaded = activeHighlighter?.getLoadedLanguages() ?? [];
			if (language && loaded.includes(language)) {
				try {
					return (
						activeHighlighter?.codeToHtml(text, { lang: language, theme: THEME }) ?? ''
					);
				} catch {
					/* fall through to plain */
				}
			}
			return `<pre class="not-highlighted"><code>${escapeHtml(text)}</code></pre>`;
		}
	}
});

/**
 * Parse markdown → highlight code with Shiki → sanitize with DOMPurify.
 * Returns '' on the server (DOMPurify needs a DOM). Must be awaited because
 * the first call warms the Shiki singleton.
 */
export async function renderMarkdown(src: string): Promise<string> {
	if (!browser || !src) return '';
	if (!activeHighlighter) activeHighlighter = await getHighlighter();
	const raw = await marked.parse(src);
	return DOMPurify.sanitize(raw, {
		USE_PROFILES: { html: true },
		ADD_ATTR: ['target']
	});
}
