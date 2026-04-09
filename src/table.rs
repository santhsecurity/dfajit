use crate::error::{Error, Result};

/// DFA transition table consumed by the JIT compiler.
#[derive(Debug, Clone)]
pub struct TransitionTable {
    /// Number of states in the DFA.
    state_count: usize,
    /// Number of input classes (typically 256 for byte-indexed).
    class_count: usize,
    /// Flat transition array: `transitions[state * class_count + class]` = next state.
    /// High bit (0x8000_0000) set means the target is an accept state.
    transitions: Vec<u32>,
    /// Accept state metadata: `(state_index, pattern_id)`.
    accept_states: Vec<(u32, u32)>,
    /// Fixed pattern lengths for computing match start from match end.
    /// `pattern_lengths[pattern_id]` = byte length, or 0 for variable-length.
    pattern_lengths: Vec<u32>,
}

impl TransitionTable {
    /// Maximum states allowed in a single DFA.
    ///
    /// 65536 states × 256 classes × 4 bytes = 64 MB transition table.
    /// Any DFA exceeding this is either pathological or a DoS vector.
    /// JIT path caps at 4096 states for I-cache; this caps the interpreted fallback.
    pub const MAX_STATES: usize = 65_536;

    /// Create a new empty transition table.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooManyStates`] if `state_count` exceeds [`Self::MAX_STATES`].
    ///
    /// # Panics
    ///
    /// Panics if `state_count * class_count` would overflow `usize` (requires >72 PB RAM).
    pub fn new(state_count: usize, class_count: usize) -> Result<Self> {
        if state_count > Self::MAX_STATES {
            return Err(Error::TooManyStates {
                states: state_count,
                max: Self::MAX_STATES,
            });
        }
        if class_count == 0 {
            return Err(Error::InvalidTable {
                reason: "class_count must be greater than 0".into(),
            });
        }
        let total = state_count
            .checked_mul(class_count)
            .ok_or(Error::TooManyStates {
                states: state_count,
                max: Self::MAX_STATES,
            })?;
        // Cap total transitions at 256M entries (1GB) to prevent OOM.
        const MAX_TOTAL_TRANSITIONS: usize = 256 * 1024 * 1024;
        if total > MAX_TOTAL_TRANSITIONS {
            return Err(Error::TooManyStates {
                states: state_count,
                max: Self::MAX_STATES,
            });
        }
        Ok(Self {
            state_count,
            class_count,
            transitions: vec![0; total],
            accept_states: Vec::new(),
            pattern_lengths: Vec::new(),
        })
    }

    /// Set a single transition: from `state` on input `byte`, go to `next_state`.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if the state/byte combination is out of bounds.
    /// In release mode, out-of-bounds writes are silently ignored.
    pub fn set_transition(&mut self, state: usize, byte: u8, next_state: u32) {
        let idx = state * self.class_count + byte as usize;
        debug_assert!(
            idx < self.transitions.len(),
            "set_transition out of bounds: state={state}, byte={byte}, idx={idx}, len={}",
            self.transitions.len()
        );
        if idx < self.transitions.len() {
            self.transitions[idx] = next_state;
        }
    }

    /// Mark a state as accepting for a given pattern.
    pub fn add_accept(&mut self, state: u32, pattern_id: u32) {
        self.accept_states.push((state, pattern_id));
        if self.pattern_lengths.len() <= pattern_id as usize {
            self.pattern_lengths.resize(pattern_id as usize + 1, 0);
        }
    }

    /// Set the fixed length for a pattern (used to compute match start).
    pub fn set_pattern_length(&mut self, pattern_id: u32, length: u32) {
        if self.pattern_lengths.len() <= pattern_id as usize {
            self.pattern_lengths.resize(pattern_id as usize + 1, 0);
        }
        self.pattern_lengths[pattern_id as usize] = length;
    }

    /// Collapse each state's byte transitions into maximal consecutive ranges.
    ///
    /// Each tuple is `(lo, hi, target_state)` and represents a closed byte
    /// interval whose transitions all resolve to the same target.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dfajit::TransitionTable;
    ///
    /// let mut table = TransitionTable::new(2, 256).unwrap();
    /// for byte in b'a'..=b'z' {
    ///     table.set_transition(0, byte, 1);
    /// }
    ///
    /// let ranges = table.compute_ranges();
    /// assert!(ranges[0].contains(&(b'a', b'z', 1)));
    /// ```
    #[must_use]
    pub fn compute_ranges(&self) -> Vec<Vec<(u8, u8, u32)>> {
        let mut ranges = Vec::with_capacity(self.state_count);
        if self.class_count == 0 {
            return ranges;
        }

        for state in 0..self.state_count {
            let row_start = state.saturating_mul(self.class_count);
            let row_end = row_start
                .saturating_add(self.class_count)
                .min(self.transitions.len());
            let row = &self.transitions[row_start..row_end];
            let limit = row.len().min(usize::from(u8::MAX) + 1);
            if limit == 0 {
                ranges.push(Vec::new());
                continue;
            }

            let mut state_ranges = Vec::new();
            let mut start = 0usize;
            let mut target = row[0];
            for index in 1..limit {
                if row[index] != target {
                    state_ranges.push((start as u8, (index - 1) as u8, target));
                    start = index;
                    target = row[index];
                }
            }
            state_ranges.push((start as u8, (limit - 1) as u8, target));
            ranges.push(state_ranges);
        }

        ranges
    }

