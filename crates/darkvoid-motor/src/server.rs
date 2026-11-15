use crate::{
    VERSION,
    motor::{self, Command, Updates},
    protocol,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::atomic::Ordering,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot, watch},
    time::timeout,
};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        Message,
        handshake::server::{ErrorResponse, Request, Response},
        protocol::WebSocketConfig,
    },
};

pub async fn run(token: String, data_dir: PathBuf, stop: watch::Sender<bool>, mut stopped: watch::Receiver<bool>) -> io::Result<()> {
    // Nenhuma persistência automática do engine fora da pasta autorizada.
    std::fs::create_dir_all(data_dir.join("cache"))?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let (commands, rx) = mpsc::channel(16);
    let updates = Updates::new();
    let actor_updates = updates.clone();
    let actor_stop = stopped.clone();
    let worker = std::thread::spawn(move || motor::run(rx, actor_updates, actor_stop));
    let owner = watch::channel(0u64).0;
    println!("{}", json!({"event":"ready", "port":listener.local_addr()?.port(), "version":VERSION}));
    io::stdout().flush()?;
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let client = Client { commands: commands.clone(), updates: updates.clone(), stop: stop.clone(), owner: owner.clone() };
                let token = token.clone();
                // Handshakes incompletos não tomam posse da conexão nem crescem sem limite.
                if connections.len() < 32 { connections.spawn(connection(stream, token, client)); }
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    let _ = stop.send(true);
    connections.abort_all();
    drop(commands);
    // O callback de áudio é interrompido pelo worker antes de sairmos.
    let _ = tokio::task::spawn_blocking(move || worker.join()).await;
    Ok(())
}

struct Client {
    commands: mpsc::Sender<Command>,
    updates: Updates,
    stop: watch::Sender<bool>,
    owner: watch::Sender<u64>,
}

// Tipo de retorno exigido pelo callback de handshake do tungstenite.
#[allow(clippy::result_large_err)]
fn authenticate(request: &Request, response: Response, token: &str) -> std::result::Result<Response, ErrorResponse> {
    let expected = format!("token={token}");
    if request.uri().path() != "/" || request.uri().query() != Some(expected.as_str()) {
        let mut rejected = ErrorResponse::new(Some("Acesso negado.".into()));
        *rejected.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
        return Err(rejected);
    }
    Ok(response)
}

#[allow(clippy::result_large_err)]
async fn connection(stream: TcpStream, token: String, client: Client) {
    let config = WebSocketConfig::default().max_message_size(Some(64 * 1024)).max_frame_size(Some(64 * 1024));
    let handshake = accept_hdr_async_with_config(stream, |r: &Request, s| authenticate(r, s, &token), Some(config));
    let Ok(Ok(mut socket)) = timeout(Duration::from_secs(3), handshake).await else {
        return;
    };
    let id = client.updates.active_client.fetch_add(1, Ordering::SeqCst) + 1;
    client.owner.send_replace(id);
    let mut owner = client.owner.subscribe();
    let mut stopped = client.stop.subscribe();
    let mut frames = client.updates.frames.subscribe();
    let mut state = client.updates.state.subscribe();
    let mut stats = client.updates.stats.subscribe();
    loop {
        let mut shutdown = false;
        let outgoing = tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = owner.changed() => break,
            incoming = socket.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let reply = tokio::select! {
                            reply = handle(&text, &client, id) => reply,
                            _ = stopped.changed() => break,
                            _ = owner.changed() => break,
                        };
                        let Some((message, should_stop)) = reply else { break; };
                        shutdown = should_stop;
                        message
                    }
                    Some(Ok(Message::Ping(bytes))) => Message::Pong(bytes),
                    Some(Ok(Message::Pong(_))) => continue,
                    Some(Ok(Message::Binary(_))) => Message::Text(protocol::response(Value::Null, Err(protocol::error("invalid_request", "Envie pedidos em JSON de texto."))).to_string().into()),
                    _ => break,
                }
            }
            changed = frames.changed() => {
                if changed.is_err() { break; }
                let Some(message) = frame_message(&mut frames, &client.updates) else { continue; };
                message
            }
            changed = state.changed() => {
                if changed.is_err() { break; }
                Message::Text(state.borrow_and_update().to_string().into())
            }
            changed = stats.changed() => {
                if changed.is_err() { break; }
                Message::Text(stats.borrow_and_update().to_string().into())
            }
        };
        // Um cliente lento não bloqueia o áudio; no máximo uma imagem em envio.
        let sent = tokio::select! {
            sent = timeout(Duration::from_secs(2), socket.send(outgoing)) => matches!(sent, Ok(Ok(()))),
            _ = stopped.changed() => false,
            _ = owner.changed() => false,
        };
        if shutdown {
            let _ = client.stop.send(true);
            break;
        }
        if !sent {
            break;
        }
    }
    let _ = timeout(Duration::from_millis(100), socket.close(None)).await;
    if *client.owner.borrow() == id {
        let _ = client.commands.send(Command::Disconnect(id)).await;
    }
}

fn frame_message(frames: &mut watch::Receiver<Option<std::sync::Arc<crate::preview::Frame>>>, updates: &Updates) -> Option<Message> {
    let frame = frames.borrow_and_update().clone()?;
    (frame.epoch == updates.epoch.load(Ordering::Acquire)).then(|| Message::Binary(frame.bytes.clone().into()))
}

async fn handle(text: &str, client: &Client, id: u64) -> Option<(Message, bool)> {
    let (reply, shutdown) = match protocol::parse(text) {
        Err(reply) => (reply, false),
        Ok(request) => {
            let shutdown = request.cmd == "shutdown";
            let (tx, rx) = oneshot::channel();
            client.commands.send(Command::Request(id, request, tx)).await.ok()?;
            (rx.await.ok()?, shutdown)
        }
    };
    Some((Message::Text(reply.to_string().into()), shutdown))
}
