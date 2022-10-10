use crate::utils::*;
use chrono::Duration;
use serde::Serialize;
use std::{net::IpAddr, path::Path};
use worker::wasm_bindgen::JsValue;
use worker::*;

/// KV store used to count downloaders
/// Key: &str = IP address of user
/// Value: [u8;4] = Download count in little endian (for portability, this is specified)
pub const DOWNLOADERS_KV_STORE: &str = "MODRINTH_DOWNLOADERS";

/// Route handler for download counting, redirecting, and caching
/// URL: /data/<hash>/versions/<version>/<file>
pub async fn handle_version_download(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let (hash, version, file) = (
        get_param(&ctx, "hash"),
        get_param(&ctx, "version").replace('+', "%2B"),
        get_param(&ctx, "file").replace('+', "%2B"),
    );
    let cdn = ctx.env.var(CDN_BACKEND_URL)?.to_string();

    let url =
        make_cdn_url(&cdn, &format!("data/{hash}/versions/{version}/{file}"))?;

    if let Err(error) = count_download(&req, &ctx, &url).await {
        console_error!(
            "Error encountered while trying to count download: {error}",
        );
        console_debug!("Full object: {error:?}");
    }

    console_debug!("[DEBUG]: Downloading version from {url}...");

    Response::redirect(url)?.with_cors(&CORS_POLICY)
}

/// Redirect all other requests to the backend
/// URL: /...
pub fn handle_download(
    _req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let cdn = ctx.env.var(CDN_BACKEND_URL)?.to_string();
    let file = get_param(&ctx, "file");
    let url = make_cdn_url(&cdn, file)?;
    console_debug!("[DEBUG]: Falling back to CDN for {url}...");
    Response::redirect(url)?.with_cors(&CORS_POLICY)
}

/// Tries to count a download, provided the IP address is discernable and the limit hasn't already been reachedy
async fn count_download(
    req: &Request,
    ctx: &RouteContext<()>,
    forward_url: &Url,
) -> Result<()> {
    if let Some(ip) = req.headers().get(CF_IP_HEADER)? {
        let (project, file) = (get_param(ctx, "hash"), get_param(ctx, "file"));

        if !is_counted(file) {
            console_debug!("[DEBUG]: Not counting {file} due to extension");
            return Ok(());
        }
        console_debug!("[DEBUG]: Attempting to count download from IP {ip} in project {project}");

        let ip = u64::from_le_bytes(
            match ip.parse::<IpAddr>().map_err(|err| err.to_string())? {
                IpAddr::V4(it) => {
                    [it.octets(), [0u8; 4]].concat().try_into().unwrap()
                }
                IpAddr::V6(it) => it.octets()[..8].try_into().unwrap(),
            },
        )
        .to_string();
        console_debug!("Ip: {ip}");

        let download_ctx = format!("{project}-{ip}");

        let store_name = ctx.var(DOWNLOADERS_KV_STORE)?.to_string();
        let downloaders = ctx.kv(&store_name).unwrap_or_else(|_| panic!("[FATAL]: No downloader KV store is set, this should be in the {DOWNLOADERS_KV_STORE} environment variable!"));

        let downloader_downloads = downloaders
            .get(&download_ctx)
            .bytes()
            .await?
            .map(|it| u32::from_le_bytes(it[0..4].try_into().unwrap()))
            .unwrap_or(0);

        let expiration_time: u64 = ctx.var(DOWNLOAD_STORAGE_TIME)
            .map(|it| it.to_string())
            .map_err(|err| format!("Environment error: {err}"))
            .and_then(|it| it.parse::<i64>().map_err(|err| format!("Parse error: {err}")))
            .map(Duration::minutes)
            .unwrap_or_else(|err| {
                console_warn!("[WARN]: Could not parse {DOWNLOAD_STORAGE_TIME} as number of minutes: {err}. Using default...");
                Duration::minutes(6 * 60)
            })
            .num_seconds()
            .try_into()
            .unwrap();

        console_debug!("[DEBUG]: Number of downloads: {downloader_downloads}");
        if downloader_downloads == u32::MAX {
            console_warn!("[WARN]: This user is likely a bot, their download count has hit the 32 bit integer limit. Either that or I somehow introduced an integer underflow.");
            downloaders
                .put_bytes(&download_ctx, &[0xFF, 4])?
                .expiration_ttl(expiration_time)
                .execute()
                .await?;
            return Ok(());
        }

        downloaders
            .put_bytes(
                &download_ctx,
                &u32::to_le_bytes(downloader_downloads + 1),
            )?
            .expiration_ttl(expiration_time)
            .execute()
            .await?;

        let max_downloads = ctx.var(MAX_COUNTED_DOWNLOADS)
            .map(|it| it.to_string())
            .map_err(|err| format!("Environment error: {err}"))
            .and_then(|it| it
                      .parse::<i64>()
                      .map_err(|err| format!("Parse error: {err}")))
            .unwrap_or_else(|err| {
                console_warn!("[WARN]: Could not parse {MAX_COUNTED_DOWNLOADS} environment veriable: {err}. Using default...");
                5
            });

        if (downloader_downloads as i64) < max_downloads {
            let labrinth_url = ctx.var(LABRINTH_URL)?.to_string();
            let labrinth_secret = ctx.secret(LABRINTH_SECRET)?.to_string();
            let hash = get_param(ctx, "hash").to_owned();
            let version_name = get_param(ctx, "version").to_owned();
            let forward_url = forward_url.to_string();

            wasm_bindgen_futures::spawn_local(async move {
                match request_download_count(
                    &labrinth_url,
                    &labrinth_secret,
                    &hash,
                    &version_name,
                    &forward_url,
                )
                .await
                {
                    Ok(mut response)
                        if !http::StatusCode::from_u16(
                            response.status_code(),
                        )
                        .unwrap()
                        .is_success() =>
                    {
                        console_warn!(
                            "[WARN] Non-success response when counting download: {}",
                            response.text().await.unwrap_or_else(|_| String::from("?"))
                        )
                    }
                    Err(error) => {
                        console_error!(
                            "[ERROR] Error counting download: {error}"
                        )
                    }
                    _ => (),
                }
            });
        }
    };

    Ok(())
}

#[derive(Serialize)]
struct DownloadRequest {
    pub url: String,
    pub hash: String,
    pub version_name: String,
}

async fn request_download_count(
    labrinth_url: &str,
    labrinth_secret: &str,
    hash: &str,
    version_name: &str,
    req_url: &str,
) -> Result<Response> {
    let url = format!(
        "{url}/v2/admin/_count-download",
        url = labrinth_url.trim_end_matches('/'),
    );
    console_debug!("[DEBUG]: Counting via url: {url}");

    let headers = {
        let mut h = Headers::new();
        h.set("Modrinth-Admin", labrinth_secret)?;
        h.set("Content-Type", "application/json")?;
        CORS_POLICY.apply_headers(&mut h)?;

        h
    };

    let init = RequestInit {
        headers,
        method: Method::Patch,
        body: Some(JsValue::from_str(&serde_json::to_string(
            &DownloadRequest {
                url: req_url.to_string(),
                hash: hash.to_string(),
                version_name: version_name.to_string(),
            },
        )?)),
        ..Default::default()
    };
    Fetch::Request(Request::new_with_init(&url, &init)?)
        .send()
        .await
}

fn is_counted(file: &str) -> bool {
    if file.is_empty() {
        return false;
    }

    !matches!(
        Path::new(file)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        "md" | "markdown"
    )
}
