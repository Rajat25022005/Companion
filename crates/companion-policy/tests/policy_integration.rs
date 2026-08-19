use std::path::{Path, PathBuf};
use chrono::{Duration, Utc};
use companion_domain::{TenantId, WorkspaceId};
use companion_policy::{SecurityRedactor, TenantAuthClaims, TenantSecurityManager};

#[test]
fn test_security_redactor_secrets_and_pii() {
    let redactor = SecurityRedactor::new();

    // 1. Redact API Keys
    let text_with_keys = "Here is my openai key sk-abcdef1234567890abcdef123456 and aws key AKIAIOSFODNN7EXAMPLE and github ghp_1234567890abcdef1234567890abcdef123456";
    let redacted_keys = redactor.redact(text_with_keys);
    assert!(!redacted_keys.contains("sk-abcdef"));
    assert!(!redacted_keys.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(!redacted_keys.contains("ghp_123456"));
    assert!(redacted_keys.contains("[REDACTED_API_KEY]"));
    assert!(redacted_keys.contains("[REDACTED_AWS_KEY]"));
    assert!(redacted_keys.contains("[REDACTED_GITHUB_TOKEN]"));

    // 2. Redact JWT Token
    let text_with_jwt = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c in request";
    let redacted_jwt = redactor.redact(text_with_jwt);
    assert!(!redacted_jwt.contains("eyJhbGci"));
    assert!(redacted_jwt.contains("[REDACTED_JWT]"));

    // 3. Redact Email PII
    let text_with_email = "Contact user at john.doe@enterprise.com for credentials";
    let redacted_email = redactor.redact(text_with_email);
    assert!(!redacted_email.contains("john.doe@enterprise.com"));
    assert!(redacted_email.contains("jo***@enterprise.com"));

    // 4. JSON recursive redaction
    let json_payload = serde_json::json!({
        "db_password": "super_secret_db_pass",
        "api_token": "sk-1234567890abcdef123456",
        "user": {
            "name": "Alice",
            "email": "alice@test.org"
        }
    });

    let sanitized_json = redactor.redact_json(&json_payload);
    assert_eq!(
        sanitized_json.get("db_password"),
        Some(&serde_json::json!("[REDACTED_SECRET]"))
    );
    assert_eq!(
        sanitized_json.get("api_token"),
        Some(&serde_json::json!("[REDACTED_SECRET]"))
    );
}

#[test]
fn test_tenant_security_manager_isolation_and_tokens() {
    let base_dir = PathBuf::from("/var/data/companion/workspaces");
    let manager = TenantSecurityManager::new(base_dir.clone(), "super-secure-enterprise-hmac-key");

    let tenant_id = TenantId::new();
    let workspace_id = WorkspaceId::new();

    // 1. Resolve workspace root
    let workspace_root = manager.resolve_tenant_workspace(&tenant_id, &workspace_id);
    assert_eq!(
        workspace_root,
        base_dir.join(tenant_id.to_string()).join(workspace_id.to_string())
    );

    // 2. Validate safe subpath
    let safe_subpath = Path::new("src/main.rs");
    let validated = manager.validate_path_isolation(&tenant_id, &workspace_id, safe_subpath);
    assert!(validated.is_ok());
    assert_eq!(validated.unwrap(), workspace_root.join("src/main.rs"));

    // 3. Reject path traversal
    let traversal = Path::new("../../etc/passwd");
    let traversal_res = manager.validate_path_isolation(&tenant_id, &workspace_id, traversal);
    assert!(traversal_res.is_err());

    // 4. Issue and validate tenant token
    let claims = TenantAuthClaims {
        tenant_id,
        workspace_id,
        roles: vec!["admin".into(), "builder".into()],
        expires_at: Utc::now() + Duration::hours(2),
    };

    let token_str = manager.issue_token(&claims);
    let decoded = manager.validate_token(&token_str);
    assert!(decoded.is_ok());
    let claims_out = decoded.unwrap();
    assert_eq!(claims_out.tenant_id, tenant_id);
    assert_eq!(claims_out.roles, vec!["admin", "builder"]);

    // 5. Expired token rejection
    let expired_claims = TenantAuthClaims {
        tenant_id,
        workspace_id,
        roles: vec!["viewer".into()],
        expires_at: Utc::now() - Duration::minutes(10),
    };
    let expired_token = manager.issue_token(&expired_claims);
    let expired_res = manager.validate_token(&expired_token);
    assert!(expired_res.is_err());
}
