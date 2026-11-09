//! End to end: two editors and a viewer over a real socket.

use std::sync::Arc;
use std::time::Duration;

use cadcraft_collab::message::Snapshot;
use cadcraft_collab::{Applied, ClientInfo, ClientMessage, Op, OpContext, OpKind, Role, ServerMessage};
use cadcraft_collab_client::{Client, Connection};
use cadcraft_collab_server::auth::{Claims, StaticAuthenticator};
use cadcraft_collab_server::{Room, Server};
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

fn line_op(actor: &str, op_id: &str, x: f64) -> Op {
    Op {
        op_id: op_id.into(),
        actor: actor.into(),
        base_version: 0,
        kind: OpKind::Command,
        command: "line".into(),
        params: json!({ "points": [[x, 0.0], [x + 1.0, 0.0]] }),
        context: OpContext::in_space(Space::Model),
    }
}

fn next_applied(connection: &mut Connection, version: u64) -> Applied {
    for _ in 0..32 {
        match connection.receive().expect("a message") {
            ServerMessage::Applied { applied } if applied.version == version => return applied,
            _ => continue,
        }
    }
    panic!("no applied answer for version {version}");
}

fn welcome_drawing(message: ServerMessage) -> Drawing {
    match message {
        ServerMessage::Welcome { snapshot, .. } => {
            assert!(!snapshot.is_empty(), "a first connection needs the drawing");
            let bytes = Snapshot::decode(&snapshot).expect("the parts join");
            cadcraft_io::read(&bytes, "snapshot.dxf").expect("the snapshot is a drawing")
        }
        other => panic!("expected welcome, got {other:?}"),
    }
}

#[test]
fn two_editors_and_a_viewer_over_websocket() {
    let room = Room::new(Drawing::new_metric()).unwrap();
    let auth = Arc::new(StaticAuthenticator::new([
        ("token-a".to_string(), Claims { sub: "a".into(), doc: "d1".into(), role: Role::Editor }),
        ("token-b".to_string(), Claims { sub: "b".into(), doc: "d1".into(), role: Role::Editor }),
        ("token-v".to_string(), Claims { sub: "v".into(), doc: "d1".into(), role: Role::Viewer }),
    ]));
    let server = Arc::new(Server::new(room, auth, "s1", "cadcraft"));
    let (addr, _accept_loop) = server.bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{addr}");
    let timeout = Duration::from_secs(3);

    let (mut a, welcome_a) = Connection::connect(&url, &hello("token-a"), timeout).unwrap();
    let (mut b, welcome_b) = Connection::connect(&url, &hello("token-b"), timeout).unwrap();
    let start = welcome_drawing(welcome_a);
    assert_eq!(welcome_drawing(welcome_b).space(&Space::Model), start.space(&Space::Model));
    let mut replica_a = Client::new(start.clone(), 0);
    let mut replica_b = Client::new(start, 0);

    a.send(&ClientMessage::Op { op: line_op("a", "a1", 0.0) }).unwrap();
    let applied_a = next_applied(&mut a, 1);
    let applied_b = next_applied(&mut b, 1);
    assert_eq!(applied_a, applied_b, "both participants see the same change set");
    replica_a.applied(&applied_a).unwrap();
    replica_b.applied(&applied_b).unwrap();

    b.send(&ClientMessage::Op { op: line_op("b", "b1", 5.0) }).unwrap();
    let applied_b = next_applied(&mut b, 2);
    let applied_a = next_applied(&mut a, 2);
    replica_a.applied(&applied_a).unwrap();
    replica_b.applied(&applied_b).unwrap();

    assert_eq!(replica_a.version(), 2);
    assert_eq!(replica_a.drawing().space(&Space::Model), replica_b.drawing().space(&Space::Model));
    assert_eq!(replica_a.drawing().entity_count(), 2);

    // b undoes its own last operation; both replicas follow the authoritative change set.
    b.send(&ClientMessage::Undo { op_id: "undo-b1".into() }).unwrap();
    let undo_b = next_applied(&mut b, 3);
    let undo_a = next_applied(&mut a, 3);
    replica_a.applied(&undo_a).unwrap();
    replica_b.applied(&undo_b).unwrap();
    assert_eq!(replica_a.version(), 3);
    assert_eq!(replica_a.drawing().entity_count(), 1);
    assert_eq!(replica_a.drawing().space(&Space::Model), replica_b.drawing().space(&Space::Model));

    // b redoes the step its undo removed; the redo is a journal entry of its own.
    b.send(&ClientMessage::Redo { op_id: "redo-b1".into() }).unwrap();
    let redo_b = next_applied(&mut b, 4);
    let redo_a = next_applied(&mut a, 4);
    replica_a.applied(&redo_a).unwrap();
    replica_b.applied(&redo_b).unwrap();
    assert_eq!(replica_a.version(), 4);
    assert_eq!(replica_a.drawing().entity_count(), 2);
    assert_eq!(replica_a.drawing().space(&Space::Model), replica_b.drawing().space(&Space::Model));

    // a's own step is now older than the drawing: the server refuses it.
    a.send(&ClientMessage::Undo { op_id: "undo-a1".into() }).unwrap();
    match a.receive().unwrap() {
        ServerMessage::Rejected { rejected } => assert_eq!(rejected.code, "undo-conflict"),
        other => panic!("expected an undo conflict, got {other:?}"),
    }

    let (mut viewer, _welcome_v) = Connection::connect(&url, &hello("token-v"), timeout).unwrap();
    viewer.send(&ClientMessage::Op { op: line_op("v", "v1", 9.0) }).unwrap();
    match viewer.receive().unwrap() {
        ServerMessage::Rejected { rejected } => assert_eq!(rejected.code, "forbidden"),
        other => panic!("a viewer must be refused, got {other:?}"),
    }
}

