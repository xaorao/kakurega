mod attachments;
pub mod backup;
mod budget;
pub mod coding;
mod compaction;
pub mod crash;
pub mod deliver;
pub mod experts;
mod extraction;
mod fixfiles;
pub mod keys;
pub mod report;
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

/// Entry points for the live test in `kk-cli`, which asks a real model for notes. They add no behaviour.
#[doc(hidden)]
pub fn extraction_request_for_tests(
    turns: &[(String, String)],
    today: &str,
) -> Vec<kk_types::chat::ChatMessage> {
    extraction::request(turns, today)
}

/// The notes kept from a model's answer and how many were dropped as instruction-like, secret or invalid.
#[doc(hidden)]
pub fn extraction_parse_for_tests(answer: &str) -> Option<(Vec<String>, [usize; 5])> {
    extraction::parse(answer).map(|e| {
        (
            e.notes.into_iter().map(|c| c.text).collect(),
            [
                e.dropped.instruction_like,
                e.dropped.secret,
                e.dropped.invalid,
                e.dropped.assistant_voice,
                e.dropped.relative_date,
            ],
        )
    })
}
