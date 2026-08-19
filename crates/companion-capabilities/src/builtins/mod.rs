pub mod filesystem;
pub mod gmail;
pub mod process;
pub mod web;

use std::sync::Arc;

/// Register all built-in capabilities.
pub fn register_builtins(registry: &mut crate::CapabilityRegistry) {
    registry.register(Arc::new(filesystem::FileRead::new()));
    registry.register(Arc::new(filesystem::FileWrite::new()));
    registry.register(Arc::new(filesystem::FileList::new()));
    registry.register(Arc::new(process::ProcessExecute::new()));
    registry.register(Arc::new(gmail::GmailFetchUnread::new()));
    registry.register(Arc::new(gmail::GmailCreateDraft::new()));
    registry.register(Arc::new(gmail::GmailSendReply::new()));
    registry.register(Arc::new(web::WebFetch::new()));
    registry.register(Arc::new(web::WebExtractLinks::new()));
}
