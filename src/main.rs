use axum::{
    extract::{FromRef, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get, Router,
};
use axum_extra::routing::SpaRouter;
use serde_json::json;
use sigh::{PrivateKey, PublicKey};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use std::{panic, process};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod error;
mod config;
mod actor;
mod api;
mod db;
mod digest;
mod fetch;
mod send;
mod relay;
mod post;
mod trends;
mod timeline;
mod completion;
mod descendants;
mod activitypub;
mod endpoint;


#[derive(Clone)]
struct State {
    database: db::Database,
    client: Arc<reqwest::Client>,
    hostname: Arc<String>,
    priv_key: PrivateKey,
    pub_key: PublicKey,
}


impl FromRef<State> for Arc<reqwest::Client> {
    fn from_ref(state: &State) -> Arc<reqwest::Client> {
        state.client.clone()
    }
}

async fn get_completion_actor(
    axum::extract::State(state): axum::extract::State<State>,
) -> Response {
    let target = actor::Actor {
        host: state.hostname.clone(),
        kind: actor::ActorKind::CompletionRelay,
    };
    target.as_activitypub(&state.pub_key)
        .into_response()
}

async fn get_trends_actor(
    axum::extract::State(state): axum::extract::State<State>,
    Path(instance): Path<String>
) -> Response {
    let target = actor::Actor {
        host: state.hostname.clone(),
        kind: actor::ActorKind::TrendsRelay(instance.to_lowercase()),
    };
    target.as_activitypub(&state.pub_key)
        .into_response()
}

async fn post_completion_relay(
    axum::extract::State(state): axum::extract::State<State>,
    endpoint: endpoint::Endpoint<'_>
) -> Response {
    let target = actor::Actor {
        host: state.hostname.clone(),
        kind: actor::ActorKind::CompletionRelay,
    };
    post_relay(state, endpoint, target).await
}

async fn post_trends_relay(
    axum::extract::State(state): axum::extract::State<State>,
    Path(instance): Path<String>,
    endpoint: endpoint::Endpoint<'_>
) -> Response {
    let target = actor::Actor {
        host: state.hostname.clone(),
        kind: actor::ActorKind::TrendsRelay(instance.to_lowercase()),
    };
    post_relay(state, endpoint, target).await
}

async fn post_relay(
    state: State,
    endpoint: endpoint::Endpoint<'_>,
    target: actor::Actor
) -> Response {
    let remote_actor = match endpoint.remote_actor(&state.client, &target.key_id(), &state.priv_key).await {
        Ok(remote_actor) => remote_actor,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Bad actor: {:?}", e)
            ).into_response();
        }
    };
    let action = match serde_json::from_value::<activitypub::Action<serde_json::Value>>(endpoint.payload.clone()) {
        Ok(action) => action,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Bad action: {:?}", e)
            ).into_response();
        }
    };
    let object_type = action.object
        .and_then(|object| object.get("type").cloned())
        .and_then(|object_type| object_type.as_str().map(std::string::ToString::to_string));

    if action.action_type == "Follow" {
        let priv_key = state.priv_key.clone();
        let client = state.client.clone();
        tokio::spawn(async move {
            let accept_id = format!(
                "https://{}/activity/accept/{}/{}",
                state.hostname,
                urlencoding::encode(&target.uri()),
                urlencoding::encode(&remote_actor.inbox),
            );
            let accept = activitypub::Action {
                jsonld_context: serde_json::Value::String("https://www.w3.org/ns/activitystreams".to_string()),
                action_type: "Accept".to_string(),
                actor: target.uri(),
                to: Some(json!(remote_actor.id.clone())),
                id: accept_id,
                object: Some(endpoint.payload),
            };
            let result = send::send(
                client.as_ref(), &remote_actor.inbox,
                &target.key_id(),
                &priv_key,
                &accept,
            ).await;
            match result {
                Ok(()) => {
                    match state.database.add_follow(
                        &remote_actor.id,
                        &remote_actor.inbox,
                        &target.uri(),
                    ).await {
                        Ok(()) => {}
                        Err(e) => {
                            // duplicate key constraint
                            tracing::error!("add_follow: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("post accept: {}", e);
                }
            }
        });

        (StatusCode::ACCEPTED,
         [("content-type", "application/activity+json")],
         "{}"
        ).into_response()
    } else if action.action_type == "Undo" && object_type == Some("Follow".to_string()) {
        match state.database.del_follow(
            &remote_actor.id,
            &target.uri(),
        ).await {
            Ok(()) => {
                (StatusCode::ACCEPTED,
                 [("content-type", "application/activity+json")],
                 "{}"
                ).into_response()
            }
            Err(e) => {
                tracing::error!("del_follow: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR,
                 format!("{}", e)
                 ).into_response()
            }
        }
    } else {
        (StatusCode::BAD_REQUEST, "Not a recognized request").into_response()
    }
}

#[tokio::main]
async fn main() {
    exit_on_panic();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "courier=trace,tower_http=trace,axum=trace".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::load(
        &std::env::args().nth(1)
            .expect("Call with config.yaml")
    );
    let priv_key = config.priv_key();
    let pub_key = config.pub_key();

    let database = db::Database::connect(&config.db).await;
    let client = Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .pool_max_idle_per_host(1)
            .pool_idle_timeout(Some(Duration::from_secs(5)))
            .build()
            .unwrap()
    );
    let hostname = Arc::new(config.hostname.clone());
    let completion_actor = actor::Actor {
        host: hostname.clone(),
        kind: actor::ActorKind::CompletionRelay,
    };
    let tx = relay::spawn(client.clone(), hostname.clone(), priv_key.clone());
    trends::spawn(database.clone(), tx.clone(), client.clone());
    timeline::spawn(completion_actor.clone(), database.clone(), client.clone());
    descendants::spawn(database.clone(), client.clone());
    completion::spawn(completion_actor.clone(), database.clone(), tx.clone());

    let app = Router::new()
        .route("/completion", get(get_completion_actor).post(post_completion_relay))
        .route("/trends/:instance", get(get_trends_actor).post(post_trends_relay))
        .with_state(State {
            database,
            client,
            hostname,
            priv_key,
            pub_key,
        })
        .merge(SpaRouter::new("/", "static"));

    let addr = SocketAddr::from(([127, 0, 0, 1], config.listen_port));
    let server = axum::Server::bind(&addr)
        .serve(app.into_make_service());

    tracing::info!("serving on {}", addr);
    systemd::daemon::notify(false, [(systemd::daemon::STATE_READY, "1")].iter())
        .unwrap();
    server.await
        .unwrap();
}

fn exit_on_panic() {
    let orig_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // invoke the default handler and exit the process
        orig_hook(panic_info);
        process::exit(1);
    }));
}
