use std::sync::Arc;
use tempfile::tempdir;

use companion_domain::{
    CapabilityDefinition, CapabilityEnvironment, CapabilityPermission,
    Constraint, McpCallToolResult, McpToolInfo, McpToolResultContent, RateLimitPolicy,
    RiskLevel, SandboxPolicy, TaskBudget, TaskContract, TaskId, ToolCall,
};
use companion_capabilities::{
    builtins::register_builtins,
    mcp::{McpClient, MockMcpTransport},
    CapabilityPermissionGate, CapabilityRegistry, RateLimiter, WasmCapability,
};

#[tokio::test]
async fn test_native_builtins_execution_and_hashing() {
    let mut registry = CapabilityRegistry::new();
    register_builtins(&mut registry);

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test_file.txt").to_str().unwrap().to_string();

    // 1. Execute filesystem.write
    let write_call = ToolCall {
        id: "call_1".into(),
        name: "filesystem.write".into(),
        arguments: serde_json::json!({
            "path": file_path,
            "content": "Hello Companion Enterprise Capability Engine!"
        }),
    };

    let write_res = registry.execute(&write_call).await.unwrap();
    assert!(write_res.success);
    assert!(write_res.content_hash.is_some());

    // 2. Execute filesystem.read
    let read_call = ToolCall {
        id: "call_2".into(),
        name: "filesystem.read".into(),
        arguments: serde_json::json!({
            "path": file_path
        }),
    };

    let read_res = registry.execute(&read_call).await.unwrap();
    assert!(read_res.success);
    assert_eq!(
        read_res.output.get("content").and_then(|v| v.as_str()),
        Some("Hello Companion Enterprise Capability Engine!")
    );
}

#[tokio::test]
async fn test_wasm_sandbox_fuel_and_memory_isolation() {
    let mut registry = CapabilityRegistry::new();

    // 1. WASM capability with fuel limit
    let def_strict_fuel = CapabilityDefinition::new(
        "wasm.parser",
        "WASM sandboxed parser",
        serde_json::json!({}),
        vec![],
        RiskLevel::Low,
    )
    .with_sandbox_policy(SandboxPolicy {
        max_memory_bytes: 10 * 1024 * 1024,
        fuel_limit: 100, // Very low fuel limit
        allow_network: false,
        allow_fs_write: false,
        allowed_paths: vec![],
    });

    let mock_wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let wasm_strict = Arc::new(WasmCapability::new(def_strict_fuel, mock_wasm_bytes.clone()));
    registry.register(wasm_strict);

    let call_strict = ToolCall {
        id: "wasm_call_1".into(),
        name: "wasm.parser".into(),
        arguments: serde_json::json!({"payload": "large text that consumes fuel"}),
    };

    let res_fuel = registry.execute(&call_strict).await;
    assert!(res_fuel.is_err(), "Should fail due to fuel limit exceeded");

    // 2. WASM capability with normal fuel budget
    let def_ok = CapabilityDefinition::new(
        "wasm.crypto",
        "WASM sandboxed crypto hash",
        serde_json::json!({}),
        vec![],
        RiskLevel::Low,
    )
    .with_sandbox_policy(SandboxPolicy {
        max_memory_bytes: 10 * 1024 * 1024,
        fuel_limit: 1_000_000,
        allow_network: false,
        allow_fs_write: false,
        allowed_paths: vec![],
    });

    let wasm_ok = Arc::new(WasmCapability::new(def_ok, mock_wasm_bytes));
    registry.register(wasm_ok);

    let call_ok = ToolCall {
        id: "wasm_call_2".into(),
        name: "wasm.crypto".into(),
        arguments: serde_json::json!({"data": "sha256 computation"}),
    };

    let res_ok = registry.execute(&call_ok).await.unwrap();
    assert!(res_ok.success);
    assert_eq!(res_ok.output.get("wasm_executed"), Some(&serde_json::json!(true)));
    assert!(res_ok.content_hash.is_some());
}

