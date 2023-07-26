use std::{time::Duration, collections::HashMap, sync::Arc};
use tokio::{
    sync::mpsc::{channel, Sender},
    time::sleep,
};
use reqwest::Client;
use crate::{post::Post, api::FediApi, error::Error, db::Database};

async fn update_post(post: &Post, api: &FediApi, db: &Database, client: &Client) -> Result<(), Error> {
    let descendants = api.get_descendants_of(post, client).await?;
    db.insert_descendants(post, descendants.into_iter()).await?;
    Ok(())
}

fn spawn_worker(host: String, db: Database, client: Arc<Client>) -> Sender<Post> {
    let (tx, mut rx) = channel(16);
    tokio::spawn(async move {
        match FediApi::from_host(&host, &db, &client).await {
            Ok(api) => {
                while let Some(post) = rx.recv().await {
                    if let Err(e) = update_post(&post, &api, &db, &client).await {
                        tracing::error!("descendants: update {}: {:?}", post.uri, e);
                    }
                }
            },
            Err(e) => {
                while let Some(post) = rx.recv().await {
                    tracing::error!("Failed to get api of {}: {:?}", post.uri, e);
                }
            },
        };
    });
    tx
}

async fn update(db: &Database, workers: &mut HashMap<String, Sender<Post>>, client: &Arc<Client>) -> Result<(), Error> {
    let posts = db.get_all_posts().await?;
    for post in posts {
        let host = match post.host() {
            Some(host_str) => host_str,
            None => {
                tracing::error!("descendants: host unknown: {}", post.uri);
                continue;
            }
        };
        let tx = workers.entry(host.clone())
                .or_insert_with(|| spawn_worker(host, db.clone(), client.clone()));
        if let Err(e) = tx.send(post).await {
            tracing::error!("descendants: send post to worker: {:?}", e);
        }
    }
    Ok(())
}


pub fn spawn(db: Database, client: Arc<Client>) {
    tokio::spawn(async move {
        let mut workers = HashMap::new();
        loop {
            if let Err(e) = update(&db, &mut workers, &client).await {
                tracing::error!("descendants: {:?}", e);
            }
            sleep(Duration::from_secs(60)).await;
        }
    });
}
