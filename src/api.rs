use std::time::Duration;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::{json, Value, Map};
use reqwest::{Client, StatusCode};
use async_recursion::async_recursion;
use crate::{db::Database, post::Post, error::Error};

// FIXME: Refactor for better extensibility

pub enum FediApi {
    Mastodon,
    Misskey,
    Calckey,
}

#[derive(Deserialize)]
struct Context {
    // ancestors: Vec<Post>,
    descendants: Vec<Post>,
}

impl FediApi {

    pub async fn determine(host: &str, client: &Client) -> Result<Self, Error> {
        let mastodon_meta_url = format!("https://{host}/api/v1/instance");
        let misskey_meta_url = format!("https://{host}/api/meta");

        let res = client.get(mastodon_meta_url)
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        let mastodon_compatible = res.status() == StatusCode::OK;

        let res = client.post(misskey_meta_url)
            .json(&json!({ "detail": false }))
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        let misskey_compatible = res.status() == StatusCode::OK;

        match (mastodon_compatible, misskey_compatible) {
            (true, true)   => Ok(FediApi::Calckey),
            (true, false)  => Ok(FediApi::Mastodon),
            (false, true)  => Ok(FediApi::Misskey),
            (false, false) => Err(Error::Api(format!("Failed to determine api variant of {host}"))),
        }
    }

    pub async fn from_host(host: &str, db: &Database, client: &Client) -> Result<Self, Error> {
        match db.get_instance_type(host).await {
            Ok(instance_type) => {
                Self::from_str(&instance_type)
            },
            Err(_) => {
                let host_api = Self::determine(host, client).await?;
                db.add_instance(host, &host_api.to_str()).await?;
                Ok(host_api)
            }
        }
    }

    pub fn from_str(host_type: &str) -> Result<Self, Error> {
        match host_type {
            "mastodon" => Ok(FediApi::Mastodon),
            "misskey"  => Ok(FediApi::Misskey),
            "calckey"  => Ok(FediApi::Calckey),
            _          => Err(Error::Api(format!("Unknown host type: {host_type}"))),
        }
    }

    pub fn to_str(&self) -> String {
        match self {
            FediApi::Mastodon => "mastodon".to_string(),
            FediApi::Misskey => "misskey".to_string(),
            FediApi::Calckey => "calckey".to_string(),
        }
    }

    pub async fn get_trending_posts(&self, host: &str, client: &Client) -> Result<Vec<Post>, Error> {
        match self {
            FediApi::Mastodon => Self::mastodon_get_trending_posts(host, client).await,
            _                 => Self::misskey_get_trending_posts(host, client).await,
        }
    }

    pub async fn get_global_timeline(&self, host: &str, since_id: &Option<String>, client: &Client) -> Result<Vec<Post>, Error> {
        match self {
            FediApi::Mastodon => Self::mastodon_get_global_timeline(host, since_id, client).await,
            _                 => Self::misskey_get_global_timeline(host, since_id, client).await,
        }
    }

    // pub async fn get_ancester_of(&self, post: &Post) -> Result<Post, Error> {
    //     match self {
    //         FediApi::Mastodon => Self::mastodon_get_ancester_of(&post).await,
    //         _                 => Self::misskey_get_ancester_of(&post).await,
    //     }
    // }

    pub async fn get_descendants_of(&self, post: &Post, client: &Client) -> Result<Vec<Post>, Error> {
        match self {
            FediApi::Mastodon => Self::mastodon_get_descendants_of(post, client).await,
            _                 => Self::misskey_get_descendants_of(post, client).await,
        }
    }

    async fn mastodon_get_trending_posts(host: &str, client: &Client) -> Result<Vec<Post>, Error> {
        let trends_url = format!("https://{host}/api/v1/trends/statuses?limit=10");
        let res = client.get(trends_url)
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        if res.status() != StatusCode::OK {
            return Err(Error::Api(format!("Failed to get trending posts of {}: status: {}, response: {}",
                                           host, res.status(), res.text().await?)));
        }
        
        let posts: Vec<Post> = res.json().await?;
        Ok(posts.into_iter().map(|post| post.origin()).collect())
    }

