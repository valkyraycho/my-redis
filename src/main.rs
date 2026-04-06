use my_redis::{connection::Connection, frame::FrameError};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<(), FrameError> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(process(stream));
    }
}

async fn process(stream: TcpStream) -> Result<(), FrameError> {
    let mut connection = Connection::new(stream);
    loop {
        match connection.read_frame().await {
            Ok(Some(frame)) => {
                // Echo the frame back for now
                if let Err(e) = connection.write_frame(&frame).await {
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
