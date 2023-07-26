use std::{sync::Arc, time::Duration};
use tokio::{
    sync::mpsc::Sender,
    time::sleep,
};
use crate::{post::Post, actor::{Actor, RemoteActor}, error::Error, db::Database};

async fn relay_new_posts(actor: &Arc<Actor>,
                         remote_actor: RemoteActor,
                         db: &Database,
                         tx: &Sender<(Arc<Actor>, Arc<RemoteActor>, Arc<Post>)>,) -> Result<(), Error> {
    let monitoring_posts = db.get_monitoring_posts_of(&remote_actor).await?;
    let remote_actor = Arc::new(remote_actor);
    for (post, update_sequence) in monitoring_posts {
        let new_posts = db.get_descendants_after(&post, update_sequence).await?;
        let mut new_update_sequence = update_sequence;
        for (new_post, sequence) in new_posts {
            let new_post = Arc::new(new_post);
            if let Err(e) = tx.send((actor.clone(), remote_actor.clone(), new_post.clone())).await {
                tracing::error!("send new posts to {}: {:?}", remote_actor.inbox, e);
                break;
            }
            if sequence > new_update_sequence {
                new_update_sequence = sequence;
            }
        }
        if let Err(e) = db.update_monitoring_post(&remote_actor, &post, new_update_sequence).await {
            tracing::error!("set update sequence of post: {:?}", e);
        };
    }
    Ok(())
}

async fn run(actor: &Arc<Actor>, db: &Database, tx: &Sender<(Arc<Actor>, Arc<RemoteActor>, Arc<Post>)>) -> Result<(), Error> {
    let remote_actors = db.get_following_remote_actors(actor).await?;
    for remote_actor in remote_actors {
        if let Err(e) = relay_new_posts(actor, remote_actor, db, tx).await {
            tracing::error!("relay new posts: {:?}", e);
        }
    }
    Ok(())
}

pub fn spawn(actor: Actor, db: Database, tx: Sender<(Arc<Actor>, Arc<RemoteActor>, Arc<Post>)>) {
    tokio::spawn(async move {
        let actor = Arc::new(actor);
        loop {
            if let Err(e) = run(&actor, &db, &tx).await {
                tracing::error!("completion: {:?}", e);
            }
            sleep(Duration::from_secs(60)).await;
        }
    });
}
