use std::{sync::Arc, time::Duration};
use tokio::{
    sync::mpsc::Sender,
    time::sleep,
};
use reqwest::Client;
use crate::{post::Post, actor::{Actor, ActorKind, RemoteActor}, api::FediApi, error::Error, db::Database};

async fn update_trends(db: &Database, tx: &Sender<(Arc<Actor>, Arc<RemoteActor>, Arc<Post>)>, client: &Client) -> Result<(), Error> {
    let actors = db.get_all_actors().await?;
    for actor in actors {
        let remote_actors = match db.get_following_remote_actors(&actor).await {
            Ok(actors) => actors,
            Err(e) => {
                tracing::error!("trends: get following actors: {:?}", e);
                continue;
            },
        };

        let actor = Arc::new(actor);
        let remote_actors: Vec<Arc<RemoteActor>> = remote_actors.into_iter().map(Arc::new).collect();

        if let ActorKind::TrendsRelay(instance_host) = &actor.kind {
            let api = match FediApi::from_host(instance_host, db, client).await {
                Ok(api) => api,
                Err(e) => {
                    tracing::error!("get api of {}: {:?}", instance_host, e);
                    continue;
                },
            };
            match api.get_trending_posts(instance_host, client).await {
                Ok(posts) => {
                    for post in posts {
                        let post = Arc::new(post);
                        for remote_actor in &remote_actors {
                            if let Err(e) = tx.send((actor.clone(), remote_actor.clone(), post.clone())).await {
                                tracing::error!("send trends to {}: {:?}", remote_actor.id, e);
                            };
                        }
                    }
                },
                Err(e) => tracing::error!("fetch trends of {}: {:?}", instance_host, e),
            };
        }
    }
    Ok(())
}

pub fn spawn(db: Database, tx: Sender<(Arc<Actor>, Arc<RemoteActor>, Arc<Post>)>, client: Arc<Client>) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = update_trends(&db, &tx, &client).await {
                tracing::error!("trends: {:?}", e);
            };
            sleep(Duration::from_secs(60)).await;
        }
    });
}
