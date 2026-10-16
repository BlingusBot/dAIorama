//! The runtime service: the real dispatcher behind `daiorama serve`.
//!
//! This is the seam where the runtime side replaces the contract layer's `SkeletonHandler`:
//! it owns the session registry and speaks the pinned wire shapes for the lifecycle verbs —
//! `create_session`, `list_sessions`, `session_status`, `close_session` — plus the live
//! `status`/`readiness` reads. Methods later phases own (views, history, config, drain) answer
//! the phase-1 stub envelope until their slices land.
//!
//! Slice scope (serve wiring, first piece):
//! - Sessions run through the injected [`SessionSpawner`]; the service never spawns directly.
//! - One scope exists for now: the implicit `operator` scope every CLI call runs as (04). Its
//!   quota comes from `scopes/operator.ron` (`sessions.max`, read per call — 08 D4) with the
//!   global ceiling from `sessions.max_global` (08).
//! - `view: true` on create is refused with `view_unavailable`: this build has no view
//!   provider yet (views land with P4). Nothing is spawned when it refuses.
//! - Renderer marks are `unverified` until the spawner's renderer gate lands (its own W2
//!   line); nothing is silently `verified`.
//! - `list_sessions` caps a page at [`DEFAULT_LIST_LIMIT`] when `limit` is absent (04 leaves
//!   the default open; provisional and additive to change).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::config::{CoreConfig, Paths, ScopeFile};
use crate::ipc::{
    ErrorCode, ErrorEnvelope, HandshakeResult, MIN_SUPPORTED, PROTOCOL_VERSION, Response, methods,
};
use crate::manager::{
    Backend, DrainState, Handler, InstanceMode, InstanceState, InstanceStatus, Origin, Registry,
    RendererMark, Resolution, SessionRecord, SessionSpawner, SessionSpec, SessionState,
    VersionInfo,
};

/// `list_sessions` page size when the caller sends no `limit` (provisional; additive).
pub const DEFAULT_LIST_LIMIT: u64 = 100;

/// The implicit scope every CLI call runs as (04).
pub const OPERATOR_SCOPE: &str = "operator";

/// What the service needs to run: identity, its socket, the config, the path contract.
#[derive(Debug, Clone)]
pub struct ServiceOptions {
    pub instance_id: String,
    pub socket_path: PathBuf,
    pub mode: InstanceMode,
    pub config: CoreConfig,
    pub paths: Paths,
}

/// The live dispatcher: registry + spawner + the wire shapes.
pub struct RuntimeService {
    registry: Mutex<Registry>,
    spawner: Arc<dyn SessionSpawner>,
    config: CoreConfig,
    paths: Paths,
    socket_path: String,
    mode: InstanceMode,
    /// Serializes creates end to end (quota → spawn → insert) so two racing creates can
    /// never both pass the quota check.
    create_lock: Mutex<()>,
}

impl RuntimeService {
    pub fn new(options: ServiceOptions, spawner: Arc<dyn SessionSpawner>) -> Self {
        Self {
            registry: Mutex::new(Registry::new(options.instance_id)),
            spawner,
            config: options.config,
            paths: options.paths,
            socket_path: options.socket_path.display().to_string(),
            mode: options.mode,
            create_lock: Mutex::new(()),
        }
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry.lock().expect("registry lock")
    }

    /// The live instance-status shape (05).
    fn instance_status(&self) -> Value {
        let registry = self.registry();
        let status = InstanceStatus {
            state: InstanceState::Running,
            mode: self.mode,
            pid: std::process::id(),
            socket: self.socket_path.clone(),
            versions: VersionInfo::current(),
            sessions: registry.counts(),
            drain: DrainState::default(),
            instance_id: registry.instance_id.clone(),
        };
        serde_json::to_value(&status).expect("instance status serializes")
    }

    // -----------------------------------------------------------------------
    // Lifecycle verbs
    // -----------------------------------------------------------------------

