//! The AI Assistant's host on the desktop, on the Craft AI layer.
//!
//! The FANTASTICO account is a connection in the layer's shared store, the one every Craft app and
//! the launcher read: an account connected here works in the others, and one connected there
//! works here. The key lives in the system keychain (`craft-ai-native`), never in a file of ours,
//! and stays write-only for the interface: [`AssistantHost::connect`] and
//! [`AssistantHost::change_key`] store one after FANTASTICO has checked it, and only this file
//! reads it back, to read the account's credit. Requests go through the layer
//! (`craft-ai-client`), which checks the user's agreement before anything is sent and records
//! each job; the `craft-ai-fantastico` adapter carries the assistant's conversation.
//!
//! A key stored by an older CADCraft in `<settings dir>/assistant/fantastico.key` moves into the
//! keychain the first time the assistant needs it, and the file is then removed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cadcraft_assistant::Provider;
use cadcraft_ui_egui::assistant::{Account, AssistantEvent, AssistantHost, Consent, ModelInfo, Service};
use craft_ai_client::{AiClient, ConnectionManager, ProviderFactory};
use craft_ai_core::job::{Event, JobState};
use craft_ai_core::{
    AiError, AppId, Billing, CapabilityId, Connection, ConnectionDraft, ConnectionId, ConsentLedger, ErrorKind, InputRole, ModelId, Payload, Request,
    Role, RoleId,
};
use craft_ai_native::secrets::{EnvStore, KeychainStore, LayeredStore, SecretStore, secret_for};
use craft_ai_native::shared::SharedStore;
use craft_ai_native::{ConsentFile, HistoryFile};
use fantastico_sdk::{Fantastico, Options};
use serde_json::Value;

/// The app, as the layer's store and consent records name it.
const APP: &str = "cadcraft";
/// The assistant's tool in the layer: which connection and model it runs on.
const ROLE: &str = "cadcraft-assistant";
/// The account service a new connection points at.
const ENDPOINT: &str = "https://fantastico.cc/api";
/// The name a new connection gets, in the launcher and every app.
const LABEL: &str = "FANTASTICO";
/// The account service's site for CADCraft: where a key comes from and the credit is topped up.
const SITE: &str = "https://artcraft.fantastico.cc";
/// The largest key file of an older CADCraft read back (a key is under a hundred bytes).
const MAX_KEY_BYTES: u64 = 4096;
/// How often a running reply is polled.
const POLL: Duration = Duration::from_millis(25);
/// The longest one model call may take before it is cancelled.
const CALL_TIMEOUT: Duration = Duration::from_secs(600);
/// How long `has_key`'s answer is reused: the palette asks every frame.
const HAS_KEY_TTL: Duration = Duration::from_secs(2);

fn options() -> Options {
    Options { title: "CADCraft".into(), referer: "https://github.com/storytold/cadcraft".into(), ..Options::default() }
}

fn chat_role() -> Result<Role, AiError> {
    let invalid = |e: craft_ai_core::ids::IdError| AiError::new(ErrorKind::Internal, e.to_string());
    Ok(Role {
        id: RoleId::new(ROLE).map_err(invalid)?,
        label: "AI Assistant".into(),
        capability: CapabilityId::parse("text.complete").map_err(|e| AiError::new(ErrorKind::Internal, e.to_string()))?,
        inputs: vec![InputRole::parse(craft_ai_fantastico::TURN).map_err(invalid)?],
    })
}

/// The adapter registered under its name, naming CADCraft in the provider's usage reports.
fn with_fantastico(mut factory: ProviderFactory) -> ProviderFactory {
    factory.register(craft_ai_fantastico::PROVIDER_NAME, craft_ai_fantastico::build_titled("CADCraft"));
    factory
}

/// A failed model call in the words the palette shows (and translates).
fn call_error(e: &AiError) -> String {
    match e.kind {
        ErrorKind::Quota => "The credit on your FANTASTICO account has run out: top it up to continue".into(),
        ErrorKind::Network => "FANTASTICO cannot be reached: check the internet connection".into(),
        ErrorKind::ConsentRequired => "Nothing was sent: the AI Assistant needs your agreement first.".into(),
        _ => e.message.clone(),
    }
}

