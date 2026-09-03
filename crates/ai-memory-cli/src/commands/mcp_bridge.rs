//! Secret-safe stdio bridge for a remote ai-memory MCP server.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo,
};
use rmcp::service::{RequestContext, RoleClient, RoleServer, ServiceError};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, stdio};
use rmcp::{ErrorData as McpError, Peer, ServerHandler, ServiceExt};

use crate::cli::{McpBridgeArgs, McpToolProfile};
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

fn upstream_config(
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
            HeaderValue::from_str(session_id).context(
                "CLAUDE_CODE_SESSION_ID contains characters that are invalid in an HTTP header",
            )?,
        );
    }

    let mut config =
        StreamableHttpClientTransportConfig::with_uri(server_url).custom_headers(headers);
    if let Some(token) = auth_token {
        config = config.auth_header(token);
    }
    Ok(config)
}

/// Run the secret-safe stdio-to-HTTP bridge.
///
/// # Errors
/// Returns an error when Claude did not provide a lifecycle session id, the
/// upstream HTTP MCP server cannot initialize, or either transport fails.
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

pub async fn run(config: &Config, args: McpBridgeArgs) -> Result<()> {
    let scope_policy = ScopePolicy::from_args(&args)?;
    let session_id = config.runtime_env.claude_code_session_id();
    let server_url = args
        .server_url
        .as_deref()
        .map(mcp_server_url_from_base)
        .unwrap_or_else(|| mcp_server_url_from_base(&config.server_url));
    let transport = StreamableHttpClientTransport::from_config(upstream_config(
        &server_url,
        session_id,
        config.auth.bearer_token.as_deref(),
    )?);

    let mut upstream = ()
        .serve(transport)
        .await
        .with_context(|| format!("connecting stdio bridge to {server_url}"))?;
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
    let downstream = bridge
        .serve(stdio())
        .await
        .context("starting stdio-to-HTTP MCP bridge")?;

    downstream
        .waiting()
        .await
        .context("waiting for downstream stdio MCP transport")?;
    upstream
        .close()
        .await
        .context("closing upstream HTTP MCP transport")?;
    Ok(())
}

#[cfg(test)]
mod tests {

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
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use rmcp::model::{Content, ServerCapabilities, Tool};
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

        let upstream_transport = StreamableHttpClientTransport::from_config(
            upstream_config(
                &format!("http://{address}/mcp"),
                Some("claude-session-244"),
                None,
            )
            .unwrap(),
        );
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
}
