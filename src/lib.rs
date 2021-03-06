/*
    CloudFlare workers for Modrinth
    Copyright (C) 2022 Rinth, Inc.

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU Affero General Public License as published
    by the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU Affero General Public License for more details.

    You should have received a copy of the GNU Affero General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use utils::*;
use worker::*;

mod routes;
mod utils;

fn log_request(req: &Request) {
    console_log!(
        "{} - [{}], located at: {:?}, within: {}",
        Date::now().to_string(),
        req.path(),
        req.cf().coordinates().unwrap_or_default(),
        req.cf().region().unwrap_or_else(|| "unknown region".into())
    );
}

#[event(fetch)]
pub async fn main(
    req: Request,
    env: Env,
    _ctx: worker::Context,
) -> Result<Response> {
    log_request(&req);
    utils::set_panic_hook();

    Router::new()
        .get_async(
            "/data/:hash/versions/:version/:file",
            routes::download::handle_version_download,
        )
        .options("/*route", |_req, _ctx| {
            Response::ok("")?.with_cors(&CORS_POLICY)
        })
        .get("/teapot", routes::teapot::teapot)
        .head_async("/*file", |_req, ctx| async move {
            let cdn = ctx.env.var(CDN_BACKEND_URL)?.to_string();
            let url = make_cdn_url(&cdn, get_param(&ctx, "file"))?.to_string();
            let resp =
                Fetch::Request(Request::new(url.as_str(), Method::Head)?)
                    .send()
                    .await?;
            resp.with_cors(&CORS_POLICY)
        })
        .or_else_any_method("/*file", routes::download::handle_download)
        .run(req, env)
        .await
}
