use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::main]
async fn main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(process(stream));
    }
}

async fn process(mut stream: TcpStream) -> io::Result<()> {
    let mut buf = vec![0; 1024];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }

        stream.write_all(&buf[..n]).await?
    }
}
