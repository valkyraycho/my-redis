use crate::frame::{Frame, FrameError};

pub enum Command {
    Ping,
    Echo(String),
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
            _ => Err(FrameError::Invalid),
        }
    }

    pub fn execute(self) -> Frame {
        match self {
            Command::Ping => Frame::Simple("PONG".to_string()),
            Command::Echo(s) => Frame::Bulk(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn execute_ping() {
        let response = Command::Ping.execute();
        assert!(matches!(response, Frame::Simple(s) if s == "PONG"));
    }

    #[test]
    fn execute_echo() {
        let response = Command::Echo("hello".to_string()).execute();
        assert!(matches!(response, Frame::Bulk(s) if s == "hello"));
    }
}
