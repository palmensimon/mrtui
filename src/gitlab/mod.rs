pub mod client;
pub mod types;

pub use client::GitLabClient;
pub use types::{FileDiff, MergeRequest, Pipeline, User};
