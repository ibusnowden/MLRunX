#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::redundant_pub_crate,
    clippy::future_not_send,
    clippy::significant_drop_tightening,
    clippy::option_if_let_else
)]

pub mod auth;
pub mod config;
pub mod observability;
pub mod queue;
pub mod services;
pub mod storage;
