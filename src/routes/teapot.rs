use worker::*;

/// Testing route handler
/// URL: /teapot
pub fn teapot(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    Ok(Response::ok("Error 418: I'm short and stout!")?.with_status(418))
}
