use std::io::Cursor;

use bytes::{Buf, BytesMut};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::frame::{Frame, FrameError};

pub struct Connection {
    stream: TcpStream,
    buf: BytesMut,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buf: BytesMut::with_capacity(4096),
        }
    }

    pub async fn read_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        loop {
            if let Some(frame) = self.parse_frame()? {
                return Ok(Some(frame));
            }

            if self.stream.read_buf(&mut self.buf).await? == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                } else {
                    return Err(FrameError::Invalid);
                }
            }
        }
    }

    fn parse_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let mut cursor = Cursor::new(&self.buf[..]);

        match Frame::check(&mut cursor) {
            Ok(_) => {
                let len = cursor.position() as usize;

                cursor.set_position(0);
                let frame = Frame::parse(&mut cursor)?;
                self.buf.advance(len);
                Ok(Some(frame))
            }
            Err(FrameError::Incomplete) => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub async fn write_frame(&mut self, frame: &Frame) -> Result<(), FrameError> {
        let mut write_buf = BytesMut::new();
        frame.serialize(&mut write_buf);
        self.stream.write_all_buf(&mut write_buf).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Helper: start a server that accepts one connection and returns the Connection
    async fn setup() -> (Connection, Connection) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();

        (
            Connection::new(server_stream),
            Connection::new(client_stream),
        )
    }

    #[tokio::test]
    async fn read_write_simple() {
        let (mut server, mut client) = setup().await;

        client
            .write_frame(&Frame::Simple("OK".to_string()))
            .await
            .unwrap();

        let frame = server.read_frame().await.unwrap().unwrap();
        assert!(matches!(frame, Frame::Simple(s) if s == "OK"));
    }

    #[tokio::test]
    async fn read_write_bulk() {
        let (mut server, mut client) = setup().await;

        client
            .write_frame(&Frame::Bulk("hello".to_string()))
            .await
            .unwrap();

        let frame = server.read_frame().await.unwrap().unwrap();
        assert!(matches!(frame, Frame::Bulk(s) if s == "hello"));
    }

    #[tokio::test]
    async fn read_write_array() {
        let (mut server, mut client) = setup().await;

        let cmd = Frame::Array(vec![
            Frame::Bulk("SET".to_string()),
            Frame::Bulk("key".to_string()),
            Frame::Bulk("value".to_string()),
        ]);
        client.write_frame(&cmd).await.unwrap();

        let frame = server.read_frame().await.unwrap().unwrap();
        match frame {
            Frame::Array(frames) => {
                assert_eq!(frames.len(), 3);
                assert!(matches!(&frames[0], Frame::Bulk(s) if s == "SET"));
                assert!(matches!(&frames[1], Frame::Bulk(s) if s == "key"));
                assert!(matches!(&frames[2], Frame::Bulk(s) if s == "value"));
            }
            _ => panic!("expected Array"),
        }
    }

    #[tokio::test]
    async fn read_write_null() {
        let (mut server, mut client) = setup().await;

        client.write_frame(&Frame::Null).await.unwrap();

        let frame = server.read_frame().await.unwrap().unwrap();
        assert!(matches!(frame, Frame::Null));
    }

    #[tokio::test]
    async fn read_multiple_frames() {
        let (mut server, mut client) = setup().await;

        client
            .write_frame(&Frame::Simple("PONG".to_string()))
            .await
            .unwrap();
        client.write_frame(&Frame::Integer(42)).await.unwrap();

        let frame1 = server.read_frame().await.unwrap().unwrap();
        assert!(matches!(frame1, Frame::Simple(s) if s == "PONG"));

        let frame2 = server.read_frame().await.unwrap().unwrap();
        assert!(matches!(frame2, Frame::Integer(42)));
    }

    #[tokio::test]
    async fn read_eof_returns_none() {
        let (mut server, client) = setup().await;

        drop(client); // close the connection

        let result = server.read_frame().await.unwrap();
        assert!(result.is_none());
    }
}
