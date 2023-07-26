use std::sync::Arc;
use futures::future::join_all;
use tokio_postgres::{Client, Error, NoTls, Statement};
use crate::{post::Post, actor::{Actor, RemoteActor}};

const CREATE_SCHEMA_COMMANDS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS
        instances (
            host     TEXT PRIMARY KEY,
            api_type TEXT NOT NULL
        )",

    "CREATE TABLE IF NOT EXISTS 
        remote_actors (
            id    TEXT PRIMARY KEY,
            inbox TEXT NOT NULL
        )",

    "CREATE TABLE IF NOT EXISTS
        follows (
            remote_actor TEXT REFERENCES remote_actors (id) ON DELETE CASCADE,
            actor        TEXT NOT NULL,
            UNIQUE (remote_actor, actor)
        )",

    "CREATE TABLE IF NOT EXISTS
        posts (
            uri         TEXT PRIMARY KEY,
            fetch_time  BIGINT NOT NULL
        )",

    "CREATE TABLE IF NOT EXISTS
        descendants (
            uri        TEXT PRIMARY KEY,
            fetch_time BIGINT NOT NULL,
            ancester   TEXT REFERENCES posts (uri) ON DELETE CASCADE,
            sequence   BIGSERIAL
        )",

    "CREATE TABLE IF NOT EXISTS
        monitor (
            remote_actor TEXT REFERENCES remote_actors (id) ON DELETE CASCADE,
            uri          TEXT REFERENCES posts (uri) ON DELETE CASCADE,
            update_sequence BIGINT DEFAULT 0,
            UNIQUE (remote_actor, uri)
        )",

    "CREATE TABLE IF NOT EXISTS
        timeline (
            remote_actor TEXT REFERENCES remote_actors (id) ON DELETE CASCADE,
            latest_id    TEXT,
            PRIMARY KEY (remote_actor)
        )"
];

#[derive(Clone)]
pub struct Database {
    inner: Arc<DatabaseInner>,
}

struct DatabaseInner {
    client: Client,

    get_instance: Statement,
    add_instance: Statement,

    add_remote_actor: Statement,

    add_follow: Statement,
    del_follow: Statement,
    get_all_actors: Statement,
    get_following_remote_actors: Statement,

    get_all_posts: Statement,
    add_post: Statement,
    add_descendant: Statement,
    get_descendants_after: Statement,

    add_monitoring_post: Statement,
    get_monitoring_posts: Statement,
    update_monitoring_post: Statement,

    update_timeline: Statement,
    get_latest_id: Statement,
}

