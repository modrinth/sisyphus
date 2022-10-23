use cfg_if::cfg_if;
use worker::*;

/// The header used to determine the IP address of incoming requests
pub const CF_IP_HEADER: &str = "CF-Connecting-IP";

/// The environment variable containing the URL of the backing CDN
pub const CDN_BACKEND_URL: &str = "CDN_BACKEND_URL";

/// The environment variable contianing the API URL, used to make API requests
pub const LABRINTH_URL: &str = "LABRINTH_URL";

/// The secret contianing the admin key for Labrinth
pub const LABRINTH_SECRET: &str = "LABRINTH_ADMIN_SECRET";

/// How long downloader download counts should be stored, in minutes
pub const DOWNLOAD_STORAGE_TIME: &str = "DOWNLOAD_STORAGE_TIME";

/// The maximum number of downloads per downloader in order to be counted
pub const MAX_COUNTED_DOWNLOADS: &str = "DOWNLOAD_STORAGE_LIMIT";

lazy_static::lazy_static! {
    /// CORS policy
    pub static ref CORS_POLICY: Cors = Cors::new()
        .with_origins(["*"])
        .with_methods([Method::Get, Method::Options, Method::Head]);
}

/// Small helper to make CDN download URLs from metadata.
pub fn make_cdn_url(cdn: &str, path: &str) -> Result<Url> {
    let cdn = cdn.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let url = format!("{cdn}/{path}");
    Url::parse(&url).map_err(Error::from)
}

const URL_PARAM_ERROR: &str =
    "Tried to get nonexistent parameter, the router should not have matched this route!";
pub fn get_param<'a, T>(ctx: &'a RouteContext<T>, param: &str) -> &'a String {
    ctx.param(param).expect(URL_PARAM_ERROR)
}

cfg_if! {
    // https://github.com/rustwasm/console_error_panic_hook#readme
    if #[cfg(feature = "console_error_panic_hook")] {
        extern crate console_error_panic_hook;
        pub use self::console_error_panic_hook::set_once as set_panic_hook;
    } else {
        #[inline]
        pub fn set_panic_hook() {}
    }
}
