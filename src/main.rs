use async_std::{net::{SocketAddr, TcpListener, TcpStream}};
use async_tungstenite::{accept_async, async_std::connect_async, tungstenite::{Error, Message, Result}};
use futures::{channel::mpsc::{UnboundedSender, unbounded}, pin_mut, prelude::*};
use log::*;
use std::{collections::HashMap, sync::{Arc, Mutex}, thread, time::Duration};

type Tx = UnboundedSender<Message>;
type SessionMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;

async fn accept_connection(sessions: SessionMap, socket_addr: SocketAddr, stream: TcpStream) {
    if let Err(e) = handle_connection(sessions,socket_addr, stream).await {
        match e {
            Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => (),
            err => error!("error processing connection: {}", err),
        }
    }
}

async fn handle_connection(sessions: SessionMap, socket_addr: SocketAddr ,stream: TcpStream) -> Result<()> {
    let ws_stream = accept_async(stream).await.expect("failed to accept");
    info!("New websocket connection: {}", socket_addr);

    // 保存新的会话
    let (tx, rx) = unbounded();
    sessions.lock().unwrap().insert(socket_addr.clone(),tx);

    let (outgoing, incoming) = ws_stream.split();

    // 处理收到的消息
    let process_incoming = incoming.try_for_each(|msg| {
        if msg.is_text() {
            info!("recv: {}",msg.to_text().unwrap());
        }
        future::ok(())
    });

    // 处理发送的消息
    let process_recv_msg = rx.map(Ok).forward(outgoing);
    pin_mut!(process_incoming,process_recv_msg);

    // select 处理读取和写入
    future::select(process_incoming,process_recv_msg).await;

    // 会话结束，移除会话
    sessions.lock().unwrap().remove(&socket_addr);
    Ok(())
}

async fn run_server(){
    env_logger::init();
    // session map
    let sessions_map = SessionMap::new(Mutex::new(HashMap::new()));

    let addr = "127.0.0.1:9002";
    let listener = TcpListener::bind(&addr).await.expect("can't listen");
    info!("listening on :{}",addr);

    async_std::task::spawn(write_some_msg(sessions_map.clone()));

    while let Ok((stream,_)) = listener.accept().await {
        let peer = stream.peer_addr()
            .expect("connected streams should have a peer address");

        info!("peer address: {}", peer);

        async_std::task::spawn(accept_connection(sessions_map.clone(),peer, stream));
    }
}

async fn write_some_msg (sessions: SessionMap) {
    // 每秒广播一条消息
    let mut count: u64 = 0;
    loop {
        let s = sessions.lock().unwrap();
        for session in s.values() {
            session.unbounded_send(Message::Text(format!("{}",s.len()))).unwrap();
        }
        thread::sleep(Duration::from_millis(5));
    }
}

// 启动客户端
async fn run_client () -> Result<()> {
    let (mut ws_stream, _) = connect_async("ws://127.0.0.1:9002").await?;
    info!("client: success connect server");
    let mut count: u64 = 0;
    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if msg.is_text() {
            count = count + 1;
            info!("client: recv: {} {}", msg.to_string(),count)
        }
    }
    Ok(())
}

fn main () {
    async_std::task::spawn(run_server());

    thread::sleep(Duration::from_secs(1));
    for i in 1..10000 {
        async_std::task::spawn(run_client());
    }
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