#[test]
fn a_connection_with_an_unknown_token_is_refused() {
    let room = Room::new(Drawing::new_metric()).unwrap();
    let auth = Arc::new(StaticAuthenticator::default());
    let server = Arc::new(Server::new(room, auth, "s1", "cadcraft"));
    let (addr, _accept_loop) = server.bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{addr}");

    let (_connection, message) = Connection::connect(&url, &hello("nobody"), Duration::from_secs(3)).unwrap();
    match message {
        ServerMessage::Error { code, .. } => assert_eq!(code, "auth"),
        other => panic!("expected an auth error, got {other:?}"),
    }
}

fn next_session_applied(session: &mut cadcraft_collab_client::Session, version: u64) -> Applied {
    use cadcraft_collab_client::Event;
    for _ in 0..16 {
        if let Event::Applied(applied) = session.next_event().expect("an event")
            && applied.version == version
        {
            return applied;
        }
    }
    panic!("no applied answer for version {version}");
}

#[test]
fn a_reconnecting_client_catches_up() {
    use cadcraft_collab_client::Session;

    let room = Room::new(Drawing::new_metric()).unwrap();
    let auth = Arc::new(StaticAuthenticator::new([
        ("token-a".to_string(), Claims { sub: "a".into(), doc: "d1".into(), role: Role::Editor }),
        ("token-b".to_string(), Claims { sub: "b".into(), doc: "d1".into(), role: Role::Editor }),
    ]));
    let server = Arc::new(Server::new(room, auth, "s1", "cadcraft"));
    let (addr, _accept_loop) = server.bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{addr}");
    let timeout = Duration::from_secs(3);

    let mut a = Session::connect(&url, "token-a", "d1", None, timeout).unwrap();
    let mut b = Session::connect(&url, "token-b", "d1", None, timeout).unwrap();

    a.submit(line_op("a", "a1", 0.0)).unwrap();
    let _ = next_session_applied(&mut a, 1);
    let _ = next_session_applied(&mut b, 1);

    // b leaves and misses one operation.
    let replica = b.drawing().clone();
    drop(b);
    a.submit(line_op("a", "a2", 5.0)).unwrap();
    let _ = next_session_applied(&mut a, 2);

    // it comes back with its replica and catches up from the journal.
    let mut b = Session::connect_from(&url, "token-b", "d1", replica, 1, timeout).unwrap();
    assert_eq!(b.version(), 1);
    b.resync(b.version()).unwrap();
    for _ in 0..8 {
        let _ = b.next_event().unwrap();
        if b.version() == 2 {
            break;
        }
    }
    assert_eq!(b.version(), 2);
    assert_eq!(b.drawing().space(&Space::Model), a.drawing().space(&Space::Model));
}

