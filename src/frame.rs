use std::io::Cursor;

use atoi::atoi;
use bytes::{Buf, BufMut, BytesMut};

pub enum Frame {
    Simple(String),
    Error(String),
    Integer(i64),
    Bulk(String),
    Array(Vec<Frame>),
    Null,
}

#[derive(Debug)]
pub enum FrameError {
    Incomplete,
    Invalid,
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

    pub fn check(src: &mut Cursor<&[u8]>) -> Result<(), FrameError> {
        match get_u8(src)? {
            b'+' | b'-' => {
                get_line(src)?;
                Ok(())
            }
            b':' => {
                get_decimal(src)?;
                Ok(())
            }
            b'$' => {
                let n = get_decimal(src)?;
                if n == -1 {
                    return Ok(());
                }

                if src.remaining() < n as usize + 2 {
                    return Err(FrameError::Incomplete);
                }

                src.advance(n as usize + 2);
                Ok(())
            }
            b'*' => {
                let count = get_decimal(src)?;
                for _ in 0..count {
                    Frame::check(src)?;
                }
                Ok(())
            }
            _ => Err(FrameError::Invalid),
        }
    }

    #[allow(dead_code)]
    fn parse(src: &mut Cursor<&[u8]>) -> Result<Frame, FrameError> {
        match get_u8(src)? {
            b'+' => {
                let line = get_line(src)?;
                let s = std::str::from_utf8(line).map_err(|_| FrameError::Invalid)?;
                Ok(Frame::Simple(s.to_string()))
            }
            b'-' => {
                let line = get_line(src)?;
                let s = std::str::from_utf8(line).map_err(|_| FrameError::Invalid)?;
                Ok(Frame::Error(s.to_string()))
            }
            b':' => {
                let num = get_decimal(src)?;
                Ok(Frame::Integer(num))
            }
            b'$' => {
                let length = get_decimal(src)?;
                if length == -1 {
                    return Ok(Frame::Null);
                }

                let start = src.position() as usize;
                let data = &src.get_ref()[start..start + length as usize];
                src.advance(length as usize + 2);
                let s = std::str::from_utf8(data).map_err(|_| FrameError::Invalid)?;
                Ok(Frame::Bulk(s.to_string()))
            }
            b'*' => {
                let count = get_decimal(src)?;
                let mut result = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let frame = Frame::parse(src)?;
                    result.push(frame);
                }
                Ok(Frame::Array(result))
            }
            _ => Err(FrameError::Invalid),
        }
    }
}

#[allow(dead_code)]
fn peek_u8(src: &mut Cursor<&[u8]>) -> Result<u8, FrameError> {
    if !src.has_remaining() {
        return Err(FrameError::Incomplete);
    }

    Ok(src.get_ref()[src.position() as usize])
}

fn get_u8(src: &mut Cursor<&[u8]>) -> Result<u8, FrameError> {
    if !src.has_remaining() {
        return Err(FrameError::Incomplete);
    }

    Ok(src.get_u8())
}

fn get_decimal(src: &mut Cursor<&[u8]>) -> Result<i64, FrameError> {
    let line = get_line(src)?;
    atoi::<i64>(line).ok_or(FrameError::Invalid)
}

