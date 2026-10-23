//! Stdio MCP bridge to an ai-memory HTTP MCP server.
//!
//! Two modes, chosen by the entry that launches it, never by the environment:
//!
//! - **Session-aware** (the default, `install-mcp --client claude-code` and
//!   `--client muse`): forwards the launching harness's session id as
//!   `X-Memory-Actor-Session-Id` on every upstream request, and refuses to
//!   start without it. `--agent` names that harness and so the one variable
//!   read: `claude-code` (the default) reads `CLAUDE_CODE_SESSION_ID`,
//!   `muse` reads `MUSE_SESSION_ID`. No variable wins over another: the
//!   other one is never consulted, so a Claude Code started from a Muse shell
//!   (or the reverse) cannot forward the outer harness's id. The refusal is
//!   deliberate (`docs/auto-scope.md`): a registration installed to give a
//!   client per-session routing must not silently degrade to the shared
//!   single slot when the id is missing.
//! - **Session-less** (`--no-session-id`, every other client): a plain
//!   stdio-to-HTTP relay that forwards no session identity, so the client
//!   is a static MCP client and passes `workspace` + `project` explicitly,
//!   exactly as with a direct HTTP entry. It ignores `CLAUDE_CODE_SESSION_ID`
//!   even when present: a client started from a Claude Code terminal inherits
//!   that session's id, and forwarding it would steer this client's unscoped
//!   calls through another session's active-project pointer.
//!
//! Either way the bridge authenticates with a configured bearer
//! (`AI_MEMORY_AUTH_TOKEN`, `[auth].bearer_token`, or the file
//! `[auth].bearer_token_file` names). With none of those, it sends the key
//! `install-hooks --apply --auth-token` stored for this install's hooks in
//! `<data_dir>/auth-token`, so a client whose hooks hold a native API key
//! reaches the server as the same user, and only then the root token
//! `Config::load` discovers on a loopback install, so a host running both the
//! server and a client never sends root over a stored user key. That fallback
//! is the bridge's alone:
//! `Config::load` never reads the file, so `serve` neither requires nor
//! accepts the key because it is stored there. `install-mcp --apply` relies on
//! it to write an entry without the key when the key was given explicitly; an
//! entry written before that, or by the print path, still carries it in
//! `AI_MEMORY_AUTH_TOKEN`, which wins.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientJsonRpcMessage, ClientRequest, ErrorCode,
    ListToolsResult, PaginatedRequestParams, ServerInfo, ServerJsonRpcMessage,
};
use rmcp::service::{RequestContext, RoleClient, RoleServer, ServiceError};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, Transport, stdio};
use rmcp::{ErrorData as McpError, Peer, ServerHandler, ServiceExt};

use crate::cli::{BridgeAgent, McpBridgeArgs, McpToolProfile};
use crate::commands::install_mcp::mcp_server_url_from_base;
use crate::config::Config;

const ACTOR_SESSION_HEADER: HeaderName = HeaderName::from_static("x-memory-actor-session-id");

const SCOPED_TOOLS: &[&str] = &[
    "memory_query",
    "memory_recent",
    "memory_feedback",
    "memory_forget_sweep",
    "memory_lint",
    "memory_auto_improve",
    "memory_write_page",
    "memory_read_page",
    "memory_read_session_observations",
    "memory_delete_page",
    "memory_handoff_begin",
    "memory_handoff_accept",
    "memory_handoff_cancel",
    "memory_status",
    "memory_briefing",
    "memory_explore",
];

const RECALL_TOOLS: &[&str] = &[
    "memory_query",
    "memory_recent",
    "memory_read_page",
    "memory_read_session_observations",
    "memory_status",
    "memory_briefing",
    "memory_explore",
];

const SESSION_TOOLS: &[&str] = &[
    "memory_query",
    "memory_recent",
    "memory_read_page",
    "memory_read_session_observations",
    "memory_status",
    "memory_briefing",
    "memory_explore",
    "memory_feedback",
    "memory_write_page",
    "memory_handoff_begin",
    "memory_handoff_accept",
    "memory_handoff_cancel",
];

