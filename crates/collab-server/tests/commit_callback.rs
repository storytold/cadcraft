//! The cabinet commit callback (M87): two participants see the cabinet's version, a `409` offers
//! the room's file as a copy instead of overwriting the cabinet (TASK §6).

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use cadcraft_collab::{ClientInfo, ClientMessage, Op, OpContext, OpKind, Role, ServerMessage};
use cadcraft_collab_client::Connection;
use cadcraft_collab_server::auth::{Claims, StaticAuthenticator};
use cadcraft_collab_server::{Cabinet, CabinetConfig, Room, Server};
use cadcraft_doc::{Drawing, Space};
use serde_json::json;

fn hello(token: &str) -> ClientMessage {
    ClientMessage::Hello {
        app: "cadcraft".into(),
        doc: "d1".into(),
        jwt: token.into(),
        client: ClientInfo { name: "test".into(), ver: "0".into() },
        last_version: None,
    }
}

fn line_op(x: f64) -> Op {
    Op {
        op_id: "a1".into(),
        actor: "a".into(),
        base_version: 0,
        kind: OpKind::Command,
        command: "line".into(),
        params: json!({ "points": [[x, 0.0], [x + 1.0, 0.0]] }),
        context: OpContext::in_space(Space::Model),
    }
}

fn sign_in(addr: std::net::SocketAddr, token: &str) -> Connection {
    let url = format!("ws://{addr}");
    let (connection, welcome) = Connection::connect(&url, &hello(token), Duration::from_secs(3)).expect("connected");
    let ServerMessage::Welcome { snapshot, .. } = welcome else { panic!("expected a welcome, got {welcome:?}") };
    assert!(!snapshot.is_empty(), "a first connection needs the drawing");
    connection
}

/// A loopback cabinet that answers each request with the next canned answer and sends every
/// request body it saw.
fn fake_cabinet(answers: Vec<&'static str>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a listener");
    let addr = listener.local_addr().expect("an address");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for answer in answers {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let request = read_request(&mut stream);
            let _ = tx.send(request);
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    (format!("http://{addr}"), rx)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).expect("the request head");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let head_end = request.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(request.len());
    let length = String::from_utf8_lossy(&request[..head_end])
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
        })
        .unwrap_or(0);
    while request.len() < head_end + length {
        let read = stream.read(&mut chunk).expect("the request body");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8_lossy(&request).to_string()
}

fn body_of(request: &str) -> serde_json::Value {
    let body = request.split_once("\r\n\r\n").map(|(_, body)| body).expect("a body");
    serde_json::from_str(body).expect("the body is JSON")
}

fn room_with_cabinet(url: String) -> Room {
    let cabinet = Cabinet::new(CabinetConfig {
        url: format!("{url}/api/v1/internal/office/sessions/{{sessionId}}/commit"),
        token: "service-token".into(),
        file_id: "42".into(),
        timeout: Duration::from_secs(3),
        attempts: 1,
    });
    Room::new(Drawing::new_metric()).unwrap().with_cabinet(Arc::new(cabinet))
}

fn next_committed(connection: &mut Connection) -> (u64, u64, String) {
    for _ in 0..16 {
        if let ServerMessage::Committed { version, cabinet, .. } = connection.receive().expect("a message") {
            return (version, cabinet.version, cabinet.etag);
        }
    }
    panic!("no committed answer");
}

#[test]
fn two_participants_see_the_cabinet_commit_and_the_next_expected_etag() {
    let (url, requests) = fake_cabinet(vec![
        "HTTP/1.1 200 OK\r\nContent-Length: 30\r\n\r\n{\"version\":11,\"etag\":\"e11\"}",
        "HTTP/1.1 200 OK\r\nContent-Length: 30\r\n\r\n{\"version\":12,\"etag\":\"e12\"}",
    ]);
    let auth = Arc::new(StaticAuthenticator::new([
        ("token-a".to_string(), Claims { sub: "a".into(), doc: "d1".into(), role: Role::Owner }),
        ("token-b".to_string(), Claims { sub: "b".into(), doc: "d1".into(), role: Role::Editor }),
    ]));
    let server = Arc::new(Server::new(room_with_cabinet(url), auth, "s1", "cadcraft"));
    let (addr, _accept_loop) = server.bind("127.0.0.1:0").unwrap();

    let mut a = sign_in(addr, "token-a");
    let mut b = sign_in(addr, "token-b");
    // A drawing both participants can commit from: the room is at version 1.
    a.send(&ClientMessage::Op { op: line_op(0.0) }).unwrap();
    for _ in 0..16 {
        if let ServerMessage::Applied { .. } = b.receive().unwrap() {
            break;
        }
    }

    a.send(&ClientMessage::Commit { expected_version: 1 }).unwrap();
    assert_eq!(next_committed(&mut a), (1, 11, "e11".into()));
    assert_eq!(next_committed(&mut b), (1, 11, "e11".into()), "the other participant sees the cabinet's version");

    // The cabinet's first answer becomes expectedVersion on the next commit, whichever
    // participant makes it.
    b.send(&ClientMessage::Commit { expected_version: 1 }).unwrap();
    assert_eq!(next_committed(&mut b), (1, 12, "e12".into()));
    assert_eq!(next_committed(&mut a), (1, 12, "e12".into()));

    let first = body_of(&requests.recv().unwrap());
    assert!(first.get("expectedVersion").is_none(), "{first}");
    assert_eq!(first["fileId"], json!("42"));
    assert_eq!(first["version"], json!(1));
    let bytes = cadcraft_collab::message::decode_bytes(first["bytes"].as_str().unwrap()).unwrap();
    let reopened = cadcraft_io::read(&bytes, "commit.dxf").unwrap();
    assert_eq!(reopened.space(&Space::Model), server.room().drawing().unwrap().space(&Space::Model));

    let second = body_of(&requests.recv().unwrap());
    assert_eq!(second["expectedVersion"], json!("e11"), "the cabinet's etag guards the next commit");
}