/// Everything the host and its conversations share.
struct Layer {
    client: AiClient,
    manager: ConnectionManager,
    shared: SharedStore,
    secrets: Arc<dyn SecretStore>,
    /// Where agreements are kept; in memory for this run when there is no settings folder.
    consent_file: Option<ConsentFile>,
    ledger: Mutex<ConsentLedger>,
    role: Role,
}

impl Layer {
    fn new(
        app: AppId,
        shared: SharedStore,
        secrets: Arc<dyn SecretStore>,
        factory: ProviderFactory,
        probes: ProviderFactory,
        consent_file: Option<ConsentFile>,
    ) -> Result<Self, AiError> {
        let client = AiClient::new(app, shared.clone(), secrets.clone()).with_factory(factory);
        let manager = ConnectionManager::new(shared.clone(), secrets.clone()).with_probe_factory(probes);
        Ok(Self { client, manager, shared, secrets, consent_file, ledger: Mutex::default(), role: chat_role()? })
    }

    /// The FANTASTICO connection the assistant uses: the one its tool is bound to, else the first.
    fn connection(&self) -> Result<Option<Connection>, String> {
        let store = self.shared.load().map_err(|e| AiError::from(e).message)?;
        if let Ok(r) = store.resolve(self.client.app(), &self.role)
            && r.connection.provider == craft_ai_fantastico::PROVIDER_NAME
        {
            return Ok(Some(r.connection.clone()));
        }
        Ok(store.connections.into_iter().find(|c| c.enabled && c.provider == craft_ai_fantastico::PROVIDER_NAME))
    }

    /// The model the tool is bound to now, when the binding works.
    fn bound(&self) -> Option<(ConnectionId, ModelId)> {
        let store = self.shared.load().ok()?;
        let r = store.resolve(self.client.app(), &self.role).ok()?;
        Some((r.connection.id.clone(), r.model.id.clone()))
    }

    fn key(&self, connection: &Connection) -> Result<String, String> {
        match secret_for(self.secrets.as_ref(), connection).map_err(|e| e.message)? {
            Some(key) => Ok(key.expose().to_string()),
            None => Err("No account key is set".into()),
        }
    }

    fn account_client(&self, connection: &Connection) -> Result<Fantastico, String> {
        Fantastico::new(self.key(connection)?, options()).map_err(|e| e.message)
    }

    /// Store `key` on a new connection, or on `existing`, with the models the account offers.
    fn save_key(&self, key: &str, existing: Option<&Connection>) -> Result<ConnectionId, String> {
        let mut draft = match existing {
            Some(c) => ConnectionDraft::from_connection(c),
            None => {
                let mut d = ConnectionDraft::default();
                d.label = LABEL.into();
                d.provider = craft_ai_fantastico::PROVIDER_NAME.into();
                d.endpoint = ENDPOINT.into();
                d.billing = Billing::Metered;
                d.uses_key = true;
                d.enabled = true;
                d
            }
        };
        draft.key = key.to_string();
        let report = self.manager.test_draft(&draft).map_err(|e| e.message)?;
        draft.set_models(report.models);
        let saved = self.manager.save(&draft).map_err(|e| e.to_string())?;
        if let Some(w) = saved.warning {
            log::warn!("AI Assistant: {w}");
        }
        Ok(saved.id)
    }

    /// Run the tool on `model`. A model the saved list doesn't have yet is looked for in the
    /// account's current list once.
    fn bind(&self, connection: &ConnectionId, model: &str) -> Result<(), String> {
        let id = ModelId::new(model).map_err(|_| format!("{model} is not a model name"))?;
        if self.client.bind(&self.role, connection, &id).is_ok() {
            return Ok(());
        }
        self.manager.test_saved(connection, true).map_err(|e| e.message)?;
        self.client.bind(&self.role, connection, &id).map_err(|_| format!("The account does not offer the model {model}"))
    }

    /// The tool bound to `model` on the assistant's connection, unless it already is.
    fn ensure_bound(&self, model: &str) -> Result<(), String> {
        let connection = self.connection()?.ok_or("No account key is set")?;
        if self.bound().is_some_and(|(c, m)| c == connection.id && m.as_str() == model) {
            return Ok(());
        }
        self.bind(&connection.id, model)
    }

