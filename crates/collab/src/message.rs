//! The messages of a collaboration session, as the shared envelope defines them.
//!
//! Names and fields follow `.craft/protocol/ARCHITECTURE.md` §2: `hello`, `welcome`, `op`,
//! `applied`, `rejected`, `presence`, `resync`, `snapshot`, `commit`, `commit-result`, `error`,
//! `ping`/`pong`. `app` and the payload's shape select the application's adapter; this one is
//! `cadcraft`.

use serde::{Deserialize, Serialize};

use crate::{Applied, Op, Presence, Reject};

/// What a participant may do in a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Everything, including closing the session.
    Owner,
    /// The drawing: entities, layers, blocks, dimensions, export.
    Editor,
    /// Presence and the drawing; no operations.
    Viewer,
}

impl Role {
    /// Whether this role may send operations.
    pub fn may_write(self) -> bool {
        !matches!(self, Role::Viewer)
    }

    /// Whether this role may close the session.
    pub fn may_close(self) -> bool {
        matches!(self, Role::Owner)
    }
}

/// Who is connecting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// The client's name (`"cadcraft"`, `"cadcraft-web"`).
    pub name: String,
    /// The client's version, for diagnostics.
    pub ver: String,
}

/// A message from a participant to the server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMessage {
    /// The first message: the token, the drawing and where to resume from.
    Hello {
        /// The application id; a room only accepts its own.
        app: String,
        /// The drawing the session is about.
        doc: String,
        /// The JWT; never in the URL or the logs (ADR-0008).
        jwt: String,
        /// Who is connecting.
        client: ClientInfo,
        /// The version the client shows, for a resume.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_version: Option<u64>,
    },
    /// An operation.
    Op { op: Op },
    /// Undo this participant's last operation, if nothing changed since.
    Undo { op_id: String },
    /// Redo the operation this participant last undid, if nothing changed since.
    Redo { op_id: String },
    /// Where the participant is looking; never journaled.
    Presence { presence: Presence },
    /// Ask for the journal tail after `last_version`, or a snapshot when it is gone.
    Resync { last_version: u64 },
    /// Ask for the file to be written (ADR-0009).
    Commit { expected_version: u64 },
    /// Ask for the file a commit the cabinet refused with `409` built (TASK §6).
    ///
    /// The room keeps the refused file until the next accepted commit; the answer is
    /// [`ServerMessage::SaveCopyResult`]. The cabinet is never written by this request: the owner
    /// saves the room's work under a path of their own.
    SaveCopy { op_id: String },
    /// Store the bytes of an attachment under their content address (ADR 0021).
    ///
    /// The room checks the address against the bytes and its limits before anything is written;
    /// an attachment is never read by a name a client chose.
    AssetPut {
        /// SHA-256 of `bytes_base64`, lower-case hexadecimal.
        sha256: String,
        /// The attachment's bytes, base64 for JSON.
        bytes_base64: String,
    },
    /// Fetch the bytes of an attachment the drawing refers to (ADR 0021).
    AssetGet {
        /// SHA-256 of the bytes, lower-case hexadecimal.
        sha256: String,
    },
    /// Heartbeat; also renews the lease.
    Ping,
}

