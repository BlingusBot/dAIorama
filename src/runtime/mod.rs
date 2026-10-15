//! Runtime: the pieces that need a real machine — containment, spawning, health, teardown.
//!
//! **P2 / W2 workstream.** The contract layer (`manager`) defines the seams this module fills:
//! [`crate::manager::startup::ContainmentProbe`], [`crate::manager::SessionSpawner`] and
//! [`crate::manager::ViewOps`]. Slices land here in order:
//!
//! 1. `containment` — cgroup v2 delegation detection + the keeper prep. Class rules and raw
//!    evidence: `p2/cgroup-matrix/` and `fixtures/README.md` §Containment classes.
//! 2. `spawner` — the L1 chain (mutter `--headless` parent → nested Hyprland → on-demand
//!    outputs) as a daemon-managed child, with health checks, teardown and the startup sweep.
//!
//! Nothing here is wired into `daiorama serve` yet — the CLI keeps answering the phase-1 stub
//! envelope until the slices land and the wiring point flips (W3).
#![allow(dead_code)] // wires in as the slices land; unit tests exercise everything today

pub mod containment;
pub mod spawner;