/// A message from `connection` that satisfies `pick`, skipping anything else it receives.
fn next_where<T>(connection: &mut Connection, mut pick: impl FnMut(ServerMessage) -> Option<T>) -> T {
    for _ in 0..32 {
        if let Some(value) = pick(connection.receive().expect("a message")) {
            return value;
        }
    }
    panic!("no expected message");
}

#[test]
fn attachments_are_stored_and_served_by_content_address() {
    let dir = std::env::temp_dir().join(format!(
        "cadcraft-ws-assets-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.subsec_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(cadcraft_collab_server::AssetStore::new(&dir, "s1").unwrap());
    let room = Room::new(Drawing::new_metric()).unwrap().with_assets(Arc::clone(&store));
    let auth = Arc::new(StaticAuthenticator::new([
        ("token-a".to_string(), Claims { sub: "a".into(), doc: "d1".into(), role: Role::Editor }),
        ("token-v".to_string(), Claims { sub: "v".into(), doc: "d1".into(), role: Role::Viewer }),
    ]));
    let server = Arc::new(Server::new(room, auth, "s1", "cadcraft").with_assets(store));
    let (addr, _accept_loop) = server.bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{addr}");
    let timeout = Duration::from_secs(3);

    let (mut a, _welcome_a) = Connection::connect(&url, &hello("token-a"), timeout).unwrap();
    let (mut v, _welcome_v) = Connection::connect(&url, &hello("token-v"), timeout).unwrap();

    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    let bytes = cadcraft_collab::message::decode_bytes(PNG_1X1).unwrap();
    let id = cadcraft_collab::asset_id(&bytes);

    a.send(&ClientMessage::AssetPut { sha256: id.clone(), bytes_base64: PNG_1X1.into() }).unwrap();
    let stored = next_where(&mut a, |message| match message {
        ServerMessage::AssetStored { sha256, bytes, stored } => Some((sha256, bytes, stored)),
        _ => None,
    });
    assert_eq!(stored, (id.clone(), bytes.len() as u64, true));

    // A repeat is deduplicated, not a second file.
    a.send(&ClientMessage::AssetPut { sha256: id.clone(), bytes_base64: PNG_1X1.into() }).unwrap();
    let again = next_where(&mut a, |message| match message {
        ServerMessage::AssetStored { stored, .. } => Some(stored),
        _ => None,
    });
    assert!(!again, "the second put finds the blob");

    // A viewer may read an attachment; the bytes come back unchanged.
    v.send(&ClientMessage::AssetGet { sha256: id.clone() }).unwrap();
    let fetched = next_where(&mut v, |message| match message {
        ServerMessage::Asset { sha256, bytes_base64 } => Some((sha256, bytes_base64)),
        _ => None,
    });
    assert_eq!(fetched.0, id);
    assert_eq!(cadcraft_collab::message::decode_bytes(&fetched.1).unwrap(), bytes);

    // A viewer cannot store one.
    v.send(&ClientMessage::AssetPut { sha256: id.clone(), bytes_base64: PNG_1X1.into() }).unwrap();
    let refused = next_where(&mut v, |message| match message {
        ServerMessage::Error { code, .. } => Some(code),
        _ => None,
    });
    assert_eq!(refused, "forbidden");

    // A foreign address is not found rather than read from somewhere else.
    v.send(&ClientMessage::AssetGet { sha256: "0".repeat(64) }).unwrap();
    let missing = next_where(&mut v, |message| match message {
        ServerMessage::Error { code, .. } => Some(code),
        _ => None,
    });
    assert_eq!(missing, "not-found");

    // The operation refers to the stored bytes by address; both replicas get the same label.
    let mut op = line_op("a", "image", 0.0);
    op.command = "imageattach".into();
    op.params = json!({ "path": "../../etc/passwd.png", "asset": id, "at": [0.0, 0.0], "width": 10.0 });
    a.send(&ClientMessage::Op { op }).unwrap();
    let applied_a = next_applied(&mut a, 1);
    let applied_v = next_applied(&mut v, 1);
    assert_eq!(applied_a, applied_v);
    assert_eq!(server.room().drawing().unwrap().entity_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