    fn create_session(&self, params: Option<&Value>) -> Result<Value, ErrorEnvelope> {
        let map = params_object(params)?;
        unknown_params(&map, &["backend", "resolution", "scale", "view", "label"])?;

        let backend = match string_field(&map, "backend")? {
            Some(raw) => Backend::parse(&raw)?,
            None => Backend::Headless,
        };
        let resolution = match string_field(&map, "resolution")? {
            Some(raw) => Resolution::parse(&raw)?.as_str(),
            None => Resolution::DEFAULT.as_str(),
        };
        let scale = match map.get("scale") {
            None => 1.0f64,
            Some(Value::Number(number)) => number
                .as_f64()
                .ok_or_else(|| ErrorEnvelope::bad_request("scale must be a finite number"))?,
            Some(other) => {
                return Err(ErrorEnvelope::bad_request(format!(
                    "scale must be a number, got {other}"
                )));
            }
        };
        if !scale.is_finite() || scale <= 0.0 {
            return Err(ErrorEnvelope::bad_request(format!(
                "invalid scale {scale}; expected a positive number"
            )));
        }
        let view = bool_field(&map, "view")?.unwrap_or(false);
        let label = string_field(&map, "label")?;

        if view {
            // Refused before anything spawns: a failed request must not leave a session behind.
            return Err(ErrorEnvelope::view_unavailable(
                label.as_deref().unwrap_or("new session"),
                "views are not available in this build yet (P4 wires the view provider)",
            ));
        }

        let spec = SessionSpec {
            backend,
            resolution: resolution.clone(),
            scale,
            view: false,
            label: label.clone(),
        };

        let _create_guard = self.create_lock.lock().expect("create lock");
        let scope_max = self.scope_max()?;
        let global_max = self.config.sessions.max_global;
        let instance_id;
        {
            let registry = self.registry();
            let scope_live = registry
                .list()
                .iter()
                .filter(|record| record.scope == OPERATOR_SCOPE && record.state.is_live())
                .count() as u32;
            match scope_max {
                Some(max) => registry.quota_check(scope_live, max, global_max)?,
                None => registry.quota_check(scope_live, global_max, global_max)?,
            }
            instance_id = registry.instance_id.clone();
        }

        let id = self.spawner.spawn(&spec)?;
        let record = SessionRecord {
            id,
            label: label.unwrap_or_default(),
            state: SessionState::Running,
            backend,
            resolution,
            scale,
            scope: OPERATOR_SCOPE.to_string(),
            origin: Origin::Shell,
            idle_since: utc_now_rfc3339(),
            view: None,
            instance_id,
            // Flips to `verified` / refuses at spawn once the spawner's renderer gate lands
            // (its own W2 line); nothing is silently verified before then.
            renderer: RendererMark::Unverified,
            cause: None,
        };
        let value = record.to_json();
        self.registry().insert(record);
        Ok(value)
    }

    fn list_sessions(&self, params: Option<&Value>) -> Result<Value, ErrorEnvelope> {
        let map = params_object(params)?;
        unknown_params(&map, &["limit", "cursor"])?;
        let limit = match map.get("limit") {
            None => DEFAULT_LIST_LIMIT,
            Some(Value::Number(number)) => match number.as_u64() {
                Some(0) => {
                    return Err(ErrorEnvelope::bad_request("limit must be at least 1"));
                }
                Some(limit) => limit,
                None => {
                    return Err(ErrorEnvelope::bad_request(format!(
                        "limit must be a positive integer, got {number}"
                    )));
                }
            },
            Some(other) => {
                return Err(ErrorEnvelope::bad_request(format!(
                    "limit must be a number, got {other}"
                )));
            }
        };
        let start = match map.get("cursor") {
            None => 0usize,
            Some(Value::String(raw)) => raw
                .parse::<usize>()
                .map_err(|_| ErrorEnvelope::bad_request(format!("invalid cursor {raw:?}")))?,
            Some(other) => {
                return Err(ErrorEnvelope::bad_request(format!(
                    "cursor must be a string, got {other}"
                )));
            }
        };

        let registry = self.registry();
        let all = registry.list();
        let start = start.min(all.len());
        let end = start.saturating_add(limit as usize).min(all.len());
        let sessions: Vec<Value> = all[start..end]
            .iter()
            .map(|record| record.to_json())
            .collect();
        let mut out = json!({ "sessions": sessions });
        if end < all.len() {
            out["next_cursor"] = json!(end.to_string());
        }
        Ok(out)
    }