impl McpToolProfile {
    fn allows(self, tool_name: &str) -> bool {
        match self {
            Self::Full => true,
            Self::Recall => RECALL_TOOLS.contains(&tool_name),
            Self::Session => SESSION_TOOLS.contains(&tool_name),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScopePin {
    workspace: String,
    project: String,
    read_projects: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ScopePolicy {
    Passthrough,
    Pinned(ScopePin),
}

impl ScopePolicy {
    fn from_args(args: &McpBridgeArgs) -> Result<Self> {
        match (&args.workspace, &args.project) {
            (Some(workspace), Some(project)) => {
                let workspace = workspace.trim();
                let project = project.trim();
                if workspace.is_empty() || project.is_empty() {
                    bail!("--workspace and --project must not be empty");
                }
                let mut read_projects = args
                    .read_project
                    .iter()
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty() && *value != project)
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                read_projects.sort();
                read_projects.dedup();
                if args
                    .read_project
                    .iter()
                    .any(|value| value.trim().is_empty())
                {
                    bail!("--read-project must not be empty");
                }
                Ok(Self::Pinned(ScopePin {
                    workspace: workspace.to_string(),
                    project: project.to_string(),
                    read_projects,
                }))
            }
            (None, None) if !args.read_project.is_empty() => {
                bail!("--read-project requires both --workspace and --project")
            }
            (None, None) if !args.require_scope_pin => Ok(Self::Passthrough),
            (None, None) => bail!("--require-scope-pin requires both --workspace and --project"),
            _ => bail!("--workspace and --project must be supplied together"),
        }
    }
}

#[derive(Clone)]
struct HttpBridge {
    upstream: Peer<RoleClient>,
    server_info: ServerInfo,
    scope_policy: ScopePolicy,
    tool_profile: McpToolProfile,
}

impl ServerHandler for HttpBridge {
    fn get_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let result = self
            .upstream
            .list_tools(request)
            .await
            .map_err(upstream_error)?;
        Ok(filter_tool_list(result, self.tool_profile))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        enforce_tool_profile(request.name.as_ref(), self.tool_profile)?;
        let request = enforce_scope_policy(request, &self.scope_policy)?;
        self.upstream
            .call_tool(request)
            .await
            .map_err(upstream_error)
    }
}

fn enforce_tool_profile(tool_name: &str, profile: McpToolProfile) -> Result<(), McpError> {
    if profile.allows(tool_name) {
        Ok(())
    } else {
        Err(scope_error(format!(
            "tool {tool_name} is disabled by the {profile} MCP tool profile"
        )))
    }
}

fn filter_tool_list(mut result: ListToolsResult, profile: McpToolProfile) -> ListToolsResult {
    result
        .tools
        .retain(|tool| profile.allows(tool.name.as_ref()));
    result
}

fn enforce_scope_policy(
    mut request: CallToolRequestParams,
    policy: &ScopePolicy,
) -> Result<CallToolRequestParams, McpError> {
    let ScopePolicy::Pinned(pin) = policy else {
        return Ok(request);
    };
    let tool_name = request.name.as_ref();

    if tool_name == "memory_install_self_routing" {
        return Ok(request);
    }
    if tool_name == "memory_consolidate" {
        return Err(scope_error(
            "memory_consolidate cannot prove an explicit project scope and is disabled by a pinned bridge",
        ));
    }
    if !SCOPED_TOOLS.contains(&tool_name) {
        return Err(scope_error(format!(
            "tool {tool_name} is not approved by the pinned bridge"
        )));
    }

    let mut arguments = request.arguments.take().unwrap_or_default();
    if tool_name == "memory_query" {
        if pin.read_projects.is_empty() {
            validate_query_scope(&mut arguments, pin)?;
        } else {
            validate_allowlisted_query_scope(&mut arguments, pin)?;
        }
    }
    if tool_name == "memory_write_page"
        && arguments.get("scope").and_then(serde_json::Value::as_str) == Some("global")
    {
        return Err(scope_error(
            "global page writes are disabled by a pinned bridge",
        ));
    }
    if !pin.read_projects.is_empty() && tool_name == "memory_query" {
        // The query validator leaves or injects a complete allowlisted scope set.
    } else if !pin.read_projects.is_empty() && tool_name == "memory_read_page" {
        enforce_allowlisted_read_arguments(&mut arguments, pin)?;
    } else {
        enforce_exact_argument(&mut arguments, "workspace", &pin.workspace)?;
        enforce_exact_argument(&mut arguments, "project", &pin.project)?;
    }
    request.arguments = Some(arguments);
    Ok(request)
}

fn validate_allowlisted_query_scope(
    arguments: &mut serde_json::Map<String, serde_json::Value>,
    pin: &ScopePin,
) -> Result<(), McpError> {
    if arguments.get("global").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(scope_error(
            "global memory queries are disabled by an allowlisted bridge",
        ));
    }

    if let Some(scopes) = arguments.get("scopes") {
        if arguments
            .get("workspace")
            .is_some_and(|value| !value.is_null())
            || arguments
                .get("project")
                .is_some_and(|value| !value.is_null())
        {
            return Err(scope_error(
                "memory_query scopes cannot be combined with workspace or project",
            ));
        }
        let serde_json::Value::Array(scopes) = scopes else {
            return Err(scope_error("memory_query scopes must be an array"));
        };
        if !scopes.is_empty() {
            if !scopes
                .iter()
                .all(|scope| read_scope_value_allowed(scope, pin))
            {
                return Err(scope_error(
                    "memory_query scopes are outside the bridge's read allowlist",
                ));
            }
            arguments.remove("global");
            return Ok(());
        }
        arguments.remove("scopes");
    }

    let has_workspace = arguments
        .get("workspace")
        .is_some_and(|value| !value.is_null());
    let has_project = arguments
        .get("project")
        .is_some_and(|value| !value.is_null());
    if has_workspace || has_project {
        if !has_workspace || !has_project {
            return Err(scope_error(
                "workspace and project must be supplied together for an exact query",
            ));
        }
        enforce_exact_argument(arguments, "workspace", &pin.workspace)?;
        let project = arguments
            .get("project")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| scope_error("project must be a non-empty string"))?;
        if !read_project_allowed(project, pin) {
            return Err(scope_error(
                "project is outside the bridge's read allowlist",
            ));
        }
        arguments.remove("global");
        return Ok(());
    }

    // A missing scope means current project plus the explicit shared/read
    // projects. There is no workspace wildcard and no server-global query.
    let scopes = std::iter::once(pin.project.as_str())
        .chain(pin.read_projects.iter().map(String::as_str))
        .map(|project| {
            serde_json::json!({
                "workspace": pin.workspace,
                "project": project,
            })
        })
        .collect();
    arguments.insert("scopes".into(), serde_json::Value::Array(scopes));
    arguments.remove("global");
    Ok(())
}

fn read_scope_value_allowed(value: &serde_json::Value, pin: &ScopePin) -> bool {
    let Some(scope) = value.as_object() else {
        return false;
    };
    scope.len() == 2
        && scope.get("workspace").and_then(serde_json::Value::as_str)
            == Some(pin.workspace.as_str())
        && scope
            .get("project")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|project| read_project_allowed(project, pin))
}

fn read_project_allowed(project: &str, pin: &ScopePin) -> bool {
    project == pin.project || pin.read_projects.iter().any(|allowed| allowed == project)
}

fn enforce_allowlisted_read_arguments(
    arguments: &mut serde_json::Map<String, serde_json::Value>,
    pin: &ScopePin,
) -> Result<(), McpError> {
    enforce_exact_argument(arguments, "workspace", &pin.workspace)?;
    match arguments.get("project") {
        None | Some(serde_json::Value::Null) => {
            arguments.insert(
                "project".into(),
                serde_json::Value::String(pin.project.clone()),
            );
            Ok(())
        }
        Some(serde_json::Value::String(project)) if read_project_allowed(project, pin) => Ok(()),
        Some(_) => Err(scope_error(
            "project is outside the bridge's read allowlist",
        )),
    }
}

