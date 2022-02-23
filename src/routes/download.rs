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
}

/// Route handler for download counting, redirecting, and caching
/// URL: /data/<hash>/versions/<version>/<file>
pub async fn handle_download(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    count_download(&req, &ctx).await.ok();
    get_file(&ctx)
}

/// Tries to count a download, provided the IP address is discernable and the limit hasn't already been reachedy
async fn count_download(req: &Request, ctx: &RouteContext<()>) -> Result<()> {
    if let Some(ip) = req.headers().get(CF_IP_HEADER)? {
        console_debug!("[DEBUG]: Attempting to count download from IP {}", ip);
        let downloaders = ctx.kv(&ctx.var(DOWNLOADERS_KV_STORE)?.to_string())?;
        let downloader_downloads = downloaders
            .get(&ip)
            .bytes()
            .await?
            .map(|it| u32::from_le_bytes(it[0..4].try_into().unwrap()))
            .unwrap_or(0);

        let downloader_dl_increment_future = downloaders
            .put_bytes(&ip, &u32::to_le_bytes(downloader_downloads + 1))?
            .expiration_ttl(EXPIRATION_TIME.num_seconds() as u64)
            .execute();

        if downloader_downloads <= MAX_COUNTED_DOWNLOADS {
            let labrinth_url = ctx.var(LABRINTH_URL)?.to_string();
            let labrinth_secret = ctx.secret(LABRINTH_SECRET)?.to_string();
            let url = format!(
                "{labrinth_url}/v2/version/{version}/_count-download",
                version = get_param(ctx, "version")
            );

            futures::future::poll_immediate(Fetch::Request(Request::new_with_init(
                &url,
                &RequestInit {
                    headers: {
                        let mut headers = Headers::new();
                        headers.set("Modrinth-Admin", &labrinth_secret)?;

                        headers
                    },
                    ..Default::default()
                },
            )?)
            .send()).await;
        }

        downloader_dl_increment_future.await?;
    };

    Ok(())
}

const URL_PARAM_ERROR: &str =
    "Tried to get nonexistent file, the router should not have matched this route!";
fn get_param<'a, T>(ctx: &'a RouteContext<T>, param: &str) -> &'a String {
    ctx.param(param).expect(URL_PARAM_ERROR)
}

/// Small helper to make CDN download URLs from metadata.
fn make_cdn_download_url(cdn: &str, hash: &str, version: &str, file: &str) -> Result<Url> {
    let url = format!("{cdn}/data/{hash}/versions/{version}/{file}");
    Url::parse(&url).map_err(Error::from)
}

/// Tries to get a file from the CDN
fn get_file(ctx: &RouteContext<()>) -> Result<Response> {
    let (hash, version, file) = (
        get_param(ctx, "hash"),
        get_param(ctx, "version"),
        get_param(ctx, "file"),
    );
    let cdn = ctx.env.var(CDN_BACKEND_URL)?.to_string();

    let url = make_cdn_download_url(&cdn, hash, version, file)?;
    console_debug!("[DEBUG]: Redirecting to {url}...");
    Response::redirect(url)
}
