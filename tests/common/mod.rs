#![allow(dead_code)] // Shared support is compiled separately by each integration test.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

use serde_json::{Value, json};
use ssteak::helius::{Helius, RequestContext};

pub fn context() -> RequestContext {
    RequestContext::new(Duration::from_secs(5), Arc::new(AtomicBool::new(false)))
}

pub fn key(byte: u8) -> String {
    bs58::encode([byte; 32]).into_string()
}

pub fn fixture(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(format!("tests/fixtures/{name}.json")).unwrap())
        .unwrap()
}

pub fn rpc(result: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":1, "result":result})
}

pub fn server(replies: Vec<Value>) -> (Helius, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((key, value)) = line.split_once(':')
                    && key.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            requests.push(serde_json::from_slice(&body).unwrap());
            let body = reply.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (
        Helius::with_endpoint(&url, "test-secret", Duration::from_secs(2)).unwrap(),
        thread,
    )
}
