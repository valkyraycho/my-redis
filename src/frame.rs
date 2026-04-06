use bytes::{BufMut, BytesMut};

pub enum Frame {
    Simple(String),
    Error(String),
    Integer(i64),
    Bulk(String),
    Array(Vec<Frame>),
    Null,
}

impl Frame {
    pub fn serialize(&self, buf: &mut BytesMut) {
        match self {
            Frame::Simple(s) => {
                buf.put_slice(b"+");
                buf.put_slice(s.as_bytes());
                buf.put_slice(b"\r\n");
            }
            Frame::Error(s) => {
                buf.put_slice(b"-");
                buf.put_slice(s.as_bytes());
                buf.put_slice(b"\r\n");
            }
            Frame::Integer(n) => {
                buf.put_slice(b":");
                let mut num_buf = itoa::Buffer::new();
                buf.put_slice(num_buf.format(*n).as_bytes());
                buf.put_slice(b"\r\n");
            }
            Frame::Bulk(s) => {
                buf.put_slice(b"$");
                let mut num_buf = itoa::Buffer::new();
                buf.put_slice(num_buf.format(s.len()).as_bytes());
                buf.put_slice(b"\r\n");
                buf.put_slice(s.as_bytes());
                buf.put_slice(b"\r\n");
            }
            Frame::Array(frames) => {
                buf.put_slice(b"*");
                let mut num_buf = itoa::Buffer::new();
                buf.put_slice(num_buf.format(frames.len()).as_bytes());
                buf.put_slice(b"\r\n");
                for frame in frames {
                    frame.serialize(buf);
                }
            }
            Frame::Null => {
                buf.put_slice(b"$-1\r\n");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_simple() {
        let mut buf = BytesMut::new();
        Frame::Simple("OK".to_string()).serialize(&mut buf);
        assert_eq!(buf, &b"+OK\r\n"[..]);
    }

    #[test]
    fn serialize_error() {
        let mut buf = BytesMut::new();
        Frame::Error("ERR unknown command".to_string()).serialize(&mut buf);
        assert_eq!(buf, &b"-ERR unknown command\r\n"[..]);
    }

    #[test]
    fn serialize_integer() {
        let mut buf = BytesMut::new();
        Frame::Integer(1000).serialize(&mut buf);
        assert_eq!(buf, &b":1000\r\n"[..]);
    }

    #[test]
    fn serialize_negative_integer() {
        let mut buf = BytesMut::new();
        Frame::Integer(-42).serialize(&mut buf);
        assert_eq!(buf, &b":-42\r\n"[..]);
    }

    #[test]
    fn serialize_bulk() {
        let mut buf = BytesMut::new();
        Frame::Bulk("hello".to_string()).serialize(&mut buf);
        assert_eq!(buf, &b"$5\r\nhello\r\n"[..]);
    }

    #[test]
    fn serialize_empty_bulk() {
        let mut buf = BytesMut::new();
        Frame::Bulk("".to_string()).serialize(&mut buf);
        assert_eq!(buf, &b"$0\r\n\r\n"[..]);
    }

    #[test]
    fn serialize_null() {
        let mut buf = BytesMut::new();
        Frame::Null.serialize(&mut buf);
        assert_eq!(buf, &b"$-1\r\n"[..]);
    }

    #[test]
    fn serialize_array() {
        let mut buf = BytesMut::new();
        Frame::Array(vec![
            Frame::Bulk("GET".to_string()),
            Frame::Bulk("key".to_string()),
        ])
        .serialize(&mut buf);
        assert_eq!(buf, &b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n"[..]);
    }

    #[test]
    fn serialize_nested_array() {
        let mut buf = BytesMut::new();
        Frame::Array(vec![
            Frame::Integer(1),
            Frame::Array(vec![Frame::Simple("OK".to_string()), Frame::Null]),
        ])
        .serialize(&mut buf);
        assert_eq!(buf, &b"*2\r\n:1\r\n*2\r\n+OK\r\n$-1\r\n"[..]);
    }

    #[test]
    fn serialize_empty_array() {
        let mut buf = BytesMut::new();
        Frame::Array(vec![]).serialize(&mut buf);
        assert_eq!(buf, &b"*0\r\n"[..]);
    }
}