    async fn mastodon_get_global_timeline(host: &str, since_id: &Option<String>, client: &Client) -> Result<Vec<Post>, Error> {
        let timeline_url = match since_id {
            Some(id) => format!("https://{}/api/v1/timelines/public?limit=40?since_id={}", host, id),
            None => format!("https://{}/api/v1/timelines/public?limit=40", host),
        };
        let res = client.get(timeline_url)
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        if res.status() != StatusCode::OK {
            return Err(Error::Api(format!("Failed to get global timeline of {}: status: {}, response: {}",
                                           host, res.status(), res.text().await?)));
        }

        let mut posts: Vec<Post> = res.json().await?;
        posts.sort_by_key(|p| p.created_at.clone().unwrap());
        Ok(posts)
    }

    // async fn mastodon_get_ancester_of(post: &Post) -> Result<Post, Error> {
    //     if !post.is_reply() {
    //         return Ok(post.clone());
    //     }

    //     let context_url = format!("https://{}/api/v1/statuses/{}/context",
    //                               post.host().ok_or_else(|| Error::Api(format!("Failed to get host of {}", post.uri)))?,
    //                               post.id().ok_or_else(|| Error::Api(format!("Failed to get id of {}", post.uri)))?);

    //     let client = Client::new();
    //     let res = client.get(context_url)
    //         .timeout(Duration::MAX)
    //         .send()
    //         .await
    //         .map_err(Error::Http)?;
    //     if res.status() != StatusCode::OK {
    //         return Err(Error::Api(format!("Failed to get context of {}", post.uri)));
    //     }

    //     let context: Context = serde_json::from_str(&res.text().await?).map_err(Error::Json)?;         
    //     Ok(context.ancestors.into_iter().find(|post| !post.is_reply())
    //         .ok_or_else(|| Error::Api(format!("Failed to find the ancestor of {}", post.uri)))?)
    // }

    async fn mastodon_get_descendants_of(post: &Post, client: &Client) -> Result<Vec<Post>, Error> {
        let context_url = format!("https://{}/api/v1/statuses/{}/context",
                                  post.host().ok_or_else(|| Error::Api(format!("Failed to get host of {}", post.uri)))?,
                                  post.id().ok_or_else(|| Error::Api(format!("Failed to get id of {}", post.uri)))?);

        let res = client.get(context_url)
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        if res.status() != StatusCode::OK {
            return Err(Error::Api(format!("Failed to get context of {}: status: {}, response: {}",
                                           post.uri, res.status(), res.text().await?)));
        }

        let context: Context = res.json().await?;         
        Ok(context.descendants)
    }

    async fn misskey_posts_from_response(host: &str, response: reqwest::Response) -> Result<Vec<Post>, Error> {
        let supplement_uri = |p: &mut Map<String, Value>| -> Result<(), Error> {
            let id_value = p.get("id")
                           .ok_or_else(||Error::Api(format!("Failed to parse response from {host}: missing field `id`")))?
                           .clone();
            let id = id_value.as_str()
                             .ok_or_else(||Error::Api(format!("Failed to parse response from {host}: `id` is not string")))?;
            p.entry("uri".to_string()).or_insert(serde_json::to_value(
                 format!("https://{}/notes/{}", host, id))?);
            Ok(())
        };
        let mut posts: Vec<Map<String, Value>> = response.json().await?;
        for post in &mut posts {
            supplement_uri(post)?;
            if let Some(Value::Object(renote)) = post.get_mut("renote") {
                supplement_uri(renote)?;
            }

            if let Some(Value::Object(reply)) = post.get_mut("reply") {
                supplement_uri(reply)?;
            }
        }
        let value = serde_json::to_value(posts)?;
        Ok(serde_json::from_value(value)?)
    }