    /// The tool bound to something: the model it has, else the account's suggestion, else the
    /// first model of the connection.
    fn ensure_some_binding(&self) -> Result<(), String> {
        if self.bound().is_some() {
            return Ok(());
        }
        let connection = self.connection()?.ok_or("No account key is set")?;
        let suggested = self.account_client(&connection).ok().and_then(|c| c.config().ok()).and_then(|c| c.model);
        let first = connection.models.first().map(|m| m.id.as_str().to_string());
        let model = suggested.or(first).ok_or("The account offers no models")?;
        self.bind(&connection.id, &model)
    }

    fn load_ledger(&self) -> ConsentLedger {
        match &self.consent_file {
            Some(f) => f.load_or_empty(),
            None => self.ledger.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        }
    }

    fn save_ledger(&self, ledger: &ConsentLedger) -> Result<(), String> {
        match &self.consent_file {
            Some(f) => f.save(ledger).map_err(|e| e.message),
            None => {
                *self.ledger.lock().unwrap_or_else(PoisonError::into_inner) = ledger.clone();
                Ok(())
            }
        }
    }

    /// A request of the kind every turn is (text turns, tool definitions), for the agreement:
    /// agreeing to it covers them all.
    fn sample_plan(&self) -> Result<craft_ai_client::Plan, String> {
        let turn = InputRole::parse(craft_ai_fantastico::TURN).map_err(|e| e.to_string())?;
        let request = Request::new(self.role.capability.clone())
            .with_input(turn, Payload::text(r#"{"role":"user","content":""}"#))
            .with_param("tools", Value::Array(Vec::new()));
        self.client.plan(&self.role, request).map_err(|e| e.message)
    }

    /// One model call: the conversation in, the whole reply out.
    fn call(&self, request: &Value) -> Result<Value, String> {
        let model = request.get("model").and_then(Value::as_str).filter(|m| !m.trim().is_empty()).ok_or("The request names no model")?;
        self.ensure_bound(model.trim())?;
        let messages = request.get("messages").and_then(Value::as_array).ok_or("The request has no messages")?;
        let turn = InputRole::parse(craft_ai_fantastico::TURN).map_err(|e| e.to_string())?;
        let mut req = Request::new(self.role.capability.clone());
        for m in messages {
            req = req.with_input(turn.clone(), Payload::text(m.to_string()));
        }
        if let Some(tools) = request.get("tools") {
            req = req.with_param("tools", tools.clone());
        }
        let plan = self.client.plan(&self.role, req).map_err(|e| e.message)?;
        let job = self.client.start(&plan, &self.load_ledger()).map_err(|e| call_error(&e))?;
        let deadline = Instant::now() + CALL_TIMEOUT;
        let mut reply = None;
        loop {
            let snap = job.poll().map_err(|e| call_error(&e))?;
            for event in snap.events {
                if let Event::Output(o) = event
                    && o.label.as_deref() == Some(craft_ai_fantastico::COMPLETION)
                    && let Payload::Inline { data, .. } = o.payload
                {
                    reply = serde_json::from_slice::<Value>(&data).ok();
                }
            }
            match snap.state {
                JobState::Succeeded => break,
                JobState::Failed { error } => return Err(call_error(&error)),
                JobState::Cancelled => return Err("The request was cancelled".into()),
                JobState::Queued | JobState::Running { .. } => {}
            }
            if Instant::now() > deadline {
                let _ = job.cancel();
                return Err("FANTASTICO did not answer in time".into());
            }
            std::thread::sleep(POLL);
        }
        reply.ok_or_else(|| "FANTASTICO sent an incomplete reply".to_string())
    }
}

/// A conversation's model endpoint: each call is one job on the layer.
struct LayerProvider {
    layer: Arc<Layer>,
}

impl Provider for LayerProvider {
    fn complete(&mut self, request: &Value) -> Result<Value, String> {
        self.layer.call(request)
    }
}

pub struct LayerHost {
    layer: Arc<Layer>,
    /// The key file of an older CADCraft, moved into the keychain when found.
    legacy: PathBuf,
    has_key: Mutex<Option<(Instant, bool)>>,
}

impl LayerHost {
    /// The host on the layer's shared store and the system keychain.
    pub fn new(settings_dir: &Path) -> Result<Self, String> {
        let app = AppId::new(APP).map_err(|e| e.to_string())?;
        let shared = SharedStore::at_default_location().map_err(|e| AiError::from(e).message)?;
        let secrets: Arc<dyn SecretStore> = Arc::new(LayeredStore::new(vec![Box::new(EnvStore::new()), Box::new(KeychainStore::new())]));
        let consent_file = ConsentFile::at_default_location(&app);
        let mut layer =
            Layer::new(app, shared, secrets, with_fantastico(ProviderFactory::standard()), with_fantastico(ProviderFactory::probing()), consent_file)
                .map_err(|e| e.message)?;
        if let Some(history) = HistoryFile::at_default_location() {
            layer.client = layer.client.with_history(history);
        }
        Ok(Self::with_layer(layer, settings_dir))
    }

    fn with_layer(layer: Layer, settings_dir: &Path) -> Self {
        Self { layer: Arc::new(layer), legacy: settings_dir.join("assistant").join("fantastico.key"), has_key: Mutex::new(None) }
    }

    fn forget_has_key(&self) {
        *self.has_key.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Check `key` with the account service before anything is stored: a mistyped key is never
    /// kept. Returns the trimmed key and its account.
    fn verify(key: &str) -> Result<(String, Account), String> {
        let key = key.trim().to_string();
        if !key.starts_with("ck-fant-") {
            return Err("This is not an account key: an account key starts with ck-fant-".into());
        }
        let client = Fantastico::new(key.clone(), options()).map_err(|e| e.message)?;
        let config = client.config().map_err(|e| match e.code.as_str() {
            "INVALID_CONTO_KEY" => "FANTASTICO does not recognise this key".to_string(),
            "NETWORK_ERROR" => "FANTASTICO cannot be reached: check the internet connection".to_string(),
            _ => e.message,
        })?;
        Ok((key, Self::account_of(config)))
    }

    fn account_of(config: fantastico_sdk::EdgeConfig) -> Account {
        Account { conto_id: config.account_id, model: config.model.unwrap_or_default(), remaining_usd: config.balance.as_ref().map(Self::credit) }
    }

    /// The credit as the user should see it: what is left including the unspent reservations
    /// of live keys (`totalRemaining`, worked out by the account service), so a block set aside
    /// by a key doesn't read as spent. An older server only says what is spendable right now.
    fn credit(b: &fantastico_sdk::Balance) -> f64 {
        b.total_remaining_usd.filter(|t| t.is_finite()).unwrap_or(b.remaining_usd)
    }

    fn read_legacy(&self) -> Option<String> {
        let size = std::fs::metadata(&self.legacy).ok()?.len();
        if size > MAX_KEY_BYTES {
            return None;
        }
        let key = std::fs::read_to_string(&self.legacy).ok()?.trim().to_string();
        (!key.is_empty()).then_some(key)
    }

    /// Move an older CADCraft's key file into the keychain, when there is one and no connection
    /// yet. The file goes once the key is stored, or at once when a connection is already there.
    fn import_legacy(&self) -> Result<(), String> {
        if !self.legacy.is_file() {
            return Ok(());
        }
        // A connection is there already (made here, in the launcher or in another app): the old
        // file is an unused copy of a key in plain text, so it goes.
        if self.layer.connection()?.is_some() {
            self.drop_legacy();
            return Ok(());
        }
        let Some(key) = self.read_legacy() else { return Ok(()) };
        // The model is bound when the assistant first needs one (`provider`).
        self.layer.save_key(&key, None)?;
        self.drop_legacy();
        log::info!("AI Assistant: the account key moved into the system keychain");
        self.forget_has_key();
        Ok(())
    }

    /// Remove an older CADCraft's key file, once the key in use is stored in the keychain.
    fn drop_legacy(&self) {
        if self.legacy.is_file()
            && let Err(e) = std::fs::remove_file(&self.legacy)
        {
            log::warn!("AI Assistant: the old key file could not be removed: {e}");
        }
    }

    fn connection(&self) -> Result<Connection, String> {
        self.import_legacy()?;
        self.layer.connection()?.ok_or_else(|| "No account key is set".to_string())
    }
}

impl AssistantHost for LayerHost {
    fn service(&self) -> Service {
        Service { name: "FANTASTICO".into(), url: SITE.into(), key_prefix: "ck-fant-".into() }
    }

    fn has_key(&self) -> bool {
        let mut cached = self.has_key.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, answer)) = *cached
            && at.elapsed() < HAS_KEY_TTL
        {
            return answer;
        }
        let answer = self.legacy.is_file() || self.layer.connection().ok().flatten().is_some();
        *cached = Some((Instant::now(), answer));
        answer
    }

    fn connect(&self, key: String) -> Result<Account, String> {
        self.forget_has_key();
        // Only a stored connection counts: an older key file is replaced by a checked new key.
        if self.layer.connection()?.is_some() {
            return Err("A key is already set on this computer".into());
        }
        let (key, account) = Self::verify(&key)?;
        let id = self.layer.save_key(&key, None)?;
        self.drop_legacy();
        if let Err(e) = self.layer.bind(&id, &account.model) {
            log::warn!("AI Assistant: {e}");
        }
        self.forget_has_key();
        Ok(account)
    }

    fn change_key(&self, key: String) -> Result<Account, String> {
        let (key, account) = Self::verify(&key)?;
        // The checked new key replaces whatever was there, an older key file included.
        let existing = self.layer.connection()?;
        let id = self.layer.save_key(&key, existing.as_ref())?;
        self.drop_legacy();
        if self.layer.bound().is_none() {
            let _ = self.layer.bind(&id, &account.model);
        }
        self.forget_has_key();
        Ok(account)
    }

    /// The account's chat models, read again from the account and kept on the connection.
    fn models(&self) -> Result<Vec<ModelInfo>, String> {
        let connection = self.connection()?;
        let report = self.layer.manager.test_saved(&connection.id, true).map_err(|e| e.message)?;
        let mut models: Vec<ModelInfo> = report
            .models
            .into_iter()
            .filter(|m| ModelInfo::is_chat(m.id.as_str()))
            .map(|m| ModelInfo {
                id: m.id.as_str().to_string(),
                name: (m.label != m.id.as_str()).then(|| m.label.clone()),
                // Millionths of a dollar per million tokens, as dollars per million tokens.
                #[allow(clippy::cast_precision_loss)]
                output_usd_per_million: m.pricing.and_then(|p| p.output_micros_per_mtok).map(|micros| micros as f64 / 1_000_000.0),
            })
            .collect();
        models.sort_by_key(|m| m.name.as_deref().unwrap_or(&m.id).to_lowercase());
        Ok(models)
    }

    fn account(&self) -> Option<Account> {
        let connection = self.connection().ok()?;
        let config = self.layer.account_client(&connection).ok()?.config().ok()?;
        Some(Self::account_of(config))
    }

    /// The conversation's endpoint, and the account's suggested model (the model the tool is
    /// bound to when the account can't be read).
    fn provider(&self) -> Result<(Box<dyn Provider + Send>, String), String> {
        let connection = self.connection()?;
        self.layer.ensure_some_binding()?;
        let suggested = self.layer.account_client(&connection).ok().and_then(|c| c.config().ok()).and_then(|c| c.model);
        // `ensure_some_binding` just made sure the tool is bound to a model.
        let model = suggested.or_else(|| self.layer.bound().map(|(_, m)| m.as_str().to_string())).ok_or("The account offers no models")?;
        Ok((Box::new(LayerProvider { layer: self.layer.clone() }), model))
    }

    fn balance(&self) -> Option<f64> {
        let connection = self.connection().ok()?;
        self.layer.account_client(&connection).ok()?.balance().ok().map(|b| Self::credit(&b))
    }

    fn event(&self, event: &AssistantEvent) {
        match event {
            AssistantEvent::KeyRegistered { conto_id } => log::info!("AI Assistant: the key of FANTASTICO account {conto_id} is now stored"),
            AssistantEvent::KeyChanged { conto_id } => {
                log::info!("AI Assistant: the stored key was replaced; it belongs to FANTASTICO account {conto_id}")
            }
        }
    }

    /// Asked while CADCraft has no agreement for the assistant's connection. A connection that
    /// can't be planned for asks nothing here: the turn then fails with the reason.
    fn consent(&self) -> Option<Consent> {
        self.connection().ok()?;
        self.layer.ensure_some_binding().ok()?;
        let plan = self.layer.sample_plan().ok()?;
        plan.needs_consent(&self.layer.load_ledger())
            .then(|| Consent { service: "FANTASTICO".into(), destination: plan.disclosure().destination.clone() })
    }

    fn agree(&self) -> Result<(), String> {
        let plan = self.layer.sample_plan()?;
        let mut ledger = self.layer.load_ledger();
        ledger.grant(plan.disclosure());
        self.layer.save_ledger(&ledger)
    }
}

#[cfg(test)]
mod tests;
