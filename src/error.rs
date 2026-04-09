//! Error types for DFA JIT compilation.

/// Errors from DFA compilation or execution.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The DFA has zero states.
    #[error("DFA has zero states. Fix: provide at least one state in the transition table.")]
    EmptyDfa,

    /// The transition table dimensions are inconsistent.
    #[error("invalid transition table: {reason}. Fix: ensure transitions.len() == state_count * class_count. Validate your TransitionTable inputs carefully.")]
    InvalidTable {
        /// Description of the inconsistency.
        reason: String,
    },

    /// Executable memory allocation failed.
    #[error("failed to allocate executable memory: {reason}. Fix: check OS memory limits and mmap permissions.")]
    MemoryAllocation {
        /// Underlying reason.
        reason: String,
    },

    /// The DFA state count exceeds the JIT compiler's limit.
    #[error("DFA has {states} states, exceeding the {max}-state JIT limit. Fix: use the interpreted fallback for large DFAs.")]
    TooManyStates {
        /// Actual state count.
        states: usize,
        /// Maximum supported by JIT.
        max: usize,
    },

    /// Input is longer than the x86_64 JIT scanner can index (32-bit position in the generated loop).
    #[error("input length {len} bytes exceeds JIT limit of {max} bytes (32-bit scan index). Fix: scan in chunks of at most {max} bytes and adjust match offsets, or use a non-JIT build where the interpreted path uses `usize` positions.")]
    InputTooLong {
        /// Actual input length in bytes.
        len: usize,
        /// Maximum length the JIT implementation accepts (`u32::MAX`).
        max: usize,
    },
}

/// Result type alias.
pub type Result<T> = std::result::Result<T, Error>;