    async fn misskey_get_trending_posts(host: &str, client: &Client) -> Result<Vec<Post>, Error> {
        let trends_url = format!("https://{host}/api/notes/featured");
        let res = client.post(trends_url)
            .json(&json!({ "limit": 10 }))
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        if res.status() != StatusCode::OK {
            return Err(Error::Api(format!("Failed to get trending posts of {}: status: {}, response: {}",
                                           host, res.status(), res.text().await?)));
        }
        
        let posts = Self::misskey_posts_from_response(host, res).await?;
        Ok(posts.into_iter().map(|post| post.origin()).collect())
    }

    async fn misskey_get_global_timeline(host: &str, since_id: &Option<String>, client: &Client) -> Result<Vec<Post>, Error> {
        let timeline_url = format!("https://{}/api/notes/global-timeline", host);
        let body_json = match since_id {
            Some(id) => json!({ "limit": 100, "sinceId": id }),
            None     => json!({ "limit": 100 }),
        };

        let res = client.post(timeline_url)
            .json(&body_json)
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        if res.status() != StatusCode::OK {
            return Err(Error::Api(format!("Failed to get global timline of {}: status: {}, response: {}",
                                           host, res.status(), res.text().await?)));
        }

        let mut posts = Self::misskey_posts_from_response(host, res).await?;
        posts.sort_by_key(|p| p.created_at.clone().unwrap());
        Ok(posts)
    }

    // async fn misskey_get_ancester_of(post: &Post) -> Result<Post, Error> {
    //     if !post.is_reply() {
    //         return Ok(post.clone());
    //     }
    //     let conversation_url = format!("https://{}/api/notes/conversation",
    //                               post.host().ok_or_else(|| Error::Api(format!("Failed to get host of {}", post.uri)))?);

    //     let client = Client::new();
    //     let res = client.post(conversation_url)
    //         .body(to_vec(&json!({ "noteId": post.id().ok_or_else(|| Error::Api(format!("Failed to get id of {}", post.uri)))? }))?)
    //         .timeout(Duration::MAX)
    //         .send()
    //         .await
    //         .map_err(Error::Http)?;
    //     if res.status() != StatusCode::OK {
    //         return Err(Error::Api(format!("Failed to get conversation of {}", post.uri)));
    //     }

    //     let conversation: Vec<Post> = serde_json::from_str(&res.text().await?).map_err(Error::Json)?;         
    //     Ok(conversation.into_iter().find(|p| !p.is_reply())
    //         .ok_or_else(|| Error::Api(format!("Failed to find the ancestor of {}", post.uri)))?)
    // }

    #[async_recursion]
    async fn misskey_get_replies(uri: &str, host: &str, timeline_id: &str, client: &Client) -> Result<Vec<Post>, Error> {
        let replies_url = format!("https://{}/api/notes/replies", &host);

        let res = client.post(replies_url)
            .json(&json!({ "noteId": timeline_id }))
            .timeout(Duration::MAX)
            .send()
            .await
            .map_err(Error::Http)?;
        if res.status() != StatusCode::OK {
            return Err(Error::Api(format!("Failed to get replies of {}: status: {}, response: {}",
                                           uri, res.status(), res.text().await?)));
        }

        let replies: Vec<Post> = Self::misskey_posts_from_response(host, res).await?;
        let recursive_replies = join_all(replies.clone()
                                         .into_iter()
                                         .map(|p| async move {
                                             let id = p.timeline_id
                                                       .ok_or_else(||Error::Api(format!("{uri}: Posts does not have a timeline_id")))?;
                                             Self::misskey_get_replies(uri, host, &id, client).await
                                         }))
                                         .await
                                         .into_iter()
                                         .collect::<Result<Vec<Vec<Post>>, Error>>()?;
        Ok(recursive_replies.into_iter().fold(replies, |mut acc, val| { acc.extend(val); acc }))
    }

    async fn misskey_get_descendants_of(post: &Post, client: &Client) -> Result<Vec<Post>, Error> {
        let host = post.host().ok_or_else(|| Error::Api(format!("Failed to get host of {}", post.uri)))?;
        let id = post.id().ok_or_else(|| Error::Api(format!("Failed to get id of {}", post.uri)))?;
        Self::misskey_get_replies(&post.uri, &host, &id, client).await
    }
}
