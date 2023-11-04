const allowedMethods = ['GET', 'HEAD', 'OPTIONS'];
const defaultCorsHeaders = {
	allow: allowedMethods.join(', '),
	'Access-Control-Allow-Headers': '*',
	'Access-Control-Allow-Methods': allowedMethods.join(', '),
	'Access-Control-Allow-Origin': '*',
};

function makeError(code: number, error: String, description: String): Response {
	return new Response(JSON.stringify({ error, description }), {
		status: code,
		headers: {
			...defaultCorsHeaders,
			'content-type': 'application/json',
		},
	});
}

interface UrlData {
	projectId: string;
	versionId: string;
}

function extractUrlData(path: string): UrlData | null {
	// Pattern: data/:hash/versions/:version/:file
	const parts = path.split('/');
	console.log(parts);

	const hashIndex = parts.findIndex((part) => part === 'data') + 1;
	const versionIndex = parts.findIndex((part) => part === 'versions') + 1;

	console.log(hashIndex);
	console.log(versionIndex);

	if (hashIndex === -1 || versionIndex === -1) {
		return null;
	}

	const projectId = parts[hashIndex];
	const versionId = parts[versionIndex];

	return { projectId, versionId };
}

async function countDownload(request: Request, env: Env, urlData: UrlData) {
	console.log(`Attempting to count download of project ID ${urlData.projectId} with version ID ${urlData.versionId}`);

	const url = `${env.LABRINTH_URL}admin/_count-download`;

	console.log(`url: ${url}`);

	let headersObj: Record<string, string> = {};
	request.headers.forEach((value, key) => {
		headersObj[key] = value;
	});

	const res = await fetch(
		new Request(url, {
			method: 'PATCH',
			headers: {
				'Modrinth-Admin': env.LABRINTH_ADMIN_KEY,
				'x-ratelimit-key': env.RATE_LIMIT_IGNORE_KEY,
				'content-type': 'application/json',
			},
			body: JSON.stringify({
				url: request.url,
				project_id: urlData.projectId,
				version_name: urlData.versionId,
				ip: request.headers.get('CF-Connecting-IP') ?? '127.0.0.1',
				headers: headersObj,
			}),
		}),
	);

	console.log(`Finished counting download. Status code: ${res.status}`);
	console.log(`Response body: ${await res.text()}`);
}

export default {
	async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
		if (allowedMethods.indexOf(request.method) === -1) {
			return makeError(405, 'method_not_allowed', 'request method is not allowed');
		}

		if (request.method === 'OPTIONS') {
			return new Response(null, { headers: { allow: allowedMethods.join(', ') } });
		}

		const cache = caches.default;
		const cacheResponse = await cache.match(request);

		const url = new URL(request.url);
		const key = url.pathname.slice(1);

		const urlData = extractUrlData(key);

		console.log(urlData);
		if (urlData && request.method === 'GET') {
			ctx.waitUntil(countDownload(request, env, urlData));
		}

		if (!cacheResponse || !(cacheResponse.ok || cacheResponse.status == 304)) {
			console.warn('Cache miss. Fetching origin.');

			if (key.endsWith('/')) {
				return makeError(404, 'not_found', 'the requested resource does not exist');
			}

			const object: R2Object | R2ObjectBody | null =
				request.method === 'HEAD' ? await env.MODRINTH_CDN.head(key) : await env.MODRINTH_CDN.get(key);

			if (object === null) {
				return makeError(404, 'not_found', 'the requested resource does not exist');
			}

			const response = new Response(request.method === 'HEAD' ? null : (object as R2ObjectBody).body, {
				status: 200,
				headers: {
					...defaultCorsHeaders,
					etag: object.httpEtag,
					// if the 404 file has a custom cache control, we respect it
					'cache-control': 's-maxage=2678400',
					'last-modified': object.uploaded.toUTCString(),

					'content-encoding': object.httpMetadata?.contentEncoding ?? '',
					'content-type': object.httpMetadata?.contentType ?? 'application/octet-stream',
					'content-language': object.httpMetadata?.contentLanguage ?? '',
					'content-disposition': object.httpMetadata?.contentDisposition ?? '',
					'content-length': object.size.toString(),
				},
			});

			if (request.method === 'GET') ctx.waitUntil(cache.put(request, response.clone()));

			return response;
		} else {
			return cacheResponse;
		}
	},
};
