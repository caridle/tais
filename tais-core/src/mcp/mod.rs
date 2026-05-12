// MCP Gateway — Model Context Protocol integration layer
//
// Three-layer architecture:
//   Layer 1: Skills Bus (TAIS skills call OO skills directly, zero latency)
//   Layer 2: MCP Gateway (JSON-RPC over stdio/SSE for external tools)
//   Layer 3: Tool Providers (OO capsules, external APIs, databases)

pub mod protocol;
pub mod registry;

use crate::rpc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The MCP Gateway manages tool discovery and invocation across all providers.
#[derive(Clone)]
pub struct Gateway {
    registry: Arc<RwLock<registry::ToolRegistry>>,
    /// Direct skill-to-skill connections (zero latency for TAIS→OO calls)
    direct_pipes: Arc<RwLock<HashMap<String, Box<dyn DirectPipe>>>>,
    /// External MCP server connections
    external_servers: Arc<RwLock<Vec<ExternalServer>>>,
}

/// A direct pipe for TAIS skill → OO skill calls (no MCP overhead)
#[async_trait::async_trait]
pub trait DirectPipe: Send + Sync {
    async fn invoke(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, GatewayError>;
}

/// Connection info for an external MCP server
#[derive(Debug, Clone)]
pub struct ExternalServer {
    pub name: String,
    pub transport: Transport,
    pub tools: Vec<crate::McpTool>,
}

#[derive(Debug, Clone)]
pub enum Transport {
    Stdio { command: String, args: Vec<String> },
    Sse { url: String },
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("MCP protocol error: {0}")]
    ProtocolError(String),
    #[error("Transport error: {0}")]
    TransportError(String),
    #[error("Timeout")]
    Timeout,
}

impl Gateway {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(registry::ToolRegistry::new())),
            direct_pipes: Arc::new(RwLock::new(HashMap::new())),
            external_servers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a TAIS or OO skill as a tool
    pub async fn register_skill_tool(&self, tool: crate::McpTool) {
        self.registry.write().await.register(tool);
    }

    /// Register a direct pipe (TAIS → OO skill fast path)
    pub async fn register_direct_pipe(
        &self,
        name: &str,
        pipe: Box<dyn DirectPipe>,
    ) {
        self.direct_pipes.write().await.insert(name.into(), pipe);
    }

    /// Register an external MCP server
    pub async fn register_external_server(&self, server: ExternalServer) {
        let mut servers = self.external_servers.write().await;
        for tool in &server.tools {
            self.registry.write().await.register(tool.clone());
        }
        servers.push(server);
    }

    /// List all available tools (MCP tools/list)
    pub async fn list_tools(&self) -> Vec<crate::McpTool> {
        self.registry.read().await.list_all()
    }

    /// Invoke a tool — tries direct pipe first, then external MCP
    pub async fn call_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, GatewayError> {
        // 1. Try direct pipe (TAIS → OO fast path)
        let pipes = self.direct_pipes.read().await;
        if let Some(pipe) = pipes.get(name) {
            return pipe.invoke(name, params).await;
        }
        drop(pipes);

        // 2. Try external MCP servers
        let servers = self.external_servers.read().await;
        for server in servers.iter() {
            if server.tools.iter().any(|t| t.name == name) {
                let request = rpc::Request::new(
                    "tools/call",
                    Some(serde_json::json!({
                        "name": name,
                        "arguments": params,
                    })),
                );
                return self.invoke_external(&server, &request).await;
            }
        }

        Err(GatewayError::ToolNotFound(name.into()))
    }

    async fn invoke_external(
        &self,
        server: &ExternalServer,
        request: &rpc::Request,
    ) -> Result<serde_json::Value, GatewayError> {
        match &server.transport {
            Transport::Sse { url } => {
                let client = reqwest::Client::new();
                let resp = client
                    .post(url)
                    .json(request)
                    .timeout(std::time::Duration::from_secs(30))
                    .send()
                    .await
                    .map_err(|e| GatewayError::TransportError(e.to_string()))?;

                let rpc_resp: rpc::Response = resp
                    .json()
                    .await
                    .map_err(|e| GatewayError::ProtocolError(e.to_string()))?;

                rpc_resp
                    .result
                    .ok_or_else(|| GatewayError::ProtocolError("no result".into()))
            }
            Transport::Stdio { .. } => {
                // stdio transport would spawn child processes — for now, unsupported
                Err(GatewayError::TransportError(
                    "stdio transport not yet implemented".into(),
                ))
            }
        }
    }
}

impl Default for Gateway {
    fn default() -> Self {
        Self::new()
    }
}
