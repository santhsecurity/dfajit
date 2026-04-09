//! x86_64 JIT codegen for DFA transition tables.
//!
//! Compiles a DFA into native machine code. The generated function scans input
//! bytes, looks up next state from an embedded transition table, and records
//! matches when an accept-state flag (bit 31) is detected. After each match,
//! the DFA state resets to 0 (start state).
//!
//! # Register allocation (inside generated code)
//!
//! | Register | Usage                              |
//! |----------|------------------------------------|
//! | `r12`    | input data pointer                 |
//! | `r13`    | current byte position              |
//! | `r14`    | match output pointer               |
//! | `r15`    | match count                        |
//! | `rbx`    | max matches                        |
//! | `rbp`    | input length                       |
//! | `r11`    | current DFA state (clean, no flags)|
//! | `rax`    | scratch                            |
//! | `rcx`    | scratch                            |
//! | `rdx`    | scratch                            |
//! | `rdi`    | scratch                            |

#[cfg(target_arch = "x86_64")]
use crate::error::{Error, Result};
#[cfg(target_arch = "x86_64")]
use crate::TransitionTable;
#[cfg(target_arch = "x86_64")]
use matchkit::Match;

/// Executable buffer backed by mmap'd memory (W^X: written as RW, flipped to RX).
#[cfg(target_arch = "x86_64")]
pub struct ExecutableBuffer {
    ptr: *mut u8,
    len: usize,
    table: Option<TransitionTable>,
    is_jit: bool,
    accept_pattern: Vec<u32>,
    output_links: Vec<u32>,
}

type JitFn = unsafe extern "sysv64" fn(*const u8, u64, *mut Match, u64) -> u64;

#[cfg(target_arch = "x86_64")]
impl std::fmt::Debug for ExecutableBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutableBuffer")
            .field("len", &self.len)
            .field("is_jit", &self.is_jit)
            .finish_non_exhaustive()
    }
}

#[cfg(target_arch = "x86_64")]
unsafe impl Send for ExecutableBuffer {}
#[cfg(target_arch = "x86_64")]
unsafe impl Sync for ExecutableBuffer {}