#[tokio::test]
async fn test_mcp_client_discovery_and_tool_execution() {
    let mock_tools = vec![
        McpToolInfo {
            name: "github.get_repo".into(),
            description: Some("Get repository metadata".into()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "repo": { "type": "string" } },
                "required": ["repo"]
            }),
        },
        McpToolInfo {
            name: "weather.current".into(),
            description: Some("Get current weather report".into()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "city": { "type": "string" } },
                "required": ["city"]
            }),
        },
    ];

    let transport = Arc::new(MockMcpTransport::new(mock_tools, |name, args| {
        if name == "weather.current" {
            let city = args.get("city").and_then(|v| v.as_str()).unwrap_or("Unknown");
            McpCallToolResult {
                content: vec![McpToolResultContent {
                    content_type: "text".into(),
                    text: Some(format!("Weather in {city}: 72F and sunny")),
                }],
                is_error: false,
            }
        } else {
            McpCallToolResult {
                content: vec![McpToolResultContent {
                    content_type: "text".into(),
                    text: Some(format!("Repo info for {}", args.get("repo").and_then(|v| v.as_str()).unwrap_or(""))),
                }],
                is_error: false,
            }
        }
    }));

    let mcp_client = Arc::new(McpClient::new(transport));
    mcp_client.initialize().await.unwrap();

    let mut registry = CapabilityRegistry::new();
    let registered_names = registry.register_mcp_client(mcp_client).await.unwrap();

    assert_eq!(registered_names.len(), 2);
    assert!(registered_names.contains(&"github.get_repo".to_string()));
    assert!(registered_names.contains(&"weather.current".to_string()));

    // Verify MCP environment filtering
    let mcp_defs = registry.filter_by_environment(CapabilityEnvironment::Mcp);
    assert_eq!(mcp_defs.len(), 2);

    // Execute MCP tool via registry
    let call = ToolCall {
        id: "mcp_call_1".into(),
        name: "weather.current".into(),
        arguments: serde_json::json!({ "city": "San Francisco" }),
    };

    let result = registry.execute(&call).await.unwrap();
    assert!(result.success);
    assert!(result.content_hash.is_some());
}

#[tokio::test]
async fn test_rate_limiter_and_circuit_breaker() {
    let rate_limiter = RateLimiter::new();
    let policy = RateLimitPolicy {
        requests_per_minute: 60, // 1 req/sec
        max_concurrent: 2,
        circuit_breaker_threshold: 2,
        cooldown_seconds: 1,
    };

    // 1. Normal acquisition
    assert!(rate_limiter.check_and_acquire("api.query", &policy).await.is_ok());
    rate_limiter.record_outcome("api.query", &policy, true).await;

    // 2. Trigger consecutive failures -> trip circuit breaker
    assert!(rate_limiter.check_and_acquire("api.query", &policy).await.is_ok());
    rate_limiter.record_outcome("api.query", &policy, false).await;

    assert!(rate_limiter.check_and_acquire("api.query", &policy).await.is_ok());
    rate_limiter.record_outcome("api.query", &policy, false).await; // 2nd failure trips breaker

    // 3. Next invocation must be rejected by OPEN circuit breaker
    let blocked = rate_limiter.check_and_acquire("api.query", &policy).await;
    assert!(blocked.is_err());
    assert!(blocked.unwrap_err().message.contains("Circuit breaker for `api.query` is OPEN"));

    // 4. Wait cooldown
    tokio::time::sleep(tokio::time::Duration::from_millis(1100)).await;

    // 5. HalfOpen probe permitted
    let probe = rate_limiter.check_and_acquire("api.query", &policy).await;
    assert!(probe.is_ok());
    // Successful probe closes circuit breaker
    rate_limiter.record_outcome("api.query", &policy, true).await;

    // Circuit is now closed again
    assert!(rate_limiter.check_and_acquire("api.query", &policy).await.is_ok());
}

#[tokio::test]
async fn test_capability_permission_gate() {
    let contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: companion_domain::TenantId::new(),
        workspace_id: companion_domain::WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        objective: "Read config and parse".into(),
        mode_profile: companion_domain::ModeProfile::from_mode(companion_domain::Mode::Ask),
        required_capabilities: vec![],
        allowed_tools: vec!["filesystem.read".into()],
        completion_conditions: vec![],
        constraints: vec![Constraint::NoNetwork],
        risk_level: RiskLevel::Low,
        budget: TaskBudget::default(),
        user_input: "#ask Read config".into(),
        created_at: chrono::Utc::now(),
    };

    let allowed_cap = CapabilityDefinition::new(
        "filesystem.read",
        "Read file",
        serde_json::json!({}),
        vec![CapabilityPermission::WorkspaceRead],
        RiskLevel::Low,
    );

    let unallowed_cap = CapabilityDefinition::new(
        "filesystem.write",
        "Write file",
        serde_json::json!({}),
        vec![CapabilityPermission::WorkspaceWrite],
        RiskLevel::Medium,
    );

    let network_cap = CapabilityDefinition::new(
        "network.http",
        "HTTP request",
        serde_json::json!({}),
        vec![CapabilityPermission::NetworkRead],
        RiskLevel::High,
    );

    // Permitted tool check -> Ok
    assert!(CapabilityPermissionGate::check_permission(&contract, &allowed_cap).is_ok());

    // Tool not in contract allowed_tools -> Denied
    let denied_tool = CapabilityPermissionGate::check_permission(&contract, &unallowed_cap);
    assert!(denied_tool.is_err());

    // Tool violating constraint (NoNetwork) -> Denied
    let denied_network = CapabilityPermissionGate::check_permission(&contract, &network_cap);
    assert!(denied_network.is_err());
}
