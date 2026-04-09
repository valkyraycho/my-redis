use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use my_redis::{
    channel::Channels,
    command::Command,
    connection::Connection,
    frame::{Frame, FrameError},
    store::Store,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast::{self},
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{StreamExt, StreamMap};

#[tokio::main]
async fn main() -> Result<(), FrameError> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    let store: Store = Arc::new(Mutex::new(HashMap::new()));
    let purge_store = store.clone();
    let channels: Channels = Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let mut purge_store = purge_store.lock().unwrap();
            purge_store.retain(|_, entry| !entry.is_expired());
        }
    });

    loop {
        let (stream, _) = listener.accept().await?;
        let store = store.clone();
        tokio::spawn(process(stream, store, channels.clone()));
    }
}

async fn process(stream: TcpStream, store: Store, pubsub: Channels) -> Result<(), FrameError> {
    let mut connection = Connection::new(stream);
    loop {
        match connection.read_frame().await {
            Ok(Some(frame)) => {
                let command = Command::from_frame(frame)?;
                let response: Frame = match command {
                    Command::Subscribe { channels } => {
                        let mut stream_map = StreamMap::new();

                        let mut confirmations = Vec::new();
                        {
                            let mut pubsub = pubsub.lock().unwrap();
                            for (i, channel) in channels.iter().enumerate() {
                                let sender = pubsub
                                    .entry(channel.clone())
                                    .or_insert_with(|| broadcast::channel(64).0);

                                let receiver = sender.subscribe();
                                stream_map.insert(channel, BroadcastStream::new(receiver));

                                let confirmation = Frame::Array(vec![
                                    Frame::Bulk("subscribe".to_string()),
                                    Frame::Bulk(channel.clone()),
                                    Frame::Integer((i + 1) as i64),
                                ]);
                                confirmations.push(confirmation);
                            }
                        }

                        for confirmation in confirmations {
                            connection.write_frame(&confirmation).await?;
                        }

                        loop {
                            match stream_map.next().await {
                                Some((channel_name, Ok(message))) => {
                                    connection
                                        .write_frame(&Frame::Array(vec![
                                            Frame::Bulk("message".to_string()),
                                            Frame::Bulk(channel_name.clone()),
                                            Frame::Bulk(message),
                                        ]))
                                        .await?;
                                }
                                Some((_, Err(_))) => continue,
                                None => return Ok(()),
                            }
                        }
                    }
                    Command::Publish { channel, message } => {
                        let pubsub = pubsub.lock().unwrap();
                        match pubsub.get(&channel) {
                            None => Frame::Integer(0),
                            Some(sender) => {
                                Frame::Integer(sender.send(message).unwrap_or(0) as i64)
                            }
                        }
                    }
                    command => command.execute(&store),
                };
                if let Err(e) = connection.write_frame(&response).await {
                    eprintln!("write error: {:?}", e);
                    return Err(FrameError::Invalid);
                };
            }
            Ok(None) => return Ok(()), // client disconnected
            Err(e) => {
                eprintln!("read error: {:?}", e);
                return Err(FrameError::Invalid);
            }
        }
    }
}
