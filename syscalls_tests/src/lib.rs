//! This crate contains tests for `libtock_platform` functionality that use the
//! `Syscalls` implementation.
//!
//! These tests are not in `libtock_platform` because adding them to
//! `libtock_platform causes two incompatible copies of `libtock_platform` to be
//! compiled:
//!   1. The `libtock_platform` with `cfg(test)` enabled
//!   2. The `libtock_platform` that `libtock_unittest` depends on, which has
//!      `cfg(test)` disabled.
//! Mixing types from the two `libtock_platform` instantiations in tests results
//! in confusing error messages, so instead those tests live in this crate.

// TODO: Add Allow.

#[cfg(test)]
mod command_tests;

#[cfg(test)]
mod exit_on_drop;

// TODO: Add Exit.

// TODO: Add Memop.

#[cfg(test)]
mod subscribe_tests;

#[cfg(test)]
mod yield_tests;
