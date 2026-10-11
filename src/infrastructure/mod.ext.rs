// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The module's scheduled job. `jobs/` is declared `user_owned` in metaphor.codegen.yaml
// (whole-tree glob), matching the inventory module's job layout.
pub mod jobs;
