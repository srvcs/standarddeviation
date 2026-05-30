use axum::body::Body;
use axum::extract::Json as JsonExtract;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use srvcs_standarddeviation::{api::Deps, health, router, telemetry};
use tower::ServiceExt;

const DEAD_URL: &str = "http://127.0.0.1:1";

async fn serve(app: AxumRouter) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Mock `srvcs-populationstddev` that ACTUALLY COMPUTES the population standard
/// deviation of the `values` array (sqrt of the mean of squared deviations from
/// the mean) and returns `{"values", "result": <f64>}`.
async fn spawn_computing_populationstddev() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|JsonExtract(req): JsonExtract<Value>| async move {
            let nums: Vec<f64> = req["values"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default();
            let n = nums.len() as f64;
            let mean = nums.iter().sum::<f64>() / n;
            let variance = nums.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
            let result = variance.sqrt();
            Json(json!({ "values": req["values"], "result": result }))
        }),
    );
    serve(app).await
}

/// Mock that always answers with a fixed status + body (used to simulate a
/// `422` rejection forwarded from a dependency).
async fn spawn_fixed(status: StatusCode, body: Value) -> String {
    let app = AxumRouter::new().route(
        "/",
        post(move || {
            let body = body.clone();
            async move { (status, Json(body)) }
        }),
    );
    serve(app).await
}

fn app(populationstddev_url: &str) -> axum::Router {
    router(
        telemetry::metrics_handle_for_tests(),
        Deps {
            populationstddev_url: populationstddev_url.to_string(),
        },
    )
}

async fn eval(populationstddev_url: &str, values: Value) -> (StatusCode, Value) {
    let res = app(populationstddev_url)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "values": values }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn status_of(uri: &str) -> StatusCode {
    app(DEAD_URL)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

fn approx(got: &Value, expected: f64) -> bool {
    got.as_f64().map(|x| (x - expected).abs() < 1e-9) == Some(true)
}

// --- Standard endpoints ---

#[tokio::test]
async fn healthz_ok() {
    assert_eq!(status_of("/healthz").await, StatusCode::OK);
}

#[tokio::test]
async fn readyz_reflects_state() {
    health::set_ready(true);
    assert_eq!(status_of("/readyz").await, StatusCode::OK);
}

#[tokio::test]
async fn openapi_ok() {
    assert_eq!(status_of("/openapi.json").await, StatusCode::OK);
}

// --- Correctness cases, exercised against a REAL computing dependency ---

#[tokio::test]
async fn stddev_one_through_five() {
    let pop = spawn_computing_populationstddev().await;
    let (status, body) = eval(&pop, json!([1, 2, 3, 4, 5])).await;
    assert_eq!(status, StatusCode::OK);
    // standarddeviation([1,2,3,4,5]) ~= 1.4142135623730951 == sqrt(2)
    assert!(
        approx(&body["result"], std::f64::consts::SQRT_2),
        "got {:?}",
        body["result"]
    );
    assert_eq!(body["values"], json!([1, 2, 3, 4, 5]));
}

#[tokio::test]
async fn stddev_identical_values_is_zero() {
    let pop = spawn_computing_populationstddev().await;
    let (status, body) = eval(&pop, json!([5, 5, 5, 5])).await;
    assert_eq!(status, StatusCode::OK);
    assert!(approx(&body["result"], 0.0), "got {:?}", body["result"]);
}

#[tokio::test]
async fn stddev_singleton_is_zero() {
    let pop = spawn_computing_populationstddev().await;
    let (status, body) = eval(&pop, json!([42])).await;
    assert_eq!(status, StatusCode::OK);
    assert!(approx(&body["result"], 0.0), "got {:?}", body["result"]);
}

#[tokio::test]
async fn stddev_two_element_pair() {
    let pop = spawn_computing_populationstddev().await;
    // mean = 1.5; deviations +-0.5; variance = 0.25; sqrt = 0.5
    let (status, body) = eval(&pop, json!([1, 2])).await;
    assert_eq!(status, StatusCode::OK);
    assert!(approx(&body["result"], 0.5), "got {:?}", body["result"]);
}

#[tokio::test]
async fn stddev_with_floats_and_negatives() {
    let pop = spawn_computing_populationstddev().await;
    // [-2, -1, 0, 1, 2]: mean 0, variance = (4+1+0+1+4)/5 = 2, sqrt = sqrt(2)
    let (status, body) = eval(&pop, json!([-2.0, -1.0, 0.0, 1.0, 2.0])).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approx(&body["result"], std::f64::consts::SQRT_2),
        "got {:?}",
        body["result"]
    );
}

// --- Error / edge cases ---

#[tokio::test]
async fn forwards_422_from_populationstddev() {
    let pop = spawn_fixed(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "error": "value is not a number" }),
    )
    .await;
    let (status, body) = eval(&pop, json!([1, "nope", 3])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "value is not a number");
}

#[tokio::test]
async fn degrades_when_populationstddev_unreachable() {
    let (status, body) = eval(DEAD_URL, json!([1, 2, 3])).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-populationstddev");
}
