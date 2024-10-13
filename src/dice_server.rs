use actix_web::{get, App, HttpResponse, HttpServer, Responder};
use opentelemetry::trace::{Span, Status, TraceError, Tracer};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace as sdktrace;
use opentelemetry_sdk::{runtime, Resource};
use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
use rand::Rng;

#[get("/randnum")]
async fn randnum() -> impl Responder {
    // 获取全局追踪器
    let tracer = global::tracer("randnum");
    // 创建一个span，用于追踪当前请求
    let mut span = tracer.start("randnum");
    span.set_status(Status::Ok);

    let random_number = rand::thread_rng().gen_range(1..7);
    println!("Generated random number: {}", random_number);
    HttpResponse::Ok().body(random_number.to_string())
}

// 初始化追踪提供者 (Tracer Provider)，该函数返回一个全局的 `TracerProvider`
fn init_tracer_provider() -> Result<opentelemetry_sdk::trace::TracerProvider, TraceError> {
    opentelemetry_otlp::new_pipeline()
        .tracing()
        // 配置一个 OTLP 导出器，用于将追踪数据发送到指定的后端（在这里是 Jaeger 或 OpenTelemetry Collector）
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic() // 使用 Tonic 作为 gRPC 客户端
                .with_endpoint("http://localhost:4317"), // 指定 OTLP 接收器的地址
        )
        // 配置追踪器的资源信息，例如服务名称等
        .with_trace_config(
            sdktrace::Config::default().with_resource(Resource::new(vec![KeyValue::new(
                SERVICE_NAME,
                "tracing-jaeger", // 设置服务名称为 "tracing-jaeger"
            )])),
        )
        // 使用批量处理器进行追踪数据的导出，`runtime::Tokio` 用于支持异步操作
        .install_batch(runtime::Tokio)
}

// 初始化全局追踪器，将 `TracerProvider` 设置为全局
fn init_tracer() {
    let tracer_provider = init_tracer_provider().expect("Failed to initialize tracer provider.");
    global::set_tracer_provider(tracer_provider.clone());
}

// 主函数，启动异步运行时
#[tokio::main]
async fn main() -> std::io::Result<()> {
    init_tracer();

    HttpServer::new(|| App::new().service(randnum))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}
