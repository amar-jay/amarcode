export interface ProxyEnv {
	OPENROUTER_MODEL: string;
	OPENROUTER_TRANSCRIPTION_MODEL: string;
	OPENROUTER_API_KEY: string;
}

const inferencePaths = new Set(['/chat/completions', '/completions', '/responses', '/embeddings', '/audio/transcriptions']);
const catalogPaths = new Set(['/models', '/embeddings/models', '/providers']);
const responseHeaders = ['content-type', 'retry-after', 'x-request-id', 'x-generation-id', 'openrouter-processing-ms'];
const corsHeaders = {
	'Access-Control-Allow-Origin': '*',
	'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
	'Access-Control-Allow-Headers': 'Authorization, Content-Type, Accept, HTTP-Referer, X-Title',
	'Access-Control-Expose-Headers': responseHeaders.join(', '),
};

function error(status: number, message: string, type: string): Response {
	return Response.json({ error: { message, type, code: status } }, {
		status, headers: { ...corsHeaders, 'Cache-Control': 'no-store' },
	});
}

export async function handleRequest(request: Request, env: ProxyEnv, upstreamFetch: typeof fetch = fetch): Promise<Response> {
	const url = new URL(request.url);
	const path = url.pathname.replace(/^\/(?:api\/)?v1(?=\/|$)/, '');
	const hasPrefix = path !== url.pathname;
	const inference = hasPrefix && inferencePaths.has(path);
	const catalog = hasPrefix && (catalogPaths.has(path) || /^\/models\/[^/]+\/[^/]+\/endpoints$/.test(path));
	if (request.method === 'OPTIONS') {
		return inference || catalog
			? new Response(null, { status: 204, headers: corsHeaders })
			: error(404, 'Route not found.', 'invalid_request_error');
	}
	if (typeof env.OPENROUTER_API_KEY !== 'string' || !env.OPENROUTER_API_KEY.trim()) {
		return error(503, 'Proxy configuration is incomplete.', 'configuration_error');
	}

	if (!inference && !catalog) return error(404, 'Route not found.', 'invalid_request_error');
	if (request.method !== (inference ? 'POST' : 'GET')) {
		return error(405, 'Method not allowed.', 'invalid_request_error');
	}

	let body: string | FormData | undefined;
	if (inference) {
		const model = path === '/audio/transcriptions' ? env.OPENROUTER_TRANSCRIPTION_MODEL : env.OPENROUTER_MODEL;
		if (typeof model !== 'string' || !model.trim()) {
			return error(503, 'Model configuration is incomplete for this route.', 'configuration_error');
		}
		const contentType = request.headers.get('Content-Type')?.split(';')[0]?.trim().toLowerCase();
		if (path === '/audio/transcriptions' && contentType === 'multipart/form-data') {
			try { body = await request.formData(); }
			catch { return error(400, 'Invalid multipart body.', 'invalid_request_error'); }
			body.set('model', model);
			body.delete('models');
		} else {
			if (contentType !== 'application/json') {
				return error(415, path === '/audio/transcriptions'
					? 'Expected application/json or multipart/form-data.'
					: 'Expected application/json.', 'invalid_request_error');
			}
			let payload: unknown;
			try { payload = await request.json(); }
			catch { return error(400, 'Invalid JSON body.', 'invalid_request_error'); }
			if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
				return error(400, 'Expected a JSON object.', 'invalid_request_error');
			}
			const data = payload as Record<string, unknown>;
			data.model = model;
			delete data.models;
			body = JSON.stringify(data);
		}
	}
	const headers = new Headers({ Authorization: `Bearer ${env.OPENROUTER_API_KEY}` });
	for (const name of ['Accept', 'HTTP-Referer', 'X-Title']) {
		const value = request.headers.get(name);
		if (value) headers.set(name, value);
	}
	// Fetch generates the multipart boundary when sending FormData.
	if (typeof body === 'string') headers.set('Content-Type', 'application/json');
	try {
		const upstream = await upstreamFetch(`https://openrouter.ai/api/v1${path}${url.search}`, {
			method: request.method, headers, body, signal: request.signal, redirect: 'manual',
		});
		if (upstream.status >= 300 && upstream.status < 400) {
			await upstream.body?.cancel();
			return error(502, 'Unexpected upstream redirect.', 'upstream_error');
		}
		const outgoing = new Headers({ ...corsHeaders, 'Cache-Control': 'no-store' });
		for (const [name, value] of upstream.headers) {
			if (responseHeaders.includes(name.toLowerCase()) || name.toLowerCase().startsWith('x-ratelimit-')) outgoing.set(name, value);
		}
		// Keep the original stream: no buffering, SSE parsing, or automatic retries.
		return new Response(upstream.body, { status: upstream.status, statusText: upstream.statusText, headers: outgoing });
	} catch {
		return error(502, 'Unable to connect to OpenRouter.', 'upstream_error');
	}
}

export default { fetch: (request: Request, env: ProxyEnv) => handleRequest(request, env) } satisfies ExportedHandler<ProxyEnv>;
