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
    Exists {
        key: String,
    },
    Incr {
        key: String,
    },
    Decr {
        key: String,
    },
    Del {
        keys: Vec<String>,
    },
    Mget {
        keys: Vec<String>,
    },
    Mset {
        pairs: Vec<(String, String)>,
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
            "EXISTS" => {
                let key = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Exists { key })
            }
            "INCR" => {
                let key = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Incr { key })
            }
            "DECR" => {
                let key = match frames.next() {
                    Some(Frame::Bulk(s)) => s,
                    _ => return Err(FrameError::Invalid),
                };
                Ok(Command::Decr { key })
            }
            "DEL" => {
                let keys: Result<Vec<String>, FrameError> = frames
                    .map(|frame| match frame {
                        Frame::Bulk(s) => Ok(s),
                        _ => Err(FrameError::Invalid),
                    })
                    .collect();

                Ok(Command::Del { keys: keys? })
            }
            "MGET" => {
                let keys: Result<Vec<String>, FrameError> = frames
                    .map(|frame| match frame {
                        Frame::Bulk(s) => Ok(s),
                        _ => Err(FrameError::Invalid),
                    })
                    .collect();

                Ok(Command::Mget { keys: keys? })
            }
            "MSET" => {
                let mut pairs = Vec::new();
                loop {
                    match (frames.next(), frames.next()) {
                        (Some(Frame::Bulk(key)), Some(Frame::Bulk(val))) => {
                            pairs.push((key, val));
                        }
                        (None, None) => break,
                        _ => return Err(FrameError::Invalid),
                    }
                }
                Ok(Command::Mset { pairs })
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
            Command::Exists { key } => {
                let mut db = db.lock().unwrap();
                if let Some(entry) = db.get(&key) {
                    if entry.is_expired() {
                        db.remove(&key);
                        Frame::Integer(0)
                    } else {
                        Frame::Integer(1)
                    }
                } else {
                    Frame::Integer(0)
                }
            }
            Command::Incr { key } => match Command::incr_by(db, key, 1) {
                Ok(new_val) => Frame::Integer(new_val),
                Err(_) => Frame::Error("ERR value is not an integer or out of range".to_string()),
            },
            Command::Decr { key } => match Command::incr_by(db, key, -1) {
                Ok(new_val) => Frame::Integer(new_val),
                Err(_) => Frame::Error("ERR value is not an integer or out of range".to_string()),
            },
            Command::Del { keys } => {
                let mut db = db.lock().unwrap();
                let mut count = 0;
                for key in keys {
                    if let Some(entry) = db.remove(&key)
                        && !entry.is_expired()
                    {
                        count += 1
                    }
                }
                Frame::Integer(count)
            }
            Command::Mget { keys } => {
                let mut db = db.lock().unwrap();
                let mut result = Vec::new();

                for key in keys {
                    match db.get(&key) {
                        Some(entry) if entry.is_expired() => {
                            db.remove(&key);
                            result.push(Frame::Null);
                        }
                        Some(entry) => {
                            result.push(Frame::Bulk(entry.value.clone()));
                        }
                        None => result.push(Frame::Null),
                    }
                }

                Frame::Array(result)
            }
            Command::Mset { pairs } => {
                let mut db = db.lock().unwrap();
                for (key, value) in pairs {
                    db.insert(
                        key,
                        Entry {
                            value,
                            expires_at: None,
                        },
                    );
                }
                Frame::Simple("OK".to_string())
            }
        }
    }
    fn incr_by(db: &Store, key: String, delta: i64) -> Result<i64, FrameError> {
        let mut db = db.lock().unwrap();
        let current = match db.get(&key) {
            Some(entry) if !entry.is_expired() => entry.value.as_str(),
            _ => "0",
        };

        let new_val = match current.parse::<i64>() {
            Ok(n) => n + delta,
            Err(_) => {
                return Err(FrameError::Invalid);
            }
        };

        db.insert(
            key,
            Entry {
                value: new_val.to_string(),
                expires_at: None,
            },
        );
        Ok(new_val)
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

    // --- DEL tests ---

    #[test]
    fn parse_del() {
        let cmd = Command::from_frame(cmd_array(&["DEL", "a", "b", "c"])).unwrap();
        assert!(matches!(cmd, Command::Del { keys } if keys == vec!["a", "b", "c"]));
    }

    #[test]
    fn execute_del_existing_keys() {
        let store = new_store();
        set_key(&store, "a", "1");
        set_key(&store, "b", "2");
        set_key(&store, "c", "3");

        let response = Command::Del {
            keys: vec!["a".to_string(), "c".to_string()],
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(2)));

        // "b" should still exist
        let response = Command::Get {
            key: "b".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "2"));
    }

    #[test]
    fn execute_del_missing_key() {
        let store = new_store();
        let response = Command::Del {
            keys: vec!["nope".to_string()],
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(0)));
    }

    #[test]
    fn execute_del_expired_key() {
        let store = new_store();
        Command::Set {
            key: "k".to_string(),
            value: "v".to_string(),
            expiry: Some(Duration::from_secs(0)),
        }
        .execute(&store);
        std::thread::sleep(Duration::from_millis(10));

        let response = Command::Del {
            keys: vec!["k".to_string()],
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(0)));
    }

    // --- EXISTS tests ---

    #[test]
    fn parse_exists() {
        let cmd = Command::from_frame(cmd_array(&["EXISTS", "mykey"])).unwrap();
        assert!(matches!(cmd, Command::Exists { key } if key == "mykey"));
    }

    #[test]
    fn execute_exists_present() {
        let store = new_store();
        set_key(&store, "k", "v");
        let response = Command::Exists {
            key: "k".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(1)));
    }

    #[test]
    fn execute_exists_missing() {
        let store = new_store();
        let response = Command::Exists {
            key: "k".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(0)));
    }

    #[test]
    fn execute_exists_expired() {
        let store = new_store();
        Command::Set {
            key: "k".to_string(),
            value: "v".to_string(),
            expiry: Some(Duration::from_secs(0)),
        }
        .execute(&store);
        std::thread::sleep(Duration::from_millis(10));

        let response = Command::Exists {
            key: "k".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(0)));
    }

    // --- INCR tests ---

    #[test]
    fn parse_incr() {
        let cmd = Command::from_frame(cmd_array(&["INCR", "counter"])).unwrap();
        assert!(matches!(cmd, Command::Incr { key } if key == "counter"));
    }

    #[test]
    fn execute_incr_new_key() {
        let store = new_store();
        let response = Command::Incr {
            key: "counter".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(1)));
    }

    #[test]
    fn execute_incr_existing() {
        let store = new_store();
        set_key(&store, "counter", "10");
        let response = Command::Incr {
            key: "counter".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(11)));
    }

    #[test]
    fn execute_incr_not_integer() {
        let store = new_store();
        set_key(&store, "name", "hello");
        let response = Command::Incr {
            key: "name".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Error(_)));
    }

    #[test]
    fn execute_incr_expired_key() {
        let store = new_store();
        Command::Set {
            key: "k".to_string(),
            value: "100".to_string(),
            expiry: Some(Duration::from_secs(0)),
        }
        .execute(&store);
        std::thread::sleep(Duration::from_millis(10));

        let response = Command::Incr {
            key: "k".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(1)));
    }

    // --- DECR tests ---

    #[test]
    fn parse_decr() {
        let cmd = Command::from_frame(cmd_array(&["DECR", "counter"])).unwrap();
        assert!(matches!(cmd, Command::Decr { key } if key == "counter"));
    }

    #[test]
    fn execute_decr_new_key() {
        let store = new_store();
        let response = Command::Decr {
            key: "counter".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(-1)));
    }

    #[test]
    fn execute_decr_existing() {
        let store = new_store();
        set_key(&store, "counter", "10");
        let response = Command::Decr {
            key: "counter".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Integer(9)));
    }

    // --- MGET tests ---

    #[test]
    fn parse_mget() {
        let cmd = Command::from_frame(cmd_array(&["MGET", "a", "b"])).unwrap();
        assert!(matches!(cmd, Command::Mget { keys } if keys == vec!["a", "b"]));
    }

    #[test]
    fn execute_mget() {
        let store = new_store();
        set_key(&store, "a", "1");
        set_key(&store, "b", "2");

        let response = Command::Mget {
            keys: vec!["a".to_string(), "missing".to_string(), "b".to_string()],
        }
        .execute(&store);

        match response {
            Frame::Array(frames) => {
                assert_eq!(frames.len(), 3);
                assert!(matches!(&frames[0], Frame::Bulk(s) if s == "1"));
                assert!(matches!(&frames[1], Frame::Null));
                assert!(matches!(&frames[2], Frame::Bulk(s) if s == "2"));
            }
            _ => panic!("expected Array"),
        }
    }

    // --- MSET tests ---

    #[test]
    fn parse_mset() {
        let cmd = Command::from_frame(cmd_array(&["MSET", "a", "1", "b", "2"])).unwrap();
        assert!(matches!(cmd, Command::Mset { pairs } if pairs == vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]));
    }

    #[test]
    fn parse_mset_odd_args() {
        let result = Command::from_frame(cmd_array(&["MSET", "a", "1", "b"]));
        assert!(result.is_err());
    }

    #[test]
    fn execute_mset() {
        let store = new_store();
        let response = Command::Mset {
            pairs: vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string()),
            ],
        }
        .execute(&store);
        assert!(matches!(response, Frame::Simple(s) if s == "OK"));

        let response = Command::Get {
            key: "a".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "1"));

        let response = Command::Get {
            key: "b".to_string(),
        }
        .execute(&store);
        assert!(matches!(response, Frame::Bulk(s) if s == "2"));
    }

    // --- helpers ---

    fn set_key(store: &Store, key: &str, value: &str) {
        Command::Set {
            key: key.to_string(),
            value: value.to_string(),
            expiry: None,
        }
        .execute(store);
    }
}
