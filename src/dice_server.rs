use lazy_static::lazy_static;
use opentelemetry::metrics::Counter;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use actix_web::{get, App, HttpRequest, HttpResponse, HttpServer, Responder};
use awc::http::header::{HeaderMap, HeaderName, HeaderValue};
use opentelemetry::metrics::MetricsError;
use opentelemetry::trace::{SpanKind, TraceContextExt, TraceError, Tracer};
use opentelemetry::Context;
use opentelemetry::{global, KeyValue};
use opentelemetry_http::HeaderInjector;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace as sdktrace;
use opentelemetry_sdk::{runtime, Resource};
use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
use rand::Rng;

// 定义一个结构体来保存我们的计数器
struct HttpMetrics {
    success_counter: Counter<u64>,
    failure_counter: Counter<u64>,
}

// 使用 lazy_static 创建一个全局的 HttpMetrics 实例
lazy_static! {
    static ref HTTP_METRICS: Arc<HttpMetrics> = Arc::new({
        let meter = global::meter("http_metrics");
        HttpMetrics {
            success_counter: meter
                .u64_counter("http_requests_success")
                .with_description("成功的 HTTP 请求总数")
                .init(),
            failure_counter: meter
                .u64_counter("http_requests_failure")
                .with_description("失败的 HTTP 请求总数")
                .init(),
        }
    });
}

fn inject_context(request: &mut HeaderMap, cx: &Context) {
    // 使用 OpenTelemetry 的 HTTP 传播器 (propagator) 注入追踪上下文到 HTTP 请求头

    let mut r_headers = http::HeaderMap::new();

    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut HeaderInjector(&mut r_headers));
    });

    println!("randnum: r_headers: {:?}", &r_headers);

    for (key, value) in r_headers.iter() {
        let header_name = HeaderName::from_str(key.as_str()).unwrap();
        let header_value = HeaderValue::from_str(value.to_str().unwrap()).unwrap();

        request.insert(header_name, header_value);
    }
}

fn extract_context(req: &HttpRequest) -> Context {
    global::get_text_map_propagator(|propagator| {
        let mut headers: HashMap<String, String> = HashMap::new();

        for (key, value) in req.headers().iter() {
            headers.insert(key.to_string(), value.to_str().unwrap().to_string());
        }
        propagator.extract(&headers)
    })
}

fn get_cx_from_parent_cx<'a>(
    tracer_name: String,
    spam_name: String,
    parent_cx: Option<&Context>,
) -> Context {
    let span;
    let tracer = global::tracer(tracer_name);
    match parent_cx {
        Some(cx) => {
            // 使用提取到的上下文作为父上下文，创建一个新的 span
            span = tracer
                .span_builder(spam_name)
                .with_kind(SpanKind::Server)
                .start_with_context(&tracer, cx);
        }
        None => {
            span = tracer
                .span_builder(spam_name)
                .with_kind(SpanKind::Server)
                .start(&tracer);
        }
    }

    Context::current_with_span(span)
}

#[get("/randnum")]
async fn randnum() -> impl Responder {
    let cx = get_cx_from_parent_cx("dice_server".to_string(), "randnum".to_string(), None);

    println!("randnum: 当前上下文: {:?}", cx);
    println!("randnum: 当前 span: {:?}", cx.span());

    let mut request = awc::Client::default().get("http://127.0.0.1:8080/gen_num");

    let req_headers = request.headers_mut();
    inject_context(req_headers, &cx);

    match request.send().await {
        Ok(mut response) => match response.body().await {
            Ok(body) => {
                HTTP_METRICS.success_counter.add(1, &[]);
                cx.span().add_event("从 gen_num 收到响应", vec![]);
                HttpResponse::Ok().body(body)
            }
            Err(_) => {
                HTTP_METRICS.failure_counter.add(1, &[]);
                cx.span().add_event("读取响应体失败", vec![]);
                HttpResponse::InternalServerError().body("读取响应体失败")
            }
        },
        Err(_) => {
            HTTP_METRICS.failure_counter.add(1, &[]);
            cx.span().add_event("发送请求失败", vec![]);
            HttpResponse::InternalServerError().body("发送请求失败")
        }
    }
}

#[get("/gen_num")]
async fn gen_num(req: HttpRequest) -> impl Responder {
    // 使用 OpenTelemetry 的 HTTP 传播器 (propagator) 从 HTTP 请求头中提取追踪上下文
    let parent_cx = extract_context(&req);

    println!("gen_num: parent_cx: {:?}", parent_cx);
    println!("gen_num: parent_cx.span: {:?}", parent_cx.span());

    let cx = get_cx_from_parent_cx(
        "dice_server".to_string(),
        "gen_num".to_string(),
        Some(&parent_cx),
    );

    let mut random_number: i32 = rand::thread_rng().gen_range(1..10);
    random_number *= 2;

    // 生成奇数 or 偶数?
    let is_odd = is_odd(&cx);
    if is_odd {
        random_number += 1;
    }

    cx.span().add_event(
        "Generated random number",
        vec![opentelemetry::KeyValue::new(
            "number",
            random_number.to_string(),
        )],
    );

    HTTP_METRICS.success_counter.add(1, &[]);
    HttpResponse::Ok().body(random_number.to_string())
}

fn is_odd(cx: &Context) -> bool {
    let cx = get_cx_from_parent_cx("dice_server".to_string(), "is_odd".to_string(), Some(cx));

    // 50% 的概率返回 true，50% 的概率返回 false
    let res = rand::thread_rng().gen_bool(0.5);
    cx.span().add_event(
        "odd or even",
        vec![opentelemetry::KeyValue::new("is odd?", res.to_string())],
    );
    res
}

fn init_meter_provider() -> Result<opentelemetry_sdk::metrics::SdkMeterProvider, MetricsError> {
    opentelemetry_otlp::new_pipeline()
        .metrics(runtime::Tokio)
        .with_period(Duration::from_secs(5))
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic() // 使用 Tonic 作为 gRPC 客户端
                .with_endpoint("http://localhost:4317"), // TODO: 给出一个metric收集器的方案
        )
        .build()
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
    global::set_tracer_provider(tracer_provider);
    let meter_provider = init_meter_provider().expect("Failed to initialize meter provider.");
    global::set_meter_provider(meter_provider);

    global::set_text_map_propagator(TraceContextPropagator::new());
}

// 主函数，启动异步运行时
#[tokio::main]
async fn main() -> std::io::Result<()> {
    init_tracer();

    HttpServer::new(|| App::new().service(randnum).service(gen_num))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}
