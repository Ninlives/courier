use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use reqwest::Client;
use crate::{api::FediApi, actor::{Actor, RemoteActor}, error::Error, db::Database};

async fn update_timeline(remote_actor: &RemoteActor, db: &Database, client: &Client) -> Result<(), Error> {
    let host = remote_actor.host()
               .ok_or_else(|| Error::Api(format!("Failed to get host of {}", remote_actor.id)))?;
    let api = FediApi::from_host(&host, db, client).await?;
    let latest_id = db.get_latest_id_of(remote_actor).await;
    let posts = api.get_global_timeline(&host, &latest_id, client).await?;
    if let Some(post) = posts.last() {
        let new_latest_id = post.timeline_id.clone();
        db.monitor_posts(remote_actor,
                         posts.into_iter()
                              .map(|post| post.origin())
                              .filter(|post| !post.is_reply()))
           .await?;
        db.update_timeline(remote_actor, &new_latest_id).await?;
    }
    Ok(())
}

async fn update(actor: &Actor, db: &Database, client: &Client) -> Result<(), Error> {
    let remote_actors = db.get_following_remote_actors(actor).await?;
    for remote_actor in remote_actors {
        if let Err(e) = update_timeline(&remote_actor, db, client).await {
            tracing::error!("timeline: update timline: {:?}", e);
        }
    }
    Ok(())
}

pub fn spawn(actor: Actor, db: Database, client: Arc<Client>) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = update(&actor, &db, &client).await {
                tracing::error!("timeline: update: {:?}", e);
            }
            sleep(Duration::from_secs(60)).await;
        }
    });
}
