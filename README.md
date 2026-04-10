# my-redis

A Redis clone built from scratch in Rust with Tokio.

This is a learning project focused on understanding how Redis works internally -- async networking, wire protocols, shared state, pub/sub messaging, and persistence.

## Features

- **RESP Protocol** -- full serialization and parsing of the Redis Serialization Protocol (simple strings, errors, integers, bulk strings, arrays, null)
- **Commands** -- PING, ECHO, GET, SET (with EX/PX), DEL, EXISTS, INCR, DECR, MGET, MSET, SUBSCRIBE, PUBLISH
- **TTL Expiration** -- lazy expiration on read + background purge task
- **Pub/Sub** -- multi-channel subscribe with broadcast fan-out
- **Persistence** -- periodic RDB-style snapshots with automatic reload on startup
- **Concurrent Connections** -- one Tokio task per client, shared state via `Arc<Mutex<T>>`

## Architecture

```
src/
  main.rs        -- TCP listener, task spawning, background tasks
  frame.rs       -- RESP protocol: Frame enum, serialize, check, parse
  connection.rs  -- Connection struct: read_frame / write_frame over TcpStream
  command.rs     -- Command enum: from_frame parsing + execute logic
  store.rs       -- Key-value store: Entry, TTL, save/load persistence
  channel.rs     -- Pub/Sub channel registry type
  lib.rs         -- Module exports
```

```
Client                          Server
  |                               |
  |---- RESP bytes over TCP ----->|
  |                               |  Connection::read_frame()
  |                               |    -> Frame::check() + Frame::parse()
  |                               |  Command::from_frame()
  |                               |  Command::execute(&store)
  |                               |    -> response Frame
  |<--- RESP bytes over TCP ------|
  |                               |  Connection::write_frame()
```

## Usage

```sh
# Start the server
cargo run

# Connect with any Redis client
redis-cli PING           # -> PONG
redis-cli SET name ray   # -> OK
redis-cli GET name       # -> "ray"
redis-cli SET key val EX 10  # expires in 10 seconds
redis-cli DEL name       # -> (integer) 1
redis-cli INCR counter   # -> (integer) 1

# Pub/Sub (two terminal windows)
redis-cli SUBSCRIBE news        # window 1: listening
redis-cli PUBLISH news "hello"  # window 2: sends message
```

Or test with `nc` using raw RESP:

```sh
printf '*1\r\n$4\r\nPING\r\n' | nc -w 1 127.0.0.1 6379
```

## Running Tests

```sh
cargo test
```

89 tests covering RESP serialization/parsing, connection framing, command parsing/execution, TTL expiration, and persistence save/load.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` | Async runtime, TCP, timers |
| `bytes` | Efficient byte buffer management |
| `itoa` | Zero-allocation integer formatting |
| `atoi` | Byte slice to integer parsing |
| `tokio-stream` | BroadcastStream + StreamMap for pub/sub |
| `serde` | Serialization framework |
| `bincode` | Binary format for RDB snapshots |

## Build Order

This project was built incrementally across 9 milestones:

1. TCP listener + echo server
2. RESP serialization (Frame -> bytes)
3. RESP parsing (bytes -> Frame) + Connection struct
4. Command dispatch (PING, ECHO)
5. Key-value store (GET, SET) with shared state
6. TTL expiration (SET EX/PX, lazy + background purge)
7. Additional commands (DEL, EXISTS, INCR, DECR, MGET, MSET)
8. Pub/Sub (SUBSCRIBE, PUBLISH) with broadcast channels
9. Persistence (periodic RDB snapshots, load on startup)