#[cfg(target_arch = "x86_64")]
impl Drop for ExecutableBuffer {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.len > 0 {
            // SAFETY: `ptr` was returned by `libc::mmap` in `compile_x86_64` with
            // exactly `len` bytes. We own this region exclusively (Send/Sync impls
            // guarantee no concurrent access) and unmap exactly once here.
            unsafe {
                libc::munmap(self.ptr.cast::<libc::c_void>(), self.len);
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
impl ExecutableBuffer {
    /// Scan input bytes, placing matches directly into the output slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InputTooLong`] when `self.is_jit` and `input.len()` exceeds
    /// `u32::MAX` (the JIT loop uses a 32-bit position counter).
    pub fn scan(&self, input: &[u8], matches: &mut [Match]) -> Result<usize> {
        if self.is_jit && input.len() > u32::MAX as usize {
            return Err(Error::InputTooLong {
                len: input.len(),
                max: u32::MAX as usize,
            });
        }
        if self.is_jit {
            Ok(self.scan_jit(input, matches))
        } else {
            Ok(self.scan_interpreted(input, matches))
        }
    }

    fn scan_jit(&self, input: &[u8], matches: &mut [Match]) -> usize {
        if input.is_empty() {
            return 0;
        }

        let max_matches = matches.len();

        // SAFETY: `self.ptr` points to a valid RX-mapped region containing
        // compiled x86_64 code that conforms to the sysv64 ABI signature
        // `fn(*const u8, u64, *mut Match, u64) -> u64`. The buffer was
        // emitted by `compile_x86_64` and mprotect'd to PROT_READ|PROT_EXEC.
        let func: JitFn = unsafe { std::mem::transmute(self.ptr) };

        // SAFETY: The JIT function reads from `input` (valid slice) and writes
        // at most `max_matches` entries into `matches`.
        // Note: The JIT returns total matches found, which may exceed max_matches.
        // We cap at max_matches to match scan_interpreted behavior.
        let count = unsafe {
            func(
                input.as_ptr(),
                input.len() as u64,
                matches.as_mut_ptr(),
                max_matches as u64,
            )
        };

        // Cap at buffer size - JIT may find more matches than buffer can hold
        (count as usize).min(max_matches)
    }

    /// # Errors
    ///
    /// Returns [`Error::InputTooLong`] when `self.is_jit` and `input.len()` exceeds
    /// `u32::MAX`. See [`Self::scan`].
    pub fn scan_count(&self, input: &[u8]) -> Result<usize> {
        if self.is_jit && input.len() > u32::MAX as usize {
            return Err(Error::InputTooLong {
                len: input.len(),
                max: u32::MAX as usize,
            });
        }
        if self.is_jit {
            Ok(self.scan_count_jit(input))
        } else {
            Ok(self.scan_count_interpreted(input))
        }
    }

    fn scan_count_jit(&self, input: &[u8]) -> usize {
        if input.is_empty() {
            return 0;
        }
        let func: JitFn = unsafe { std::mem::transmute(self.ptr) };
        let count = unsafe { func(input.as_ptr(), input.len() as u64, std::ptr::null_mut(), 0) };
        count as usize
    }

    fn scan_count_interpreted(&self, input: &[u8]) -> usize {
        let Some(table) = self.table.as_ref() else {
            return 0;
        };
        let mut state = 0u32;
        let mut count = 0usize;

        for &byte in input {
            let idx = state as usize * table.class_count() + byte as usize;
            let next = table.transitions().get(idx).copied().unwrap_or(0);
            let clean_next = next & 0x7FFF_FFFF;

            if self
                .accept_pattern
                .get(clean_next as usize)
                .copied()
                .unwrap_or(0xFFFF_FFFF)
                != 0xFFFF_FFFF
            {
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

    fn scan_interpreted(&self, input: &[u8], matches: &mut [Match]) -> usize {
        let Some(table) = self.table.as_ref() else {
            return 0;
        };
        let mut state = 0u32;
        let mut count = 0usize;

        for (pos, &byte) in input.iter().enumerate() {
            let idx = state as usize * table.class_count() + byte as usize;
            let next = table.transitions().get(idx).copied().unwrap_or(0);
            let clean_next = next & 0x7FFF_FFFF;

            if self
                .accept_pattern
                .get(clean_next as usize)
                .copied()
                .unwrap_or(0xFFFF_FFFF)
                != 0xFFFF_FFFF
            {
                let mut output_state = clean_next;
                while output_state != 0xFFFF_FFFF {
                    let pid = self.accept_pattern[output_state as usize];
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
}

/// Maximum states for JIT (I-cache ≈ 32KB).
#[cfg(target_arch = "x86_64")]
const MAX_JIT_STATES: usize = 4096;

/// Compile a DFA transition table to native x86_64 machine code.
#[cfg(target_arch = "x86_64")]
pub fn compile_x86_64(table: &TransitionTable, output_links: &[u32]) -> Result<ExecutableBuffer> {
    if table.state_count() > 65_536 {
        return Err(Error::TooManyStates {
            states: table.state_count(),
            max: 65_536,
        });
    }
    if table.state_count() > MAX_JIT_STATES {
        return compile_interpreted_fallback(table, output_links);
    }

    // Build flagged transition table: bit 31 marks accept-state targets.
    let mut flagged = table.transitions().to_vec();
    let mut accept_pattern: Vec<u32> = vec![0xFFFF_FFFF; table.state_count()];
    for &(state, pattern_id) in table.accept_states() {
        if (state as usize) < accept_pattern.len() {
            accept_pattern[state as usize] = pattern_id;
        }
    }
    for t in &mut flagged {
        let target = (*t & 0x7FFF_FFFF) as usize;
        if target < accept_pattern.len() && accept_pattern[target] != 0xFFFF_FFFF {
            *t = target as u32 | 0x8000_0000;
        } else {
            *t = target as u32;
        }
    }

    let mut output_link = output_links.to_vec();
    if output_link.len() < table.state_count() {
        output_link.resize(table.state_count(), 0xFFFF_FFFF);
    }

    let mut c: Vec<u8> = Vec::with_capacity(4096);

    // Prologue: save callee-saved registers
    c.extend_from_slice(&[0x53]); // push rbx
    c.extend_from_slice(&[0x55]); // push rbp
    c.extend_from_slice(&[0x41, 0x54]); // push r12
    c.extend_from_slice(&[0x41, 0x55]); // push r13
    c.extend_from_slice(&[0x41, 0x56]); // push r14
    c.extend_from_slice(&[0x41, 0x57]); // push r15

    // Shuffle arguments:  rdi→r12, rsi→rbp, rdx→r14, rcx→rbx
    c.extend_from_slice(&[0x49, 0x89, 0xFC]); // mov r12, rdi  (input ptr)
    c.extend_from_slice(&[0x48, 0x89, 0xF5]); // mov rbp, rsi  (input len)
    c.extend_from_slice(&[0x49, 0x89, 0xD6]); // mov r14, rdx  (match buf)
    c.extend_from_slice(&[0x48, 0x89, 0xCB]); // mov rbx, rcx  (max matches)

    // Zero working registers
    c.extend_from_slice(&[0x45, 0x31, 0xED]); // xor r13d, r13d  (position=0)
    c.extend_from_slice(&[0x45, 0x31, 0xFF]); // xor r15d, r15d  (match_count=0)
    c.extend_from_slice(&[0x45, 0x31, 0xDB]); // xor r11d, r11d  (state=0)

    // if (pos >= len) goto epilogue
    c.extend_from_slice(&[0x49, 0x39, 0xED]); // cmp r13, rbp
    c.extend_from_slice(&[0x0F, 0x83]); // jae rel32
    let empty_patch = c.len();
    c.extend_from_slice(&[0; 4]);

    // === SCAN LOOP TOP ===
    let scan_top = c.len();

    // movzx eax, byte [r12 + r13*1]
    c.extend_from_slice(&[0x43, 0x0F, 0xB6, 0x04, 0x2C]);

    // imul edx, r11d, <class_count>
    c.extend_from_slice(&[0x41, 0x69, 0xD3]);
    c.extend_from_slice(&(table.class_count() as u32).to_le_bytes());

    // add edx, eax
    c.extend_from_slice(&[0x01, 0xC2]);

    // mov rdi, <transition_table_address>  (patched later)
    let trans_patch = c.len();
    c.push(0x48);
    c.push(0xBF);
    c.extend_from_slice(&[0; 8]);

    // mov eax, [rdi + rdx*4]
    c.extend_from_slice(&[0x8B, 0x04, 0x97]);

    // Save raw (flagged) value in ecx, strip bit 31 into r11d (clean state)
    c.extend_from_slice(&[0x89, 0xC1]); // mov ecx, eax
    c.push(0x25);
    c.extend_from_slice(&0x7FFF_FFFFu32.to_le_bytes()); // and eax, 0x7FFFFFFF
    c.extend_from_slice(&[0x41, 0x89, 0xC3]); // mov r11d, eax

    // test ecx, 0x80000000  (was it an accept state?)
    c.extend_from_slice(&[0xF7, 0xC1]);
    c.extend_from_slice(&0x8000_0000u32.to_le_bytes());

    // jz skip_match
    c.extend_from_slice(&[0x0F, 0x84]);
    let skip_match_patch = c.len();
    c.extend_from_slice(&[0; 4]);

    // --- Accept state: follow output-link chain ---
    // mov r8d, r11d  (first output state = current DFA state)
    c.extend_from_slice(&[0x45, 0x89, 0xD8]);

    let accept_loop = c.len();

    // Check match_count < max_matches: cmp r15, rbx
    c.extend_from_slice(&[0x49, 0x39, 0xDF]); // cmp r15, rbx
    c.extend_from_slice(&[0x0F, 0x83]); // jae skip_write_match
    let skip_write_match_patch = c.len();
    c.extend_from_slice(&[0; 4]);

    // Load pattern_id: accept_table[r8d]
    let accept_patch = c.len();
    c.push(0x48);
    c.push(0xBF); // mov rdi, <accept_table_addr>
    c.extend_from_slice(&[0; 8]);
    c.extend_from_slice(&[0x42, 0x8B, 0x04, 0x87]); // mov eax, [rdi + r8*4]

    // Compute output address: match_buf + match_count * 16 (sizeof(Match))
    c.extend_from_slice(&[0x4C, 0x89, 0xFF]); // mov rdi, r15
    c.extend_from_slice(&[0x48, 0x6B, 0xFF, 0x10]); // imul rdi, rdi, 16
    c.extend_from_slice(&[0x4C, 0x01, 0xF7]); // add rdi, r14

    // Write pattern_id: mov [rdi], eax
    c.extend_from_slice(&[0x89, 0x07]);

    // Save pattern_id for length lookup
    c.extend_from_slice(&[0x89, 0xC1]); // mov ecx, eax

    // Load pattern length
    let patlen_patch = c.len();
    c.push(0x48);
    c.push(0xBA); // mov rdx, <patlen_table_addr>
    c.extend_from_slice(&[0; 8]);
    c.extend_from_slice(&[0x8B, 0x0C, 0x8A]); // mov ecx, [rdx + rcx*4]

    // end = pos + 1
    c.extend_from_slice(&[0x44, 0x89, 0xEA]); // mov edx, r13d
    c.extend_from_slice(&[0x83, 0xC2, 0x01]); // add edx, 1
    c.extend_from_slice(&[0x89, 0x57, 0x08]); // mov [rdi+8], edx  (end)

    // start = max(0, end - pattern_length)
    c.extend_from_slice(&[0x29, 0xCA]); // sub edx, ecx
    c.extend_from_slice(&[0x73, 0x02]); // jnc +2 (no underflow)
    c.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    c.extend_from_slice(&[0x89, 0x57, 0x04]); // mov [rdi+4], edx  (start)

    // skip_write_match:
    let skip_write_match_target = c.len();
    patch_rel32(&mut c, skip_write_match_patch, skip_write_match_target);

    // Increment match count
    c.extend_from_slice(&[0x49, 0xFF, 0xC7]); // inc r15

    // Load next output link: output_link[r8d]
    let output_patch = c.len();
    c.push(0x48);
    c.push(0xBF); // mov rdi, <output_link_addr>
    c.extend_from_slice(&[0; 8]);
    c.extend_from_slice(&[0x46, 0x8B, 0x04, 0x87]); // mov r8d, [rdi + r8*4]

    // cmp r8d, 0xFFFFFFFF
    c.extend_from_slice(&[0x41, 0x81, 0xF8]);
    c.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());

    // jne accept_loop
    c.extend_from_slice(&[0x0F, 0x85]);
    let accept_loop_patch = c.len();
    c.extend_from_slice(&[0; 4]);
    patch_rel32(&mut c, accept_loop_patch, accept_loop);

    // Reset DFA state to 0 after match
    c.extend_from_slice(&[0x45, 0x31, 0xDB]); // xor r11d, r11d

    // skip_match:
    let skip_match_target = c.len();
    patch_rel32(&mut c, skip_match_patch, skip_match_target);

    // Advance position: inc r13
    c.extend_from_slice(&[0x49, 0xFF, 0xC5]);

    // Prefetch the next cache line of input before the loop-back branch.
    c.extend_from_slice(&[0x43, 0x0F, 0x18, 0x44, 0x2C, 0x40]);

    // Loop: cmp r13, rbp; jb scan_top
    c.extend_from_slice(&[0x49, 0x39, 0xED]); // cmp r13, rbp
    c.extend_from_slice(&[0x0F, 0x82]); // jb rel32
    let loop_patch = c.len();
    c.extend_from_slice(&[0; 4]);
    patch_rel32(&mut c, loop_patch, scan_top);

    // === EPILOGUE ===
    let epilogue = c.len();
    patch_rel32(&mut c, empty_patch, epilogue);

    c.extend_from_slice(&[0x4C, 0x89, 0xF8]); // mov rax, r15 (return count)
    c.extend_from_slice(&[0x41, 0x5F]); // pop r15
    c.extend_from_slice(&[0x41, 0x5E]); // pop r14
    c.extend_from_slice(&[0x41, 0x5D]); // pop r13
    c.extend_from_slice(&[0x41, 0x5C]); // pop r12
    c.push(0x5D); // pop rbp
    c.push(0x5B); // pop rbx
    c.push(0xC3); // ret

    // === DATA SECTION (8-byte aligned) ===
    while c.len() % 8 != 0 {
        c.push(0xCC);
    }

    let trans_offset = c.len();
    for &t in &flagged {
        c.extend_from_slice(&t.to_le_bytes());
    }

    let accept_offset = c.len();
    for &p in &accept_pattern {
        c.extend_from_slice(&p.to_le_bytes());
    }

    let patlen_offset = c.len();
    if table.pattern_lengths().is_empty() {
        c.extend_from_slice(&0u32.to_le_bytes());
    } else {
        for &l in table.pattern_lengths() {
            c.extend_from_slice(&l.to_le_bytes());
        }
    }

    let output_offset = c.len();
    for &o in &output_link {
        c.extend_from_slice(&o.to_le_bytes());
    }

    // Allocate RW memory
    let page_size = 4096usize;
    let alloc_size = (c.len() + page_size - 1) & !(page_size - 1);

    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            alloc_size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(Error::MemoryAllocation {
            reason: format!(
                "mmap(RW, {alloc_size}) failed: {}",
                std::io::Error::last_os_error()
            ),
        });
    }

    let buf = ptr.cast::<u8>();
    unsafe {
        std::ptr::copy_nonoverlapping(c.as_ptr(), buf, c.len());
    }

    // Patch absolute addresses
    let base = buf as u64;
    patch_imm64(&mut c, buf, trans_patch + 2, base + trans_offset as u64);
    patch_imm64(&mut c, buf, accept_patch + 2, base + accept_offset as u64);
    patch_imm64(&mut c, buf, patlen_patch + 2, base + patlen_offset as u64);
    patch_imm64(&mut c, buf, output_patch + 2, base + output_offset as u64);

    let prot = unsafe { libc::mprotect(ptr, alloc_size, libc::PROT_READ | libc::PROT_EXEC) };
    if prot != 0 {
        unsafe {
            libc::munmap(ptr, alloc_size);
        }
        return Err(Error::MemoryAllocation {
            reason: format!("mprotect(RX) failed: {}", std::io::Error::last_os_error()),
        });
    }

    Ok(ExecutableBuffer {
        ptr: buf,
        len: alloc_size,
        table: None,
        is_jit: true,
        accept_pattern,
        output_links: output_link,
    })
}

#[cfg(target_arch = "x86_64")]
fn patch_rel32(code: &mut [u8], site: usize, target: usize) {
    let rel = target as isize - (site + 4) as isize;
    let rel = i32::try_from(rel).unwrap_or(0);
    // In our JIT, code will never exceed 2GB so rel32 is always safe.
    // If it did exceed, returning 0 would break the jump, but we bound states max < 65536
    // which generates well under a megabyte of code.
    code[site..site + 4].copy_from_slice(&rel.to_le_bytes());
}

#[cfg(target_arch = "x86_64")]
fn patch_imm64(code: &mut [u8], buf: *mut u8, offset: usize, value: u64) {
    let bytes = value.to_le_bytes();
    code[offset..offset + 8].copy_from_slice(&bytes);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.add(offset), 8);
    }
}

#[cfg(target_arch = "x86_64")]
fn compile_interpreted_fallback(
    table: &TransitionTable,
    output_links: &[u32],
) -> Result<ExecutableBuffer> {
    const FALLBACK_CODE: [u8; 1] = [0xC3]; // ret

    let page_size = 4096usize;
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            page_size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(Error::MemoryAllocation {
            reason: format!("mmap failed: {}", std::io::Error::last_os_error()),
        });
    }

    unsafe {
        std::ptr::copy_nonoverlapping(
            FALLBACK_CODE.as_ptr(),
            ptr.cast::<u8>(),
            FALLBACK_CODE.len(),
        );
    }

    let prot = unsafe { libc::mprotect(ptr, page_size, libc::PROT_READ | libc::PROT_EXEC) };
    if prot != 0 {
        unsafe {
            libc::munmap(ptr, page_size);
        }
        return Err(Error::MemoryAllocation {
            reason: format!("mprotect failed: {}", std::io::Error::last_os_error()),
        });
    }

    let mut accept_pattern = vec![0xFFFF_FFFF; table.state_count()];
    for &(state, pid) in table.accept_states() {
        if (state as usize) < accept_pattern.len() {
            accept_pattern[state as usize] = pid;
        }
    }

    let mut output_link = output_links.to_vec();
    if output_link.len() < table.state_count() {
        output_link.resize(table.state_count(), 0xFFFF_FFFF);
    }

    Ok(ExecutableBuffer {
        ptr: ptr.cast::<u8>(),
        len: page_size,
        table: Some(table.clone()),
        is_jit: false,
        accept_pattern,
        output_links: output_link,
    })
}