fn validate_query_scope(
    arguments: &mut serde_json::Map<String, serde_json::Value>,
    pin: &ScopePin,
) -> Result<(), McpError> {
    if arguments.get("global").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(scope_error(
            "global memory queries are disabled by a pinned bridge",
        ));
    }

    let Some(scopes) = arguments.remove("scopes") else {
        return Ok(());
    };
    let serde_json::Value::Array(scopes) = scopes else {
        return Err(scope_error("memory_query scopes must be an array"));
    };
    if scopes.is_empty() {
        return Ok(());
    }
    if scopes.len() != 1 || !scope_value_matches(&scopes[0], pin) {
        return Err(scope_error(
            "memory_query scopes conflict with the pinned workspace and project",
        ));
    }
    Ok(())
}

fn scope_value_matches(value: &serde_json::Value, pin: &ScopePin) -> bool {
    let Some(scope) = value.as_object() else {
        return false;
    };
    scope.len() == 2
        && scope.get("workspace").and_then(serde_json::Value::as_str)
            == Some(pin.workspace.as_str())
        && scope.get("project").and_then(serde_json::Value::as_str) == Some(pin.project.as_str())
}

fn enforce_exact_argument(
    arguments: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
    expected: &str,
) -> Result<(), McpError> {
    match arguments.get(name) {
        None | Some(serde_json::Value::Null) => {
            arguments.insert(
                name.to_string(),
                serde_json::Value::String(expected.to_string()),
            );
            Ok(())
        }
        Some(serde_json::Value::String(value)) if value == expected => Ok(()),
        Some(_) => Err(scope_error(format!(
            "{name} conflicts with the bridge's pinned scope"
        ))),
    }
}

fn scope_error(message: impl Into<String>) -> McpError {
    McpError::invalid_params(message.into(), None)
}

fn upstream_error(error: ServiceError) -> McpError {
    match error {
        ServiceError::McpError(error) => error,
        other => McpError::internal_error(format!("upstream MCP request failed: {other}"), None),
    }
}

/// The stdio transport, kept open through a dual-era client's version probe.
///
/// rmcp 2.2's server handshake answers `ping` before `initialize` and ends
/// the session on any other message. A client that speaks both the 2026-07-28
/// revision and the `initialize` handshake (Antigravity CLI 1.4, the
/// Antigravity 2.24 app) first sends `server/discover`, and falls back to
/// `initialize` when the server answers with any error or not at all
/// (<https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio#backward-compatibility>).
/// Exiting instead closes the pipe under that fallback. Until `initialize`
/// arrives this answers every other request with Method not found, so the
/// client sees a legacy server, and drops notifications: a client that stopped
/// waiting for its probe may cancel it. From `initialize` on, every message
/// passes through to rmcp unchanged.
///
/// Revisit on an rmcp 3.x bump: its handshake answers the probe itself
/// (modelcontextprotocol/rust-sdk#1248).
struct PreInitializeGate<T> {
    inner: T,
    initialize_seen: bool,
}

impl<T> PreInitializeGate<T> {
    fn new(inner: T) -> Self {
        Self {
            inner,
            initialize_seen: false,
        }
    }
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for PreInitializeGate<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        while !self.initialize_seen {
            let ClientJsonRpcMessage::Request(request) = self.inner.receive().await? else {
                continue;
            };
            match request.request {
                ClientRequest::InitializeRequest(_) => self.initialize_seen = true,
                ClientRequest::PingRequest(_) => {}
                _ => {
                    let error = McpError::new(
                        ErrorCode::METHOD_NOT_FOUND,
                        format!(
                            "{} is not available before initialize",
                            request.request.method()
                        ),
                        None,
                    );
                    self.inner
                        .send(ServerJsonRpcMessage::error(error, Some(request.id)))
                        .await
                        .ok()?;
                    continue;
                }
            }
            return Some(ClientJsonRpcMessage::Request(request));
        }
        self.inner.receive().await
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

pub(crate) fn upstream_config(
    server_url: &str,
    session_id: Option<&str>,
    auth_token: Option<&str>,
) -> Result<StreamableHttpClientTransportConfig> {
    // Every transport in this module is built from a config this function
    // produced, so installing here covers each of them. Doing it only in
    // `run` left `StreamableHttpClientTransport::from_config` reachable
    // without a provider, and reqwest 0.13 under `rustls-no-provider`
    // *panics* rather than erroring when one is missing.
    install_crypto_provider()?;

    let mut headers = HashMap::new();
    if let Some(session_id) = session_id {
        headers.insert(
            ACTOR_SESSION_HEADER,
            HeaderValue::from_str(session_id)
                .context("the session id contains characters that are invalid in an HTTP header")?,
        );
    }

    let mut config =
        StreamableHttpClientTransportConfig::with_uri(server_url).custom_headers(headers);
    if let Some(token) = auth_token {
        config = config.auth_header(token);
    }
    Ok(config)
}

/// The upstream transport for `server_url`, built on a client that ignores
/// `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY`, as the hooks' client does
/// (`hook_capture::build_client`). rmcp's default client reads them, so a
/// sandbox that exports a proxy with an empty `NO_PROXY` (ai-jail) sent the
/// loopback `initialize` to that proxy, which answered 405. Pooling and
/// redirects are off, as in rmcp's default.
pub(crate) fn upstream_transport(
    server_url: &str,
    session_id: Option<&str>,
    auth_token: Option<&str>,
) -> Result<StreamableHttpClientTransport<reqwest_mcp::Client>> {
    // First: it installs the crypto provider the client builder needs.
    let config = upstream_config(server_url, session_id, auth_token)?;
    let client = reqwest_mcp::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .redirect(reqwest_mcp::redirect::Policy::none())
        .build()
        .context("building the MCP bridge HTTP client")?;
    Ok(StreamableHttpClientTransport::with_client(client, config))
}

/// The session id to forward: in the session-aware mode, the one `agent`
/// sets (`CLAUDE_CODE_SESSION_ID` or `MUSE_SESSION_ID`), refusing to run
/// without it; in the session-less mode nothing at all, whatever the
/// environment holds (see the module docs for why an inherited id must not be
/// forwarded there).
fn bridge_session_id(
    env: &crate::config::RuntimeEnv,
    agent: BridgeAgent,
    no_session_id: bool,
) -> Result<Option<&str>> {
    if no_session_id {
        return Ok(None);
    }
    let inherited = match agent {
        BridgeAgent::ClaudeCode => env.claude_code_session_id(),
        BridgeAgent::Muse => env.muse_session_id(),
    };
    let (variable, harness) = match agent {
        BridgeAgent::ClaudeCode => (agent.session_env(), "Claude Code"),
        BridgeAgent::Muse => (agent.session_env(), "Muse Code"),
    };
    inherited.map(Some).with_context(|| {
        format!(
            "{variable} is missing; this session-aware bridge must be launched by {harness} \
             as an stdio MCP server (other clients register it with `--no-session-id`)"
        )
    })
}

/// Install a process-wide rustls crypto provider before the bridge opens an HTTPS
/// transport.
///
/// The bridge uses rmcp's `reqwest-tls-no-provider`, which supplies the platform
/// certificate verifier without pinning a crypto provider — that is what keeps
/// aws-lc-rs, and the C toolchain and Android JNI stack it needs, out of every build
/// of this workspace. The trade is that rustls then has no default provider of its
/// own, and without one the first `https://` request fails inside the handshake with
/// an error that does not name the cause. `ring` is already compiled for this binary
/// via the reqwest 0.12 the rest of the workspace uses, so install that.
fn install_crypto_provider() -> Result<()> {
    // `Err` means another component installed a provider first, which is fine — the
    // postcondition we need is only that *some* provider is present.
    let _ = rustls::crypto::ring::default_provider().install_default();
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        anyhow::bail!(
            "failed to install a rustls crypto provider; the MCP bridge cannot open an \
             https:// connection without one"
        );
    }
    Ok(())
}