    fn session_status(&self, params: Option<&Value>) -> Result<Value, ErrorEnvelope> {
        let id = required_session_id(params)?;
        let registry = self.registry();
        let record = registry.call_gate(&id)?;
        Ok(record.to_json())
    }

    fn close_session(&self, params: Option<&Value>) -> Result<Value, ErrorEnvelope> {
        let id = required_session_id(params)?;
        {
            let registry = self.registry();
            match registry.get(&id) {
                None => return Err(registry.tombstone_error(&id)),
                Some(record) if record.state == SessionState::Closed => {
                    return Err(ErrorEnvelope::session_closed());
                }
                Some(_) => {}
            }
        }
        // Completion guarantee: the tree dies before the entry is removed. A session the
        // spawner no longer tracks has nothing left to kill; that is not an error here.
        match self.spawner.kill_tree(&id) {
            Ok(()) => {}
            Err(err) if err.code == ErrorCode::SessionNotFound => {}
            Err(err) => return Err(err),
        }
        let mut registry = self.registry();
        let _ = registry.close(&id)?;
        Ok(json!({ "id": id, "state": "closed" }))
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// The operator scope's quota, read per call (08 D4). A missing scope file means "no
    /// scope quota"; an unreadable or invalid one is a load error, surfaced as itself.
    fn scope_max(&self) -> Result<Option<u32>, ErrorEnvelope> {
        let path = self.paths.scope_file(OPERATOR_SCOPE);
        if !path.exists() {
            return Ok(None);
        }
        Ok(ScopeFile::load(&path)?.sessions.max)
    }
}

impl Handler for RuntimeService {
    fn handshake_result(&self) -> HandshakeResult {
        HandshakeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            min_supported: Some(MIN_SUPPORTED.to_string()),
            instance_id: self.registry().instance_id.clone(),
        }
    }

    fn dispatch(&self, method: &str, params: Option<&Value>) -> Response {
        let outcome: Result<Value, ErrorEnvelope> = match method {
            methods::STATUS => Ok(self.instance_status()),
            methods::READINESS => Ok(self.instance_status()),
            methods::CREATE_SESSION => self.create_session(params),
            methods::LIST_SESSIONS => self.list_sessions(params),
            methods::SESSION_STATUS => self.session_status(params),
            methods::CLOSE_SESSION => self.close_session(params),
            _ => Err(ErrorEnvelope::not_implemented()),
        };
        match outcome {
            Ok(value) => Response::Result(value),
            Err(envelope) => Response::Error(envelope),
        }
    }
}

// ---------------------------------------------------------------------------
// Parameter helpers
// ---------------------------------------------------------------------------

fn params_object(params: Option<&Value>) -> Result<Map<String, Value>, ErrorEnvelope> {
    match params {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(other) => Err(ErrorEnvelope::bad_request(format!(
            "params must be an object or null, got {other}"
        ))),
    }
}

fn unknown_params(map: &Map<String, Value>, valid: &[&str]) -> Result<(), ErrorEnvelope> {
    for key in map.keys() {
        if !valid.contains(&key.as_str()) {
            return Err(ErrorEnvelope::bad_request(format!(
                "unknown parameter {key:?}; valid: {}",
                valid.join(", ")
            )));
        }
    }
    Ok(())
}

