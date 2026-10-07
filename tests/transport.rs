use serde_json::json;
use ssteak::helius::{Helius, RequestContext};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

fn read_request(stream: &mut TcpStream) {
    // Accepted sockets can inherit a listener's nonblocking mode on macOS.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(stream);
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
            length = value.trim().parse::<u64>().unwrap();
        }
    }
    let mut body = Vec::new();
    reader.take(length).read_to_end(&mut body).unwrap();
    assert_eq!(body.len() as u64, length);
}

fn server(
    replies: Vec<(u16, String, &'static str)>,
) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let thread = std::thread::spawn(move || {
        for (status, body, headers) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            read_request(&mut stream);
            count.fetch_add(1, Ordering::SeqCst);
            write!(stream,"HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{headers}\r\n{body}",body.len()).unwrap();
        }
    });
    (endpoint, calls, thread)
}
fn context() -> RequestContext {
    RequestContext::new(Duration::from_secs(3), Arc::new(AtomicBool::new(false)))
}
fn ok(result: serde_json::Value) -> String {
    json!({"jsonrpc":"2.0","id":1,"result":result}).to_string()
}
#[test]
fn retries_transient_failures_but_never_exposes_endpoint_credentials() {
    let (url, calls, thread) = server(vec![
        (429, "secret-from-provider".into(), "Retry-After: 0\r\n"),
        (503, "down".into(), ""),
        (200, ok(json!(42)), ""),
    ]);
    let client = Helius::with_endpoint(&url, "DO_NOT_LEAK", Duration::from_secs(1)).unwrap();
    assert_eq!(
        client.call("getEpochInfo", json!([]), &context()).unwrap(),
        42
    );
    thread.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
#[test]
fn auth_is_not_retried_and_provider_message_is_omitted() {
    let (url, calls, thread) = server(vec![(401, "DO_NOT_LEAK".into(), "")]);
    let client = Helius::with_endpoint(&url, "DO_NOT_LEAK", Duration::from_secs(1)).unwrap();
    let error = client
        .call("getEpochInfo", json!([]), &context())
        .unwrap_err();
    assert_eq!(error.code, "PROVIDER_AUTH");
    assert!(!format!("{error:?} {error}").contains("DO_NOT_LEAK"));
    thread.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn malformed_or_mismatched_rpc_responses_are_rejected() {
    for body in [
        "not-json".to_owned(),
        json!({"jsonrpc":"2.0","id":99,"result":1}).to_string(),
        json!({"jsonrpc":"2.0","id":1}).to_string(),
    ] {
        let (url, _, thread) = server(vec![(200, body, "")]);
        let client = Helius::with_endpoint(&url, "k", Duration::from_secs(1)).unwrap();
        assert_eq!(
            client
                .call("getEpochInfo", json!([]), &context())
                .unwrap_err()
                .code,
            "INVALID_RESPONSE"
        );
        thread.join().unwrap();
    }
}
#[test]
fn cancelled_or_expired_work_never_connects() {
    let client =
        Helius::with_endpoint("http://127.0.0.1:1", "secret", Duration::from_millis(10)).unwrap();
    let c = RequestContext::new(Duration::from_secs(1), Arc::new(AtomicBool::new(true)));
    assert_eq!(
        client.call("getEpochInfo", json!([]), &c).unwrap_err().code,
        "INTERRUPTED"
    );
    let c = RequestContext::new(Duration::ZERO, Arc::new(AtomicBool::new(false)));
    assert_eq!(
        client.call("getEpochInfo", json!([]), &c).unwrap_err().code,
        "DEADLINE"
    );
}
#[test]
fn retry_after_cannot_extend_deadline() {
    let (url, calls, thread) = server(vec![(429, "{}".into(), "Retry-After: 120\r\n")]);
    let client = Helius::with_endpoint(&url, "k", Duration::from_secs(1)).unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        client
            .call("getEpochInfo", json!([]), &context())
            .unwrap_err()
            .code,
        "DEADLINE"
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    thread.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn wrong_network_is_rejected() {
    let (url, _, thread) = server(vec![(200, ok(json!("other-network")), "")]);
    let client = Helius::with_endpoint(&url, "k", Duration::from_secs(1)).unwrap();
    assert_eq!(
        client.verify_mainnet(&context()).unwrap_err().code,
        "WRONG_NETWORK"
    );
    thread.join().unwrap();
}

#[test]
fn interrupted_response_body_is_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\nConnection: close\r\n\r\n{")
            .unwrap();
        drop(stream);
        listener.set_nonblocking(true).unwrap();
        let start = std::time::Instant::now();
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                read_request(&mut stream);
                let body = ok(json!(1));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                return;
            }
            if start.elapsed() > Duration::from_secs(2) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let client = Helius::with_endpoint(&url, "secret", Duration::from_secs(1)).unwrap();
    let result = client.call("getEpochInfo", json!([]), &context());
    server.join().unwrap();
    assert_eq!(result.unwrap(), 1);
}

#[test]
fn stalled_requests_time_out_with_bounded_retries() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(60));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        }
    });
    let client = Helius::with_endpoint(&url, "secret", Duration::from_millis(20)).unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        client
            .call("getEpochInfo", json!([]), &context())
            .unwrap_err()
            .code,
        "PROVIDER_TIMEOUT"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    server.join().unwrap();
}

#[test]
fn requests_across_clones_never_exceed_four_in_flight() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let peak = Arc::new(AtomicUsize::new(0));
    let max = peak.clone();
    let server = std::thread::spawn(move || {
        let active = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let (mut stream, _) = listener.accept().unwrap();
            let active = active.clone();
            let max = max.clone();
            handles.push(std::thread::spawn(move || {
                read_request(&mut stream);
                let n = active.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(n, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(30));
                let body = ok(json!(1));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    });
    let client = Helius::with_endpoint(&url, "secret", Duration::from_secs(1)).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let client = &client;
            scope.spawn(move || {
                assert_eq!(
                    client.call("getEpochInfo", json!([]), &context()).unwrap(),
                    1
                )
            });
        }
    });
    server.join().unwrap();
    assert!(peak.load(Ordering::SeqCst) <= 4);
}