/// A message from the server to a participant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMessage {
    /// The session is open.
    Welcome {
        /// The room's id.
        session_id: String,
        /// The participant's id (the token's `sub`).
        actor_id: String,
        /// The participant's role.
        role: Role,
        /// The drawing's version.
        version: u64,
        /// The other participants' latest presence.
        participants: Vec<Presence>,
        /// The drawing in parts, when the client's version is too old for the journal; empty when
        /// the client is in step.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        snapshot: Vec<Snapshot>,
    },
    /// An operation was applied.
    Applied { applied: Applied },
    /// An operation was not applied.
    Rejected { rejected: Reject },
    /// Another participant's view.
    Presence { presence: Presence },
    /// The journal tail after `last_version`.
    Delta { version: u64, more: bool, applied: Vec<Applied> },
    /// The drawing at a version, when the journal does not reach back.
    Snapshot { snapshot: Snapshot },
    /// The file a commit produced (ADR-0009).
    CommitResult {
        /// The drawing's version the file was written from.
        version: u64,
        /// What the format could not keep.
        not_saved: std::collections::BTreeMap<String, usize>,
        /// The file, base64 for JSON.
        bytes_base64: String,
        /// The cabinet's answer when the room publishes the commit itself (M87). With a cabinet
        /// the file is already stored there; the client must not write it anywhere else.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cabinet: Option<CabinetCommit>,
    },
    /// The cabinet stored the room's commit (M87).
    ///
    /// This answer carries no bytes: the cabinet owns the file now, and every participant sees
    /// that it was stored.
    Committed {
        /// The drawing's version the file was written from.
        version: u64,
        /// What the format could not keep.
        not_saved: std::collections::BTreeMap<String, usize>,
        /// The cabinet's version and etag.
        cabinet: CabinetCommit,
    },
    /// The cabinet refused the commit with `409`: the file changed outside the session.
    ///
    /// The room built the file and keeps it for [`ClientMessage::SaveCopy`]; the session does not
    /// overwrite the cabinet's newer version.
    CommitConflict {
        /// The drawing's version the file was written from.
        version: u64,
        /// Why the cabinet refused it.
        message: String,
    },
    /// The file of a refused commit, for the owner to save as a copy (TASK §6).
    SaveCopyResult {
        /// The drawing's version the file was written from.
        version: u64,
        /// What the format could not keep.
        not_saved: std::collections::BTreeMap<String, usize>,
        /// The file, base64 for JSON.
        bytes_base64: String,
    },
    /// An attachment is in the room's store; it was already there when `stored` is false.
    AssetStored {
        /// The content address.
        sha256: String,
        /// Its size in bytes.
        bytes: u64,
        /// Whether this call wrote it (deduplication keeps one copy per address).
        stored: bool,
    },
    /// The bytes of an attachment the room stores.
    Asset {
        /// The content address.
        sha256: String,
        /// The bytes, base64 for JSON.
        bytes_base64: String,
    },
    /// Something went wrong outside an operation (auth, lease, limits).
    Error { code: String, message: String, retryable: bool },
    /// Heartbeat answer.
    Pong,
}

/// The cabinet's answer to a commit it stored (M87, `INTEGRATION.md` §5.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CabinetCommit {
    /// The cabinet's new file version.
    pub version: u64,
    /// The cabinet's etag for that version; the next commit sends it as `expectedVersion`.
    pub etag: String,
}

/// A drawing at one version, as the server sends it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// The drawing's version.
    pub version: u64,
    /// Part number: a snapshot is sent in parts so one message stays small.
    pub seq: u32,
    /// Whether another part follows.
    pub more: bool,
    /// This part of the DXF bytes, base64 for JSON.
    pub bytes_base64: String,
}

impl Snapshot {
    /// The largest part of a snapshot, before base64.
    pub const CHUNK_BYTES: usize = 512 * 1024;
}

impl Snapshot {
    /// Split `bytes` into parts small enough for one message, base64-encoded.
    pub fn encode(version: u64, bytes: &[u8]) -> Vec<Snapshot> {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::STANDARD;
        let chunks: Vec<&[u8]> = if bytes.is_empty() { vec![&[]] } else { bytes.chunks(Snapshot::CHUNK_BYTES).collect() };
        let last = chunks.len().saturating_sub(1);
        chunks
            .into_iter()
            .enumerate()
            .map(|(seq, chunk)| Snapshot { version, seq: seq as u32, more: seq < last, bytes_base64: engine.encode(chunk) })
            .collect()
    }

    /// Join the parts of a snapshot back into the drawing bytes.
    pub fn decode(parts: &[Snapshot]) -> Result<Vec<u8>, String> {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::STANDARD;
        let mut parts = parts.to_vec();
        parts.sort_by_key(|part| part.seq);
        let mut bytes = Vec::new();
        for (expected, part) in parts.iter().enumerate() {
            if part.seq as usize != expected {
                return Err(format!("snapshot part {expected} is missing"));
            }
            bytes.extend(engine.decode(&part.bytes_base64).map_err(|e| e.to_string())?);
        }
        Ok(bytes)
    }
}

