//! Hardware value probe: original x87 instruction bytes, relocated into a
//! compiler-owned x86_64 wrapper. Address registers use 64-bit fixture pointers;
//! operand formats and arithmetic opcodes are the pinned original encodings.
//! No simulation uses this program. FXSAVE/FXRSTOR preserve the caller's FPU state.

#[cfg(not(target_arch = "x86_64"))]
compile_error!("The hardware reference probe requires an x86_64 host");

use std::io::{self, BufRead};

#[repr(align(16))]
struct FloatingState([u8; 512]);

fn operand(format: &str, bits: u64) -> [u8; 0x360] {
    let mut memory = [0; 0x360];
    if format == "f32" {
        memory[0x330..0x334].copy_from_slice(&(bits as u32).to_le_bytes());
    } else {
        assert_eq!(format, "f64");
        memory[0x50..0x58].copy_from_slice(&bits.to_le_bytes());
    }
    memory
}

fn run(
    operation: u32,
    left: &[u8],
    right: &[u8],
    left_format: u32,
    right_format: u32,
) -> (u64, u16) {
    let mut output = [0u8; 0x360];
    let mut saved = FloatingState([0; 512]);
    let control: u16 = 0x0e7f;
    let status: u16;
    // Original fragments authenticated by x87_masked_values.py:
    // FLD32 70B743; FLD64 4B1150; FADDP4B1064; FSUBP4223F2;
    // FMUL4B105E; FDIVP4C2147; FCOMPP412073; FSTP32 70B812;
    // FSTP ST0 4B106F; FISTP qword[EAX]7C5F1B.
    unsafe {
        core::arch::asm!(
            "fxsave64 [r12]",
            "push rbp", "push rsi",
            "fninit", "fldcw word ptr [r8]",
            "mov rbp, rcx", "mov rsi, rcx",
            "cmp r10d, 0", "jne 20f",
            ".byte 0xd9,0x86,0x30,0x03,0x00,0x00", "jmp 21f",
            "20:", ".byte 0xdd,0x45,0x50",
            "21:", "cmp r13d, 0", "je 30f", "cmp r13d, 6", "je 36f",
            "mov rbp, rdx", "mov rsi, rdx",
            "cmp r11d, 0", "jne 22f",
            ".byte 0xd9,0x86,0x30,0x03,0x00,0x00", "jmp 23f",
            "22:", ".byte 0xdd,0x45,0x50",
            "23:", "cmp r13d, 1", "je 24f", "cmp r13d, 2", "je 25f",
            "cmp r13d, 3", "je 26f", "cmp r13d, 4", "je 27f",
            ".byte 0xde,0xd9", "jmp 40f",
            "24:", ".byte 0xde,0xc1", "jmp 30f",
            "25:", ".byte 0xde,0xe9", "jmp 30f",
            "26:", ".byte 0xd8,0xc9", "jmp 30f",
            "27:", ".byte 0xde,0xf9",
            "30:", "mov rsi, r9", ".byte 0xd9,0x9e,0x30,0x03,0x00,0x00",
            "cmp r13d, 3", "jne 40f", ".byte 0xdd,0xd8", "jmp 40f",
            "36:", "lea rax, [r9+0x330]", ".byte 0xdf,0x38",
            "40:", "fnstcw word ptr [r9+0x338]", "fnstsw ax", "pop rsi", "pop rbp",
            "fxrstor64 [r12]",
            in("rcx") left.as_ptr(), in("rdx") right.as_ptr(),
            in("r8") &control, in("r9") output.as_mut_ptr(),
            in("r10") left_format, in("r11") right_format,
            in("r12") saved.0.as_mut_ptr(), in("r13") operation,
            out("ax") status,
        );
    }
    assert_eq!(
        u16::from_le_bytes(output[0x338..0x33a].try_into().unwrap()),
        control
    );
    (
        u64::from_le_bytes(output[0x330..0x338].try_into().unwrap()),
        status,
    )
}

fn main() {
    let mut brand = Vec::new();
    for leaf in 0x8000_0002..=0x8000_0004 {
        let value = unsafe { core::arch::x86_64::__cpuid(leaf) };
        for word in [value.eax, value.ebx, value.ecx, value.edx] {
            brand.extend_from_slice(&word.to_le_bytes());
        }
    }
    println!(
        "cpu|{}",
        String::from_utf8_lossy(&brand)
            .trim_matches(char::from(0))
            .trim()
    );
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 6);
        let operation = match fields[1] {
            "load" => 0,
            "add" => 1,
            "sub" => 2,
            "mul" => 3,
            "div" => 4,
            "compare" => 5,
            "ftol" => 6,
            _ => panic!("operation"),
        };
        let lhs = operand(fields[2], u64::from_str_radix(fields[3], 16).unwrap());
        let rhs = operand(fields[4], u64::from_str_radix(fields[5], 16).unwrap());
        let lf = u32::from(fields[2] == "f64");
        let rf = u32::from(fields[4] == "f64");
        let (bits, status) = if operation == 5 {
            // FCOMPP compares ST0 to ST1; load rhs first, then lhs.
            run(operation, &rhs, &lhs, rf, lf)
        } else {
            run(operation, &lhs, &rhs, lf, rf)
        };
        assert_eq!(status & 0x3800, 0, "unbalanced x87 stack");
        println!("{} {bits:016x} {status:04x}", fields[0]);
    }
}