    /// Number of transitions in the table.
    #[must_use]
    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// Estimated JIT code size in bytes.
    ///
    /// Useful for checking if the DFA will fit in L1 I-cache.
    #[must_use]
    pub fn estimated_code_size(&self) -> usize {
        // Code: ~150 bytes prologue/epilogue + ~10 bytes per scan loop iteration
        // Data: state_count * class_count * 4 (transition table) +
        //       state_count * 4 (accept pattern table) +
        //       pattern_lengths.len() * 4 (pattern length table)
        let code = 256;
        let data =
            self.transitions.len() * 4 + self.state_count * 4 + self.pattern_lengths.len() * 4;
        code + data
    }

    /// Serialize the transition table to bytes.
    ///
    /// Format: state_count (u32 LE) + class_count (u32 LE) + transitions (u32 LE each)
    /// + accept_count (u32 LE) + accept_states (state u32, pattern_id u32 each)
    /// + pattern_length_count (u32 LE) + pattern_lengths (u32 LE each)
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let accept_count = self.accept_states.len();
        let pat_len_count = self.pattern_lengths.len();
        let size = 4usize
            .checked_add(4)
            .and_then(|s| s.checked_add(self.transitions.len().checked_mul(4)?))
            .and_then(|s| s.checked_add(4))
            .and_then(|s| s.checked_add(accept_count.checked_mul(8)?))
            .and_then(|s| s.checked_add(4))
            .and_then(|s| s.checked_add(pat_len_count.checked_mul(4)?))
            .unwrap_or({
                // Pathological table: return header-only so caller doesn't crash,
                // though deserialization will reject it.
                8
            });
        let mut buf = Vec::with_capacity(size);
        buf.extend_from_slice(&(self.state_count as u32).to_le_bytes());
        buf.extend_from_slice(&(self.class_count as u32).to_le_bytes());
        for &t in &self.transitions {
            buf.extend_from_slice(&t.to_le_bytes());
        }
        buf.extend_from_slice(&(accept_count as u32).to_le_bytes());
        for &(state, pid) in &self.accept_states {
            buf.extend_from_slice(&state.to_le_bytes());
            buf.extend_from_slice(&pid.to_le_bytes());
        }
        buf.extend_from_slice(&(pat_len_count as u32).to_le_bytes());
        for &l in &self.pattern_lengths {
            buf.extend_from_slice(&l.to_le_bytes());
        }
        buf
    }

    /// Deserialize a transition table from bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes are truncated or contain invalid data.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(Error::InvalidTable {
                reason: "data too short for header".into(),
            });
        }
        let state_count = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4])) as usize;
        let class_count = u32::from_le_bytes(data[4..8].try_into().unwrap_or([0; 4])) as usize;

        let trans_len =
            state_count
                .checked_mul(class_count)
                .ok_or_else(|| Error::InvalidTable {
                    reason: "state_count * class_count overflow".into(),
                })?;

        let trans_bytes = trans_len
            .checked_mul(4)
            .ok_or_else(|| Error::InvalidTable {
                reason: "transition table byte length overflow".into(),
            })?;

        let trans_end = 8usize
            .checked_add(trans_bytes)
            .ok_or_else(|| Error::InvalidTable {
                reason: "transition table end offset overflow".into(),
            })?;

        if data.len() < trans_end + 4 {
            return Err(Error::InvalidTable {
                reason: "truncated transition table".into(),
            });
        }

        let mut transitions = Vec::with_capacity(trans_len);
        for i in 0..trans_len {
            let off = 8 + i * 4;
            let val = u32::from_le_bytes(data[off..off + 4].try_into().unwrap_or([0; 4]));
            transitions.push(val);
        }

        let accept_count =
            u32::from_le_bytes(data[trans_end..trans_end + 4].try_into().unwrap_or([0; 4]))
                as usize;
        let accept_bytes = accept_count
            .checked_mul(8)
            .ok_or_else(|| Error::InvalidTable {
                reason: "accept states byte length overflow".into(),
            })?;
        let mut pos = trans_end + 4;
        if data.len()
            < pos
                .checked_add(accept_bytes)
                .ok_or_else(|| Error::InvalidTable {
                    reason: "accept states end offset overflow".into(),
                })?
        {
            return Err(Error::InvalidTable {
                reason: "truncated accept states".into(),
            });
        }
        let mut accept_states = Vec::with_capacity(accept_count);
        for _ in 0..accept_count {
            let state = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or([0; 4]));
            let pid = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap_or([0; 4]));
            accept_states.push((state, pid));
            pos += 8;
        }

        if pos + 4 > data.len() {
            return Err(Error::InvalidTable {
                reason: "truncated pattern lengths header".into(),
            });
        }
        let pat_count =
            u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or([0; 4])) as usize;
        let pat_bytes = pat_count
            .checked_mul(4)
            .ok_or_else(|| Error::InvalidTable {
                reason: "pattern lengths byte length overflow".into(),
            })?;
        pos += 4;
        if data.len()
            < pos
                .checked_add(pat_bytes)
                .ok_or_else(|| Error::InvalidTable {
                    reason: "pattern lengths end offset overflow".into(),
                })?
        {
            return Err(Error::InvalidTable {
                reason: "truncated pattern lengths".into(),
            });
        }
        let mut pattern_lengths = Vec::with_capacity(pat_count);
        for _ in 0..pat_count {
            let l = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or([0; 4]));
            pattern_lengths.push(l);
            pos += 4;
        }

        Self::from_parts(
            state_count,
            class_count,
            transitions,
            accept_states,
            pattern_lengths,
        )
    }

    /// Number of DFA states.
    #[must_use]
    pub fn state_count(&self) -> usize {
        self.state_count
    }

    /// Number of input classes.
    #[must_use]
    pub fn class_count(&self) -> usize {
        self.class_count
    }

    /// Transition array slice.
    #[must_use]
    pub fn transitions(&self) -> &[u32] {
        &self.transitions
    }

    /// Mutable transition array.
    pub fn transitions_mut(&mut self) -> &mut Vec<u32> {
        &mut self.transitions
    }

    /// Accept state metadata slice.
    #[must_use]
    pub fn accept_states(&self) -> &[(u32, u32)] {
        &self.accept_states
    }

    /// Mutable accept states vector.
    pub fn accept_states_mut(&mut self) -> &mut Vec<(u32, u32)> {
        &mut self.accept_states
    }

    /// Pattern lengths slice.
    #[must_use]
    pub fn pattern_lengths(&self) -> &[u32] {
        &self.pattern_lengths
    }

    /// Mutable pattern lengths vector.
    pub fn pattern_lengths_mut(&mut self) -> &mut Vec<u32> {
        &mut self.pattern_lengths
    }

    /// Construct a transition table from validated parts.
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions are inconsistent or out of bounds.
    pub fn from_parts(
        state_count: usize,
        class_count: usize,
        transitions: Vec<u32>,
        accept_states: Vec<(u32, u32)>,
        pattern_lengths: Vec<u32>,
    ) -> Result<Self> {
        if state_count > Self::MAX_STATES {
            return Err(Error::TooManyStates {
                states: state_count,
                max: Self::MAX_STATES,
            });
        }
        if class_count == 0 {
            return Err(Error::InvalidTable {
                reason: "class_count must be greater than 0".into(),
            });
        }
        let expected_len =
            state_count
                .checked_mul(class_count)
                .ok_or_else(|| Error::InvalidTable {
                    reason: "state_count * class_count overflow".into(),
                })?;
        if transitions.len() != expected_len {
            return Err(Error::InvalidTable {
                reason: format!(
                    "transition table has {} entries but expected {}",
                    transitions.len(),
                    expected_len,
                ),
            });
        }
        for &t in &transitions {
            let state = t & 0x7FFF_FFFF;
            if state as usize >= state_count {
                return Err(Error::InvalidTable {
                    reason: format!(
                        "transition target state {state} exceeds state count {state_count}"
                    ),
                });
            }
        }
        let pat_len = pattern_lengths.len();
        let mut seen_states = vec![false; state_count];
        for &(state, pid) in &accept_states {
            if state as usize >= state_count {
                return Err(Error::InvalidTable {
                    reason: format!("accept state {state} exceeds state count {state_count}"),
                });
            }
            if seen_states[state as usize] {
                return Err(Error::InvalidTable {
                    reason: format!(
                        "state {state} has multiple accept patterns, which is not supported"
                    ),
                });
            }
            seen_states[state as usize] = true;
            if pid as usize >= pat_len {
                return Err(Error::InvalidTable {
                    reason: format!("pattern ID {pid} in accept states has no length defined"),
                });
            }
        }
        Ok(Self {
            state_count,
            class_count,
            transitions,
            accept_states,
            pattern_lengths,
        })
    }

    /// Count distinct transition targets for a state.
    ///
    /// A state with only 1 target is a "dead" or "pass-through" state.
    /// States with few targets (2-4) are ideal for range-check optimization.
    #[must_use]
    pub fn transition_density(&self, state: usize) -> usize {
        if state >= self.state_count {
            return 0;
        }
        let base = state * self.class_count;
        let mut targets = std::collections::HashSet::new();
        for byte in 0..self.class_count {
            if let Some(&t) = self.transitions.get(base + byte) {
                targets.insert(t);
            }
        }
        targets.len()
    }

    /// Whether this DFA is small enough for JIT compilation.
    ///
    /// Returns `false` if the DFA would exceed the I-cache safety fuse
    /// and fall back to interpreted execution, or if the class count is not
    /// 256 (the JIT only supports byte-indexed tables).
    #[must_use]
    pub fn is_jit_eligible(&self) -> bool {
        self.state_count <= 4096 && self.class_count == 256
    }

    /// Minimize the DFA using Hopcroft's partition refinement algorithm.
    ///
    /// Produces a new transition table with the minimum number of states
    /// that accepts the same language. Fewer states = smaller JIT code = better I-cache.
    ///
    /// Returns the minimized table if it has fewer states, or `None` if already minimal.
    ///
    /// # Panics
    ///
    /// Panics if the minimized state count exceeds `MAX_STATES` (should be unreachable).
    #[must_use]
    pub fn minimize(&self) -> Option<Self> {
        if self.state_count <= 1 {
            return None;
        }

        // Build state -> pattern_id map for accept states.
        let mut state_to_pattern: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
        for &(s, pid) in &self.accept_states {
            state_to_pattern.insert(s, pid);
        }

        // Initial partition: group accept states by pattern_id.
        // Non-accept states remain in class 0.
        let mut partition = vec![0u32; self.state_count];
        let mut next_class = 1u32;
        let mut pattern_class: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
        for i in 0..self.state_count {
            if let Some(&pid) = state_to_pattern.get(&(i as u32)) {
                let class = *pattern_class.entry(pid).or_insert_with(|| {
                    let c = next_class;
                    next_class += 1;
                    c
                });
                partition[i] = class;
            }
        }
        let mut num_classes = next_class;

        // Iteratively refine partitions
        let mut changed = true;
        while changed {
            changed = false;
            let mut new_partition = partition.clone();
            let mut signature_map: std::collections::HashMap<Vec<u32>, u32> =
                std::collections::HashMap::new();
            let mut next_class = 0u32;

            for state in 0..self.state_count {
                // Build signature: (current_class, [transition_class for each byte])
                let current_class = partition[state];
                let mut sig = Vec::with_capacity(self.class_count + 1);
                sig.push(current_class);
                for byte in 0..self.class_count {
                    let idx = state * self.class_count + byte;
                    let target = self.transitions[idx] as usize;
                    let target_class = if target < self.state_count {
                        partition[target]
                    } else {
                        0
                    };
                    sig.push(target_class);
                }

                let class = if let Some(&existing) = signature_map.get(&sig) {
                    existing
                } else {
                    let c = next_class;
                    signature_map.insert(sig, c);
                    next_class += 1;
                    c
                };
                new_partition[state] = class;
            }

            if next_class != num_classes || new_partition != partition {
                changed = true;
                num_classes = next_class;
                partition = new_partition;
            }
        }

        let new_state_count = num_classes as usize;
        if new_state_count >= self.state_count {
            return None; // Already minimal
        }

        // Build minimized table
        let mut new_table = Self::new(new_state_count, self.class_count).ok()?;

        // Set transitions: use the representative state for each class
        let mut class_representative = vec![0usize; new_state_count];
        for (state, &class) in partition.iter().enumerate() {
            class_representative[class as usize] = state;
        }

        for new_state in 0..new_state_count {
            let repr = class_representative[new_state];
            for byte in 0..self.class_count {
                let idx = repr * self.class_count + byte;
                let old_target = self.transitions[idx] as usize;
                let new_target = if old_target < self.state_count {
                    partition[old_target]
                } else {
                    0
                };
                new_table.transitions[new_state * self.class_count + byte] = new_target;
            }
        }

        // Map accept states
        for &(old_state, pattern_id) in &self.accept_states {
            let new_state = partition[old_state as usize];
            if !new_table
                .accept_states
                .iter()
                .any(|&(s, p)| s == new_state && p == pattern_id)
            {
                new_table.add_accept(new_state, pattern_id);
            }
        }

        // Copy pattern lengths
        new_table.pattern_lengths.clone_from(&self.pattern_lengths);

        Some(new_table)
    }
}
