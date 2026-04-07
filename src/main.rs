use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use my_redis::{command::Command, connection::Connection, frame::FrameError, store::Store};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<(), FrameError> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    let store = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (stream, _) = listener.accept().await?;
        let store = store.clone();
        tokio::spawn(process(stream, store));
    }
}

async fn process(stream: TcpStream, store: Store) -> Result<(), FrameError> {
    let mut connection = Connection::new(stream);
    loop {
        match connection.read_frame().await {
            Ok(Some(frame)) => {
                let command = Command::from_frame(frame)?;
                let response = command.execute(&store);
                if let Err(e) = connection.write_frame(&response).await {
                    eprintln!("write error: {:?}", e);
                    return Err(FrameError::Invalid);
                }
            }
            Ok(None) => return Ok(()), // client disconnected
            Err(e) => {
                eprintln!("read error: {:?}", e);
                return Err(FrameError::Invalid);
            }
        }
    }
}
