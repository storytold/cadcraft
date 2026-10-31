//! The host on the layer against a scripted FANTASTICO: the connection and its key, agreement,
//! a whole conversation through the layer, model choice, and an older CADCraft's key file.

use std::io::Cursor;
use std::sync::Mutex;

use craft_ai_core::SecretString;
use craft_ai_fantastico::{FantasticoConfig, FantasticoProvider, Wire, WireResponse};
use craft_ai_native::secrets::MemoryStore;
use fantastico_sdk::{HttpRequest, Method};
use serde_json::json;

use super::*;

const BASE: &str = "https://fantastico.cc/api";
/// The model call, wherever the SDK sends it.
const CHAT: &str = "/chat/completions";
const MODEL: &str = "anthropic/claude-sonnet-5";

/// The account service and the provider behind it, scripted. Every chat body is kept.
#[derive(Default)]
struct Fake {
    bodies: Mutex<Vec<Value>>,
}

fn reply(content_type: &str, body: String) -> WireResponse {
    WireResponse { status: 200, headers: vec![("Content-Type".into(), content_type.into())], body: Box::new(Cursor::new(body)) }
}

impl Wire for Fake {
    fn send(&self, req: &HttpRequest) -> Result<WireResponse, String> {
        if req.url == format!("{BASE}/edge/key") && req.method == Method::Post {
            return Ok(reply(
                "application/json",
                json!({"runtimeKey": "rk-runtime", "expiresAt": "2099-01-01T00:00:00.000Z", "recordId": "r", "baseUrl": "https://models.example/v1"})
                    .to_string(),
            ));
        }
        if req.url == format!("{BASE}/edge/models") {
            let models = json!({"models": [
                {"id": MODEL, "name": "Claude Sonnet 5", "pricing": {"prompt": "0.000003", "completion": "0.000015"}},
                {"id": "openai/gpt-6", "name": "GPT-6"},
                {"id": "flux/image", "architecture": {"output_modalities": ["image"]}}
            ]});
            return Ok(reply("application/json", models.to_string()));
        }
        if req.url.starts_with(BASE) || !req.url.ends_with(CHAT) {
            return Err(format!("unexpected request to {}", req.url));
        }
        let body: Value = serde_json::from_str(req.body.as_deref().unwrap_or("{}")).map_err(|e| e.to_string())?;
        self.bodies.lock().unwrap().push(body);
        let whole = json!({"choices": [{"message": {"role": "assistant", "content": "Disegno il cerchio.", "tool_calls": [
            {"id": "call_7", "type": "function", "function": {"name": "run_command", "arguments": "{\"text\":\"circle 0,0 5\"}"}}]},
            "finish_reason": "tool_calls"}]});
        Ok(reply("application/json", whole.to_string()))
    }
}

/// The adapter on the scripted wire, built the way `craft_ai_fantastico::build` builds it.
fn factory(base: ProviderFactory, fake: &Arc<Fake>) -> ProviderFactory {
    let mut f = base;
    let fake = fake.clone();
    f.register(craft_ai_fantastico::PROVIDER_NAME, move |connection, key| {
        let key = key.ok_or_else(|| AiError::new(ErrorKind::Auth, "no key"))?;
        let mut config = FantasticoConfig::new(key);
        if !connection.models.is_empty() {
            config = config.with_models(connection.models.clone());
        }
        Ok(Arc::new(FantasticoProvider::with_wire(config, fake.clone() as Arc<dyn Wire>)?) as Arc<dyn craft_ai_core::Provider>)
    });
    f
}

