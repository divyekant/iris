//! The session of a job (docs/specs/2026-11-17-session-launcher.md).
//!
//! [`egress`] is the proxy that gives a session its one path out: a CONNECT
//! tunnel to the allowed hosts of its adapter, at public addresses only.

pub mod egress;
