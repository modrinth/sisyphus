use crate::utils::*;
use chrono::Duration;
use worker::*;

/// KV store used to count downloaders
/// Key: &str = IP address of user
/// Value: [u8;4] = Download count in little endian (for portability, this is specified)
pub const DOWNLOADERS_KV_STORE: &str = "MODRINTH_DOWNLOADERS";

/// The maximum number of downloads per downloader in order to be counted
/// Expires after EXPIRATION_TIME
pub const MAX_COUNTED_DOWNLOADS: u32 = 5;
lazy_static::lazy_static! {
    /// How long downloader download counts should be stored
    pub static ref EXPIRATION_TIME: Duration = Duration::minutes(30);

    /// CORS policy
    pub static ref CORS_POLICY: Cors = Cors::new()
        .with_origins(["*"])
        .with_methods([Method::Get, Method::Options]);
}

/// Route handler for download counting, redirecting, and caching
/// URL: /data/<hash>/versions/<version>/<file>
pub async fn handle_version_download(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(error) = count_download(&req, &ctx).await {
        console_error!("Error encountered while trying to count download: {error}");
    }
    get_version(&ctx)
}

/// Redirect all other requests to the backend
/// URL: /...
pub fn handle_download(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let cdn = ctx.env.var(CDN_BACKEND_URL)?.to_string();
    let file = get_param(&ctx, "file");
    let url = make_cdn_url(&cdn, &file)?;
    console_debug!("[DEBUG]: Falling back to CDN for {url}...");
    Response::redirect(url)?
        .with_cors(&CORS_POLICY)
}

/// Tries to count a download, provided the IP address is discernable and the limit hasn't already been reachedy
async fn count_download(req: &Request, ctx: &RouteContext<()>) -> Result<()> {
    if let Some(ip) = req.headers().get(CF_IP_HEADER)? {
        let project = get_param(&ctx, "hash");
        console_debug!("[DEBUG]: Attempting to count download from IP {ip} in project {project}");
        let download_ctx = format!("{project}-{ip}");

        let store_name = ctx.var(DOWNLOADERS_KV_STORE)?.to_string();
        let downloaders = ctx.kv(&store_name).expect(
            &format!("No downloader KV store is set, this should be in the {DOWNLOADERS_KV_STORE} environemnt variable!")
        );
        let downloader_downloads = downloaders
            .get(&download_ctx)            
            .bytes()
            .await?
            .map(|it| u32::from_le_bytes(it[0..4].try_into().unwrap()))
        .unwrap_or(0);

        downloaders
            .put_bytes(&download_ctx, &u32::to_le_bytes(downloader_downloads + 1))?
            .expiration_ttl(EXPIRATION_TIME.num_seconds() as u64)
            .execute()
            .await?;

        if downloader_downloads <= MAX_COUNTED_DOWNLOADS {
            request_download_count(ctx).await?;
        }
    };

    Ok(())
}

async fn request_download_count<T>(ctx: &RouteContext<T>) -> Result<()> {
    let labrinth_url = ctx.var(LABRINTH_URL)?.to_string();
    let labrinth_secret = ctx.secret(LABRINTH_SECRET)?.to_string();
    let url = format!(
        "{url}/v2/version/{version}/_count-download",
        url = labrinth_url.trim_end_matches('/'),
        version = get_param(ctx, "version"),
    );

    let res = async {
        let headers = {
            let mut h = Headers::new();
            h.set("Modrinth-Admin", &labrinth_secret)?;
            CORS_POLICY.apply_headers(&mut h)?;

            h
        };
        let init = RequestInit {
            headers, method: Method::Patch,
            ..Default::default()
        };
        Fetch::Request(Request::new_with_init(&url, &init)?)
            .send()
            .await?;

        Ok(()) as Result<()>
    };
    if let Err(error) = res.await {
        console_error!("Error incrementing download counter: {error}");
    }

    Ok(())
}

const URL_PARAM_ERROR: &str =
    "Tried to get nonexistent parameter, the router should not have matched this route!";
fn get_param<'a, T>(ctx: &'a RouteContext<T>, param: &str) -> &'a String {
    ctx.param(param).expect(URL_PARAM_ERROR)
}

/// Small helper to make CDN download URLs from metadata.
fn make_cdn_url(cdn: &str, path: &str) -> Result<Url> {
    let url = format!("{cdn}{path}");
    Url::parse(&url).map_err(Error::from)
}

/// Small helper to make CDN download URLs from metadata.
fn make_version_download_path(hash: &str, version: &str, file: &str) -> String {
    format!("/data/{hash}/versions/{version}/{file}")
}

/// Tries to get a file from the CDN
fn get_version(ctx: &RouteContext<()>) -> Result<Response> {
    let (hash, version, file) = (
        get_param(ctx, "hash"),
        get_param(ctx, "version"),
        get_param(ctx, "file"),
    );
    let cdn = ctx.env.var(CDN_BACKEND_URL)?.to_string();

    let url = make_cdn_url(&cdn, &make_version_download_path(hash, version, file))?;
    console_debug!("[DEBUG]: Downloading version from {url}...");
    Response::redirect(url)?
        .with_cors(&CORS_POLICY)
}
