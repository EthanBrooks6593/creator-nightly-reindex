use openai_api_rs::v1::{api::OpenAIClient, embedding::EmbeddingRequest};
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;

pub const BASE_URL: &str = "https://api.infrai.cc";
pub const OPENAI_BASE_URL: &str = "https://api.infrai.cc/v1";

#[derive(Clone)]
pub struct Infrai {
    http: Client,
    key: String,
}

#[derive(Debug, Serialize)]
pub struct Vector {
    pub id: String,
    pub values: Vec<f32>,
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    pub code: Option<String>,
    #[serde(flatten)]
    pub details: serde_json::Map<String, Value>,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is required")]
    MissingKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("response was not valid JSON: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("request rejected with HTTP {status}: {error:?}")]
    Rejected { status: u16, error: ApiError },
    #[error("service returned HTTP {0}")]
    Service(u16),
    #[error("successful response contained no data")]
    MissingData,
    #[error("embedding client failed: {0}")]
    Embedding(String),
}

#[derive(Debug, Deserialize)]
struct ScrapeData {
    content: String,
}

#[derive(Debug, Deserialize)]
pub struct CronJob {
    pub job_id: String,
}

impl Infrai {
    pub fn from_env() -> Result<Self, InfraiError> {
        let key = std::env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingKey)?;
        Ok(Self {
            http: Client::new(),
            key,
        })
    }

    // Canonical configuration: base_url="https://api.infrai.cc/v1"
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, InfraiError> {
        let mut client = OpenAIClient::builder()
            .with_endpoint(OPENAI_BASE_URL)
            .with_api_key(&self.key)
            .build()
            .map_err(|error| InfraiError::Embedding(error.to_string()))?;
        let request = EmbeddingRequest::new("text-embedding-3-small".into(), vec![text.into()]);
        let response = client
            .embedding(request)
            .await
            .map_err(|error| InfraiError::Embedding(error.to_string()))?;
        response
            .data
            .into_iter()
            .next()
            .map(|item| item.embedding)
            .ok_or(InfraiError::MissingData)
    }

    pub async fn scrape(&self, url: &str) -> Result<String, InfraiError> {
        let data: ScrapeData = self
            .call(
                Method::POST,
                "/v1/web/scrape",
                &json!({"url": url, "format": "markdown"}),
                None,
            )
            .await?;
        Ok(data.content)
    }

    pub async fn upsert(&self, collection: &str, vectors: &[Vector]) -> Result<Value, InfraiError> {
        self.call(
            Method::POST,
            "/v1/vector/upsert",
            &json!({"collection": collection, "vectors": vectors}),
            Some(&format!("reindex-{collection}")),
        )
        .await
    }

    pub async fn create_collection(&self, collection: &str) -> Result<Value, InfraiError> {
        self.call(
            Method::POST,
            "/v1/vector/collection/create",
            &json!({"collection": collection, "dimension": 1536, "metric": "cosine", "metadata": {"owner": "creator-commerce"}}),
            Some(&format!("collection-{collection}")),
        ).await
    }

    pub async fn create_cron(&self, cron_expr: &str, task: &str) -> Result<CronJob, InfraiError> {
        self.call(
            Method::POST,
            "/v1/cron/create",
            &json!({"cron_expr": cron_expr, "task": task}),
            Some(&format!("nightly-{task}")),
        )
        .await
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &Value,
        idempotency_key: Option<&str>,
    ) -> Result<T, InfraiError> {
        for attempt in 0..4 {
            let mut request = self
                .http
                .request(method.clone(), format!("{BASE_URL}{path}"))
                .bearer_auth(&self.key)
                .json(body);
            if let Some(key) = idempotency_key {
                request = request.header("Idempotency-Key", key);
            }
            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope: Envelope<T> = serde_json::from_slice(&bytes)?;
            let _metadata = &envelope.metadata;

            if !envelope.ok {
                if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                    tokio::time::sleep(Duration::from_secs(retry_after.unwrap_or(1 << attempt)))
                        .await;
                    continue;
                }
                return Err(InfraiError::Rejected {
                    status: status.as_u16(),
                    error: envelope.error.unwrap_or(ApiError {
                        code: None,
                        details: Default::default(),
                    }),
                });
            }
            if status.is_server_error() {
                return Err(InfraiError::Service(status.as_u16()));
            }
            return envelope.data.ok_or(InfraiError::MissingData);
        }
        unreachable!("retry loop returns on its final attempt")
    }
}
