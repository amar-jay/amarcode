//! Isolated, cancellable ACP setup probes. Never sends prompts or changes a live run.
use super::*;
use crate::acp::AcpInbound;
use crate::protocol::rpc::{
    AgentAuthMethod, AgentSetupAction, AgentSetupParams, AgentSetupResult, AgentSetupStatus,
};
use std::sync::Mutex;
use std::time::Duration;

type Job = Arc<Mutex<SetupJob>>;
pub(super) type SetupJobs = Mutex<HashMap<String, Job>>;
pub(super) struct SetupJob {
    result: AgentSetupResult,
    client: Option<Arc<AcpClient>>,
    cancelled: bool,
}

impl SessionManager {
    pub(super) fn shutdown_setup(&self) {
        let clients = self
            .setup
            .lock()
            .map(|jobs| {
                jobs.values()
                    .filter_map(|job| {
                        let mut job = job.lock().unwrap_or_else(|e| e.into_inner());
                        job.cancelled = true;
                        job.client.clone()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for client in clients {
            let _ = client.kill();
        }
    }
    pub fn agent_setup(&self, params: AgentSetupParams) -> Result<AgentSetupResult> {
        let mut jobs = self
            .setup
            .lock()
            .map_err(|_| Error::msg("setup lock poisoned"))?;
        self.agents.shutdown.check()?;
        if let Some(job) = jobs.get(&params.agent_id) {
            let mut job = job
                .lock()
                .map_err(|_| Error::msg("setup job lock poisoned"))?;
            if matches!(params.action, AgentSetupAction::Cancel) {
                job.cancelled = true;
                if let Some(client) = &job.client {
                    let _ = client.kill();
                }
                job.result.status = AgentSetupStatus::Cancelled;
                job.result.message = Some("Setup cancelled.".into());
                return Ok(job.result.clone());
            }
            if matches!(params.action, AgentSetupAction::Status)
                || matches!(
                    job.result.status,
                    AgentSetupStatus::Checking | AgentSetupStatus::SigningIn
                )
            {
                if matches!(job.result.status, AgentSetupStatus::SigningIn) {
                    if let Some(client) = &job.client {
                        let output = client.stderr_tail();
                        if !output.trim().is_empty() {
                            job.result.output = Some(output);
                        }
                    }
                }
                return Ok(job.result.clone());
            }
        } else if matches!(
            params.action,
            AgentSetupAction::Status | AgentSetupAction::Cancel
        ) {
            return Err(Error::msg("No setup check has been started for this agent"));
        }
        // Resolve before allocating a job; arbitrary ids cannot fill the job table.
        let resolved = self.agents.resolve(&params.agent_id)?;
        let result = AgentSetupResult {
            output: None,
            status: if matches!(params.action, AgentSetupAction::SignIn) {
                AgentSetupStatus::SigningIn
            } else {
                AgentSetupStatus::Checking
            },
            methods: vec![],
            message: None,
        };
        let job = Arc::new(Mutex::new(SetupJob {
            result: result.clone(),
            client: None,
            cancelled: false,
        }));
        jobs.insert(params.agent_id, job.clone());
        std::thread::spawn(move || {
            let outcome = run_setup(&job, &resolved, params.action, params.method_id);
            let mut guard = job.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(client) = guard.client.take() {
                let _ = client.kill();
            }
            if !guard.cancelled {
                match outcome {
                    Ok(()) => {
                        guard.result.output = None;
                        guard.result.status = AgentSetupStatus::Ready;
                        guard.result.message =
                            Some("Session creation verified. You can start a chat.".into());
                    }
                    Err(error) => {
                        let failure = classify_message(&error.to_string());
                        guard.result.status = if matches!(
                            failure.kind,
                            crate::protocol::AgentFailureKind::AuthRequired
                        ) {
                            AgentSetupStatus::AuthRequired
                        } else {
                            AgentSetupStatus::Error
                        };
                        guard.result.message = Some(failure.message);
                    }
                }
            }
        });
        Ok(result)
    }
}

fn run_setup(
    job: &Job,
    resolved: &crate::service::agent_manager::ResolvedAgent,
    action: AgentSetupAction,
    method_id: Option<String>,
) -> Result<()> {
    let (client, inbound) = AcpClient::spawn(
        &resolved.command.to_string_lossy(),
        &resolved.arguments,
        &resolved.environment,
        None,
    )?;
    let client = Arc::new(client);
    {
        let mut guard = job.lock().unwrap_or_else(|e| e.into_inner());
        if guard.cancelled {
            let _ = client.kill();
            return Err(Error::msg("Setup cancelled"));
        }
        guard.client = Some(client.clone());
    }
    // We do not advertise terminal auth or other interactive client capabilities.
    // Answer unexpected requests explicitly instead of leaving the agent hanging.
    let responder = client.clone();
    let progress = job.clone();
    std::thread::spawn(move || {
        for incoming in inbound {
            match incoming {
                AcpInbound::Request { id, .. } => {
                    let _ = responder.respond_error(id, -32601, "This setup flow does not support client interaction. Use the agent's external login instructions.", None);
                }
                AcpInbound::Notification { envelope, .. } => {
                    if let Some(text) = envelope
                        .payload
                        .pointer("/update/content/text")
                        .or_else(|| envelope.payload.pointer("/content/text"))
                        .and_then(Value::as_str)
                    {
                        let mut guard = progress.lock().unwrap_or_else(|e| e.into_inner());
                        if !guard.cancelled
                            && matches!(guard.result.status, AgentSetupStatus::SigningIn)
                        {
                            guard.result.message = Some(text.chars().take(4096).collect());
                        }
                    }
                }
                AcpInbound::Barrier(done) => {
                    let _ = done.send(());
                }
                AcpInbound::Disconnected => break,
                _ => {}
            }
        }
    });
    let initialize = setup_request(
        job,
        &client,
        AgentRpcMethod::Initialize,
        json!({
            "protocolVersion": 1, "clientCapabilities": {},
            "clientInfo": {"name": "amarcode-daemon", "version": env!("CARGO_PKG_VERSION")}
        }),
        Duration::from_secs(30),
    )?;
    let methods = auth_methods(&initialize);
    job.lock().unwrap_or_else(|e| e.into_inner()).result.methods = methods.clone();
    if matches!(action, AgentSetupAction::SignIn) {
        let method = methods
            .iter()
            .find(|m| Some(&m.id) == method_id.as_ref())
            .ok_or_else(|| {
                Error::msg("Choose an authentication method advertised by this agent.")
            })?;
        if !method.supported {
            return Err(Error::msg("This method requires external terminal login or API-key configuration. Complete the agent's setup, then check again."));
        }
        setup_request(
            job,
            &client,
            AgentRpcMethod::Authenticate,
            json!({"methodId": method.id}),
            Duration::from_secs(10 * 60),
        )?;
        // Verify credentials survive the login process, as real chats use a
        // separate process too. A transient in-memory login is not readiness.
        let _ = client.kill();
        return run_setup(job, resolved, AgentSetupAction::Check, None);
    }
    // initialize succeeding proves protocol compatibility, not authentication.
    // session/new exercises the agent's authentication gate without submitting a prompt.
    let session = setup_request(
        job,
        &client,
        AgentRpcMethod::CreateSession,
        json!({
            "cwd": std::env::temp_dir().to_string_lossy(), "mcpServers": []
        }),
        Duration::from_secs(30),
    )?;
    if session.get("sessionId").and_then(Value::as_str).is_none() {
        return Err(Error::msg(
            "Agent did not return a session id; readiness could not be verified.",
        ));
    }
    Ok(())
}

fn setup_request(
    job: &Job,
    client: &AcpClient,
    method: AgentRpcMethod,
    params: Value,
    timeout: Duration,
) -> Result<Value> {
    client.request(method, params, timeout).map_err(|error| {
        let failure = classify_acp_failure(&error);
        if let Some(methods) = &failure.auth_methods {
            let parsed = auth_methods(&json!({"authMethods": methods}));
            if !parsed.is_empty() {
                job.lock().unwrap_or_else(|e| e.into_inner()).result.methods = parsed;
            }
        }
        Error::msg(failure.message)
    })
}

fn auth_methods(initialize: &Value) -> Vec<AgentAuthMethod> {
    initialize
        .get("authMethods")
        .or_else(|| initialize.get("auth_methods"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_owned();
            let kind = m.get("type").and_then(Value::as_str).unwrap_or("agent");
            Some(AgentAuthMethod {
                name: m
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_owned(),
                description: m
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                supported: kind == "agent",
                id,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn methods_are_agent_advertised_and_external_methods_are_not_callable() {
        let methods = auth_methods(&json!({"authMethods": [
            {"id":"oauth", "name":"Browser login"},
            {"id":"cli", "type":"terminal"},
            {"id":"key", "type":"env"},
            {"name":"missing id"}
        ]}));
        assert_eq!(methods.len(), 3);
        assert!(methods[0].supported);
        assert!(!methods[1].supported);
        assert!(!methods[2].supported);
        assert_eq!(
            auth_methods(&json!({"auth_methods":[{"id":"legacy"}]}))[0].id,
            "legacy"
        );
    }
    #[cfg(unix)]
    fn fixture() -> (tempfile::TempDir, SessionManager) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("agent.py");
        std::fs::write(&script, r#"
import json, os, sys, time
root = sys.argv[1]
for line in sys.stdin:
    q = json.loads(line)
    method = q.get('method')
    if method is None:
        continue
    with open(root + '/requests', 'a') as log:
        log.write(str(os.getpid()) + ' ' + method + '\n')
    response = {'jsonrpc':'2.0', 'id':q['id']}
    if method == 'initialize':
        response['result'] = {'authMethods':[{'id':'browser','name':'Browser login'},{'id':'terminal','type':'terminal','name':'CLI login'}]}
    elif method == 'authenticate':
        if q['params']['methodId'] != 'browser':
            raise Exception('unadvertised method')
        if os.path.exists(root + '/wait'):
            print('Visit https://example.com/device and enter ABCD-1234', file=sys.stderr, flush=True)
            time.sleep(30)
        if not os.path.exists(root + '/transient'):
            open(root + '/credentials', 'w').close()
        response['result'] = {}
    elif method == 'session/new':
        if os.path.exists(root + '/credentials'):
            response['result'] = {'sessionId':'verified-session'}
        else:
            response['error'] = {'code':-32000, 'message':'AUTH_REQUIRED'}
    else:
        raise Exception('unexpected request ' + method)
    print(json.dumps(response), flush=True)
"#).unwrap();
        let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
        store
            .save_agent(&crate::protocol::AgentDefinition {
                id: "setup-test".into(),
                name: "Setup test".into(),
                command: "python3".into(),
                arguments: vec![
                    script.to_string_lossy().into_owned(),
                    dir.path().to_string_lossy().into_owned(),
                ],
                environment: vec![],
                available: true,
                created_at: "now".into(),
                updated_at: "now".into(),
            })
            .unwrap();
        let (events, _) = broadcast::channel(8);
        let manager = SessionManager::new(
            store.clone(),
            AgentManager::new(store, dir.path()),
            events,
            dir.path().join("attachments"),
        );
        (dir, manager)
    }

    #[cfg(unix)]
    fn call(
        manager: &SessionManager,
        action: AgentSetupAction,
        method: Option<&str>,
    ) -> AgentSetupResult {
        manager
            .agent_setup(AgentSetupParams {
                agent_id: "setup-test".into(),
                action,
                method_id: method.map(str::to_owned),
            })
            .unwrap()
    }

    #[cfg(unix)]
    fn completed(manager: &SessionManager) -> AgentSetupResult {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            let status = call(manager, AgentSetupAction::Status, None);
            if !matches!(
                status.status,
                AgentSetupStatus::Checking | AgentSetupStatus::SigningIn
            ) {
                return status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "setup did not complete"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[cfg(unix)]
    fn checks_auth_gate_and_verifies_login_in_a_fresh_process() {
        let (dir, manager) = fixture();
        call(&manager, AgentSetupAction::Check, None);
        let result = completed(&manager);
        assert!(matches!(result.status, AgentSetupStatus::AuthRequired));
        assert_eq!(result.methods.len(), 2);
        call(&manager, AgentSetupAction::SignIn, Some("browser"));
        assert!(matches!(
            completed(&manager).status,
            AgentSetupStatus::Ready
        ));
        let log = std::fs::read_to_string(dir.path().join("requests")).unwrap();
        let login_pid = log
            .lines()
            .find(|l| l.ends_with(" authenticate"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        let verification_pid = log
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_ne!(login_pid, verification_pid);
        assert!(!log.contains("session/prompt"));
    }

    #[test]
    #[cfg(unix)]
    fn login_success_without_persisted_credentials_is_not_ready() {
        let (dir, manager) = fixture();
        std::fs::write(dir.path().join("transient"), "").unwrap();
        call(&manager, AgentSetupAction::SignIn, Some("browser"));
        assert!(matches!(
            completed(&manager).status,
            AgentSetupStatus::AuthRequired
        ));
    }

    #[test]
    #[cfg(unix)]
    fn rejects_unknown_and_terminal_methods_without_authenticating() {
        let (dir, manager) = fixture();
        for method in ["forged", "terminal"] {
            call(&manager, AgentSetupAction::SignIn, Some(method));
            assert!(!matches!(
                completed(&manager).status,
                AgentSetupStatus::Ready
            ));
        }
        assert!(!std::fs::read_to_string(dir.path().join("requests"))
            .unwrap()
            .contains(" authenticate"));
    }

    #[test]
    #[cfg(unix)]
    fn cancellation_stops_login_and_allows_retry() {
        let (dir, manager) = fixture();
        std::fs::write(dir.path().join("wait"), "").unwrap();
        call(&manager, AgentSetupAction::SignIn, Some("browser"));
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while !std::fs::read_to_string(dir.path().join("requests"))
            .unwrap_or_default()
            .contains(" authenticate")
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        while !call(&manager, AgentSetupAction::Status, None)
            .output
            .unwrap_or_default()
            .contains("ABCD-1234")
        {
            assert!(
                std::time::Instant::now() < deadline,
                "login instructions were not surfaced"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(
            call(&manager, AgentSetupAction::Check, None).status,
            AgentSetupStatus::SigningIn
        ));
        assert!(matches!(
            call(&manager, AgentSetupAction::Cancel, None).status,
            AgentSetupStatus::Cancelled
        ));
        std::fs::remove_file(dir.path().join("wait")).unwrap();
        call(&manager, AgentSetupAction::Check, None);
        assert!(matches!(
            completed(&manager).status,
            AgentSetupStatus::AuthRequired
        ));
    }
}