fn string_field(map: &Map<String, Value>, key: &str) -> Result<Option<String>, ErrorEnvelope> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(ErrorEnvelope::bad_request(format!(
            "{key} must be a string, got {other}"
        ))),
    }
}

fn bool_field(map: &Map<String, Value>, key: &str) -> Result<Option<bool>, ErrorEnvelope> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(other) => Err(ErrorEnvelope::bad_request(format!(
            "{key} must be a boolean, got {other}"
        ))),
    }
}

fn required_session_id(params: Option<&Value>) -> Result<String, ErrorEnvelope> {
    let map = params_object(params)?;
    unknown_params(&map, &["session_id"])?;
    match map.get("session_id") {
        Some(Value::String(id)) if !id.is_empty() => Ok(id.clone()),
        Some(Value::String(_)) => Err(ErrorEnvelope::bad_request("session_id must not be empty")),
        Some(other) => Err(ErrorEnvelope::bad_request(format!(
            "session_id must be a string, got {other}"
        ))),
        None => Err(ErrorEnvelope::bad_request("session_id is required")),
    }
}

// ---------------------------------------------------------------------------
// Timestamps (RFC 3339 UTC, dependency-free)
// ---------------------------------------------------------------------------

/// The current UTC time as `YYYY-MM-DDTHH:MM:SSZ` (the pinned `idle_since` form).
pub fn utc_now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    rfc3339_from_unix_secs(secs)
}

/// Days-to-civil (Howard Hinnant's algorithm) — correct for the full range; no dependencies.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn rfc3339_from_unix_secs(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

