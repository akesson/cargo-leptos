use actix_web::*;
use futures::StreamExt;
use leptos::*;
use leptos_meta::*;
use leptos_router::*;

/// Returns an Actix [Route](actix_web::Route) that listens for a `GET` request and tries
/// to route it using [leptos_router], serving an HTML stream of your application.
///
/// The provides a [MetaContext] and a [RouterIntegrationContext] to app’s context before
/// rendering it, and includes any meta tags injected using [leptos_meta].
///
/// The HTML stream is rendered using [render_to_stream], and includes everything described in
/// the documentation for that function.
///
/// This can then be set up at an appropriate route in your application:
/// ```
/// use actix_web::{HttpServer, App};
/// use leptos::*;
/// use std::{env,net::SocketAddr};
///
/// #[component]
/// fn MyApp(cx: Scope) -> Element {
///   view! { cx, <main>"Hello, world!"</main> }
/// }
///
/// # if false { // don't actually try to run a server in a doctest...
/// #[actix_web::main]
/// async fn main() -> std::io::Result<()> {
///
///     let addr = SocketAddr::from(([127,0,0,1],3000));
///     HttpServer::new(move || {
///         let render_options: RenderOptions = RenderOptions::builder().pkg_path("/pkg/leptos_example").reload_port(3001).socket_address(addr.clone()).environment(&env::var("RUST_ENV")).build();
///         render_options.write_to_file();
///         App::new()
///             // {tail:.*} passes the remainder of the URL as the route
///             // the actual routing will be handled by `leptos_router`
///             .route("/{tail:.*}", leptos_actix::render_app_to_stream(render_options, |cx| view! { cx, <MyApp/> }))
///     })
///     .bind(&addr)?
///     .run()
///     .await
/// }
/// # }
/// ```
pub fn render_app_to_stream(
    options: RenderOptions,
    app_fn: impl Fn(leptos::Scope) -> Element + Clone + 'static,
) -> Route {
    web::get().to(move |req: HttpRequest| {
        let options = options.clone();
        let app_fn = app_fn.clone();
        async move {
            let path = req.path();

            let query = req.query_string();
            let path = if query.is_empty() {
                "http://leptos".to_string() + path
            } else {
                "http://leptos".to_string() + path + "?" + query
            };

            let app = {
                let app_fn = app_fn.clone();
                move |cx| {
                    let integration = ServerIntegration { path: path.clone() };
                    provide_context(cx, RouterIntegrationContext::new(integration));
                    provide_context(cx, MetaContext::new());
                    provide_context(cx, req.clone());

                    (app_fn)(cx)
                }
            };

            let pkg_path = &options.pkg_path;
            let socket_ip = &options.socket_address.ip().to_string();
            let reload_port = options.reload_port;

            let leptos_autoreload = match options.environment {
                RustEnv::DEV => format!(
                    r#"
                        <script crossorigin="">(function () {{
                            var ws = new WebSocket('ws://{socket_ip}:{reload_port}/live_reload');
                            ws.onmessage = (ev) => {{
                                console.log(`Reload message: `);
                                if (ev.data === 'reload') window.location.reload();
                            }};
                            ws.onclose = () => console.warn('Live-reload stopped. Manual reload necessary.');
                        }})()
                        </script>
                    "#
                ),
                RustEnv::PROD => "".to_string(),
            };

            let head = format!(
              r#"<!DOCTYPE html>
              <html lang="en">
                  <head>
                      <meta charset="utf-8"/>
                      <meta name="viewport" content="width=device-width, initial-scale=1"/>
                      <link rel="stylesheet" href="{pkg_path}.css">
                      <link rel="modulepreload" href="{pkg_path}.js">
                      <link rel="preload" href="{pkg_path}.wasm" as="fetch" type="application/wasm" crossorigin="">
                      <script type="module">import init, {{ hydrate }} from '{pkg_path}.js'; init('{pkg_path}.wasm').then(hydrate);</script>
                      {leptos_autoreload}
                      "#
          );

            let tail = "</body></html>";

            HttpResponse::Ok().content_type("text/html").streaming(
                futures::stream::once(async move { head.clone() })
                    // TODO this leaks a runtime once per invocation
                    .chain(render_to_stream(move |cx| {
                        let app = app(cx);
                        let head = use_context::<MetaContext>(cx)
                            .map(|meta| meta.dehydrate())
                            .unwrap_or_default();
                        format!("{head}</head><body>{app}")
                    }))
                    .chain(futures::stream::once(async { tail.to_string() }))
                    .map(|html| Ok(web::Bytes::from(html)) as Result<web::Bytes>),
            )
        }
    })
}
