use crate::frame::{Frame, FrameError};
use crate::store::Store;

pub enum Command {
    Ping,
    Echo(String),
    Get(String),
    Set(String, String),
}

impl Command {
    pub fn from_frame(frame: Frame) -> Result<Command, FrameError> {
        let mut frames = match frame {
            Frame::Array(frames) => frames.into_iter(),
            _ => return Err(FrameError::Invalid),
        };

        let command = match frames.next() {
            Some(Frame::Bulk(s)) => s,
            _ => return Err(FrameError::Invalid),
        };

        match command.to_ascii_uppercase().as_str() {
            "PING" => Ok(Command::Ping),
            "ECHO" => {
                let arg = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Echo(arg))
            }
            "GET" => {
                let key = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Get(key))
            }
            "SET" => {
                let key = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                let val = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Set(key, val))
            }
            _ => Err(FrameError::Invalid),
        }
    }

    pub fn execute(self, db: &Store) -> Frame {
        match self {
            Command::Ping => Frame::Simple("PONG".to_string()),
            Command::Echo(s) => Frame::Bulk(s),
            Command::Get(key) => {
                let db = db.lock().unwrap();
                match db.get(&key) {
                    None => Frame::Null,
                    Some(s) => Frame::Bulk(s.to_owned()),
                }
            }
            Command::Set(key, val) => {
                let mut db = db.lock().unwrap();
                db.insert(key, val);
                Frame::Simple("OK".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn bulk(s: &str) -> Frame {
        Frame::Bulk(s.to_string())
    }

    fn cmd_array(args: &[&str]) -> Frame {
        Frame::Array(args.iter().map(|s| bulk(s)).collect())
    }

    #[test]
    fn parse_ping() {
        let cmd = Command::from_frame(cmd_array(&["PING"])).unwrap();
        assert!(matches!(cmd, Command::Ping));
    }

    #[test]
    fn parse_ping_lowercase() {
        let cmd = Command::from_frame(cmd_array(&["ping"])).unwrap();
        assert!(matches!(cmd, Command::Ping));
    }

    #[test]
    fn parse_echo() {
        let cmd = Command::from_frame(cmd_array(&["ECHO", "hello"])).unwrap();
        assert!(matches!(cmd, Command::Echo(s) if s == "hello"));
    }

    #[test]
    fn parse_echo_missing_arg() {
        let result = Command::from_frame(cmd_array(&["ECHO"]));
        assert!(result.is_err());
    }

    #[test]
    fn parse_unknown_command() {
        let result = Command::from_frame(cmd_array(&["FOOBAR"]));
        assert!(result.is_err());
    }

    #[test]
    fn parse_non_array_frame() {
        let result = Command::from_frame(Frame::Simple("PING".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_array() {
        let result = Command::from_frame(Frame::Array(vec![]));
        assert!(result.is_err());
    }

    fn new_store() -> Store {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn execute_ping() {
        let store = new_store();
        let response = Command::Ping.execute(&store);
        assert!(matches!(response, Frame::Simple(s) if s == "PONG"));
    }

    #[test]
    fn execute_echo() {
        let store = new_store();
        let response = Command::Echo("hello".to_string()).execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "hello"));
    }

    #[test]
    fn execute_set_and_get() {
        let store = new_store();
        let response = Command::Set("key".to_string(), "value".to_string()).execute(&store);
        assert!(matches!(response, Frame::Simple(s) if s == "OK"));

        let response = Command::Get("key".to_string()).execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "value"));
    }

    #[test]
    fn execute_get_missing_key() {
        let store = new_store();
        let response = Command::Get("nokey".to_string()).execute(&store);
        assert!(matches!(response, Frame::Null));
    }

    #[test]
    fn execute_set_overwrites() {
        let store = new_store();
        Command::Set("key".to_string(), "first".to_string()).execute(&store);
        Command::Set("key".to_string(), "second".to_string()).execute(&store);

        let response = Command::Get("key".to_string()).execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "second"));
    }

    #[test]
    fn parse_get() {
        let cmd = Command::from_frame(cmd_array(&["GET", "mykey"])).unwrap();
        assert!(matches!(cmd, Command::Get(k) if k == "mykey"));
    }

    #[test]
    fn parse_set() {
        let cmd = Command::from_frame(cmd_array(&["SET", "mykey", "myval"])).unwrap();
        assert!(matches!(cmd, Command::Set(k, v) if k == "mykey" && v == "myval"));
    }

    #[test]
    fn parse_get_missing_arg() {
        let result = Command::from_frame(cmd_array(&["GET"]));
        assert!(result.is_err());
    }

    #[test]
    fn parse_set_missing_val() {
        let result = Command::from_frame(cmd_array(&["SET", "key"]));
        assert!(result.is_err());
    }
}
