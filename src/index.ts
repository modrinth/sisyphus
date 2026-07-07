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

// Downloadable artifacts must not be cached so every download reaches the worker and gets counted.
// https://github.com/modrinth/code/blob/517c3d2d72ef4f667cbc454d6ae22f60fa21911d/packages/utils/utils.ts#L243
const noStoreExtensions = ['.jar', '.zip', '.litemod', '.mrpack', '.sig', '.asc', '.gpg'];

function isNoStoreArtifact(key: string): boolean {
	const lowerKey = key.toLowerCase();
	return noStoreExtensions.some((extension) => lowerKey.endsWith(extension));
}

interface UrlData {
	projectId: string;
	versionId: string;
}

function extractUrlData(path: string): UrlData | null {
	// Pattern: data/:hash/versions/:version/:file
	const parts = path.split('/');

	const hashIndex = parts.findIndex((part) => part === 'data');
	const versionIndex = parts.findIndex((part) => part === 'versions');

	if (hashIndex === -1 || versionIndex === -1) {
		return null;
	}

	const projectId = parts[hashIndex + 1];
	const versionId = parts[versionIndex + 1];

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

	let queryObj: Record<string, string> = {};
	const reqUrl = new URL(request.url);
	reqUrl.searchParams.forEach((value, key) => {
		if (key.startsWith('mr_')) {
			queryObj[key] = value;
		}
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
				query: queryObj,
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
			return new Response(null, {
				headers: {
					allow: allowedMethods.join(', '),
					'cache-control': 'public, max-age=86400, stale-while-revalidate=86400',
				},
			});
		}

		const url = new URL(request.url);
		const key = decodeURIComponent(url.pathname.replace(/\/+/g, '/').slice(1));
		const isHead = request.method === 'HEAD';

		const urlData = extractUrlData(key);

		if (urlData && request.method === 'GET') {
			ctx.waitUntil(countDownload(request, env, urlData));
		}

		if (key.endsWith('/')) {
			return makeError(404, 'not_found', 'the requested resource does not exist');
		}

		const object: R2Object | R2ObjectBody | null = isHead ? await env.MODRINTH_CDN.head(key) : await env.MODRINTH_CDN.get(key);

		if (object === null) {
			return makeError(404, 'not_found', 'the requested resource does not exist');
		}

		return new Response(isHead ? null : (object as R2ObjectBody).body, {
			status: 200,
			headers: {
				...defaultCorsHeaders,
				etag: object.httpEtag,
				'cache-control': isNoStoreArtifact(key) ? 'no-store' : 'public, max-age=86400, stale-while-revalidate=86400',
				'last-modified': object.uploaded.toUTCString(),

				'content-encoding': object.httpMetadata?.contentEncoding ?? '',
				'content-type': object.httpMetadata?.contentType ?? 'application/octet-stream',
				'content-language': object.httpMetadata?.contentLanguage ?? '',
				'content-disposition': object.httpMetadata?.contentDisposition ?? '',
				'content-length': object.size.toString(),
			},
		});
	},
};
