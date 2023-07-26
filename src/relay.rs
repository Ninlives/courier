use std::{sync::Arc, collections::{HashMap}};
use futures::{channel::mpsc::{channel as future_channel, Sender as FutureSender}, StreamExt};
use serde_json::json;
use sigh::PrivateKey;
use tokio::{
    sync::mpsc::{channel, Sender},
};
use crate::{post::Post, send, actor::{Actor, RemoteActor, ActorKind::CompletionRelay}};

struct Job {
    post_uri: Arc<String>,
    actor_id: Arc<String>,
    body: Arc<Vec<u8>>,
    key_id: String,
    private_key: Arc<PrivateKey>,
    inbox_url: reqwest::Url,
}

fn spawn_worker(client: Arc<reqwest::Client>) -> FutureSender<Job> {
    let (tx, mut rx) = future_channel(1024);

    tokio::spawn(async move {
        while let Some(Job { post_uri, actor_id, key_id, private_key, body, inbox_url }) = rx.next().await {
            tracing::debug!("relay {} from {} to {}", post_uri, actor_id, inbox_url);
            if let Err(e) = send::send_raw(
                &client, inbox_url.as_str(),
                &key_id, &private_key, body
            ).await {
                tracing::error!("relay::send {:?}", e);
            } else {
                // success
                systemd::daemon::notify(
                    false, [
                        (systemd::daemon::STATE_WATCHDOG, "1")
                    ].iter()
                ).unwrap();
            }
        }

        panic!("Worker dead");
    });

    tx
}

pub fn spawn(
    client: Arc<reqwest::Client>,
    hostname: Arc<String>,
    private_key: PrivateKey,
    ) -> Sender<(Arc<Actor>, Arc<RemoteActor>, Arc<Post>)> {
    let private_key = Arc::new(private_key);
    let (tx, mut rx) = channel::<(Arc<Actor>, Arc<RemoteActor>, Arc<Post>)>(16);

    tokio::spawn(async move {
        let mut workers = HashMap::new();

        while let Some((actor, remote_actor, post)) = rx.recv().await {
            let post = post.origin();

            let Ok(inbox_url) = reqwest::Url::parse(&remote_actor.inbox) else { continue; };
            let Ok(post_uri) = reqwest::Url::parse(&post.uri) else { continue; };
            // Prevent relaying back to the originating instance.
            if inbox_url.host_str() == post_uri.host_str() {
                continue;
            }

            let announce_id = format!("https://{}/announce/{}", hostname, urlencoding::encode(&post.uri));
            let actor_id = Arc::new(actor.uri());
            let body = json!({
                "@context": "https://www.w3.org/ns/activitystreams",
                "type": if let CompletionRelay = actor.kind { vec![ "Announce", "Relay" ] } else { vec![ "Announce" ] },
                "actor": *actor_id,
                "to": ["https://www.w3.org/ns/activitystreams#Public"],
                "object": &post.uri,
                "id": announce_id,
            });
            let body = Arc::new(
                serde_json::to_vec(&body)
                .unwrap()
                );


            // Lookup/create worker queue per inbox.
            let tx = workers.entry(inbox_url.host_str().unwrap_or("").to_string())
                .or_insert_with(|| spawn_worker(client.clone()));
            // Create queue item.
            let job = Job {
                post_uri: Arc::new(post.uri.to_string()),
                actor_id: actor_id.clone(),
                body: body.clone(),
                key_id: actor.key_id(),
                private_key: private_key.clone(),
                inbox_url,
            };
            // Enqueue job for worker.
            let _ = tx.try_send(job);
        }
    });
    tx
}
