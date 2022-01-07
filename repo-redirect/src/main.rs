use std::{path::Path, sync::Arc};

use actix_web::{get, http, middleware, post, web, App, Error, HttpResponse, HttpServer};
use dashmap::DashMap;
use sailfish::TemplateOnce;
use serde::Deserialize;

pub type SharedDistMap = Arc<DashMap<String, parser::Tarball>>;

mod parser;

#[derive(Deserialize, Debug)]
struct DownloadRequest {
    #[serde(rename = "distro-variant")]
    distro_variant: String,
}

#[derive(TemplateOnce)]
#[template(path = "thank-you.html")]
#[template(rm_whitespace = true)]
struct HelpContent {
    variant: String,
    arch: String,
    url: String,
    sha256: String,
}

#[derive(TemplateOnce)]
#[template(path = "404.html")]
#[template(rm_whitespace = true)]
struct NotFoundPage {
    variant: String,
    arch: String,
}

#[post("/download/alt")]
async fn download_distribution(
    params: web::Form<DownloadRequest>,
    tarballs: web::Data<(SharedDistMap, SharedDistMap)>,
) -> Result<HttpResponse, Error> {
    if params.distro_variant.starts_with("https://") {
        return Ok(HttpResponse::Found()
            .append_header((http::header::LOCATION, params.distro_variant.clone()))
            .finish());
    }
    let mut splitted = params.distro_variant.split('.');
    let variant_name = splitted.next().unwrap_or("(?)");
    if let Some(tarball) = tarballs.0.get(&params.distro_variant) {
        let url = format!("https://releases.aosc.io/{}", tarball.path);
        let help_content = HelpContent {
            variant: variant_name.to_string(),
            arch: tarball.arch.clone(),
            sha256: tarball.sha256sum.clone(),
            url: url.clone(),
        }
        .render_once()
        .unwrap_or_else(|_| url.clone());

        Ok(HttpResponse::Ok()
            .append_header((http::header::CONTENT_TYPE, "text/html"))
            .body(help_content))
    } else {
        let arch = splitted.next().unwrap_or("(?)");
        Ok(HttpResponse::NotFound()
            .append_header((http::header::CONTENT_TYPE, "text/html"))
            .body(
                NotFoundPage {
                    variant: variant_name.to_string(),
                    arch: arch.to_string(),
                }
                .render_once()
                .unwrap_or_else(|_| "Not Found".to_string()),
            ))
    }
}

#[post("/download/livekit")]
async fn download_livekit(
    params: web::Form<DownloadRequest>,
    tarballs: web::Data<(SharedDistMap, SharedDistMap)>,
) -> Result<HttpResponse, Error> {
    if let Some(tarball) = tarballs.1.get(&params.distro_variant) {
        let url = format!("https://releases.aosc.io/{}", tarball.path);
        let help_content = HelpContent {
            variant: "Livekit".to_string(),
            arch: tarball.arch.clone(),
            sha256: tarball.sha256sum.clone(),
            url: url.clone(),
        }
        .render_once()
        .unwrap_or_else(|_| url.clone());

        Ok(HttpResponse::Ok()
            .append_header((http::header::CONTENT_TYPE, "text/html"))
            .body(help_content))
    } else {
        Ok(HttpResponse::NotFound()
            .append_header((http::header::CONTENT_TYPE, "text/html"))
            .body(
                NotFoundPage {
                    variant: "Livekit".to_string(),
                    arch: params.distro_variant.clone(),
                }
                .render_once()
                .unwrap_or_else(|_| "Not Found".to_string()),
            ))
    }
}

#[get("/download/alt")]
async fn fallback_distribution() -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Found()
        .append_header((http::header::LOCATION, "https://aosc.io/downloads/"))
        .finish())
}

#[get("/download/livekit")]
async fn fallback_livekit() -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Found()
        .append_header((http::header::LOCATION, "https://aosc.io/downloads/"))
        .finish())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let listen = std::env::var("LISTEN_ADDRESS").expect("LISTEN_ADDRESS not set");
    let manifest_path = std::env::var("MANIFEST_PATH").expect("MANIFEST_PATH not set");
    let manifest_path = Path::new(&manifest_path);

    let shared_map = Arc::new(DashMap::new());
    let shared_map_lk = Arc::new(DashMap::new());
    let monitor_worker =
        parser::monitor_recipe(manifest_path.join("recipe.json"), Arc::clone(&shared_map));
    let monitor_worker_lk = parser::monitor_livekit(
        manifest_path.join("livekit.json"),
        Arc::clone(&shared_map_lk),
    );

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new((shared_map.clone(), shared_map_lk.clone())))
            .service(download_distribution)
            .service(download_livekit)
            .service(fallback_distribution)
            .service(fallback_livekit)
    })
    .bind(listen)?
    .run();

    let res = tokio::select! {
        v = server => v,
        v = async {
            monitor_worker
                .await
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
        } => v,
        v = async {
            monitor_worker_lk
                .await
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
        } => v
    };
    res?;

    Ok(())
}