/// The base URL the bridge connects to. An empty `--server-url` counts as
/// not given: the Claude Code plugin passes its unset `server_url` option
/// through as "", and the install's configured server then applies.
fn bridge_base_url<'a>(flag: Option<&'a str>, configured: &'a str) -> &'a str {
    flag.filter(|url| !url.trim().is_empty())
        .unwrap_or(configured)
}

/// The bearer the bridge sends: a configured one, else the user key stored
/// for this install's hooks, else the root key `Config::load` discovered.
/// Configured and stored keys follow the entry's `--server-url`: the stored
/// key is the user's own, which the hooks already send to a remote server
/// (#1490). The discovered root key never leaves loopback.
fn bridge_bearer(config: &Config, server_url: &str) -> Result<Option<String>> {
    let usable = |token: &str| !token.trim().is_empty();
    if let Some(token) = config
        .auth
        .configured_bearer_token()
        .filter(|token| usable(token))
        .map(str::to_owned)
    {
        return Ok(Some(token));
    }

    let stored_path = crate::config::hook_auth_token_path_in(&config.data_dir);
    match std::fs::read_to_string(&stored_path) {
        Ok(raw) => {
            let token = raw.trim();
            if token.is_empty() {
                anyhow::bail!(
                    "stored bridge credential {} is blank; repair or remove it instead of falling back to root",
                    stored_path.display()
                );
            }
            return Ok(Some(token.to_owned()));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!("reading stored bridge credential {}", stored_path.display())
            });
        }
    }

    if crate::http_client::is_loopback_url(server_url) {
        return Ok(config
            .auth
            .bearer_token
            .clone()
            .filter(|token| usable(token)));
    }
    Ok(None)
}

