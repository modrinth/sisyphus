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

lazy_static::lazy_static! {
    /// CORS policy
    pub static ref CORS_POLICY: Cors = Cors::new()
        .with_origins(["*"])
        .with_methods([Method::Get, Method::Options, Method::Head]);
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
