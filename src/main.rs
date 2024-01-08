use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::{IntoResponse, Html},
    routing::get,
    Router,
};
use futures::{StreamExt, SinkExt};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

async fn fallback() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "Not found")
}

// our shared state
struct AppState {
    user_set: Mutex<HashSet<String>>,
    tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() {
    let user_set = Mutex::new(HashSet::new());
    let (tx, _rx) = broadcast::channel(100);

    let app_state = Arc::new(AppState { user_set, tx });

    let app = Router::new()
        .route("/", get(index))
        .route("/websocket", get(websocket_handler))
        .with_state(app_state)
        .fallback(fallback);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = stream.split();

    let mut username = String::new();

    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(name) = message {
            // if username that is sent by client is not taken, fill username string
            check_username(&state, &mut username, &name);

            if !username.is_empty() {
                break;
            } else {
                let _ =  sender.send(Message::Text(String::from("username already taken"))).await;

                return;
            }
        }
    }

    let mut rx =  state.tx.subscribe();

    let msg  = format!("{username} joined");
    let _  =  state.tx.send(msg);

    let mut send_task =  tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let tx = state.tx.clone();
    let name =  username.clone();

    let mut recv_task=  tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) =  receiver.next().await {
            let _  = tx.send(format!("{name}: {text}"));
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }

    let msg =  format!("{username} left");
    let _ =  state.tx.send(msg);

    state.user_set.lock().unwrap().remove(&username);
}

fn check_username(state: &AppState, string: &mut String, name: &str) {
    let mut user_set = state.user_set.lock().unwrap();

    if !user_set.contains(name) {
        user_set.insert(name.to_owned());

        string.push_str(name);
    }
}


async fn index() -> Html<&'static str>  {
    Html(std::include_str!("../chat.html"))
}