/// Run the stdio bridge, session-aware or session-less (see the module docs).
///
/// # Errors
/// Returns an error when the session-aware mode has no Claude session id, the
/// upstream HTTP MCP server refuses or cannot initialize (an unauthenticated
/// connection to a server that requires a token included), or either
/// transport fails.
pub async fn run(config: &Config, args: McpBridgeArgs) -> Result<()> {
    let scope_policy = ScopePolicy::from_args(&args)?;
    let session_id = bridge_session_id(
        &config.runtime_env,
        args.agent,
        args.no_session_id || args.require_scope_pin,
    )?;
    let server_url = mcp_server_url_from_base(bridge_base_url(
        args.server_url.as_deref(),
        &config.server_url,
    ));
    let bearer = bridge_bearer(config, &server_url)?;
    let transport = upstream_transport(&server_url, session_id, bearer.as_deref())?;

    // Connecting before serving stdio means a refused credential (401) or an
    // unreachable server ends the process with this error at once; the client
    // sees the MCP server exit instead of a session that never answers.
    let mut upstream = match ().serve(transport).await {
        Ok(upstream) => upstream,
        Err(error) => {
            let error = anyhow::Error::new(error);
            // rmcp reports the server's 401 only as "Auth required"; name it.
            let refused = format!("{error:#}").contains("Auth required");
            return Err(error).with_context(|| {
                format!(
                    "connecting the MCP bridge to {server_url}{}{}",
                    if refused {
                        ": the server answered 401 Unauthorized"
                    } else {
                        ""
                    },
                    if bearer.is_some() {
                        ""
                    } else {
                        " (sent without a bearer token; the bridge reads it from \
                         AI_MEMORY_AUTH_TOKEN, [auth].bearer_token, [auth].bearer_token_file \
                         or the key `install-hooks --apply --auth-token` stored in the data dir)"
                    }
                )
            });
        }
    };
    let server_info = upstream
        .peer_info()
        .map(|info| (*info).clone())
        .context("upstream MCP server completed initialization without server info")?;
    let bridge = HttpBridge {
        upstream: upstream.peer().clone(),
        server_info,
        scope_policy,
        tool_profile: args.tool_profile,
    };
    let (stdin, stdout) = stdio();
    let downstream = bridge
        .serve(PreInitializeGate::new(AsyncRwTransport::new_server(
            stdin, stdout,
        )))
        .await
        .context("starting stdio-to-HTTP MCP bridge")?;

    downstream
        .waiting()
        .await
        .context("waiting for the stdio MCP client transport")?;
    upstream
        .close()
        .await
        .context("closing upstream HTTP MCP transport")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The plugin's `.mcp.json` passes an unset option as "": that must reach
    /// the configured server, while a real flag value still wins.
    #[test]
    fn empty_server_url_flag_falls_back_to_the_configured_server() {
        let configured = "https://memory.example";
        assert_eq!(super::bridge_base_url(None, configured), configured);
        assert_eq!(super::bridge_base_url(Some(""), configured), configured);
        assert_eq!(super::bridge_base_url(Some("  "), configured), configured);
        assert_eq!(
            super::bridge_base_url(Some("http://127.0.0.1:5"), configured),
            "http://127.0.0.1:5"
        );
    }

    /// Building the transport must never depend on a caller having installed
    /// a provider first. reqwest 0.13 under `rustls-no-provider` **panics**
    /// inside `ClientBuilder` when none is present, so a missing install is a
    /// crash rather than an error a caller could handle.
    ///
    /// The install lives in `upstream_config` for that reason: every transport
    /// in this module is built from a config it produced.
    ///
    /// Note what this test cannot do. The provider is process-global, so once
    /// any test installs one this assertion would hold even with the install
    /// removed. The structural guarantee — install in the constructor path,
    /// not at one call site — is what actually prevents the panic; this pins
    /// the contract so the call is not quietly deleted.
    #[test]
    fn upstream_config_leaves_a_crypto_provider_installed() {
        let config = upstream_config("http://127.0.0.1:1/mcp", Some("session-for-provider"), None);
        assert!(config.is_ok(), "config build must succeed");
        assert!(
            rustls::crypto::CryptoProvider::get_default().is_some(),
            "upstream_config must guarantee a provider before any client is built"
        );
    }

    #[test]
    fn install_crypto_provider_is_idempotent_and_leaves_a_default() {
        // The bridge builds its transport with `reqwest-tls-no-provider`, so rustls has
        // no provider of its own. Both calls must succeed and a default must be present
        // afterwards — the second models another component having installed one first.
        super::install_crypto_provider().expect("first install");
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
        super::install_crypto_provider().expect("second install must not fail");
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
    use super::*;
    use crate::config::RuntimeEnv;
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use rmcp::model::{ContentBlock as Content, ServerCapabilities, Tool};
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };

    #[derive(Clone)]
    struct EchoServer {
        seen_session: Arc<Mutex<Option<String>>>,
        seen_arguments: Arc<Mutex<Option<serde_json::Value>>>,
    }

    impl ServerHandler for EchoServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, McpError> {
            Ok(ListToolsResult {
                tools: vec![Tool::new(
                    "memory_status",
                    "Echo through the bridge",
                    Arc::new(Default::default()),
                )],
                ..Default::default()
            })
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParams,
            context: RequestContext<RoleServer>,
        ) -> Result<CallToolResult, McpError> {
            let session = context
                .extensions
                .get::<axum::http::request::Parts>()
                .and_then(|parts| parts.headers.get(&ACTOR_SESSION_HEADER))
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            *self.seen_session.lock().unwrap() = session;
            *self.seen_arguments.lock().unwrap() =
                request.arguments.clone().map(serde_json::Value::Object);
            Ok(CallToolResult::success(vec![Content::text(format!(
                "echo:{}",
                request.name
            ))]))
        }
    }

    #[test]
    fn upstream_config_injects_session_id_and_raw_bearer_token() {
        let config = upstream_config(
            "https://memory.example/mcp",
            Some("550e8400-e29b-41d4-a716-446655440000"),
            Some("secret-token"),
        )
        .unwrap();

        assert_eq!(config.auth_header.as_deref(), Some("secret-token"));
        assert_eq!(
            config
                .custom_headers
                .get(&ACTOR_SESSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert!(config.allow_stateless);
        assert!(config.reinit_on_expired_session);
    }

    #[test]
    fn upstream_config_rejects_an_invalid_header_value() {
        let error = upstream_config(
            "https://memory.example/mcp",
            Some("session\ninjected"),
            None,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("invalid in an HTTP header"),
            "{error:#}"
        );
    }

    #[test]
    fn upstream_config_allows_clients_without_a_claude_session_id() {
        let config =
            upstream_config("https://memory.example/mcp", None, Some("secret-token")).unwrap();

        assert_eq!(config.auth_header.as_deref(), Some("secret-token"));
        assert!(!config.custom_headers.contains_key(&ACTOR_SESSION_HEADER));
    }

    #[test]
    fn tool_profiles_filter_discovery_and_reject_direct_calls() {
        let listed = ListToolsResult {
            tools: vec![
                Tool::new("memory_query", "Recall", Arc::new(Default::default())),
                Tool::new("memory_write_page", "Write", Arc::new(Default::default())),
                Tool::new("memory_delete_page", "Delete", Arc::new(Default::default())),
            ],
            ..Default::default()
        };

        let recall = filter_tool_list(listed.clone(), McpToolProfile::Recall);
        assert_eq!(
            recall
                .tools
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<Vec<_>>(),
            ["memory_query"]
        );
        assert!(enforce_tool_profile("memory_write_page", McpToolProfile::Recall).is_err());

        let session = filter_tool_list(listed, McpToolProfile::Session);
        assert_eq!(
            session
                .tools
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<Vec<_>>(),
            ["memory_query", "memory_write_page"]
        );
        assert!(enforce_tool_profile("memory_write_page", McpToolProfile::Session).is_ok());
        assert!(enforce_tool_profile("memory_delete_page", McpToolProfile::Session).is_err());
        assert!(enforce_tool_profile("memory_future_tool", McpToolProfile::Full).is_ok());
    }

    fn bridge_args(
        workspace: Option<&str>,
        project: Option<&str>,
        require_scope_pin: bool,
    ) -> McpBridgeArgs {
        McpBridgeArgs {
            no_session_id: true,
            agent: BridgeAgent::ClaudeCode,
            server_url: None,
            workspace: workspace.map(str::to_string),
            project: project.map(str::to_string),
            require_scope_pin,
            read_project: Vec::new(),
            tool_profile: McpToolProfile::Full,
        }
    }

    fn pinned_policy() -> ScopePolicy {
        ScopePolicy::Pinned(ScopePin {
            workspace: "personal".into(),
            project: "agent-system".into(),
            read_projects: Vec::new(),
        })
    }

    fn allowlisted_read_policy() -> ScopePolicy {
        ScopePolicy::Pinned(ScopePin {
            workspace: "rws".into(),
            project: "current-repo".into(),
            read_projects: vec!["rws-shared".into()],
        })
    }

    fn request_with_arguments(name: &str, arguments: serde_json::Value) -> CallToolRequestParams {
        CallToolRequestParams::new(name.to_string())
            .with_arguments(arguments.as_object().unwrap().clone())
    }

    fn enforced_arguments(name: &str, arguments: serde_json::Value) -> serde_json::Value {
        let request =
            enforce_scope_policy(request_with_arguments(name, arguments), &pinned_policy())
                .unwrap();
        serde_json::Value::Object(request.arguments.unwrap())
    }

    #[test]
    fn scope_policy_requires_a_complete_non_empty_pin() {
        assert_eq!(
            ScopePolicy::from_args(&bridge_args(None, None, false)).unwrap(),
            ScopePolicy::Passthrough
        );
        assert!(ScopePolicy::from_args(&bridge_args(None, None, true)).is_err());
        assert!(ScopePolicy::from_args(&bridge_args(Some("personal"), None, false)).is_err());
        assert!(ScopePolicy::from_args(&bridge_args(None, Some("app"), false)).is_err());
        assert!(ScopePolicy::from_args(&bridge_args(Some("  "), Some("app"), true)).is_err());
        assert_eq!(
            ScopePolicy::from_args(&bridge_args(
                Some(" personal "),
                Some(" agent-system "),
                true,
            ))
            .unwrap(),
            pinned_policy()
        );

        let mut args = bridge_args(Some("rws"), Some("current-repo"), true);
        args.read_project = vec!["rws-shared".into()];
        assert_eq!(
            ScopePolicy::from_args(&args).unwrap(),
            allowlisted_read_policy()
        );

        let mut unpinned = bridge_args(None, None, false);
        unpinned.read_project = vec!["rws-shared".into()];
        assert!(ScopePolicy::from_args(&unpinned).is_err());
    }

    #[test]
    fn pinned_bridge_injects_missing_scope_and_accepts_exact_scope() {
        assert_eq!(
            enforced_arguments("memory_status", serde_json::json!({})),
            serde_json::json!({
                "workspace": "personal",
                "project": "agent-system",
            })
        );
        assert_eq!(
            enforced_arguments(
                "memory_status",
                serde_json::json!({
                    "workspace": "personal",
                    "project": "agent-system",
                }),
            ),
            serde_json::json!({
                "workspace": "personal",
                "project": "agent-system",
            })
        );
    }

    /// The session-aware mode still fails closed without an id; the
    /// session-less mode never forwards one, not even an inherited id.
    #[test]
    fn only_the_session_aware_mode_forwards_and_requires_a_session_id() {
        let claude = RuntimeEnv::with_session_ids_for_tests(Some("s"), None);
        let none = RuntimeEnv::default();
        let agent = BridgeAgent::ClaudeCode;
        assert_eq!(bridge_session_id(&claude, agent, false).unwrap(), Some("s"));
        assert!(bridge_session_id(&none, agent, false).is_err());
        assert_eq!(bridge_session_id(&claude, agent, true).unwrap(), None);
        assert_eq!(bridge_session_id(&none, agent, true).unwrap(), None);
    }

    /// Adversarial: `--agent` names the one variable read, so no harness's id
    /// stands in for another's. A Muse bridge forwards `MUSE_SESSION_ID` and
    /// never an inherited `CLAUDE_CODE_SESSION_ID`, and fails closed without
    /// its own id even when Claude's is set; a Claude bridge never forwards a
    /// `MUSE_SESSION_ID` it inherited from a Muse shell. Control: each
    /// forwards its own id when both are present, and the session-less mode
    /// forwards neither.
    #[test]
    fn each_harness_forwards_only_its_own_session_id() {
        let muse_id = "01a13669-dc61-7a72-b5ec-ad2f300ffd26";
        let both = RuntimeEnv::with_session_ids_for_tests(Some("claude-id"), Some(muse_id));
        assert_eq!(
            bridge_session_id(&both, BridgeAgent::Muse, false).unwrap(),
            Some(muse_id)
        );
        assert_eq!(
            bridge_session_id(&both, BridgeAgent::ClaudeCode, false).unwrap(),
            Some("claude-id")
        );

        let claude_only = RuntimeEnv::with_session_ids_for_tests(Some("claude-id"), None);
        let error = bridge_session_id(&claude_only, BridgeAgent::Muse, false).unwrap_err();
        assert!(
            error.to_string().contains("MUSE_SESSION_ID is missing"),
            "{error:#}"
        );
        let muse_only = RuntimeEnv::with_session_ids_for_tests(None, Some(muse_id));
        let error = bridge_session_id(&muse_only, BridgeAgent::ClaudeCode, false).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("CLAUDE_CODE_SESSION_ID is missing"),
            "{error:#}"
        );
        assert_eq!(
            bridge_session_id(&both, BridgeAgent::Muse, true).unwrap(),
            None
        );
    }

    /// Session-less mode sends no session header at all, even when a
    /// `CLAUDE_CODE_SESSION_ID` is inherited (it is not even consulted).
    #[test]
    fn a_session_less_upstream_config_carries_no_session_header() {
        let config = upstream_config("https://memory.example/mcp", None, Some("t")).unwrap();
        assert!(!config.custom_headers.contains_key(&ACTOR_SESSION_HEADER));
        assert_eq!(config.auth_header.as_deref(), Some("t"));
    }

    #[test]
    fn pinned_bridge_rejects_conflicting_or_untyped_scope() {
        for arguments in [
            serde_json::json!({"workspace": "rws"}),
            serde_json::json!({"project": "other"}),
            serde_json::json!({"workspace": 12}),
            serde_json::json!({"project": false}),
        ] {
            assert!(
                enforce_scope_policy(
                    request_with_arguments("memory_status", arguments),
                    &pinned_policy(),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn pinned_bridge_normalizes_one_exact_query_scope() {
        assert_eq!(
            enforced_arguments(
                "memory_query",
                serde_json::json!({
                    "query": "routing",
                    "scopes": [{
                        "workspace": "personal",
                        "project": "agent-system",
                    }],
                }),
            ),
            serde_json::json!({
                "query": "routing",
                "workspace": "personal",
                "project": "agent-system",
            })
        );
    }

    #[test]
    fn pinned_bridge_rejects_global_or_cross_scope_queries() {
        for arguments in [
            serde_json::json!({"query": "routing", "global": true}),
            serde_json::json!({
                "query": "routing",
                "scopes": [{"workspace": "rws", "project": "agent-system"}],
            }),
            serde_json::json!({
                "query": "routing",
                "scopes": [
                    {"workspace": "personal", "project": "agent-system"},
                    {"workspace": "personal", "project": "other"},
                ],
            }),
            serde_json::json!({"query": "routing", "scopes": "personal"}),
        ] {
            assert!(
                enforce_scope_policy(
                    request_with_arguments("memory_query", arguments),
                    &pinned_policy(),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn allowlisted_read_bridge_defaults_queries_to_current_and_shared_projects() {
        let request = enforce_scope_policy(
            request_with_arguments("memory_query", serde_json::json!({"query": "routing"})),
            &allowlisted_read_policy(),
        )
        .unwrap();
        assert_eq!(
            serde_json::Value::Object(request.arguments.unwrap()),
            serde_json::json!({
                "query": "routing",
                "scopes": [
                    {"workspace": "rws", "project": "current-repo"},
                    {"workspace": "rws", "project": "rws-shared"}
                ]
            })
        );
    }

    #[test]
    fn allowlisted_read_bridge_accepts_only_named_query_scopes() {
        let allowed = request_with_arguments(
            "memory_query",
            serde_json::json!({
                "query": "routing",
                "scopes": [
                    {"workspace": "rws", "project": "current-repo"},
                    {"workspace": "rws", "project": "rws-shared"}
                ]
            }),
        );
        assert!(enforce_scope_policy(allowed, &allowlisted_read_policy()).is_ok());

        for arguments in [
            serde_json::json!({
                "query": "routing",
                "scopes": [{"workspace": "personal", "project": "repo-a"}]
            }),
            serde_json::json!({
                "query": "routing",
                "global": true
            }),
            serde_json::json!({
                "query": "routing",
                "workspace": "rws",
                "project": "unlisted-repo"
            }),
        ] {
            assert!(
                enforce_scope_policy(
                    request_with_arguments("memory_query", arguments),
                    &allowlisted_read_policy(),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn allowlisted_read_bridge_allows_shared_page_reads_but_not_writes() {
        let read = enforce_scope_policy(
            request_with_arguments(
                "memory_read_page",
                serde_json::json!({
                    "workspace": "rws",
                    "project": "rws-shared",
                    "path": "decisions/routing.md"
                }),
            ),
            &allowlisted_read_policy(),
        )
        .unwrap();
        assert_eq!(
            read.arguments.unwrap().get("project"),
            Some(&serde_json::Value::String("rws-shared".into()))
        );

        assert!(
            enforce_scope_policy(
                request_with_arguments(
                    "memory_read_page",
                    serde_json::json!({
                        "workspace": "rws",
                        "project": "unlisted-repo",
                        "path": "decisions/routing.md"
                    }),
                ),
                &allowlisted_read_policy(),
            )
            .is_err()
        );
        assert!(
            enforce_scope_policy(
                request_with_arguments(
                    "memory_write_page",
                    serde_json::json!({
                        "workspace": "rws",
                        "project": "rws-shared",
                        "path": "decisions/routing.md",
                        "body": "# Routing"
                    }),
                ),
                &allowlisted_read_policy(),
            )
            .is_err()
        );
    }

    #[test]
    fn pinned_bridge_rejects_unscoped_and_unknown_tools() {
        assert!(
            enforce_scope_policy(
                request_with_arguments(
                    "memory_consolidate",
                    serde_json::json!({"session_id": "session-1"}),
                ),
                &pinned_policy(),
            )
            .is_err()
        );
        assert!(
            enforce_scope_policy(
                request_with_arguments("memory_future_tool", serde_json::json!({})),
                &pinned_policy(),
            )
            .is_err()
        );
        assert!(
            enforce_scope_policy(
                request_with_arguments(
                    "memory_write_page",
                    serde_json::json!({"path": "a.md", "body": "# A", "scope": "global"}),
                ),
                &pinned_policy(),
            )
            .is_err()
        );
    }

    #[test]
    fn passthrough_bridge_does_not_rewrite_requests() {
        let request = request_with_arguments(
            "memory_query",
            serde_json::json!({"query": "routing", "global": true}),
        );
        assert_eq!(
            enforce_scope_policy(request.clone(), &ScopePolicy::Passthrough).unwrap(),
            request
        );
    }

    async fn assert_bridge_round_trip(
        stateful: bool,
        scope_policy: ScopePolicy,
        expected_arguments: Option<serde_json::Value>,
    ) {
        let seen_session = Arc::new(Mutex::new(None));
        let seen_arguments = Arc::new(Mutex::new(None));
        let echo = EchoServer {
            seen_session: seen_session.clone(),
            seen_arguments: seen_arguments.clone(),
        };
        let service: StreamableHttpService<EchoServer, LocalSessionManager> =
            StreamableHttpService::new(
                move || Ok(echo.clone()),
                LocalSessionManager::default().into(),
                StreamableHttpServerConfig::default()
                    .with_stateful_mode(stateful)
                    .with_json_response(!stateful),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let http_server = tokio::spawn(async move {
            axum::serve(listener, Router::new().nest_service("/mcp", service))
                .await
                .unwrap();
        });

        let upstream_transport = upstream_transport(
            &format!("http://{address}/mcp"),
            Some("claude-session-244"),
            None,
        )
        .unwrap();
        let upstream = ().serve(upstream_transport).await.unwrap();
        let bridge = HttpBridge {
            upstream: upstream.peer().clone(),
            server_info: upstream.peer_info().map(|info| (*info).clone()).unwrap(),
            scope_policy,
            tool_profile: McpToolProfile::Full,
        };
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let downstream_server = tokio::spawn(async move { bridge.serve(server_io).await.unwrap() });
        let downstream_client = ().serve(client_io).await.unwrap();
        let downstream_server = downstream_server.await.unwrap();

        let tools = downstream_client.list_all_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "memory_status");
        let result = downstream_client
            .call_tool(CallToolRequestParams::new("memory_status"))
            .await
            .unwrap();
        assert_eq!(
            result
                .content
                .first()
                .and_then(|content| content.as_text())
                .map(|text| text.text.as_str()),
            Some("echo:memory_status")
        );
        assert_eq!(
            seen_session.lock().unwrap().as_deref(),
            Some("claude-session-244")
        );
        assert_eq!(*seen_arguments.lock().unwrap(), expected_arguments);

        downstream_client.cancel().await.unwrap();
        downstream_server.cancel().await.unwrap();
        upstream.cancel().await.unwrap();
        http_server.abort();
    }

    #[tokio::test]
    async fn stdio_bridge_forwards_tools_and_session_header_in_both_http_modes() {
        assert_bridge_round_trip(false, ScopePolicy::Passthrough, None).await;
        assert_bridge_round_trip(true, ScopePolicy::Passthrough, None).await;
    }

    #[tokio::test]
    async fn stdio_bridge_forwards_rewritten_scope_to_http_upstream() {
        assert_bridge_round_trip(
            false,
            pinned_policy(),
            Some(serde_json::json!({
                "workspace": "personal",
                "project": "agent-system",
            })),
        )
        .await;
    }

    /// The stored hook key is the bridge's last resort: a configured bearer
    /// (env, inline key, token file) outranks it, a blank one does not, and
    /// with neither the bridge sends nothing.
    #[test]
    fn bridge_bearer_puts_the_stored_hook_key_below_a_configured_bearer() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = Config {
            data_dir: tmp.path().to_path_buf(),
            ..Config::default()
        };
        assert_eq!(bridge_bearer(&config, &config.server_url).unwrap(), None);

        crate::config::store_hook_auth_token(tmp.path(), "aim_stored").unwrap();
        assert_eq!(
            bridge_bearer(&config, &config.server_url)
                .unwrap()
                .as_deref(),
            Some("aim_stored")
        );

        config.auth.bearer_token = Some("configured".into());
        assert_eq!(
            bridge_bearer(&config, &config.server_url)
                .unwrap()
                .as_deref(),
            Some("configured")
        );

        config.auth.bearer_token = Some("  ".into());
        assert_eq!(
            bridge_bearer(&config, &config.server_url)
                .unwrap()
                .as_deref(),
            Some("aim_stored")
        );
    }

    /// Adversarial: on a host that runs the server and a client from one data
    /// dir, `Config::load` discovers the root token, and a user's key is
    /// stored for the hooks. The bridge must send the user's key, never root.
    /// Control: without a stored key, discovery still authenticates the bridge.
    #[test]
    fn a_stored_user_key_outranks_the_discovered_root_token() {
        let tmp = tempfile::tempdir().unwrap();
        let discovered_root = || {
            let mut config = Config {
                data_dir: tmp.path().to_path_buf(),
                ..Config::default()
            };
            config.auth.bearer_token = Some("root-token".into());
            config.auth.token_file = Some(crate::config::ResolvedTokenFile {
                path: tmp.path().join("auth-tokens").join("default"),
                loaded: true,
                discovered: true,
            });
            config
        };

        let discovered = discovered_root();
        assert_eq!(discovered.auth.bearer_token.as_deref(), Some("root-token"));
        assert_eq!(
            bridge_bearer(&discovered, &discovered.server_url)
                .unwrap()
                .as_deref(),
            Some("root-token")
        );

        crate::config::store_hook_auth_token(tmp.path(), "aim_user_key").unwrap();
        let both = discovered_root();
        assert_eq!(both.auth.bearer_token.as_deref(), Some("root-token"));
        assert_eq!(
            bridge_bearer(&both, &both.server_url).unwrap().as_deref(),
            Some("aim_user_key")
        );
    }

    #[test]
    fn discovered_root_never_leaves_loopback_but_the_stored_user_key_follows_the_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = Config {
            data_dir: tmp.path().to_path_buf(),
            server_url: "http://127.0.0.1:49374".into(),
            ..Config::default()
        };
        config.auth.bearer_token = Some("discovered-root".into());
        config.auth.token_file = Some(crate::config::ResolvedTokenFile {
            path: tmp.path().join("auth-tokens/default"),
            loaded: true,
            discovered: true,
        });

        // Adversarial: a remote --server-url never receives the root key,
        // including one whose user information spells a loopback address.
        for remote in [
            "https://remote.example/mcp",
            "http://localhost:80@remote.example/mcp",
            "http://127.0.0.1:49374@remote.example/mcp",
        ] {
            assert_eq!(bridge_bearer(&config, remote).unwrap(), None, "{remote}");
        }
        // Control: a loopback server the config does not name (a packaged
        // install whose config lives outside the data dir) still gets it.
        assert_eq!(
            bridge_bearer(&config, "http://127.0.0.1:41234/mcp")
                .unwrap()
                .as_deref(),
            Some("discovered-root")
        );

        // The stored key is the user's own, the one the hooks send to that
        // remote server (#1490), so it follows the entry's URL.
        crate::config::store_hook_auth_token(tmp.path(), "stored-user").unwrap();
        assert_eq!(
            bridge_bearer(&config, "https://remote.example/mcp")
                .unwrap()
                .as_deref(),
            Some("stored-user")
        );

        config.auth.token_file = None;
        assert_eq!(
            bridge_bearer(&config, "https://remote.example/mcp")
                .unwrap()
                .as_deref(),
            Some("discovered-root"),
            "an explicitly configured credential remains remote-capable"
        );
    }

    #[test]
    fn malformed_stored_credential_fails_instead_of_escalating_to_root() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = Config {
            data_dir: tmp.path().to_path_buf(),
            ..Config::default()
        };
        config.auth.bearer_token = Some("discovered-root".into());
        config.auth.token_file = Some(crate::config::ResolvedTokenFile {
            path: tmp.path().join("auth-tokens/default"),
            loaded: true,
            discovered: true,
        });
        std::fs::write(crate::config::hook_auth_token_path_in(tmp.path()), " \n").unwrap();

        let error = bridge_bearer(&config, &config.server_url).unwrap_err();
        assert!(error.to_string().contains("is blank"), "{error:#}");
    }
}
