use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::{OpenApi, ToSchema};

use crate::client::{self, DepError};

pub const SERVICE: &str = "srvcs-standarddeviation";
pub const CONCERN: &str = "statistics: standard deviation (population)";
pub const DEPENDS_ON: &[&str] = &["srvcs-populationstddev"];

/// Dependency endpoints, injected as router state so tests can point them at
/// mock services.
#[derive(Clone)]
pub struct Deps {
    pub populationstddev_url: String,
}

#[derive(Serialize, ToSchema)]
pub struct Info {
    pub service: &'static str,
    pub concern: &'static str,
    pub depends_on: Vec<&'static str>,
}

/// `GET /` — service identity (srvcs service standard).
#[utoipa::path(get, path = "/", responses((status = 200, body = Info)))]
pub async fn index() -> Json<Info> {
    Json(Info {
        service: SERVICE,
        concern: CONCERN,
        depends_on: DEPENDS_ON.to_vec(),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct EvalRequest {
    /// The list of numbers whose population standard deviation to compute.
    #[schema(value_type = Object)]
    pub values: Vec<Value>,
}

#[derive(Serialize, ToSchema)]
pub struct StdDevResponse {
    #[schema(value_type = Object)]
    pub values: Vec<Value>,
    /// The population standard deviation, as an `f64`.
    pub result: f64,
}

fn ok(values: Vec<Value>, result: f64) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "values": values, "result": result })),
    )
        .into_response()
}

fn degraded(dependency: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "dependency unavailable", "dependency": dependency })),
    )
        .into_response()
}

/// Forward a dependency's response verbatim (used to propagate `422` for invalid
/// input from a dependency).
fn forward(status: u16, body: Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (code, Json(body)).into_response()
}

fn no_result(dependency: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": format!("{dependency} returned no usable result") })),
    )
        .into_response()
}

/// Ask `srvcs-populationstddev` to compute the population standard deviation of
/// the values, returning its `f64` result.
async fn ask_populationstddev(url: &str, values: &[Value]) -> Result<f64, Response> {
    let body = json!({ "values": values });
    match client::call(url, &body).await {
        Err(DepError::Unreachable) => Err(degraded("srvcs-populationstddev")),
        Ok((200, body)) => match body.get("result").and_then(Value::as_f64) {
            Some(r) => Ok(r),
            None => Err(no_result("srvcs-populationstddev")),
        },
        Ok((422, body)) => Err(forward(422, body)),
        Ok(_) => Err(degraded("srvcs-populationstddev")),
    }
}

/// `POST /` — the population standard deviation of a list of numbers, as an
/// `f64`.
///
/// This service does no arithmetic of its own. It delegates the entire
/// computation to `srvcs-populationstddev`:
///
/// ```text
/// result = populationstddev(values).result
/// ```
///
/// So `standarddeviation([1,2,3,4,5]) ~= 1.4142135623730951`. Validation (e.g.
/// an empty list, or a non-numeric element) is propagated from
/// `srvcs-populationstddev`'s `422`.
#[utoipa::path(
    post,
    path = "/",
    request_body = EvalRequest,
    responses(
        (status = 200, body = StdDevResponse),
        (status = 422, description = "empty list, or an element is not a valid number (forwarded)"),
        (status = 500, description = "a dependency returned an unusable response"),
        (status = 503, description = "a dependency is unavailable")
    )
)]
pub async fn evaluate(State(deps): State<Deps>, Json(req): Json<EvalRequest>) -> Response {
    let result = match ask_populationstddev(&deps.populationstddev_url, &req.values).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    ok(req.values, result)
}

#[derive(OpenApi)]
#[openapi(
    paths(index, evaluate),
    components(schemas(Info, EvalRequest, StdDevResponse))
)]
pub struct ApiDoc;

/// Serve OpenAPI document
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_documents_routes() {
        let doc = ApiDoc::openapi();
        let root = doc.paths.paths.get("/").expect("path / present");
        assert!(root.get.is_some());
        assert!(root.post.is_some());
    }

    #[tokio::test]
    async fn index_reports_all_dependencies() {
        let Json(info) = index().await;
        assert_eq!(info.service, "srvcs-standarddeviation");
        assert_eq!(info.concern, "statistics: standard deviation (population)");
        assert_eq!(info.depends_on, vec!["srvcs-populationstddev"]);
    }
}
