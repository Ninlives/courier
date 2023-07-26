use std::sync::Arc;
use http::StatusCode;
use serde::Serialize;
use sigh::{PrivateKey, SigningConfig, alg::RsaSha256};
use crate::{digest, error::Error};

pub async fn send<T: Serialize>(
    client: &reqwest::Client,
    uri: &str,
    key_id: &str,
    private_key: &PrivateKey,
    body: &T,
) -> Result<(), Error> {
    let body = Arc::new(
        serde_json::to_vec(body)
            .map_err(Error::Json)?
    );
    send_raw(client, uri, key_id, private_key, body).await
}

pub async fn send_raw(
    client: &reqwest::Client,
    uri: &str,
    key_id: &str,
    private_key: &PrivateKey,
    body: Arc<Vec<u8>>,
) -> Result<(), Error> {
    let url = reqwest::Url::parse(uri)
        .map_err(|_| Error::InvalidUri)?;
    let host = format!("{}", url.host().ok_or(Error::InvalidUri)?);
    let digest_header = digest::generate_header(&body)
        .map_err(|()| Error::Digest)?;
    let mut req = http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("host", &host)
        .header("content-type", "application/activity+json")
        .header("date", chrono::Utc::now().to_rfc2822()
            .replace("+0000", "GMT"))
        .header("digest", digest_header)
        .body(body.as_ref().clone())
        .map_err(Error::HttpReq)?;
    SigningConfig::new(RsaSha256, private_key, key_id)
        .sign(&mut req)?;
    let req: reqwest::Request = req.try_into()?;
    let res = client.execute(req)
        .await?;
    if res.status() >= StatusCode::OK && res.status() < StatusCode::MULTIPLE_CHOICES {
        Ok(())
    } else {
        tracing::error!("send_raw {} response HTTP {}", url, res.status());
        let response = res.text().await?;
        tracing::error!("send_raw {} response body: {:?}", url, response);
        Err(Error::Response(response))
    }
}