impl Database {
    pub async fn connect(conn_str: &str) -> Self {
        let (client, connection) = tokio_postgres::connect(conn_str, NoTls)
            .await
            .unwrap();

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("postgresql: {}", e);
            }
        });

        for command in CREATE_SCHEMA_COMMANDS {
            client.execute(*command, &[])
                .await
                .unwrap();
        }

        let get_instance = client.prepare("SELECT host, api_type FROM instances WHERE host=$1")
            .await
            .unwrap();
        let add_instance = client.prepare("INSERT INTO instances (host, api_type) VALUES($1, $2) ON CONFLICT DO NOTHING")
            .await
            .unwrap();

        let add_remote_actor = client.prepare("INSERT INTO remote_actors (id, inbox) VALUES($1, $2)
                                               ON CONFLICT (id)
                                               DO UPDATE SET inbox = EXCLUDED.inbox")
            .await
            .unwrap();
        let add_follow = client.prepare("INSERT INTO follows (remote_actor, actor) VALUES($1, $2) ON CONFLICT DO NOTHING")
            .await
            .unwrap();
        let del_follow = client.prepare("DELETE FROM follows WHERE remote_actor=$1 AND actor=$2")
            .await
            .unwrap();
        let get_all_actors = client.prepare("SELECT DISTINCT actor FROM follows")
            .await
            .unwrap();
        let get_following_remote_actors = client.prepare("SELECT DISTINCT id, inbox
                                                          FROM follows JOIN remote_actors
                                                          ON follows.remote_actor=remote_actors.id
                                                          WHERE actor=$1")
            .await
            .unwrap();

        let get_all_posts = client.prepare("SELECT DISTINCT uri, fetch_time FROM posts")
            .await
            .unwrap();
        let add_post = client.prepare("INSERT INTO posts (uri, fetch_time) VALUES($1, $2) ON CONFLICT DO NOTHING")
            .await
            .unwrap();
        let add_descendant = client.prepare("INSERT INTO descendants (uri, fetch_time, ancester) VALUES($1, $2, $3) ON CONFLICT DO NOTHING")
            .await
            .unwrap();
        let get_descendants_after = client.prepare("SELECT uri, fetch_time, sequence FROM descendants WHERE ancester=$1 AND sequence > $2")
            .await
            .unwrap();

        let add_monitoring_post = client.prepare("INSERT INTO monitor (remote_actor, uri) VALUES($1, $2) ON CONFLICT DO NOTHING")
            .await
            .unwrap();
        let get_monitoring_posts = client.prepare("SELECT posts.uri, posts.fetch_time, monitor.update_sequence
                                                   FROM monitor JOIN posts
                                                   ON monitor.uri = posts.uri
                                                   WHERE remote_actor=$1")
            .await
            .unwrap();
        let update_monitoring_post = client.prepare("UPDATE monitor
                                                     SET update_sequence=$3
                                                     WHERE remote_actor=$1 AND uri=$2")
            .await
            .unwrap();
        
        let update_timeline = client.prepare("INSERT INTO timeline (remote_actor, latest_id) VALUES($1, $2)
                                              ON CONFLICT (remote_actor)
                                              DO UPDATE SET latest_id = EXCLUDED.latest_id")
            .await
            .unwrap();
        let get_latest_id = client.prepare("SELECT latest_id FROM timeline WHERE remote_actor=$1")
            .await
            .unwrap();

        Database {
            inner: Arc::new(DatabaseInner {
                client,

                get_instance,
                add_instance,
                add_remote_actor,
                add_follow,
                del_follow,
                get_all_actors,
                get_following_remote_actors,
                get_all_posts,
                add_post,
                get_descendants_after,
                add_descendant,
                add_monitoring_post,
                get_monitoring_posts,
                update_monitoring_post,
                update_timeline,
                get_latest_id,
            }),
        }
    }

    pub async fn get_instance_type(&self, host: &str) -> Result<String, Error> {
        let row = self.inner.client.query_one(&self.inner.get_instance, &[&host])
            .await?;
        Ok(row.get(1))
    }

    pub async fn add_instance(&self, host: &str, api_type: &str) -> Result<(), Error> {
        self.inner.client.execute(&self.inner.add_instance, &[&host, &api_type])
            .await?;
        Ok(())
    }

    pub async fn add_follow(&self, id: &str, inbox: &str, actor: &str) -> Result<(), Error> {
        self.inner.client.execute(&self.inner.add_remote_actor, &[&id, &inbox])
            .await?;
        self.inner.client.execute(&self.inner.add_follow, &[&id, &actor])
            .await?;
        Ok(())
    }

    pub async fn del_follow(&self, id: &str, actor: &str) -> Result<(), Error> {
        self.inner.client.execute(&self.inner.del_follow, &[&id, &actor])
            .await?;
        Ok(())
    }

    pub async fn get_all_actors(&self) -> Result<impl Iterator<Item = Actor>, Error> {
        let rows = self.inner.client.query(&self.inner.get_all_actors, &[])
            .await?;
        Ok(rows.into_iter()
           .map(|row| Actor::from_uri(row.get(0)).unwrap()))
    }

    pub async fn get_following_remote_actors(&self, actor: &Actor) -> Result<impl Iterator<Item = RemoteActor>, Error> {
        let rows = self.inner.client.query(&self.inner.get_following_remote_actors, &[&actor.uri()])
            .await?;
        Ok(rows.into_iter()
           .map(|row| RemoteActor {
               id: row.get(0),
               inbox: row.get(1)
           })
        )
    }

    pub async fn get_all_posts(&self) -> Result<impl Iterator<Item = Post>, Error> {
        let rows = self.inner.client.query(&self.inner.get_all_posts, &[])
            .await?;
        Ok(rows.into_iter()
           .map(|row| Post {
               uri: row.get(0),
               fetch_time: row.get(1),
               timeline_id: None,
               created_at: None,
               in_reply_to_id: None,
               reblog: None,
           }))
    }

    pub async fn insert_descendants(&self, post: &Post, descendants: impl Iterator<Item = Post>) -> Result<(), Error> {
        let mut tasks = vec![];
        for descendant in descendants {
            tasks.push(async move {
                    self.inner.client
                        .execute(&self.inner.add_descendant,
                                 &[&descendant.uri, &descendant.fetch_time, &post.uri])
                        .await
                }
            );
        }
        join_all(tasks).await.into_iter().collect::<Result<Vec<u64>, Error>>()?;
        Ok(())
    }

    pub async fn get_descendants_after(&self, post: &Post, sequence: i64) -> Result<impl Iterator<Item = (Post, i64)>, Error> {
        let rows = self.inner.client.query(&self.inner.get_descendants_after, &[&post.uri, &sequence])
            .await?;
        Ok(rows.into_iter()
           .map(|row| (Post {
               uri: row.get(0),
               fetch_time: row.get(1),
               timeline_id: None,
               created_at: None,
               in_reply_to_id: None,
               reblog: None,
           }, row.get(2))))
    }

    pub async fn get_monitoring_posts_of(&self, remote_actor: &RemoteActor) -> Result<impl Iterator<Item = (Post, i64)>, Error> {
        let rows = self.inner.client.query(&self.inner.get_monitoring_posts, &[&remote_actor.id])
            .await?;
        Ok(rows.into_iter()
           .map(|row| (Post {
               uri: row.get(0),
               fetch_time: row.get(1),
               timeline_id: None,
               created_at: None,
               in_reply_to_id: None,
               reblog: None,
           }, row.get(2))))
    }
    
    pub async fn update_monitoring_post(&self, remote_actor: &RemoteActor, post: &Post, update_sequence: i64) -> Result<(), Error> {
        self.inner.client.execute(&self.inner.update_monitoring_post, &[&remote_actor.id, &post.uri, &update_sequence])
            .await?;
        Ok(())
    }

    pub async fn monitor_posts(&self, remote_actor: &RemoteActor, posts: impl Iterator<Item = Post>) -> Result<(), Error> {
        let mut tasks = vec![];
        for post in posts {
            tasks.push(async move {
                self.inner.client.execute(&self.inner.add_post, &[&post.uri, &post.fetch_time]).await?;
                self.inner.client.execute(&self.inner.add_monitoring_post, &[&remote_actor.id, &post.uri]).await
            });
        }
        join_all(tasks).await.into_iter().collect::<Result<Vec<u64>, Error>>()?;
        Ok(())
    }

    pub async fn get_latest_id_of(&self, remote_actor: &RemoteActor) -> Option<String> {
        match self.inner.client.query_one(&self.inner.get_latest_id, &[&remote_actor.id]).await {
            Ok(row) => Some(row.get(0)),
            Err(_) => None
        }
    }

    pub async fn update_timeline(&self, remote_actor: &RemoteActor, latest_id: &Option<String>) -> Result<(), Error> {
        self.inner.client.execute(&self.inner.update_timeline, &[&remote_actor.id, &latest_id]).await?;
        Ok(())
    }
}
