/// Phase B gate validator 3 — cc-bridge end-to-end integration tests.
///
/// Strategy: bind a real TCP listener on 127.0.0.1:0 and drive it with
/// tokio-tungstenite + reqwest.  All tests share a helper that builds the
/// axum app and returns the bound address; each test is fully isolated.
///
/// Tests:
///   1. `echo_roundtrip`           – Open → stdin bytes → stdout bytes echo back
///   2. `resize_accepted`          – Resize frame keeps connection live (no error)
///   3. `child_exit_frame`         – EOF to stdin → child exits → Exit frame arrives
///   4. `bad_origin_rejected`      – mismatched Origin closes with code 4403
///   5. `missing_origin_rejected`  – absent Origin closes with code 4403
///   6. `child_killed_on_disconnect` – spawned child is dead after WS close
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        handshake::client::generate_key,
        http::Request,
        protocol::{Message, WebSocketConfig},
    },
};

// ---------------------------------------------------------------------------
// Test server bootstrap
// ---------------------------------------------------------------------------

/// Spin up a real TCP server on an ephemeral port.  Returns the bound address
/// as `"127.0.0.1:{port}"`.  The server task runs for the lifetime of the test.
async fn start_server() -> String {
    // Use process id + nanosecond timestamp for a unique temp directory.
    // Two parallel tests could theoretically collide at nanosecond granularity;
    // the static counter below prevents that.
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "cc_bridge_test_{}_{}_{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ));
    std::fs::create_dir_all(&tmp).expect("create temp dir");

    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());
    let config = server_core::ServerConfig {
        db_url,
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        anthropic_api_key: None,
        anthropic_base_url: "https://api.anthropic.com".to_string(),
        enable_external_tools: false,
        cwd_fallback: tmp.clone(),
    };

    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr").to_string();

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("axum::serve failed");
    });

    // Give the server a moment to accept connections.
    tokio::time::sleep(Duration::from_millis(20)).await;
    addr
}

/// GET /api/v1/cc-bridge/token and return the token string.
async fn fetch_token(addr: &str) -> String {
    let url = format!("http://{}/api/v1/cc-bridge/token", addr);
    let resp = reqwest::get(&url)
        .await
        .expect("GET /token")
        .json::<serde_json::Value>()
        .await
        .expect("parse token JSON");
    resp["token"]
        .as_str()
        .expect("token field missing")
        .to_string()
}

/// Build a WS `Request` for the terminal endpoint with a matching Origin/Host
/// (passes the same-origin check).
fn ws_request_same_origin(addr: &str, target: &str, token: &str) -> Request<()> {
    let url = format!(
        "ws://{}/api/v1/cc-bridge/sessions/{}/terminal?token={}",
        addr, target, token,
    );
    Request::builder()
        .uri(&url)
        .header("Host", addr)
        .header("Origin", format!("http://{}", addr))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .body(())
        .expect("build WS request")
}

/// Build a WS `Request` with a deliberately mismatched Origin.
fn ws_request_bad_origin(addr: &str, target: &str, token: &str) -> Request<()> {
    let url = format!(
        "ws://{}/api/v1/cc-bridge/sessions/{}/terminal?token={}",
        addr, target, token,
    );
    Request::builder()
        .uri(&url)
        .header("Host", addr)
        .header("Origin", "http://evil.example.com")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .body(())
        .expect("build WS request")
}

/// The Open frame that tells the server to spawn `cat` in /tmp.
fn open_frame() -> Message {
    Message::text(
        serde_json::json!({
            "kind": "open",
            "cmd": ["cat"],
            "cwd": "/tmp",
            "env": {},
            "cols": 80,
            "rows": 24,
        })
        .to_string(),
    )
}

// ---------------------------------------------------------------------------
// Test 1: echo roundtrip
// ---------------------------------------------------------------------------

/// Open a PTY running `cat`, send "hello\n", assert "hello" appears in the
/// binary stdout frames received within 2 seconds.
///
/// `cat` runs under cooked PTY line discipline: bytes are echoed by the
/// terminal layer and the line is delivered to cat on newline, which writes
/// it back.  Both the echo and the cat-output contain "hello", so either path
/// satisfies the assertion.
#[tokio::test]
async fn echo_roundtrip() {
    let addr = start_server().await;
    let token = fetch_token(&addr).await;
    let req = ws_request_same_origin(&addr, "test-session", &token);

    let (mut ws, _) = connect_async_with_config(req, Some(WebSocketConfig::default()), false)
        .await
        .expect("WS connect");

    ws.send(open_frame()).await.expect("send Open");

    ws.send(Message::Binary(Bytes::from_static(b"hello\n")))
        .await
        .expect("send stdin");

    // Collect stdout frames for up to 2 seconds.
    let mut received = Vec::<u8>::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Binary(b)))) => {
                received.extend_from_slice(&b);
                if received.windows(5).any(|w| w == b"hello") {
                    break;
                }
            }
            // Text frame = could be an Exit, keep looping.
            Ok(Some(Ok(Message::Text(_)))) => {}
            Ok(Some(Ok(_))) | Ok(None) | Err(_) | Ok(Some(Err(_))) => break,
        }
    }

    assert!(
        received.windows(5).any(|w| w == b"hello"),
        "expected 'hello' in stdout frames; got: {:?}",
        String::from_utf8_lossy(&received)
    );
}