fn get_line<'a>(src: &mut Cursor<&'a [u8]>) -> Result<&'a [u8], FrameError> {
    let start = src.position() as usize;

    let data = src.get_ref();
    for i in start..data.len().saturating_sub(1) {
        if data[i] == b'\r' && data[i + 1] == b'\n' {
            let line = &data[start..i];
            src.set_position((i + 2) as u64);
            return Ok(line);
        }
    }

    Err(FrameError::Incomplete)
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

    // --- check tests ---

    fn check_ok(input: &[u8]) {
        let mut cursor = Cursor::new(input);
        assert!(Frame::check(&mut cursor).is_ok());
    }

    fn check_incomplete(input: &[u8]) {
        let mut cursor = Cursor::new(input);
        assert!(matches!(
            Frame::check(&mut cursor),
            Err(FrameError::Incomplete)
        ));
    }

    #[test]
    fn check_simple() {
        check_ok(b"+OK\r\n");
    }

    #[test]
    fn check_simple_incomplete() {
        check_incomplete(b"+OK");
        check_incomplete(b"+OK\r");
        check_incomplete(b"+");
    }

    #[test]
    fn check_error() {
        check_ok(b"-ERR unknown\r\n");
    }

    #[test]
    fn check_integer() {
        check_ok(b":1000\r\n");
        check_ok(b":-42\r\n");
    }

    #[test]
    fn check_bulk() {
        check_ok(b"$5\r\nhello\r\n");
        check_ok(b"$0\r\n\r\n");
    }

    #[test]
    fn check_bulk_incomplete() {
        check_incomplete(b"$5\r\nhel");
        check_incomplete(b"$5\r\nhello");
        check_incomplete(b"$5\r\nhello\r");
    }

    #[test]
    fn check_null() {
        check_ok(b"$-1\r\n");
    }

    #[test]
    fn check_array() {
        check_ok(b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n");
        check_ok(b"*0\r\n");
    }

    #[test]
    fn check_array_incomplete() {
        check_incomplete(b"*2\r\n$3\r\nGET\r\n");
    }

    #[test]
    fn check_invalid() {
        let mut cursor = Cursor::new(&b"!garbage\r\n"[..]);
        assert!(matches!(
            Frame::check(&mut cursor),
            Err(FrameError::Invalid)
        ));
    }

    // --- parse tests ---

    fn parse_frame(input: &[u8]) -> Frame {
        let mut cursor = Cursor::new(input);
        Frame::parse(&mut cursor).unwrap()
    }

    #[test]
    fn parse_simple() {
        match parse_frame(b"+OK\r\n") {
            Frame::Simple(s) => assert_eq!(s, "OK"),
            _ => panic!("expected Simple"),
        }
    }

    #[test]
    fn parse_error() {
        match parse_frame(b"-ERR unknown command\r\n") {
            Frame::Error(s) => assert_eq!(s, "ERR unknown command"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn parse_integer() {
        match parse_frame(b":1000\r\n") {
            Frame::Integer(n) => assert_eq!(n, 1000),
            _ => panic!("expected Integer"),
        }
    }

    #[test]
    fn parse_negative_integer() {
        match parse_frame(b":-42\r\n") {
            Frame::Integer(n) => assert_eq!(n, -42),
            _ => panic!("expected Integer"),
        }
    }

    #[test]
    fn parse_bulk() {
        match parse_frame(b"$5\r\nhello\r\n") {
            Frame::Bulk(s) => assert_eq!(s, "hello"),
            _ => panic!("expected Bulk"),
        }
    }

    #[test]
    fn parse_empty_bulk() {
        match parse_frame(b"$0\r\n\r\n") {
            Frame::Bulk(s) => assert_eq!(s, ""),
            _ => panic!("expected Bulk"),
        }
    }

    #[test]
    fn parse_null() {
        assert!(matches!(parse_frame(b"$-1\r\n"), Frame::Null));
    }

    #[test]
    fn parse_array() {
        match parse_frame(b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n") {
            Frame::Array(frames) => {
                assert_eq!(frames.len(), 2);
                assert!(matches!(&frames[0], Frame::Bulk(s) if s == "GET"));
                assert!(matches!(&frames[1], Frame::Bulk(s) if s == "key"));
            }
            _ => panic!("expected Array"),
        }
    }

    #[test]
    fn parse_empty_array() {
        match parse_frame(b"*0\r\n") {
            Frame::Array(frames) => assert!(frames.is_empty()),
            _ => panic!("expected Array"),
        }
    }

    // --- roundtrip tests ---

    #[test]
    fn roundtrip_simple() {
        let frame = Frame::Simple("OK".to_string());
        let mut buf = BytesMut::new();
        frame.serialize(&mut buf);
        let parsed = parse_frame(&buf);
        assert!(matches!(parsed, Frame::Simple(s) if s == "OK"));
    }

    #[test]
    fn roundtrip_array() {
        let frame = Frame::Array(vec![
            Frame::Bulk("SET".to_string()),
            Frame::Bulk("key".to_string()),
            Frame::Bulk("value".to_string()),
        ]);
        let mut buf = BytesMut::new();
        frame.serialize(&mut buf);
        match parse_frame(&buf) {
            Frame::Array(frames) => {
                assert_eq!(frames.len(), 3);
                assert!(matches!(&frames[0], Frame::Bulk(s) if s == "SET"));
                assert!(matches!(&frames[1], Frame::Bulk(s) if s == "key"));
                assert!(matches!(&frames[2], Frame::Bulk(s) if s == "value"));
            }
            _ => panic!("expected Array"),
        }
    }
}
