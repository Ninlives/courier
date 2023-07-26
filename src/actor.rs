use std::sync::Arc;
use sigh::{PublicKey, Key};

use crate::{activitypub, error::Error};

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActorKind {
    CompletionRelay,
    TrendsRelay(String),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RemoteActor {
    pub id: String,
    pub inbox: String,
}

impl RemoteActor {
    pub fn host(&self) -> Option<String> {
        reqwest::Url::parse(&self.id)
            .ok()
            .and_then(|url| url.domain()
                      .map(str::to_lowercase)
            )
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Actor {
    pub host: Arc<String>,
    pub kind: ActorKind,
}

impl Actor {
    // FIXME: Very ad-hoc, find a more robust approach
    pub fn from_uri(uri: &str) -> Result<Self, Error> {
        let parsed = reqwest::Url::parse(uri).map_err(|_| Error::InvalidUri)?;
        let host = Arc::new(parsed.host_str().ok_or_else(|| Error::InvalidUri)?.to_string());
        match parsed.path_segments().ok_or_else(|| Error::InvalidUri)?.last() {
            Some("completion") => Ok(Actor {
                host,
                kind: ActorKind::CompletionRelay,
            }),
            Some(instance) => Ok(Actor {
                host,
                kind: ActorKind::TrendsRelay(instance.to_string()),
            }),
            None => Err(Error::InvalidUri),
        }
    }

    pub fn uri(&self) -> String {
        match &self.kind {
            ActorKind::CompletionRelay =>
                format!("https://{}/completion", self.host),
            ActorKind::TrendsRelay(instance) =>
                format!("https://{}/trends/{}", self.host, instance),
        }
    }

    pub fn key_id(&self) -> String {
        format!("{}#key", self.uri())
    }

    pub fn as_activitypub(&self, pub_key: &PublicKey) -> activitypub::Actor {
        activitypub::Actor {
            jsonld_context: serde_json::Value::String("https://www.w3.org/ns/activitystreams".to_string()),
            actor_type: "Service".to_string(),
            id: self.uri(),
            name: Some(match &self.kind {
                ActorKind::CompletionRelay => "Courier Six - Mission Complete".to_string(),
                ActorKind::TrendsRelay(instance) => format!("Courier Six - Trends from [{instance}]"),
            }),
            icon: Some(activitypub::Media {
                media_type: "Image".to_string(),
                content_type: "image/jpeg".to_string(),
                url: format!("https://{}/icon.png", self.host),
            }),
            inbox: self.uri(),
            public_key: activitypub::ActorPublicKey {
                id: self.key_id(),
                owner: Some(self.uri()),
                pem: pub_key.to_pem().unwrap(),
            },
            preferred_username: Some(match &self.kind {
                ActorKind::CompletionRelay => "courier-completion".to_string(),
                ActorKind::TrendsRelay(instance) => format!("courier-{instance}"),
            }),
        }
    }
}
