#![allow(unused_imports, unused_variables)]
use actix_web::{get, middleware, web::Data, App, HttpRequest, HttpResponse, HttpServer, Responder};
use controller::{ManagerTrait, AlertPolicy, SLI, SLO, Service, AlertCondition, AlertNotificationTarget};
pub use controller::{self, Result, State, Manager, DataSource};
use prometheus::{Encoder, TextEncoder};
use tracing::{debug, error, info, trace, warn};
use tracing_subscriber::{prelude::*, EnvFilter, Registry};

#[get("/metrics")]
async fn metrics(c: Data<State>, _req: HttpRequest) -> impl Responder {
    let metrics = c.metrics();
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&metrics, &mut buffer).unwrap();
    HttpResponse::Ok().body(buffer)
}

#[get("/health")]
async fn health(_: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json("healthy")
}

#[get("/")]
async fn index(c: Data<State>, _req: HttpRequest) -> impl Responder {
    let d = c.diagnostics().await;
    HttpResponse::Ok().json(&d)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup tracing layers
    #[cfg(feature = "telemetry")]
    let telemetry = tracing_opentelemetry::layer().with_tracer(controller::telemetry::init_tracer().await);
    let logger = tracing_subscriber::fmt::layer();
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    // Decide on layers
    #[cfg(feature = "telemetry")]
    let collector = Registry::default().with(telemetry).with(logger).with(env_filter);
    #[cfg(not(feature = "telemetry"))]
    let collector = Registry::default().with(logger).with(env_filter);

    // Initialize tracing
    tracing::subscriber::set_global_default(collector).unwrap();

    // Initiatilize Kubernetes controller state
    let state = State::default();
    // Start web server

    // Ensure both the webserver and the controller gracefully shutdown
    let _ = tokio::join!(
        Manager::<DataSource>::run(state.clone()),
        Manager::<Service>::run(state.clone()),
        Manager::<SLO>::run(state.clone()),
        Manager::<SLI>::run(state.clone()),
        Manager::<AlertPolicy>::run(state.clone()),
        Manager::<AlertCondition>::run(state.clone()),
        Manager::<AlertNotificationTarget>::run(state.clone()),
        HttpServer::new(move || {
            App::new()
                .app_data(Data::new(state.clone()))
                .wrap(middleware::Logger::default().exclude("/health"))
                .service(index)
                .service(health)
                .service(metrics)
            })
            .bind("0.0.0.0:8080")
            .expect("Can not bind to 0.0.0.0:8080")
            .shutdown_timeout(5)
            .run()
    );
    Ok(())
}