// ---------------------------------------------------------------------------
// Test 2: resize accepted
// ---------------------------------------------------------------------------

/// Send Open then a Resize frame; assert the connection remains live — we can
/// still send another message without an error.
#[tokio::test]
async fn resize_accepted() {
    let addr = start_server().await;
    let token = fetch_token(&addr).await;
    let req = ws_request_same_origin(&addr, "test-session", &token);

    let (mut ws, _) = connect_async_with_config(req, Some(WebSocketConfig::default()), false)
        .await
        .expect("WS connect");

    ws.send(open_frame()).await.expect("send Open");

    let resize =
        Message::text(serde_json::json!({"kind":"resize","cols":120,"rows":40}).to_string());
    ws.send(resize).await.expect("send Resize");

    // Verify connection is still live by sending another stdin byte.
    ws.send(Message::Binary(Bytes::from_static(b"x")))
        .await
        .expect("connection must still be live after Resize");

    // Drain briefly; an unexpected error close is a failure.
    let result = tokio::time::timeout(Duration::from_millis(400), ws.next()).await;
    match result {
        Ok(Some(Ok(Message::Close(Some(cf))))) => {
            panic!("unexpected Close after Resize: code={}", u16::from(cf.code));
        }
        Ok(Some(Err(e))) => {
            panic!("WS error after Resize: {}", e);
        }
        // Binary stdout, text, timeout — all fine.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Test 3: child exit → Exit frame
// ---------------------------------------------------------------------------

/// Open `cat` and send two Ctrl-D bytes (0x04).  In cooked PTY mode a single
/// Ctrl-D flushes the current line buffer; a second one on an empty line
/// signals EOF to the reader.  `cat` receives EOF and exits.  The server
/// collects the exit code and sends an Exit JSON frame.
#[tokio::test]
async fn child_exit_frame() {
    let addr = start_server().await;
    let token = fetch_token(&addr).await;
    let req = ws_request_same_origin(&addr, "test-session", &token);

    let (mut ws, _) = connect_async_with_config(req, Some(WebSocketConfig::default()), false)
        .await
        .expect("WS connect");

    ws.send(open_frame()).await.expect("send Open");

    // Two Ctrl-D: first flushes any pending buffer; second signals EOF.
    ws.send(Message::Binary(Bytes::from_static(b"\x04\x04")))
        .await
        .expect("send EOF");

    let mut got_exit = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str())
                    && v["kind"] == "exit"
                {
                    got_exit = true;
                    break;
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                // Socket closed — the Exit frame may have been sent and received
                // just before Close; this branch only fires if we missed it (e.g.
                // the OS coalesced the frames).  Only fail if got_exit is still false.
                break;
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) | Err(_) => break,
        }
    }

    assert!(
        got_exit,
        "expected Exit frame after cat received EOF, but none arrived"
    );
}

// ---------------------------------------------------------------------------
// Test 4: mismatched Origin is rejected with close code 4403
// ---------------------------------------------------------------------------

/// A WebSocket upgrade whose Origin does NOT match Host must be rejected
/// with WebSocket close code 4403 before the PTY ever spawns.
///
/// The handler accepts the HTTP upgrade unconditionally (that is the WS spec
/// contract) and immediately sends Close(4403) — the token is consumed but
/// the PTY is never allocated.
#[tokio::test]
async fn bad_origin_rejected() {
    let addr = start_server().await;
    // Supply a valid token so only the origin check is under test.
    let token = fetch_token(&addr).await;
    let req = ws_request_bad_origin(&addr, "test-session", &token);

    let (mut ws, _) = connect_async_with_config(req, Some(WebSocketConfig::default()), false)
        .await
        .expect("WS connect (HTTP upgrade accepted; rejection is in the WS frame)");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut got_4403 = false;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Close(Some(cf))))) => {
                if u16::from(cf.code) == 4403 {
                    got_4403 = true;
                }
                break;
            }
            Ok(Some(Ok(Message::Close(None)))) | Ok(None) => break,
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) | Err(_) => break,
        }
    }

    assert!(
        got_4403,
        "expected Close(4403) for mismatched Origin, but it did not arrive"
    );
}

// ---------------------------------------------------------------------------
// Test 5: absent Origin is rejected with close code 4403
// ---------------------------------------------------------------------------

