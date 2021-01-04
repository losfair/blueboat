use rusty_workers::types::*;
use rusty_workers::tarpc;
use std::sync::Arc;
use anyhow::Result;
use thiserror::Error;

const MAX_RESPONSE_BODY_SIZE: usize = 1048576 * 8; // 8M

#[derive(Error, Debug)]
enum FetchError {
    #[error("response body too large")]
    ResponseBodyTooLarge,
}

pub struct FetchState {
    client: reqwest::Client,
}

#[derive(Clone)]
pub struct FetchServer {
    state: Arc<FetchState>,
}

impl FetchServer {
    pub fn new(state: Arc<FetchState>) -> Self {
        FetchServer {
            state,
        }
    }
}

#[tarpc::server]
impl rusty_workers::rpc::FetchService for FetchServer {
    async fn fetch(self, _: tarpc::context::Context, req: RequestObject) -> GenericResult<Result<ResponseObject, String>> {
        debug!("fetch request: {:?}", req);
        let res = run_fetch(&self.state, req).await;
        match res {
            Ok(x) => {
                Ok(Ok(x))
            }
            Err(e) => {
                Ok(Err(format!("fetch error: {:?}", e)))
            }
        }
    }
}

rusty_workers::impl_listen!(FetchServer, rusty_workers::rpc::FetchService, 1000);

impl FetchState {
    pub fn new() -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            client: reqwest::Client::builder()
                .user_agent("rusty-workers")
                .build()?
        }))
    }
}

async fn run_fetch(state: &FetchState, req: RequestObject) -> Result<ResponseObject> {
    use reqwest::{Method, Url, Request, header::{HeaderName, HeaderValue}};

    let url = Url::parse(&req.url)?;
    let method = Method::from_bytes(&req.method.as_bytes())?;
    let mut target_req = Request::new(method, url);

    let headers = target_req.headers_mut();
    for (k, v) in req.headers {
        for item in v {
            headers.append(HeaderName::from_bytes(k.as_bytes())?, HeaderValue::from_str(&item)?);
        }
    }

    let mut res = state.client.execute(target_req).await?;

    // Limiting response body size: https://github.com/seanmonstar/reqwest/issues/848
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_RESPONSE_BODY_SIZE {
            return Err(FetchError::ResponseBodyTooLarge.into());
        }
    }
    let body = match String::from_utf8(body) {
        Ok(x) => ResponseBody::Text(x),
        Err(e) => ResponseBody::Binary(e.into_bytes()),
    };
    let mut target_res = ResponseObject {
        status: res.status().as_u16(),
        body,
    };
    Ok(target_res)
}