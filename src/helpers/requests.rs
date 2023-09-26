use reqwest;
use tokio;

pub struct ReqwestValues {
    url: String,
    api_key: i32,
    user_id: i16
}

impl ReqwestValues {
    pub fn return_pods() -> Self {
        let body = reqwest::Client::new();
        let return_val = body
            .get(Self.api_key)
            .header("Api-Key", Self.url)
            .send()?;
    }
}