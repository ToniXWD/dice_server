use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};

use opentelemetry::trace::{Span, Status, TraceError, Tracer};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace as sdktrace;
use opentelemetry_sdk::{runtime, Resource};
use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
use rand::Rng;
use std::{convert::Infallible, net::SocketAddr};

// 异步处理请求的函数
async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let mut response = Response::new(Body::empty());

    // 获取全局追踪器
    let tracer = global::tracer("dice_server");

    // 创建一个span，用于追踪当前请求
    let mut span = tracer.start(format!("{} {}", req.method(), req.uri().path()));

    // 根据请求路径和方法进行处理
    match (req.method(), req.uri().path()) {
        // 如果请求路径是/rolldice，返回一个随机数
        (&Method::GET, "/rolldice") => {
            let random_number = rand::thread_rng().gen_range(1..7);
            *response.body_mut() = Body::from(random_number.to_string());
            // 设置span的属性，例如请求路径和请求方法
            span.set_status(Status::Ok);
        }
        // 其他路径返回404
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
            // 设置span的属性，例如请求路径和请求方法
            span.set_status(Status::error("404 Not Found"));
        }
    };

    Ok(response)
}

fn init_tracer_provider() -> Result<opentelemetry_sdk::trace::TracerProvider, TraceError> {
    opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint("http://localhost:4317"),
        )
        .with_trace_config(
            sdktrace::Config::default().with_resource(Resource::new(vec![KeyValue::new(
                SERVICE_NAME,
                "tracing-jaeger",
            )])),
        )
        .install_batch(runtime::Tokio)
}

// 初始化追踪器
fn init_tracer() {
    let tracer_provider = init_tracer_provider().expect("Failed to initialize tracer provider.");
    global::set_tracer_provider(tracer_provider.clone());
}

// 主函数，启动异步运行时
#[tokio::main]
async fn main() {
    init_tracer();

    // 设置服务器监听地址
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));

    // 创建服务，每个连接都会调用handle函数处理请求
    let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });

    // 绑定地址并启动服务器
    let server = Server::bind(&addr).serve(make_svc);

    println!("Listening on {addr}");
    if let Err(e) = server.await {
        eprintln!("server error: {e}");
    }
}
