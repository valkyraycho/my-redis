use std::time::Duration;

use tokio::time::Instant;

use crate::frame::{Frame, FrameError};
use crate::store::{Entry, Store};

pub enum Command {
    Ping,
    Echo {
        message: String,
    },
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
        expiry: Option<Duration>,
    },
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
                let message = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Echo { message })
            }
            "GET" => {
                let key = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Get { key })
            }
            "SET" => {
                let key = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                let value = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                match (frames.next(), frames.next()) {
                    (None, _) => Ok(Command::Set {
                        key,
                        value,
                        expiry: None,
                    }),
                    (_, None) => Err(FrameError::Invalid),
                    (Some(Frame::Bulk(scale)), Some(Frame::Bulk(duration))) => {
                        let duration = duration.parse::<u64>().map_err(|_| FrameError::Invalid)?;
                        match scale.to_ascii_uppercase().as_str() {
                            "EX" => Ok(Command::Set {
                                key,
                                value,
                                expiry: Some(Duration::from_secs(duration)),
                            }),
                            "PX" => Ok(Command::Set {
                                key,
                                value,
                                expiry: Some(Duration::from_millis(duration)),
                            }),
                            _ => Err(FrameError::Invalid),
                        }
                    }
                    _ => Err(FrameError::Invalid),
                }
            }
            _ => Err(FrameError::Invalid),
        }
    }

    pub fn execute(self, db: &Store) -> Frame {
        match self {
            Command::Ping => Frame::Simple("PONG".to_string()),
            Command::Echo { message } => Frame::Bulk(message),
            Command::Get { key } => {
                let mut db = db.lock().unwrap();
                match db.get(&key) {
                    Some(entry) if entry.is_expired() => {
                        db.remove(&key);
                        Frame::Null
                    }
                    Some(entry) => Frame::Bulk(entry.value.clone()),
                    None => Frame::Null,
                }
            }
            Command::Set { key, value, expiry } => {
                let mut db = db.lock().unwrap();
                db.insert(
                    key,
                    Entry {
                        value,
                        expires_at: expiry.map(|duration| Instant::now() + duration),
                    },
                );
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
        assert!(matches!(cmd, Command::Echo { message } if message == "hello"));
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
        let response = Command::Echo {
            message: "hello".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "hello"));
    }

    #[test]
    fn execute_set_and_get() {
        let store = new_store();
        let response = Command::Set {
            key: "key".to_string(),
            value: "value".to_string(),
            expiry: None,
        }
        .execute(&store);
        assert!(matches!(response, Frame::Simple(s) if s == "OK"));

        let response = Command::Get {
            key: "key".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "value"));
    }

    #[test]
    fn execute_get_missing_key() {
        let store = new_store();
        let response = Command::Get {
            key: "nokey".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Null));
    }

    #[test]
    fn execute_set_overwrites() {
        let store = new_store();
        Command::Set {
            key: "key".to_string(),
            value: "first".to_string(),
            expiry: None,
        }
        .execute(&store);
        Command::Set {
            key: "key".to_string(),
            value: "second".to_string(),
            expiry: None,
        }
        .execute(&store);

        let response = Command::Get {
            key: "key".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "second"));
    }

    #[test]
    fn execute_get_expired_key() {
        let store = new_store();
        // Set with 0-second expiry — already expired
        Command::Set {
            key: "key".to_string(),
            value: "value".to_string(),
            expiry: Some(Duration::from_secs(0)),
        }
        .execute(&store);

        // Small sleep to ensure expiry
        std::thread::sleep(Duration::from_millis(10));

        let response = Command::Get {
            key: "key".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Null));
    }

    #[test]
    fn execute_get_unexpired_key() {
        let store = new_store();
        Command::Set {
            key: "key".to_string(),
            value: "value".to_string(),
            expiry: Some(Duration::from_secs(60)),
        }
        .execute(&store);

        let response = Command::Get {
            key: "key".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "value"));
    }

    #[test]
    fn parse_get() {
        let cmd = Command::from_frame(cmd_array(&["GET", "mykey"])).unwrap();
        assert!(matches!(cmd, Command::Get { key } if key == "mykey"));
    }

    #[test]
    fn parse_set() {
        let cmd = Command::from_frame(cmd_array(&["SET", "mykey", "myval"])).unwrap();
        assert!(
            matches!(cmd, Command::Set { key, value, expiry } if key == "mykey" && value == "myval" && expiry.is_none())
        );
    }

    #[test]
    fn parse_set_with_ex() {
        let cmd = Command::from_frame(cmd_array(&["SET", "k", "v", "EX", "10"])).unwrap();
        assert!(
            matches!(cmd, Command::Set { expiry: Some(d), .. } if d == Duration::from_secs(10))
        );
    }

    #[test]
    fn parse_set_with_px() {
        let cmd = Command::from_frame(cmd_array(&["SET", "k", "v", "PX", "500"])).unwrap();
        assert!(
            matches!(cmd, Command::Set { expiry: Some(d), .. } if d == Duration::from_millis(500))
        );
    }

    #[test]
    fn parse_set_with_ex_lowercase() {
        let cmd = Command::from_frame(cmd_array(&["SET", "k", "v", "ex", "10"])).unwrap();
        assert!(
            matches!(cmd, Command::Set { expiry: Some(d), .. } if d == Duration::from_secs(10))
        );
    }

    #[test]
    fn parse_set_with_invalid_expiry() {
        let result = Command::from_frame(cmd_array(&["SET", "k", "v", "EX", "notanumber"]));
        assert!(result.is_err());
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
