use crate::codegen;
use crate::error::{Error, Result};
use crate::table::TransitionTable;
use matchkit::Match;
#[cfg(feature = "regex")]
use regex_automata::{
    dfa::{dense, Automaton},
    Input, MatchKind,
};
#[cfg(feature = "regex")]
use std::collections::{HashMap, VecDeque};

/// A JIT-compiled DFA that executes pattern matching as native code.
///
/// On non-x86_64 platforms, falls back to an interpreted table-driven scan.
pub struct JitDfa {
    #[cfg(target_arch = "x86_64")]
    code: codegen::ExecutableBuffer,
    #[cfg(not(target_arch = "x86_64"))]
    table: TransitionTable,
    state_count: usize,
    pattern_count: usize,
    /// Aho-Corasick output links for multi-pattern accept chains.
    /// Used by the interpreted fallback on non-x86_64; stored on x86_64 for
    /// potential runtime inspection.
    #[allow(dead_code)]
    output_links: Vec<u32>,
}

impl std::fmt::Debug for JitDfa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitDfa")
            .field("state_count", &self.state_count)
            .field("pattern_count", &self.pattern_count)
            .finish_non_exhaustive()
    }
}

impl JitDfa {
    /// Compile a DFA transition table to native code.
    ///
    /// # Errors
    ///
    /// Returns an error if the transition table is invalid or if
    /// the executable memory allocation fails.
    /// Example: `Error::EmptyDfa` when `table.state_count == 0`
    /// Example: `Error::TooManyStates` when state_count > 4096.
    pub fn compile(table: &TransitionTable) -> Result<Self> {
        Self::compile_with_output_links(table, &[])
    }

