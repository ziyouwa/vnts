use std::collections::{HashMap, HashSet};
use std::net;
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::web::Data;
use actix_web::{middleware, post, web, App, HttpRequest, HttpResponse, HttpServer};

use actix_web_static_files::ResourceFiles;

use crate::core::server::web::service::VntsWebService;
use crate::core::server::web::vo::{LoginData, ResponseMessage};
use crate::ConfigInfo;

mod service;
mod vo;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

#[post("/login")]
async fn login(service: Data<VntsWebService>, data: web::Json<LoginData>) -> HttpResponse {
    match service.login(data.0).await {
        Ok(auth) => HttpResponse::Ok().json(ResponseMessage::success(auth)),
        Err(e) => HttpResponse::Ok().json(ResponseMessage::fail(e)),
    }
}

#[post("/group_list")]
async fn group_list(_req: HttpRequest, service: Data<VntsWebService>) -> HttpResponse {
    let info = service.group_list();
    HttpResponse::Ok().json(ResponseMessage::success(info))
}

#[post("/group_info")]
async fn group_info(
    _req: HttpRequest,
    service: Data<VntsWebService>,
    group: web::Json<HashMap<String, String>>,
) -> HttpResponse {
    if let Some(group) = group.get("group") {
        let info = service.group_info(group.to_string());
        HttpResponse::Ok().json(ResponseMessage::success(info))
    } else {
        HttpResponse::Ok().json(ResponseMessage::fail("no group found".into()))
    }
}

#[derive(Clone)]
struct AuthApi {
    api_set: Arc<HashSet<String>>,
}

fn auth_api_set() -> AuthApi {
    let mut api_set = HashSet::new();
    api_set.insert("/group_info".to_string());
    api_set.insert("/group_list".to_string());
    AuthApi {
        api_set: Arc::new(api_set),
    }
}

pub async fn start(
    lst: net::TcpListener,
    config: &ConfigInfo,
) -> std::io::Result<()> {
    let web_service = VntsWebService::new(config);
    let auth_api = auth_api_set();
    HttpServer::new(move || {
        let generated = generate();
        App::new()
            .app_data(Data::new(web_service.clone()))
            .app_data(Data::new(auth_api.clone()))
            .wrap_fn(|request, srv| {
                let auth_api: &Data<AuthApi> = request.app_data().unwrap();
                let path = request.path();
                if path == "/login" || !auth_api.api_set.contains(path) {
                    return srv.call(request);
                }
                let service: &Data<VntsWebService> = request.app_data().unwrap();
                if let Some(authorization) = request.headers().get("Authorization") {
                    if let Ok(s) = authorization.to_str() {
                        if let Some(auth) = s.strip_prefix("Bearer ") {
                            if service.check_auth(&auth.to_string()) {
                                return srv.call(request);
                            }
                        }
                    }
                }
                Box::pin(async move {
                    Ok(request
                        .into_response(HttpResponse::Ok().json(ResponseMessage::unauthorized())))
                })
            })
            .wrap(middleware::Compress::default())
            .service(login)
            .service(group_list)
            .service(group_info)
            .service(ResourceFiles::new("/", generated))
    })
    .listen(lst)?
    .run()
    .await
}