/// The base64 of `bytes`, as `snapshot.bytesBase64` and `commitResult.bytesBase64` carry it.
pub fn encode_bytes(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The bytes of a base64 field.
pub fn decode_bytes(text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn a_snapshot_is_split_into_parts_and_joins_back() {
        let bytes: Vec<u8> = (0..(Snapshot::CHUNK_BYTES + 100)).map(|i| (i % 251) as u8).collect();
        let parts = Snapshot::encode(7, &bytes);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].seq, 0);
        assert!(parts[0].more);
        assert!(!parts[1].more);
        assert_eq!(Snapshot::decode(&parts).unwrap(), bytes);
    }

    #[test]
    fn a_missing_snapshot_part_is_an_error() {
        let bytes = vec![1u8; Snapshot::CHUNK_BYTES + 1];
        let mut parts = Snapshot::encode(1, &bytes);
        parts.remove(0);
        assert!(Snapshot::decode(&parts).is_err());
    }

    #[test]
    fn a_message_round_trips_through_json() {
        let message = ClientMessage::Hello {
            app: "cadcraft".into(),
            doc: "file-1".into(),
            jwt: "token".into(),
            client: ClientInfo { name: "cadcraft".into(), ver: "0.17.0".into() },
            last_version: Some(3),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("\"type\":\"hello\""));
        let decoded: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn an_applied_message_carries_the_change_set() {
        let message = ServerMessage::Applied { applied: Applied { op_id: "a".into(), version: 1, actor: "u".into(), changes: vec![] } };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], json!("applied"));
        assert_eq!(json["applied"]["version"], json!(1));
    }

    #[test]
    fn an_attachment_put_and_its_answers_round_trip() {
        let put = ClientMessage::AssetPut { sha256: "00".repeat(32), bytes_base64: "aGVsbG8=".into() };
        let json = serde_json::to_value(&put).unwrap();
        assert_eq!(json["type"], json!("assetPut"));
        assert_eq!(serde_json::from_value::<ClientMessage>(json).unwrap(), put);

        let get = ClientMessage::AssetGet { sha256: "00".repeat(32) };
        let json = serde_json::to_value(&get).unwrap();
        assert_eq!(json["type"], json!("assetGet"));
        assert_eq!(serde_json::from_value::<ClientMessage>(json).unwrap(), get);

        let stored = ServerMessage::AssetStored { sha256: "00".repeat(32), bytes: 5, stored: true };
        let json = serde_json::to_value(&stored).unwrap();
        assert_eq!(json["type"], json!("assetStored"));
        assert_eq!(serde_json::from_value::<ServerMessage>(json).unwrap(), stored);

        let asset = ServerMessage::Asset { sha256: "00".repeat(32), bytes_base64: "aGVsbG8=".into() };
        let json = serde_json::to_value(&asset).unwrap();
        assert_eq!(json["type"], json!("asset"));
        assert_eq!(serde_json::from_value::<ServerMessage>(json).unwrap(), asset);
    }

    #[test]
    fn a_cabinet_commit_and_its_refusal_round_trip() {
        let committed = ServerMessage::Committed {
            version: 4,
            not_saved: BTreeMap::from([("SAT".to_string(), 2)]),
            cabinet: CabinetCommit { version: 9, etag: "\"e9\"".into() },
        };
        let json = serde_json::to_value(&committed).unwrap();
        assert_eq!(json["type"], json!("committed"));
        assert_eq!(json["cabinet"]["version"], json!(9));
        assert_eq!(serde_json::from_value::<ServerMessage>(json).unwrap(), committed);

        let conflict = ServerMessage::CommitConflict { version: 4, message: "the file changed outside the session".into() };
        let json = serde_json::to_value(&conflict).unwrap();
        assert_eq!(json["type"], json!("commitConflict"));
        assert_eq!(serde_json::from_value::<ServerMessage>(json).unwrap(), conflict);

        let copy = ClientMessage::SaveCopy { op_id: "c1".into() };
        let json = serde_json::to_value(&copy).unwrap();
        assert_eq!(json["type"], json!("saveCopy"));
        assert_eq!(serde_json::from_value::<ClientMessage>(json).unwrap(), copy);

        let result = ServerMessage::SaveCopyResult { version: 4, not_saved: BTreeMap::new(), bytes_base64: "aGVsbG8=".into() };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["type"], json!("saveCopyResult"));
        assert_eq!(serde_json::from_value::<ServerMessage>(json).unwrap(), result);
    }

    #[test]
    fn an_old_commit_result_without_a_cabinet_still_parses() {
        let old = serde_json::json!({
            "type": "commitResult",
            "version": 2,
            "not_saved": {},
            "bytes_base64": "aGVsbG8="
        });
        let message: ServerMessage = serde_json::from_value(old).unwrap();
        assert!(matches!(message, ServerMessage::CommitResult { cabinet: None, .. }));
    }
}
