//! The session of a job (docs/specs/2026-11-17-session-launcher.md).
//!
//! [`config`] reads `agents.toml` and gives the adapter and the environment
//! of an agent. [`isolation`] is the interface that runs the agent in a
//! sandbox (R4-05 gives one for each system).
//!
//! [`hold`] is the hold helper: it starts the agent after the engine says
//! `start`, and stops and reaps the process group of the agent.
//!
//! [`launch`] registers a session, builds its workspace, binds its socket,
//! starts the held helper and runs the start gate. [`reap`] is the check
//! that each process of an ended session stopped.
//!
//! [`egress`] is the proxy that gives a session its one path out: a CONNECT
//! tunnel to the allowed hosts of its adapter, at public addresses only.

pub mod config;
pub mod egress;
pub mod hold;
pub mod isolation;
pub mod launch;
pub mod reap;