struct Setup {
    host: LayerHost,
    fake: Arc<Fake>,
    secrets: Arc<MemoryStore>,
    dir: PathBuf,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn setup(name: &str) -> Setup {
    let dir = std::env::temp_dir().join(format!("cadcraft-layer-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fake = Arc::new(Fake::default());
    let secrets = Arc::new(MemoryStore::new());
    let layer = Layer::new(
        AppId::new(APP).unwrap(),
        SharedStore::new(dir.join("connections.json")),
        secrets.clone(),
        factory(ProviderFactory::standard(), &fake),
        factory(ProviderFactory::probing(), &fake),
        Some(ConsentFile::new(dir.join("consent.json"))),
    )
    .unwrap();
    Setup { host: LayerHost::with_layer(layer, &dir), fake, secrets, dir }
}

/// A connected account bound to `MODEL`, as `connect` leaves it after FANTASTICO checked the key.
fn connected(s: &Setup) -> ConnectionId {
    let id = s.host.layer.save_key("ck-fant-test-key", None).unwrap();
    s.host.layer.bind(&id, MODEL).unwrap();
    s.host.forget_has_key();
    id
}

fn conversation(model: &str) -> Value {
    json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are CADCraft's AI Assistant."},
            {"role": "user", "content": "disegna un cerchio"},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "drawing_info", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "call_1", "content": "(empty drawing)"}
        ],
        "tools": [{"type": "function", "function": {"name": "run_command", "parameters": {"type": "object"}}}]
    })
}

#[test]
fn the_key_goes_to_the_keychain_and_the_connection_to_the_shared_store() {
    let s = setup("store");
    assert!(!s.host.has_key());
    let id = connected(&s);
    assert!(s.host.has_key());
    let c = s.host.layer.connection().unwrap().unwrap();
    assert_eq!((c.id.clone(), c.provider.as_str(), c.label.as_str()), (id, "fantastico", LABEL));
    // The account's chat models, the image model left out.
    let ids: Vec<_> = c.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, [MODEL, "openai/gpt-6"]);
    // The key is in the secret store only: not in the store file, not readable through the host.
    let store = std::fs::read_to_string(s.dir.join("connections.json")).unwrap();
    assert!(!store.contains("ck-fant-test-key"));
    assert_eq!(s.host.layer.key(&c).unwrap(), "ck-fant-test-key");
    // The settings window's list, with the price per million output tokens.
    let models = s.host.models().unwrap();
    let sonnet = models.iter().find(|m| m.id == MODEL).unwrap();
    assert_eq!(sonnet.name.as_deref(), Some("Claude Sonnet 5"));
    assert_eq!(sonnet.output_usd_per_million, Some(15.0));
}

#[test]
fn agreement_is_asked_once_and_kept() {
    let s = setup("consent");
    assert_eq!(s.host.consent(), None, "nothing to agree to without a connection");
    connected(&s);
    let asked = s.host.consent().unwrap();
    assert_eq!(asked.service, "FANTASTICO");
    assert!(asked.destination.contains("fantastico.cc"), "{}", asked.destination);
    // Without it nothing is sent.
    let mut provider = LayerProvider { layer: s.host.layer.clone() };
    let refused = provider.complete(&conversation(MODEL)).unwrap_err();
    assert!(refused.contains("agreement"), "{refused}");
    assert!(s.fake.bodies.lock().unwrap().is_empty());
    s.host.agree().unwrap();
    assert_eq!(s.host.consent(), None);
    assert!(s.dir.join("consent.json").is_file());
}

#[test]
fn a_conversation_goes_through_the_layer() {
    let s = setup("conversation");
    connected(&s);
    s.host.agree().unwrap();
    let mut provider = LayerProvider { layer: s.host.layer.clone() };
    let reply = provider.complete(&conversation(MODEL)).unwrap();
    // The agent reads the reply as it would from FANTASTICO directly, the call's id included.
    let message = &reply["choices"][0]["message"];
    assert_eq!(message["content"], "Disegno il cerchio.");
    assert_eq!(message["tool_calls"][0]["id"], "call_7");
    assert_eq!(reply["choices"][0]["finish_reason"], "tool_calls");
    // The provider got the whole conversation, in order, with the tools.
    let bodies = s.fake.bodies.lock().unwrap();
    let sent = &bodies[0];
    let roles: Vec<_> = sent["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool"]);
    assert_eq!(sent["messages"][3]["tool_call_id"], "call_1");
    assert_eq!(sent["model"], MODEL);
    assert_eq!(sent["tools"][0]["function"]["name"], "run_command");
}

