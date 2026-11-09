mod attachments;
pub mod backup;
mod budget;
mod compaction;
pub mod crash;
pub mod deliver;
mod extraction;
mod fixfiles;
pub mod keys;
pub mod service;
pub mod snapshot;
pub mod tenants;

pub use service::{ChatReply, JobRunner, Routes, Service, StreamedReply, LEASABLE_TOOLS};

/// Entry points for the live test in `kk-cli`, which writes a real summary with a real model. They add no behaviour.
#[doc(hidden)]
pub fn compaction_request_for_tests(
    previous: Option<&str>,
    older: &[(String, String)],
) -> Vec<kk_types::chat::ChatMessage> {
    compaction::summary_request(previous, older)
}

#[doc(hidden)]
pub fn compaction_accept_for_tests(text: &str) -> Result<String, String> {
    compaction::accept(text).map_err(|r| r.as_str().to_string())
}