    fn compile_with_output_links(table: &TransitionTable, output_links: &[u32]) -> Result<Self> {
        if table.state_count() == 0 {
            return Err(Error::EmptyDfa);
        }

        if table.class_count() != 256 {
            return Err(Error::InvalidTable {
                reason: format!(
                    "class_count must be 256 for JIT, got {}",
                    table.class_count()
                ),
            });
        }

        let expected_len = table
            .state_count()
            .checked_mul(table.class_count())
            .ok_or_else(|| Error::InvalidTable {
                reason: format!(
                    "state_count={} * class_count={} overflows",
                    table.state_count(),
                    table.class_count(),
                ),
            })?;

        if table.transitions().len() != expected_len {
            return Err(Error::InvalidTable {
                reason: format!(
                    "transition table has {} entries but state_count={} * class_count={} = {}",
                    table.transitions().len(),
                    table.state_count(),
                    table.class_count(),
                    expected_len,
                ),
            });
        }

        if !output_links.is_empty() && output_links.len() != table.state_count() {
            return Err(Error::InvalidTable {
                reason: format!(
                    "output_links has {} entries but state_count is {}",
                    output_links.len(),
                    table.state_count()
                ),
            });
        }

        // Build accept-state set for bit-31 validation.
        let accept_set: std::collections::HashSet<u32> =
            table.accept_states().iter().map(|&(s, _)| s).collect();

        for &t in table.transitions() {
            let state = t & 0x7FFF_FFFF;
            if state as usize >= table.state_count() {
                return Err(Error::InvalidTable {
                    reason: format!(
                        "transition target state {state} exceeds state count {}",
                        table.state_count()
                    ),
                });
            }
            if (t & 0x8000_0000) != 0 && !accept_set.contains(&state) {
                return Err(Error::InvalidTable {
                    reason: format!(
                        "transition target state {state} has bit 31 set but is not an accept state"
                    ),
                });
            }
        }

        let mut seen_states = vec![false; table.state_count()];
        let pat_len = table.pattern_lengths().len();
        for &(state, pid) in table.accept_states() {
            if state as usize >= table.state_count() {
                return Err(Error::InvalidTable {
                    reason: format!(
                        "accept state {state} exceeds state count {}",
                        table.state_count()
                    ),
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

        let pattern_count = table
            .accept_states()
            .iter()
            .map(|&(_, pid)| pid as usize + 1)
            .max()
            .unwrap_or(0);

        #[cfg(target_arch = "x86_64")]
        {
            let code = codegen::compile_x86_64(table, output_links)?;
            Ok(Self {
                code,
                state_count: table.state_count(),
                pattern_count,
                output_links: output_links.to_vec(),
            })
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            Ok(Self {
                table: table.clone(),
                state_count: table.state_count(),
                pattern_count,
                output_links: output_links.to_vec(),
            })
        }
    }

    /// Scan input bytes, appending matches to the output vector.
    ///
    /// Returns the number of new matches found.
    pub fn scan(&self, input: &[u8], matches: &mut [Match]) -> usize {
        #[cfg(target_arch = "x86_64")]
        {
            self.code.scan(input, matches)
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            self.scan_interpreted(input, matches)
        }
    }

    /// Interpreted fallback for non-x86_64 platforms.
    #[cfg(not(target_arch = "x86_64"))]
    fn scan_interpreted(&self, input: &[u8], matches: &mut [Match]) -> usize {
        let table = &self.table;
        let mut state = 0u32;
        let mut count = 0usize;

        let mut accept_pattern = vec![0xFFFF_FFFF; table.state_count()];
        for &(s, pid) in table.accept_states() {
            accept_pattern[s as usize] = pid;
        }

        for (pos, &byte) in input.iter().enumerate() {
            let idx = state as usize * table.class_count() + byte as usize;
            let next = table.transitions().get(idx).copied().unwrap_or(0);
            let clean_next = next & 0x7FFF_FFFF;

            if accept_pattern[clean_next as usize] != 0xFFFF_FFFF {
                let mut output_state = clean_next;
                while output_state != 0xFFFF_FFFF {
                    let pid = accept_pattern[output_state as usize];
                    if count < matches.len() {
                        let end = (pos + 1) as u32;
                        let pat_len = table
                            .pattern_lengths()
                            .get(pid as usize)
                            .copied()
                            .unwrap_or(0);
                        let start = end.saturating_sub(pat_len);
                        matches[count] = Match::from_parts(pid, start, end);
                    }
                    count += 1;
                    output_state = self
                        .output_links
                        .get(output_state as usize)
                        .copied()
                        .unwrap_or(0xFFFF_FFFF);
                }
                state = 0;
            } else {
                state = clean_next;
            }
        }
        count.min(matches.len())
    }

    /// Number of DFA states.
    #[must_use]
    pub fn state_count(&self) -> usize {
        self.state_count
    }

    /// Number of patterns recognized.
    #[must_use]
    pub fn pattern_count(&self) -> usize {
        self.pattern_count
    }

    /// Count matches without allocating a match vector.
    #[must_use]
    pub fn scan_count(&self, input: &[u8]) -> usize {
        #[cfg(target_arch = "x86_64")]
        {
            self.code.scan_count(input)
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            self.scan_count_interpreted(input)
        }
    }

    /// Interpreted fallback for non-x86_64 platforms.
    #[cfg(not(target_arch = "x86_64"))]
    fn scan_count_interpreted(&self, input: &[u8]) -> usize {
        let table = &self.table;
        let mut state = 0u32;
        let mut count = 0usize;

        let mut is_accept = vec![false; table.state_count()];
        for &(s, _) in table.accept_states() {
            is_accept[s as usize] = true;
        }

        for &byte in input {
            let idx = state as usize * table.class_count() + byte as usize;
            let next = table.transitions().get(idx).copied().unwrap_or(0);
            let clean_next = next & 0x7FFF_FFFF;

            if is_accept[clean_next as usize] {
                let mut output_state = clean_next;
                while output_state != 0xFFFF_FFFF {
                    count += 1;
                    output_state = self
                        .output_links
                        .get(output_state as usize)
                        .copied()
                        .unwrap_or(0xFFFF_FFFF);
                }
                state = 0;
            } else {
                state = clean_next;
            }
        }
        count
    }

    /// Find the first match, returning immediately without scanning the rest.
    #[must_use]
    pub fn scan_first(&self, input: &[u8]) -> Option<Match> {
        let mut matches = [Match::from_parts(0, 0, 0); 1];
        if self.scan(input, &mut matches) > 0 {
            Some(matches[0])
        } else {
            None
        }
    }

    /// Check if the input contains any match at all.
    #[must_use]
    pub fn has_match(&self, input: &[u8]) -> bool {
        self.scan_first(input).is_some()
    }

    /// Build a JIT DFA from a set of literal patterns.
    ///
    /// Constructs an Aho-Corasick-like DFA where each pattern has its own
    /// accept state, then compiles it to native code.
    ///
    /// # Errors
    ///
    /// Returns an error if compilation fails.
    pub fn from_patterns(patterns: &[&[u8]]) -> Result<Self> {
        if patterns.is_empty() {
            return Err(Error::EmptyDfa);
        }

        let mut state_count = 1usize;
        let mut trans = vec![[0u32; 256]; 1];
        let mut accepts = Vec::new();
        let mut lengths = vec![0; patterns.len()];

        for (pid, pattern) in patterns.iter().enumerate() {
            if pattern.is_empty() {
                continue;
            }
            let mut current = 0u32;
            for &byte in *pattern {
                let next = trans[current as usize][byte as usize];
                if next == 0 {
                    let new_state = state_count as u32;
                    state_count += 1;
                    trans.push([0u32; 256]);
                    trans[current as usize][byte as usize] = new_state;
                    current = new_state;
                } else {
                    current = next;
                }
            }
            accepts.push((current, pid as u32));
            lengths[pid] = pattern.len() as u32;
        }

        let (fail, accepts, output_links) = Self::build_failure_links(&trans, &accepts);
        let table = Self::build_dense_table(&trans, &fail, accepts, lengths)?;
        Self::compile_with_output_links(&table, &output_links)
    }

    fn build_failure_links(
        trans: &[[u32; 256]],
        accepts: &[(u32, u32)],
    ) -> (Vec<u32>, Vec<(u32, u32)>, Vec<u32>) {
        let state_count = trans.len();
        let mut fail = vec![0u32; state_count];
        let mut queue = std::collections::VecDeque::new();

        let mut acc_state = vec![Vec::new(); state_count];
        for &(state, pid) in accepts {
            acc_state[state as usize].push(pid);
        }

        for byte in 0..=255u8 {
            let next = trans[0][byte as usize];
            if next != 0 {
                fail[next as usize] = 0;
                queue.push_back(next);
            }
        }

        while let Some(state) = queue.pop_front() {
            for byte in 0..=255u8 {
                let next = trans[state as usize][byte as usize];
                if next != 0 {
                    queue.push_back(next);
                    let mut f = fail[state as usize];
                    while f != 0 && trans[f as usize][byte as usize] == 0 {
                        f = fail[f as usize];
                    }
                    let n_fail = trans[f as usize][byte as usize];
                    fail[next as usize] = n_fail;
                }
            }
        }

        // Build output links: for each state with a pattern, link to the nearest
        // ancestor via failure links that also has a pattern.
        let mut output_link = vec![0xFFFF_FFFF; state_count];
        for state in 0..state_count {
            if acc_state[state].is_empty() {
                continue;
            }
            let mut f = fail[state];
            while f != 0 {
                if !acc_state[f as usize].is_empty() {
                    output_link[state] = f;
                    break;
                }
                f = fail[f as usize];
            }
        }

        // Propagate failure-link patterns to states that don't have their own.
        for state in 0..state_count {
            if acc_state[state].is_empty() {
                let mut f = fail[state];
                while f != 0 {
                    if !acc_state[f as usize].is_empty() {
                        let pid = acc_state[f as usize][0];
                        acc_state[state].push(pid);
                        break;
                    }
                    f = fail[f as usize];
                }
            }
        }

        let mut final_accepts = Vec::new();
        for (state, pids) in acc_state.into_iter().enumerate() {
            if !pids.is_empty() {
                final_accepts.push((state as u32, pids[0]));
            }
        }

        (fail, final_accepts, output_link)
    }

    fn build_dense_table(
        trans: &[[u32; 256]],
        fail: &[u32],
        accepts: Vec<(u32, u32)>,
        lengths: Vec<u32>,
    ) -> Result<TransitionTable> {
        let state_count = trans.len();
        let mut table = TransitionTable::new(state_count, 256)?;
        for state in 0..state_count {
            for byte in 0..=255u8 {
                let mut current = state as u32;
                loop {
                    let next = trans[current as usize][byte as usize];
                    if next != 0 || current == 0 {
                        table.set_transition(state, byte, next);
                        break;
                    }
                    current = fail[current as usize];
                }
            }
        }
        for (state, pid) in accepts {
            table.add_accept(state, pid);
        }
        for (pid, len) in lengths.into_iter().enumerate() {
            table.set_pattern_length(pid as u32, len);
        }
        Ok(table)
    }

    /// Build a JIT DFA from a set of regex patterns.
    ///
    /// This constructor uses `regex-automata` to compile the patterns into a
    /// dense DFA, then expands its byte classes into a byte-indexed
    /// [`TransitionTable`] that `dfajit` can execute.
    ///
    /// The current engine records the first pattern ID associated with each
    /// accepting state and preserves fixed offsets by using the literal pattern
    /// length as the match width.
    ///
    /// # Errors
    ///
    /// Returns an error if the regex feature is disabled, the regexes fail to
    /// compile, or no start state can be discovered.
    #[cfg(feature = "regex")]
    pub fn from_regex_patterns(patterns: &[&str]) -> Result<Self> {
        if patterns.is_empty() {
            return Err(Error::EmptyDfa);
        }

        let config = dense::Config::new()
            .match_kind(MatchKind::All)
            .starts_for_each_pattern(true);
        let dfa = dense::Builder::new()
            .configure(config)
            .build_many(patterns)
            .map_err(|error| Error::InvalidTable {
                reason: format!("failed to compile regex patterns with regex-automata: {error}"),
            })?;

        let input = Input::new(&[][..]);
        let start_state = dfa
            .start_state_forward(&input)
            .map_err(|error| Error::InvalidTable {
                reason: format!("failed to compute regex DFA start state: {error}"),
            })?;

        let mut state_ids = Vec::new();
        let mut state_map = HashMap::new();
        let mut queue = VecDeque::new();

        state_map.insert(start_state, 0usize);
        state_ids.push(start_state);
        queue.push_back(start_state);

        while let Some(state) = queue.pop_front() {
            for byte in u8::MIN..=u8::MAX {
                let next = dfa.next_state(state, byte);
                if let std::collections::hash_map::Entry::Vacant(e) = state_map.entry(next) {
                    let next_index = state_ids.len();
                    e.insert(next_index);
                    state_ids.push(next);
                    queue.push_back(next);
                }
            }
        }

        let mut table = TransitionTable::new(state_ids.len(), 256)?;
        for (state_index, &state_id) in state_ids.iter().enumerate() {
            for byte in u8::MIN..=u8::MAX {
                let next = dfa.next_state(state_id, byte);
                let next_index =
                    state_map
                        .get(&next)
                        .copied()
                        .ok_or_else(|| Error::InvalidTable {
                            reason: format!(
                                "regex DFA transition to undiscovered state on byte {byte}"
                            ),
                        })?;
                table.set_transition(state_index, byte, next_index as u32);
            }

            let eoi_state = dfa.next_eoi_state(state_id);
            if dfa.is_match_state(eoi_state) {
                for match_index in 0..dfa.match_len(eoi_state) {
                    let pattern_id = dfa.match_pattern(eoi_state, match_index).as_usize() as u32;
                    if !table
                        .accept_states()
                        .iter()
                        .any(|&(state, pid)| state == state_index as u32 && pid == pattern_id)
                    {
                        table.add_accept(state_index as u32, pattern_id);
                    }
                }
            }
        }

        // Regex patterns have variable lengths; fixed pattern lengths cannot be derived
        // from the source string. Setting length to 0 prevents bogus start-offset underflow.
        for pattern_id in 0..patterns.len() {
            table.set_pattern_length(pattern_id as u32, 0);
        }

        Self::compile(&table)
    }

    /// Build a JIT DFA from a set of regex patterns.
    ///
    /// # Errors
    ///
    /// Returns an error when the crate is built without the `regex` feature.
    #[cfg(not(feature = "regex"))]
    pub fn from_regex_patterns(_patterns: &[&str]) -> Result<Self> {
        Err(Error::InvalidTable {
            reason: "regex support is disabled at compile time. Fix: enable the `regex` feature."
                .to_owned(),
        })
    }
}
