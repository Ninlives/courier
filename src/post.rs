use std::time::{SystemTime, UNIX_EPOCH};
use serde::Deserialize;

#[allow(non_snake_case)]
#[derive(Deserialize, Clone, Debug)]
pub struct Post {
    pub uri: String,
    #[serde(skip, default = "fetch_time")]
    pub fetch_time: i64,
    #[serde(rename = "id")]
    pub timeline_id: Option<String>,
    #[serde(alias = "createdAt")]
    pub created_at: Option<String>,
    #[serde(alias = "replyId")]
    pub in_reply_to_id: Option<String>,
    #[serde(alias = "renote")]
    pub reblog: Option<Box<Post>>,
}

fn fetch_time() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs().try_into().unwrap(),
        Err(_) => 0
    } 
}

impl Post {
    pub fn host(&self) -> Option<String> {
        reqwest::Url::parse(&self.uri)
            .ok()
            .and_then(|url| url.domain()
                      .map(str::to_lowercase)
            )
    }

    pub fn id(&self) -> Option<String> {
        reqwest::Url::parse(&self.uri)
            .ok()?
            .path_segments()?
            .last()
            .map(|id| id.to_string())
    }

    pub fn origin(&self) -> Self {
        match &self.reblog {
            Some(origin) => (**origin).clone(),
            None => self.clone()
        }
    }
    
    pub fn is_reply(&self) -> bool {
        self.in_reply_to_id.is_some()
    }
}
