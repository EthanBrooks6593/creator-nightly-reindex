use creator_nightly_reindex::{
    infrai::{Infrai, InfraiError},
    reindex::{refresh_creator_index, CreatorAsset},
};
use serde_json::json;
use std::{env, sync::Arc};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[derive(Debug, Error)]
enum ServiceError {
    #[error(transparent)]
    Infrai(#[from] InfraiError),
    #[error("{0} is required")]
    MissingEnv(&'static str),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("asset manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
}

#[tokio::main]
async fn main() -> Result<(), ServiceError> {
    let command = env::args().nth(1).unwrap_or_else(|| "serve".into());
    let infrai = Infrai::from_env()?;
    match command.as_str() {
        "setup" => {
            let collection = env_or("CREATOR_COLLECTION", "creator-assets");
            infrai.create_collection(&collection).await?;
            println!("collection ready: {collection}");
        }
        "install-nightly" => {
            let public_url = required("PUBLIC_REINDEX_URL")?;
            let job = infrai.create_cron("0 2 * * *", &public_url).await?;
            println!("nightly job installed: {}", job.job_id);
        }
        "serve" => serve(Arc::new(infrai)).await?,
        _ => eprintln!("commands: setup | install-nightly | serve"),
    }
    Ok(())
}

async fn serve(infrai: Arc<Infrai>) -> Result<(), ServiceError> {
    let listener = TcpListener::bind(env_or("LISTEN_ADDR", "127.0.0.1:8080")).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let client = Arc::clone(&infrai);
        tokio::spawn(async move {
            if let Err(error) = handle(stream, client).await {
                eprintln!("request failed: {error}");
            }
        });
    }
}

async fn handle(mut stream: TcpStream, infrai: Arc<Infrai>) -> Result<(), ServiceError> {
    let mut request = [0_u8; 2048];
    let size = stream.read(&mut request).await?;
    let first_line = String::from_utf8_lossy(&request[..size]);
    if !first_line.starts_with("POST /reindex ") {
        write_response(&mut stream, 404, &json!({"ok": false})).await?;
        return Ok(());
    }

    let assets: Vec<CreatorAsset> = serde_json::from_str(&required("CREATOR_ASSETS_JSON")?)?;
    let collection = env_or("CREATOR_COLLECTION", "creator-assets");
    match refresh_creator_index(&infrai, &collection, &assets).await {
        Ok(report) => {
            write_response(&mut stream, 200, &json!({"ok": true, "data": report})).await?
        }
        Err(InfraiError::Rejected { status, error }) => {
            let client_status = if (400..500).contains(&status) {
                status
            } else {
                502
            };
            write_response(
                &mut stream,
                client_status,
                &json!({"ok": false, "error": error.code}),
            )
            .await?;
        }
        Err(error) => {
            write_response(
                &mut stream,
                502,
                &json!({"ok": false, "error": error.to_string()}),
            )
            .await?
        }
    }
    Ok(())
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    body: &serde_json::Value,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(body)?;
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&bytes).await
}

fn required(name: &'static str) -> Result<String, ServiceError> {
    env::var(name).map_err(|_| ServiceError::MissingEnv(name))
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}