/// Build a WS `Request` with NO Origin header at all.
fn ws_request_no_origin(addr: &str, target: &str, token: &str) -> Request<()> {
    let url = format!(
        "ws://{}/api/v1/cc-bridge/sessions/{}/terminal?token={}",
        addr, target, token,
    );
    Request::builder()
        .uri(&url)
        .header("Host", addr)
        // Deliberately omit "Origin" — this is what Fix 2 guards against.
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .body(())
        .expect("build WS request (no Origin)")
}

/// A WebSocket upgrade with NO Origin header must be rejected with close code
/// 4403 — same result as a mismatched origin.  A legitimate browser or Tauri
/// webview always sends Origin, so this rejects non-browser scripted clients.
#[tokio::test]
async fn missing_origin_rejected() {
    let addr = start_server().await;
    let token = fetch_token(&addr).await;
    let req = ws_request_no_origin(&addr, "test-session", &token);

    let (mut ws, _) = connect_async_with_config(req, Some(WebSocketConfig::default()), false)
        .await
        .expect("WS connect (HTTP upgrade accepted; rejection is in the WS frame)");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut got_4403 = false;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Close(Some(cf))))) => {
                if u16::from(cf.code) == 4403 {
                    got_4403 = true;
                }
                break;
            }
            Ok(Some(Ok(Message::Close(None)))) | Ok(None) => break,
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) | Err(_) => break,
        }
    }

    assert!(
        got_4403,
        "expected Close(4403) for absent Origin, but it did not arrive"
    );
}

// ---------------------------------------------------------------------------
// Test 6: spawned child is killed when the WebSocket closes
// ---------------------------------------------------------------------------

/// Open a PTY running `sh -c 'echo $$ > PIDFILE; sleep 300'`.  The shell
/// writes its own PID to a temp file so the test can identify the process.
/// After the WebSocket closes, the relay must kill+reap the child.  The test
/// polls `kill -0 <pid>` (which fails with ESRCH once the process is gone)
/// until the process disappears or a 5-second deadline expires.
#[tokio::test]
async fn child_killed_on_disconnect() {
    let addr = start_server().await;
    let token = fetch_token(&addr).await;
    let req = ws_request_same_origin(&addr, "test-session", &token);

    // Create a unique PID file path in the system temp dir.
    let pid_file = std::env::temp_dir().join(format!(
        "cc_bridge_child_pid_{}_{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ));
    // Clean up any leftover from a previous (failed) run.
    let _ = std::fs::remove_file(&pid_file);

    let pid_file_str = pid_file.to_string_lossy().into_owned();

    // Open frame: spawn a shell that writes its PID then sleeps for 300s.
    // The shell is the direct child of the PTY; killing it also kills sleep.
    let open = Message::text(
        serde_json::json!({
            "kind": "open",
            "cmd": ["sh", "-c", format!("echo $$ > {}; sleep 300", pid_file_str)],
            "cwd": "/tmp",
            "env": {},
            "cols": 80,
            "rows": 24,
        })
        .to_string(),
    );

    let (mut ws, _) = connect_async_with_config(req, Some(WebSocketConfig::default()), false)
        .await
        .expect("WS connect");

    ws.send(open).await.expect("send Open");

    // Wait for the child to write its PID file (up to 5 seconds).
    let child_pid: u32 = {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::time::Instant::now() >= deadline {
                panic!("child never wrote its PID file at {}", pid_file.display());
            }
            if pid_file.exists() {
                let contents = std::fs::read_to_string(&pid_file).expect("read PID file");
                let pid_str = contents.trim().to_string();
                if let Ok(pid) = pid_str.parse::<u32>() {
                    break pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };

    // Verify the child is actually running before we close the WS.
    let probe = std::process::Command::new("kill")
        .args(["-0", &child_pid.to_string()])
        .status()
        .expect("run kill -0");
    assert!(
        probe.success(),
        "child PID {} was not running before WS close",
        child_pid
    );

    // Close the WebSocket — this triggers the relay cleanup path.
    ws.close(None).await.expect("send WS Close");
    // Drain any remaining frames (the relay may send an Exit frame back).
    let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let rem = drain_deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            break;
        }
        match tokio::time::timeout(rem, ws.next()).await {
            Ok(Some(Ok(_))) => {}
            _ => break,
        }
    }

    // Poll until the child process is gone or the deadline expires.
    let kill_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut process_gone = false;
    while tokio::time::Instant::now() < kill_deadline {
        let status = std::process::Command::new("kill")
            .args(["-0", &child_pid.to_string()])
            .status()
            .expect("run kill -0");
        if !status.success() {
            process_gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Clean up temp file regardless of outcome.
    let _ = std::fs::remove_file(&pid_file);

    assert!(
        process_gone,
        "child process {} was still alive {}ms after WS close; \
         PTY child was not killed by the relay cleanup",
        child_pid,
        kill_deadline
            .duration_since(tokio::time::Instant::now().min(kill_deadline))
            .as_millis(),
    );
}