// ---------------------------------------------------------------------------
// Tests: stub spawner, the whole verb surface, and one real-socket end to end
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;
    use crate::ipc::{Address, Client};
    use crate::manager::Listener;
    use crate::runtime::spawner::{LinuxSpawner, SpawnerConfig};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration as StdDuration;

    #[derive(Default)]
    struct TestSpawner {
        spawned: Mutex<Vec<SessionSpec>>,
        killed: Mutex<Vec<String>>,
        next_id: AtomicU64,
        fail_next: Mutex<Option<ErrorEnvelope>>,
    }

    impl SessionSpawner for TestSpawner {
        fn spawn(&self, spec: &SessionSpec) -> Result<String, ErrorEnvelope> {
            if let Some(err) = self.fail_next.lock().expect("fail slot").take() {
                return Err(err);
            }
            self.spawned.lock().expect("spawned").push(spec.clone());
            let n = self.next_id.fetch_add(1, Ordering::SeqCst);
            Ok(format!("2f7a4c1e-9b3d-4a6f-8e21-{:012}", n + 1))
        }

        fn kill_tree(&self, id: &str) -> Result<(), ErrorEnvelope> {
            self.killed.lock().expect("killed").push(id.to_string());
            Ok(())
        }
    }

    fn make_service_with(
        dir: &std::path::Path,
        spawner: Arc<dyn SessionSpawner>,
    ) -> RuntimeService {
        let paths = Paths::for_env(Some(dir), Some(dir), Some(dir), "default");
        RuntimeService::new(
            ServiceOptions {
                instance_id: "11111111-1111-4111-8111-111111111111".to_string(),
                socket_path: dir.join("manager.sock"),
                mode: InstanceMode::Attached,
                config: CoreConfig::default(),
                paths,
            },
            spawner,
        )
    }

    fn make_service(dir: &std::path::Path, spawner: Arc<TestSpawner>) -> RuntimeService {
        make_service_with(dir, spawner)
    }

    fn result(
        service: &RuntimeService,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ErrorEnvelope> {
        match service.dispatch(method, params.as_ref()) {
            Response::Result(value) => Ok(value),
            Response::Error(envelope) => Err(envelope),
        }
    }

    fn create_ok(service: &RuntimeService, params: Option<Value>) -> Value {
        result(service, methods::CREATE_SESSION, params).expect("create succeeds")
    }

    #[test]
    fn create_returns_the_pinned_05_session_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let spawner = Arc::new(TestSpawner::default());
        let service = make_service(tmp.path(), Arc::clone(&spawner));

        let value = create_ok(
            &service,
            Some(json!({
                "backend": "headless",
                "resolution": "1920x1080",
                "scale": 1.0,
                "view": false,
            })),
        );

        let object = value.as_object().expect("a session object");
        assert_eq!(object.len(), 12, "05 shape carries exactly these keys");
        assert_eq!(value["id"], "2f7a4c1e-9b3d-4a6f-8e21-000000000001");
        assert_eq!(value["label"], "");
        assert_eq!(value["state"], "running");
        assert_eq!(value["backend"], "headless");
        assert_eq!(value["resolution"], "1920x1080");
        assert_eq!(value["scale"], 1.0);
        assert_eq!(value["scope"], "operator");
        assert_eq!(value["origin"], "shell");
        assert_eq!(value["view"], Value::Null);
        assert_eq!(value["instance_id"], "11111111-1111-4111-8111-111111111111");
        assert_eq!(value["renderer"], "unverified");
        let idle = value["idle_since"]
            .as_str()
            .expect("idle_since is a string");
        assert_eq!(idle.len(), 20, "{idle:?}");
        assert!(idle.ends_with('Z') && &idle[4..5] == "-" && &idle[10..11] == "T");

        // The spawner saw the exact spec; nothing extra was invented.
        let spawned = spawner.spawned.lock().unwrap();
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0].backend, Backend::Headless);
        assert_eq!(spawned[0].resolution, "1920x1080");
        assert_eq!(spawned[0].scale, 1.0);
        assert!(!spawned[0].view);
    }

    #[test]
    fn create_defaults_apply_when_params_are_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));
        let value = create_ok(&service, None);
        assert_eq!(value["backend"], "headless");
        assert_eq!(value["resolution"], "1920x1080");
        assert_eq!(value["scale"], 1.0);
        assert_eq!(value["label"], "");

        // A non-default label rides through to both the record and the spawner.
        let value = create_ok(&service, Some(json!({ "label": "demo" })));
        assert_eq!(value["label"], "demo");
    }

    #[test]
    fn create_rejects_malformed_params_with_bad_request() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));

        let err = result(
            &service,
            methods::CREATE_SESSION,
            Some(json!({ "backend": "wayland" })),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(err.message.contains("headless, nested, host"), "{err:?}");

        let err = result(
            &service,
            methods::CREATE_SESSION,
            Some(json!({ "resolution": "1024" })),
        )
        .unwrap_err();
        assert!(err.message.contains("invalid resolution"), "{err:?}");

        for scale in [json!(0), json!(-2.0), json!("big")] {
            let err = result(
                &service,
                methods::CREATE_SESSION,
                Some(json!({ "scale": scale })),
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "scale {scale}");
        }

        let err = result(
            &service,
            methods::CREATE_SESSION,
            Some(json!({ "view": "yes" })),
        )
        .unwrap_err();
        assert!(err.message.contains("view must be a boolean"), "{err:?}");

        let err = result(
            &service,
            methods::CREATE_SESSION,
            Some(json!({ "wat": true })),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(err.message.contains("unknown parameter"), "{err:?}");
    }

    #[test]
    fn create_with_view_is_refused_provisionally_and_spawns_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let spawner = Arc::new(TestSpawner::default());
        let service = make_service(tmp.path(), Arc::clone(&spawner));

        let err = result(
            &service,
            methods::CREATE_SESSION,
            Some(json!({ "view": true, "label": "wanted-a-view" })),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ViewUnavailable);
        assert!(err.message.contains("wanted-a-view"), "{err:?}");
        assert!(spawner.spawned.lock().unwrap().is_empty());
        assert!(
            result(&service, methods::LIST_SESSIONS, None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn create_propagates_spawner_refusals_and_leaves_no_record() {
        let tmp = tempfile::tempdir().unwrap();
        let spawner = Arc::new(TestSpawner::default());
        *spawner.fail_next.lock().unwrap() = Some(ErrorEnvelope::renderer_refused("llvmpipe"));
        let service = make_service(tmp.path(), Arc::clone(&spawner));

        let err = result(&service, methods::CREATE_SESSION, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::RendererRefused);
        assert!(
            result(&service, methods::LIST_SESSIONS, None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "a refused spawn must not leave a dead record"
        );
    }

    #[test]
    fn scope_quota_from_the_scope_file_is_enforced_with_its_numbers() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));
        // scopes/operator.ron with max 2 (the rest of the scope file defaults).
        let paths = Paths::for_env(
            Some(tmp.path()),
            Some(tmp.path()),
            Some(tmp.path()),
            "default",
        );
        let scopes = paths.scopes_dir();
        std::fs::create_dir_all(&scopes).unwrap();
        std::fs::write(
            paths.scope_file("operator"),
            "(sessions: (max: Some(2), visibility: all))",
        )
        .unwrap();

        create_ok(&service, None);
        create_ok(&service, None);
        let err = result(&service, methods::CREATE_SESSION, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::QuotaExceeded);
        assert_eq!(
            err.message,
            "session limit reached: scope 'operator' 2/2; global 2/16. Close a session first."
        );
    }

    #[test]
    fn list_pages_with_limit_cursor_and_the_provisional_default() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));
        for _ in 0..3 {
            create_ok(&service, None);
        }

        let page = result(
            &service,
            methods::LIST_SESSIONS,
            Some(json!({ "limit": 2 })),
        )
        .unwrap();
        assert_eq!(page["sessions"].as_array().unwrap().len(), 2);
        assert_eq!(page["next_cursor"], "2");

        let page = result(
            &service,
            methods::LIST_SESSIONS,
            Some(json!({ "limit": 2, "cursor": "2" })),
        )
        .unwrap();
        assert_eq!(page["sessions"].as_array().unwrap().len(), 1);
        assert!(page.get("next_cursor").is_none(), "last page has no cursor");

        let page = result(
            &service,
            methods::LIST_SESSIONS,
            Some(json!({ "cursor": "9" })),
        )
        .unwrap();
        assert!(page["sessions"].as_array().unwrap().is_empty());

        let all = result(&service, methods::LIST_SESSIONS, None).unwrap();
        assert_eq!(all["sessions"].as_array().unwrap().len(), 3);

        let err = result(
            &service,
            methods::LIST_SESSIONS,
            Some(json!({ "cursor": "x" })),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(err.message.contains("invalid cursor"), "{err:?}");
        let err = result(
            &service,
            methods::LIST_SESSIONS,
            Some(json!({ "limit": 0 })),
        )
        .unwrap_err();
        assert!(err.message.contains("at least 1"), "{err:?}");
    }

    #[test]
    fn session_status_walks_the_registry_gates() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));

        let err = result(
            &service,
            methods::SESSION_STATUS,
            Some(json!({ "session_id": "nope" })),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionNotFound);
        assert_eq!(err.hint.as_deref(), Some("list sessions to find one"));

        let value = create_ok(&service, None);
        let id = value["id"].as_str().unwrap().to_string();
        let read = result(
            &service,
            methods::SESSION_STATUS,
            Some(json!({ "session_id": id })),
        )
        .unwrap();
        assert_eq!(read, value, "status reads exactly what create returned");

        // Missing id: a bad request, not a lookup.
        let err = result(&service, methods::SESSION_STATUS, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(err.message.contains("session_id is required"), "{err:?}");
    }

    #[test]
    fn close_kills_then_removes_and_closed_is_final() {
        let tmp = tempfile::tempdir().unwrap();
        let spawner = Arc::new(TestSpawner::default());
        let service = make_service(tmp.path(), Arc::clone(&spawner));

        let value = create_ok(&service, None);
        let id = value["id"].as_str().unwrap().to_string();

        let closed = result(
            &service,
            methods::CLOSE_SESSION,
            Some(json!({ "session_id": id })),
        )
        .unwrap();
        assert_eq!(closed, json!({ "id": id, "state": "closed" }));
        assert_eq!(
            spawner.killed.lock().unwrap().as_slice(),
            std::slice::from_ref(&id)
        );

        // Second close: the distinct code, not a silent success.
        let err = result(
            &service,
            methods::CLOSE_SESSION,
            Some(json!({ "session_id": id })),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionClosed);

        // Unknown stays unknown.
        let err = result(
            &service,
            methods::CLOSE_SESSION,
            Some(json!({ "session_id": "ghost" })),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionNotFound);
    }

    #[test]
    fn close_of_a_dead_session_is_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));
        let value = create_ok(&service, None);
        let id = value["id"].as_str().unwrap().to_string();
        service.registry().reap(&id).unwrap();

        let closed = result(
            &service,
            methods::CLOSE_SESSION,
            Some(json!({ "session_id": id })),
        )
        .unwrap();
        assert_eq!(closed["state"], "closed");
    }

    #[test]
    fn status_and_readiness_read_the_live_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));
        let first = create_ok(&service, None);
        create_ok(&service, None);
        service
            .registry()
            .reap(first["id"].as_str().unwrap())
            .unwrap();

        for method in [methods::STATUS, methods::READINESS] {
            let value = result(&service, method, None).unwrap();
            assert_eq!(value["state"], "running");
            assert_eq!(value["mode"], "attached");
            assert_eq!(value["pid"].as_u64(), Some(std::process::id() as u64));
            assert_eq!(value["sessions"]["running"], 1, "{method}");
            assert_eq!(value["sessions"]["dead"], 1, "{method}");
            assert_eq!(value["instance_id"], "11111111-1111-4111-8111-111111111111");
            assert_eq!(value["versions"]["protocol"], PROTOCOL_VERSION);
        }
    }

    #[test]
    fn unimplemented_methods_answer_the_stub_envelope_once() {
        let tmp = tempfile::tempdir().unwrap();
        let service = make_service(tmp.path(), Arc::new(TestSpawner::default()));
        for method in [methods::GET_VIEW, methods::LIST_HISTORY, methods::DRAIN] {
            let err = result(&service, method, None).unwrap_err();
            assert_eq!(err.code, ErrorCode::Internal);
            assert_eq!(err.message, crate::strings::NOT_IMPLEMENTED_MESSAGE);
        }
    }

    #[tokio::test]
    async fn lifecycle_over_a_real_socket() {
        let tmp = tempfile::tempdir().unwrap();
        let socket = tmp.path().join("manager.sock");
        let service = Arc::new(make_service(tmp.path(), Arc::new(TestSpawner::default())));
        let handler: Arc<dyn Handler> = service;
        let listener = Listener::bind(&socket).await.unwrap();
        let serving = tokio::spawn(listener.serve(handler));

        let address = Address::Unix(socket.clone());
        let mut client = Client::connect(&address, StdDuration::from_secs(2), None)
            .await
            .expect("connects");
        let handshake = client.handshake().await.expect("handshake");
        assert_eq!(
            handshake.instance_id,
            "11111111-1111-4111-8111-111111111111"
        );

        let created = client
            .request(methods::CREATE_SESSION, Some(json!({})), None)
            .await
            .expect("create");
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["state"], "running");

        let listed = client
            .request(methods::LIST_SESSIONS, None, None)
            .await
            .expect("list");
        assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);

        let closed = client
            .request(
                methods::CLOSE_SESSION,
                Some(json!({ "session_id": id })),
                None,
            )
            .await
            .expect("close");
        assert_eq!(closed["state"], "closed");

        let err = client
            .request(
                methods::SESSION_STATUS,
                Some(json!({ "session_id": id })),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionClosed);

        serving.abort();
    }

    #[test]
    fn rfc3339_rendering_is_exact() {
        assert_eq!(rfc3339_from_unix_secs(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_unix_secs(946_684_800), "2000-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_from_unix_secs(1_700_000_000),
            "2023-11-14T22:13:20Z"
        );
        // Leap day: 2024-02-29T12:24:56Z.
        assert_eq!(
            rfc3339_from_unix_secs(1_709_209_496),
            "2024-02-29T12:24:56Z"
        );
    }

    /// The P2 exit run: the lifecycle verbs against the **real** chain (mutter `--headless` →
    /// nested Hyprland on the render node) inside the L1 image. Ignored by default — it needs
    /// the image's compositors and render node; run there like:
    ///
    /// ```text
    /// dbus-run-session -- ./daiorama-tests real_chain --ignored --nocapture --test-threads=1
    /// ```
    #[tokio::test]
    #[ignore = "real chain: needs the L1 image (mutter + Hyprland + render node)"]
    async fn real_chain_lifecycle_verbs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let spawner = Arc::new(LinuxSpawner::new(SpawnerConfig::new(
            tmp.path().join("sessions"),
        )));

        let socket = tmp.path().join("manager.sock");
        let service = Arc::new(make_service_with(tmp.path(), spawner));
        let handler: Arc<dyn Handler> = service;
        let listener = Listener::bind(&socket).await.expect("bind");
        let serving = tokio::spawn(listener.serve(handler));

        let mut client = Client::connect(
            &Address::Unix(socket.clone()),
            StdDuration::from_secs(2),
            None,
        )
        .await
        .expect("connect");
        client.handshake().await.expect("handshake");

        // Create: the real chain — mutter, Hyprland, the output at the pinned defaults.
        let first = client
            .request(methods::CREATE_SESSION, Some(json!({})), None)
            .await
            .expect("create 1");
        eprintln!(
            "CHAIN: created {} state={} resolution={}",
            first["id"], first["state"], first["resolution"]
        );
        assert_eq!(first["state"], "running");

        // A second session at a different size and scale (mode = 1024x768 x 1.5).
        let second = client
            .request(
                methods::CREATE_SESSION,
                Some(json!({ "resolution": "1024x768", "scale": 1.5 })),
                None,
            )
            .await
            .expect("create 2");
        eprintln!("CHAIN: created {} state={}", second["id"], second["state"]);

        let listed = client
            .request(methods::LIST_SESSIONS, None, None)
            .await
            .expect("list");
        assert_eq!(listed["sessions"].as_array().unwrap().len(), 2, "{listed}");

        let first_id = first["id"].as_str().unwrap().to_string();
        let read = client
            .request(
                methods::SESSION_STATUS,
                Some(json!({ "session_id": first_id })),
                None,
            )
            .await
            .expect("status");
        assert_eq!(read["state"], "running");

        let closed = client
            .request(
                methods::CLOSE_SESSION,
                Some(json!({ "session_id": first_id })),
                None,
            )
            .await
            .expect("close 1");
        assert_eq!(closed["state"], "closed");
        eprintln!("CHAIN: closed {first_id}");

        let err = client
            .request(
                methods::SESSION_STATUS,
                Some(json!({ "session_id": first_id })),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionClosed);

        let second_id = second["id"].as_str().unwrap().to_string();
        let closed = client
            .request(
                methods::CLOSE_SESSION,
                Some(json!({ "session_id": second_id })),
                None,
            )
            .await
            .expect("close 2");
        assert_eq!(closed["state"], "closed");
        eprintln!("CHAIN: closed {second_id}");

        let listed = client
            .request(methods::LIST_SESSIONS, None, None)
            .await
            .expect("list after closes");
        assert!(
            listed["sessions"].as_array().unwrap().is_empty(),
            "closed sessions leave no live entries: {listed}"
        );

        serving.abort();
        eprintln!("CHAIN: all lifecycle verbs green on the real chain");
    }
}