#[test]
fn a_409_keeps_the_room_file_for_a_copy_and_never_overwrites_the_cabinet() {
    let (url, requests) = fake_cabinet(vec!["HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\n\r\n{}"]);
    let auth = Arc::new(StaticAuthenticator::new([("token-a".to_string(), Claims { sub: "a".into(), doc: "d1".into(), role: Role::Owner })]));
    let server = Arc::new(Server::new(room_with_cabinet(url), auth, "s1", "cadcraft"));
    let (addr, _accept_loop) = server.bind("127.0.0.1:0").unwrap();
    let mut a = sign_in(addr, "token-a");
    a.send(&ClientMessage::Op { op: line_op(0.0) }).unwrap();

    a.send(&ClientMessage::Commit { expected_version: 1 }).unwrap();
    let mut conflict = None;
    for _ in 0..16 {
        if let ServerMessage::CommitConflict { version, message } = a.receive().unwrap() {
            conflict = Some((version, message));
            break;
        }
    }
    let (version, message) = conflict.expect("the cabinet refusal reaches the owner");
    assert_eq!(version, 1);
    assert!(!message.is_empty());
    assert!(requests.try_recv().is_ok(), "the cabinet was asked once");
    assert!(requests.try_recv().is_err(), "a 409 is not retried");

    a.send(&ClientMessage::SaveCopy { op_id: "c1".into() }).unwrap();
    let mut copy = None;
    for _ in 0..16 {
        if let ServerMessage::SaveCopyResult { version, bytes_base64, .. } = a.receive().unwrap() {
            copy = Some((version, bytes_base64));
            break;
        }
    }
    let (version, bytes_base64) = copy.expect("the refused file is available as a copy");
    assert_eq!(version, 1);
    let bytes = cadcraft_collab::message::decode_bytes(&bytes_base64).unwrap();
    let reopened = cadcraft_io::read(&bytes, "copy.dxf").unwrap();
    assert_eq!(reopened.space(&Space::Model), server.room().drawing().unwrap().space(&Space::Model));
    assert_eq!(server.room().conflict(), None, "the copy clears the pending conflict");

    // The copy is sent once: a second request has nothing to give.
    a.send(&ClientMessage::SaveCopy { op_id: "c2".into() }).unwrap();
    for _ in 0..16 {
        if let ServerMessage::Error { code, .. } = a.receive().unwrap() {
            assert_eq!(code, "no-copy");
            return;
        }
    }
    panic!("a second copy request is refused");
}

#[test]
fn a_viewer_cannot_commit() {
    let (url, requests) = fake_cabinet(vec![]);
    let auth = Arc::new(StaticAuthenticator::new([("token-v".to_string(), Claims { sub: "v".into(), doc: "d1".into(), role: Role::Viewer })]));
    let server = Arc::new(Server::new(room_with_cabinet(url), auth, "s1", "cadcraft"));
    let (addr, _accept_loop) = server.bind("127.0.0.1:0").unwrap();
    let mut viewer = sign_in(addr, "token-v");
    viewer.send(&ClientMessage::Commit { expected_version: 0 }).unwrap();
    for _ in 0..16 {
        if let ServerMessage::Rejected { rejected } = viewer.receive().unwrap() {
            assert_eq!(rejected.code, "forbidden");
            assert!(requests.try_recv().is_err(), "the cabinet is never asked for a viewer");
            return;
        }
    }
    panic!("a viewer's commit is refused");
}
