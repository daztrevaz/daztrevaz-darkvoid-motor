use crate::{
    VERSION,
    motor::{self, Command, Updates},
    protocol,
};
use futures_util::{FutureExt, SinkExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
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
    let worker = std::thread::spawn(move || motor::run(rx, actor_updates, actor_stop, data_dir));
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

#[derive(Clone)]
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
    let mut jobs = client.updates.jobs.subscribe();
    let mut pending: FuturesUnordered<BoxFuture<'static, Option<(Message, bool)>>> = FuturesUnordered::new();
    loop {
        let mut shutdown = false;
        let outgoing = tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = owner.changed() => break,
            reply = pending.next(), if !pending.is_empty() => {
                let Some(Some((message, should_stop))) = reply else { break; };
                shutdown = should_stop;
                message
            }
            incoming = socket.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if pending.len() >= 16 {
                            let reply = match protocol::parse(&text) {
                                Ok(request) => protocol::response(request.id, Err(protocol::error("requests_busy", "Aguarde as respostas dos pedidos em andamento."))),
                                Err(reply) => reply,
                            };
                            Message::Text(reply.to_string().into())
                        } else {
                            let client = client.clone();
                            pending.push(async move { handle(&text, &client, id).await }.boxed());
                            continue;
                        }
                    }
                    Some(Ok(Message::Ping(bytes))) => Message::Pong(bytes),
                    Some(Ok(Message::Pong(_))) => continue,
                    Some(Ok(Message::Binary(_))) => Message::Text(protocol::response(Value::Null, Err(protocol::error("invalid_request", "Envie pedidos em JSON de texto."))).to_string().into()),
                    _ => break,
                }
            }
            _ = frames.changed() => {
                let Some(message) = frame_message(&mut frames, &client.updates) else { continue; };
                message
            }
            _ = state.changed() => {
                Message::Text(state.borrow_and_update().to_string().into())
            }
            _ = stats.changed() => {
                Message::Text(stats.borrow_and_update().to_string().into())
            }
            event = jobs.recv() => match event {
                Ok(event) => Message::Text(event.to_string().into()),
                Err(_) => break,
            },
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

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn pending_replies_stay_bounded_and_shutdown_does_not_wait_for_them() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (commands, mut held) = mpsc::channel(16);
        let stop = watch::channel(false).0;
        let client = Client { commands, updates: Updates::new(), stop: stop.clone(), owner: watch::channel(0).0 };
        let worker = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            connection(stream, "test".into(), client).await;
        });
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/?token=test")).await.unwrap();
        for id in 0..17 {
            socket.send(Message::Text(json!({"id":id,"cmd":"hello"}).to_string().into())).await.unwrap();
        }
        let reply = timeout(Duration::from_secs(2), socket.next()).await.unwrap().unwrap().unwrap();
        let Message::Text(text) = reply else { panic!("esperava resposta JSON") };
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["id"], 16);
        assert_eq!(value["error"]["code"], "requests_busy");
        // An actor consumes commands without necessarily replying yet. Leave room for Disconnect.
        let _unanswered = held.try_recv().unwrap();
        stop.send(true).unwrap();
        timeout(Duration::from_secs(2), worker).await.expect("encerra mesmo com pedidos sem resposta").unwrap();
    }
}