#[test]
fn the_requested_model_is_the_one_bound_and_an_unknown_one_is_refused() {
    let s = setup("models");
    let id = connected(&s);
    s.host.agree().unwrap();
    let mut provider = LayerProvider { layer: s.host.layer.clone() };
    provider.complete(&conversation("openai/gpt-6")).unwrap();
    assert_eq!(s.host.layer.bound(), Some((id, ModelId::new("openai/gpt-6").unwrap())));
    assert_eq!(s.fake.bodies.lock().unwrap()[0]["model"], "openai/gpt-6");
    let e = provider.complete(&conversation("nobody/nothing")).unwrap_err();
    assert!(e.contains("does not offer"), "{e}");
    assert!(provider.complete(&json!({"model": MODEL})).is_err(), "no messages");
}

#[test]
fn an_older_cadcrafts_key_file_moves_into_the_keychain() {
    let s = setup("legacy");
    let file = s.dir.join("assistant").join("fantastico.key");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "  ck-fant-old-key \n").unwrap();
    assert!(s.host.has_key(), "the old key counts before it moves");
    let c = s.host.connection().unwrap();
    assert_eq!(s.host.layer.key(&c).unwrap(), "ck-fant-old-key");
    assert!(!file.exists(), "the plain-text file is gone");
    // A second call finds the connection and leaves things as they are.
    assert_eq!(s.host.connection().unwrap().id, c.id);
}

/// An older key file never outlives a connection, and never blocks a checked new key.
#[test]
fn an_older_key_file_goes_once_a_connection_exists_and_does_not_block_a_new_key() {
    let s = setup("legacy-stale");
    let file = s.dir.join("assistant").join("fantastico.key");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "ck-fant-old-key").unwrap();
    // With only the old file there, connect goes on to check the new key (and refuses this one
    // for its shape, before any request), instead of saying a key is already set.
    let e = s.host.connect("sk-other-abc".into()).unwrap_err();
    assert!(!e.contains("already set"), "{e}");
    // A connection made elsewhere: the old file is an unused plain-text copy and goes.
    connected(&s);
    let c = s.host.connection().unwrap();
    assert!(!file.exists());
    assert_eq!(s.host.layer.key(&c).unwrap(), "ck-fant-test-key", "the stored key is kept");
}

#[test]
fn connect_refuses_a_second_key_and_a_wrong_one_before_any_request() {
    let s = setup("connect");
    assert!(s.host.connect("sk-other-abc".into()).is_err());
    assert!(s.host.change_key("sk-other-abc".into()).is_err());
    assert!(s.host.layer.connection().unwrap().is_none());
    connected(&s);
    assert!(s.host.connect("ck-fant-another".into()).unwrap_err().contains("already set"));
    assert_eq!(s.host.service().key_prefix, "ck-fant-");
}

/// The tools the assistant sends ride in the request's parameters, which the layer caps at 64 KiB.
#[test]
fn the_assistants_tools_fit_the_layers_parameter_cap() {
    let tools = cadcraft_assistant::Agent::tools(true);
    let size = serde_json::to_vec(&json!({"tools": tools})).unwrap().len();
    assert!(size < craft_ai_core::request::MAX_PARAMS_BYTES / 2, "{size} bytes");
}

#[test]
fn a_secret_string_is_what_the_store_hands_back() {
    // `key` reads through the layered store's interface, not a file of ours.
    let s = setup("secret");
    connected(&s);
    let c = s.host.layer.connection().unwrap().unwrap();
    let reference = c.auth.secret_ref().unwrap();
    assert_eq!(s.secrets.get(reference).unwrap().map(|k: SecretString| k.expose().to_string()).as_deref(), Some("ck-fant-test-key"));
}

/// The palette shows these in its own words, translated; the layer's text would be English only.
#[test]
fn quota_network_and_consent_failures_read_as_the_palette_says_them() {
    let quota = call_error(&AiError::new(ErrorKind::Quota, "402 from the provider"));
    assert!(quota.contains("credit"), "{quota}");
    let network = call_error(&AiError::new(ErrorKind::Network, "dns"));
    assert!(network.contains("cannot be reached"), "{network}");
    assert_eq!(call_error(&AiError::new(ErrorKind::Provider, "model down")), "model down");
}
