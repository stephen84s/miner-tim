// SuperscalarHash program generation and execution
// Reference: RandomX src/superscalar.cpp

use super::blake2gen::Blake2Generator;

/// A single SuperscalarHash instruction (8 bytes).
#[derive(Clone, Debug, Default)]
pub struct SuperscalarInstruction {
    pub opcode: u8,
    pub dst: u8,
    pub src: u8,
    pub mod_: u8,
    pub imm32: u32,
}

/// A SuperscalarHash program.
#[derive(Clone, Debug)]
pub struct SuperscalarProgram {
    pub instructions: Vec<SuperscalarInstruction>,
    pub address_register: usize,
}

/// Reciprocal function: calculates 2^x / divisor for highest x such that result < 2^64.
pub fn randomx_reciprocal(divisor: u32) -> u64 {
    assert!(divisor != 0);
    let p2exp63: u64 = 1u64 << 63;
    let q = p2exp63 / divisor as u64;
    let r = p2exp63 % divisor as u64;
    let shift = 64 - (divisor as u64).leading_zeros();
    (q << shift) + ((r << shift) / divisor as u64)
}

// ============================================================================
// Constants
// ============================================================================

const RANDOMX_SUPERSCALAR_LATENCY: i32 = 170;
const SUPERSCALAR_MAX_SIZE: i32 = 3 * RANDOMX_SUPERSCALAR_LATENCY + 2; // 512
const REGISTER_NEEDS_DISPLACEMENT: i32 = 5;
const CYCLE_MAP_SIZE: usize = (RANDOMX_SUPERSCALAR_LATENCY + 4) as usize; // 174
const LOOK_FORWARD_CYCLES: i32 = 4;
const MAX_THROWAWAY_COUNT: i32 = 256;

// ============================================================================
// Execution Ports (bitmask)
// ============================================================================

const PORT_NULL: i32 = 0;
const PORT_P0: i32 = 1;
const PORT_P1: i32 = 2;
const PORT_P5: i32 = 4;
const PORT_P01: i32 = PORT_P0 | PORT_P1;
const PORT_P05: i32 = PORT_P0 | PORT_P5;
const PORT_P015: i32 = PORT_P0 | PORT_P1 | PORT_P5;

// ============================================================================
// Instruction types
// ============================================================================

// Variant names mirror the RandomX spec / reference implementation.
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
enum SsType {
    ISUB_R = 0,
    IXOR_R = 1,
    IADD_RS = 2,
    IMUL_R = 3,
    IROR_C = 4,
    IADD_C7 = 5,
    IXOR_C7 = 6,
    IADD_C8 = 7,
    IXOR_C8 = 8,
    IADD_C9 = 9,
    IXOR_C9 = 10,
    IMULH_R = 11,
    ISMULH_R = 12,
    IMUL_RCP = 13,
    INVALID = -1,
}

fn is_multiplication(t: SsType) -> bool {
    matches!(t, SsType::IMUL_R | SsType::IMULH_R | SsType::ISMULH_R | SsType::IMUL_RCP)
}

// ============================================================================
// MacroOp
// ============================================================================

#[derive(Clone, Copy)]
struct MacroOp {
    size: i32,
    latency: i32,
    uop1: i32,
    uop2: i32,
    dependent: bool,
}

impl MacroOp {
    const fn new1(size: i32, latency: i32, uop: i32) -> Self {
        MacroOp { size, latency, uop1: uop, uop2: PORT_NULL, dependent: false }
    }
    const fn new2(size: i32, latency: i32, uop1: i32, uop2: i32) -> Self {
        MacroOp { size, latency, uop1, uop2, dependent: false }
    }
    const fn eliminated(size: i32) -> Self {
        MacroOp { size, latency: 0, uop1: PORT_NULL, uop2: PORT_NULL, dependent: false }
    }
    const fn with_dependent(mut self) -> Self {
        self.dependent = true;
        self
    }
    fn is_eliminated(&self) -> bool { self.uop1 == PORT_NULL }
    fn is_simple(&self) -> bool { self.uop2 == PORT_NULL }
}

const MOP_SUB_RR: MacroOp = MacroOp::new1(3, 1, PORT_P015);
const MOP_XOR_RR: MacroOp = MacroOp::new1(3, 1, PORT_P015);
const MOP_IMUL_R: MacroOp = MacroOp::new2(3, 4, PORT_P1, PORT_P5);
const MOP_MUL_R: MacroOp = MacroOp::new2(3, 4, PORT_P1, PORT_P5);
const MOP_MOV_RR: MacroOp = MacroOp::eliminated(3);
const MOP_LEA_SIB: MacroOp = MacroOp::new1(4, 1, PORT_P01);
const MOP_IMUL_RR: MacroOp = MacroOp::new1(4, 3, PORT_P1);
const MOP_ROR_RI: MacroOp = MacroOp::new1(4, 1, PORT_P05);
const MOP_ADD_RI: MacroOp = MacroOp::new1(7, 1, PORT_P015);
const MOP_XOR_RI: MacroOp = MacroOp::new1(7, 1, PORT_P015);
const MOP_MOV_RI64: MacroOp = MacroOp::new1(10, 1, PORT_P015);

// ============================================================================
// InstructionInfo - describes the macro-ops for each SuperscalarInstruction type
// ============================================================================

struct InstructionInfo {
    inst_type: SsType,
    ops: &'static [MacroOp],
    #[allow(dead_code)] // mirrors the reference implementation's table layout
    latency: i32,
    result_op: i32,
    dst_op: i32,
    src_op: i32,
}

const INFO_ISUB_R: InstructionInfo = InstructionInfo {
    inst_type: SsType::ISUB_R, ops: &[MOP_SUB_RR], latency: 1, result_op: 0, dst_op: 0, src_op: 0,
};
const INFO_IXOR_R: InstructionInfo = InstructionInfo {
    inst_type: SsType::IXOR_R, ops: &[MOP_XOR_RR], latency: 1, result_op: 0, dst_op: 0, src_op: 0,
};
const INFO_IADD_RS: InstructionInfo = InstructionInfo {
    inst_type: SsType::IADD_RS, ops: &[MOP_LEA_SIB], latency: 1, result_op: 0, dst_op: 0, src_op: 0,
};
const INFO_IMUL_R: InstructionInfo = InstructionInfo {
    inst_type: SsType::IMUL_R, ops: &[MOP_IMUL_RR], latency: 3, result_op: 0, dst_op: 0, src_op: 0,
};
const INFO_IROR_C: InstructionInfo = InstructionInfo {
    inst_type: SsType::IROR_C, ops: &[MOP_ROR_RI], latency: 1, result_op: 0, dst_op: 0, src_op: -1,
};
const INFO_IADD_C7: InstructionInfo = InstructionInfo {
    inst_type: SsType::IADD_C7, ops: &[MOP_ADD_RI], latency: 1, result_op: 0, dst_op: 0, src_op: -1,
};
const INFO_IXOR_C7: InstructionInfo = InstructionInfo {
    inst_type: SsType::IXOR_C7, ops: &[MOP_XOR_RI], latency: 1, result_op: 0, dst_op: 0, src_op: -1,
};
const INFO_IADD_C8: InstructionInfo = InstructionInfo {
    inst_type: SsType::IADD_C8, ops: &[MOP_ADD_RI], latency: 1, result_op: 0, dst_op: 0, src_op: -1,
};
const INFO_IXOR_C8: InstructionInfo = InstructionInfo {
    inst_type: SsType::IXOR_C8, ops: &[MOP_XOR_RI], latency: 1, result_op: 0, dst_op: 0, src_op: -1,
};
const INFO_IADD_C9: InstructionInfo = InstructionInfo {
    inst_type: SsType::IADD_C9, ops: &[MOP_ADD_RI], latency: 1, result_op: 0, dst_op: 0, src_op: -1,
};
const INFO_IXOR_C9: InstructionInfo = InstructionInfo {
    inst_type: SsType::IXOR_C9, ops: &[MOP_XOR_RI], latency: 1, result_op: 0, dst_op: 0, src_op: -1,
};

// Multi-op instructions need const arrays
const IMULH_R_OPS: [MacroOp; 3] = [MOP_MOV_RR, MOP_MUL_R, MOP_MOV_RR];
const ISMULH_R_OPS: [MacroOp; 3] = [MOP_MOV_RR, MOP_IMUL_R, MOP_MOV_RR];
// IMUL_RCP: Mov_ri64 + Imul_rr(dependent)
const IMUL_RCP_OPS: [MacroOp; 2] = [MOP_MOV_RI64, MOP_IMUL_RR.with_dependent()];

const INFO_IMULH_R: InstructionInfo = InstructionInfo {
    inst_type: SsType::IMULH_R, ops: &IMULH_R_OPS, latency: 4, result_op: 1, dst_op: 0, src_op: 1,
};
const INFO_ISMULH_R: InstructionInfo = InstructionInfo {
    inst_type: SsType::ISMULH_R, ops: &ISMULH_R_OPS, latency: 4, result_op: 1, dst_op: 0, src_op: 1,
};
const INFO_IMUL_RCP: InstructionInfo = InstructionInfo {
    inst_type: SsType::IMUL_RCP, ops: &IMUL_RCP_OPS, latency: 4, result_op: 1, dst_op: 1, src_op: -1,
};
const INFO_NOP: InstructionInfo = InstructionInfo {
    inst_type: SsType::INVALID, ops: &[], latency: 0, result_op: 0, dst_op: 0, src_op: 0,
};

// ============================================================================
// Decode Buffers
// ============================================================================

struct DecodeBuffer {
    slots: &'static [i32],
    index: i32, // fetchType
}

const BUF_484: DecodeBuffer = DecodeBuffer { slots: &[4, 8, 4], index: 0 };
const BUF_7333: DecodeBuffer = DecodeBuffer { slots: &[7, 3, 3, 3], index: 1 };
const BUF_3733: DecodeBuffer = DecodeBuffer { slots: &[3, 7, 3, 3], index: 2 };
const BUF_493: DecodeBuffer = DecodeBuffer { slots: &[4, 9, 3], index: 3 };
const BUF_4444: DecodeBuffer = DecodeBuffer { slots: &[4, 4, 4, 4], index: 4 };
const BUF_3310: DecodeBuffer = DecodeBuffer { slots: &[3, 3, 10], index: 5 };

const DEFAULT_BUFFERS: [&DecodeBuffer; 4] = [&BUF_484, &BUF_7333, &BUF_3733, &BUF_493];

fn fetch_next(prev_type: SsType, decode_cycle: i32, mul_count: i32, generator: &mut Blake2Generator) -> &'static DecodeBuffer {
    if prev_type == SsType::IMULH_R || prev_type == SsType::ISMULH_R {
        return &BUF_3310;
    }
    if mul_count < decode_cycle + 1 {
        return &BUF_4444;
    }
    if prev_type == SsType::IMUL_RCP {
        return if (generator.get_byte() & 1) != 0 { &BUF_484 } else { &BUF_493 };
    }
    DEFAULT_BUFFERS[(generator.get_byte() & 3) as usize]
}

// ============================================================================
// Slot selection
// ============================================================================

fn select_for_slot(generator: &mut Blake2Generator, slot_size: i32, fetch_type: i32, is_last: bool) -> &'static InstructionInfo {
    match slot_size {
        3 => {
            if is_last {
                match generator.get_byte() & 3 {
                    0 => &INFO_ISUB_R,
                    1 => &INFO_IXOR_R,
                    2 => &INFO_IMULH_R,
                    3 => &INFO_ISMULH_R,
                    _ => unreachable!(),
                }
            } else {
                if (generator.get_byte() & 1) == 0 { &INFO_ISUB_R } else { &INFO_IXOR_R }
            }
        }
        4 => {
            if fetch_type == 4 && !is_last {
                &INFO_IMUL_R
            } else {
                if (generator.get_byte() & 1) == 0 { &INFO_IROR_C } else { &INFO_IADD_RS }
            }
        }
        7 => if (generator.get_byte() & 1) == 0 { &INFO_IXOR_C7 } else { &INFO_IADD_C7 },
        8 => if (generator.get_byte() & 1) == 0 { &INFO_IXOR_C8 } else { &INFO_IADD_C8 },
        9 => if (generator.get_byte() & 1) == 0 { &INFO_IXOR_C9 } else { &INFO_IADD_C9 },
        10 => &INFO_IMUL_RCP,
        _ => unreachable!(),
    }
}

// ============================================================================
// Working instruction during generation
// ============================================================================

struct WorkingInstruction {
    info: &'static InstructionInfo,
    src: i32,
    dst: i32,
    mod_: u8,
    imm32: u32,
    op_group: SsType,
    op_group_par: i32,
    can_reuse: bool,
    group_par_is_source: bool,
}

impl WorkingInstruction {
    fn null() -> Self {
        WorkingInstruction {
            info: &INFO_NOP,
            src: -1,
            dst: -1,
            mod_: 0,
            imm32: 0,
            op_group: SsType::INVALID,
            op_group_par: -1,
            can_reuse: false,
            group_par_is_source: false,
        }
    }

    fn create(info: &'static InstructionInfo, generator: &mut Blake2Generator) -> Self {
        let mut inst = WorkingInstruction {
            info,
            src: -1,
            dst: -1,
            mod_: 0,
            imm32: 0,
            op_group: SsType::INVALID,
            op_group_par: -1,
            can_reuse: false,
            group_par_is_source: false,
        };

        match info.inst_type {
            SsType::ISUB_R => {
                inst.op_group = SsType::IADD_RS;
                inst.group_par_is_source = true;
            }
            SsType::IXOR_R => {
                inst.op_group = SsType::IXOR_R;
                inst.group_par_is_source = true;
            }
            SsType::IADD_RS => {
                inst.mod_ = generator.get_byte();
                inst.op_group = SsType::IADD_RS;
                inst.group_par_is_source = true;
            }
            SsType::IMUL_R => {
                inst.op_group = SsType::IMUL_R;
                inst.group_par_is_source = true;
            }
            SsType::IROR_C => {
                loop {
                    inst.imm32 = (generator.get_byte() & 63) as u32;
                    if inst.imm32 != 0 { break; }
                }
                inst.op_group = SsType::IROR_C;
                inst.op_group_par = -1;
            }
            SsType::IADD_C7 | SsType::IADD_C8 | SsType::IADD_C9 => {
                inst.imm32 = generator.get_u32();
                inst.op_group = SsType::IADD_C7;
                inst.op_group_par = -1;
            }
            SsType::IXOR_C7 | SsType::IXOR_C8 | SsType::IXOR_C9 => {
                inst.imm32 = generator.get_u32();
                inst.op_group = SsType::IXOR_C7;
                inst.op_group_par = -1;
            }
            SsType::IMULH_R => {
                inst.can_reuse = true;
                inst.op_group = SsType::IMULH_R;
                inst.op_group_par = generator.get_u32() as i32;
            }
            SsType::ISMULH_R => {
                inst.can_reuse = true;
                inst.op_group = SsType::ISMULH_R;
                inst.op_group_par = generator.get_u32() as i32;
            }
            SsType::IMUL_RCP => {
                loop {
                    inst.imm32 = generator.get_u32();
                    if !is_zero_or_power_of_2(inst.imm32) { break; }
                }
                inst.op_group = SsType::IMUL_RCP;
                inst.op_group_par = -1;
            }
            _ => {}
        }

        inst
    }

    fn select_source(&mut self, cycle: i32, registers: &[RegisterInfo; 8], generator: &mut Blake2Generator) -> bool {
        let mut available: Vec<i32> = Vec::new();
        for i in 0..8 {
            if registers[i].latency <= cycle {
                available.push(i as i32);
            }
        }
        // IADD_RS special case: if exactly 2 available and one is r5, force r5 as source
        if available.len() == 2 && self.info.inst_type == SsType::IADD_RS
            && (available[0] == REGISTER_NEEDS_DISPLACEMENT || available[1] == REGISTER_NEEDS_DISPLACEMENT) {
                self.op_group_par = REGISTER_NEEDS_DISPLACEMENT;
                self.src = REGISTER_NEEDS_DISPLACEMENT;
                return true;
            }
        if let Some(reg) = select_register(&available, generator) {
            self.src = reg;
            if self.group_par_is_source {
                self.op_group_par = self.src;
            }
            true
        } else {
            false
        }
    }

    fn select_destination(&mut self, cycle: i32, allow_chained_mul: bool, registers: &[RegisterInfo; 8], generator: &mut Blake2Generator) -> bool {
        let mut available: Vec<i32> = Vec::new();
        for i in 0..8u32 {
            let ii = i as i32;
            if registers[i as usize].latency <= cycle
                && (self.can_reuse || ii != self.src)
                && (allow_chained_mul || self.op_group != SsType::IMUL_R || registers[i as usize].last_op_group != SsType::IMUL_R)
                && (registers[i as usize].last_op_group != self.op_group || registers[i as usize].last_op_par != self.op_group_par)
                && (self.info.inst_type != SsType::IADD_RS || ii != REGISTER_NEEDS_DISPLACEMENT)
            {
                available.push(ii);
            }
        }
        if let Some(reg) = select_register(&available, generator) {
            self.dst = reg;
            true
        } else {
            false
        }
    }

    fn to_output(&self) -> SuperscalarInstruction {
        SuperscalarInstruction {
            opcode: self.info.inst_type as u8,
            dst: self.dst as u8,
            src: if self.src >= 0 { self.src as u8 } else { self.dst as u8 },
            mod_: self.mod_,
            imm32: self.imm32,
        }
    }
}

fn select_register(available: &[i32], generator: &mut Blake2Generator) -> Option<i32> {
    if available.is_empty() {
        return None;
    }
    let index = if available.len() > 1 {
        (generator.get_u32() % available.len() as u32) as usize
    } else {
        0
    };
    Some(available[index])
}

fn is_zero_or_power_of_2(x: u32) -> bool {
    x == 0 || (x & (x - 1)) == 0
}

// ============================================================================
// Register tracking during generation
// ============================================================================

#[derive(Clone)]
struct RegisterInfo {
    latency: i32,
    last_op_group: SsType,
    last_op_par: i32,
}

impl RegisterInfo {
    fn new() -> Self {
        RegisterInfo {
            latency: 0,
            last_op_group: SsType::INVALID,
            last_op_par: -1,
        }
    }
}

// ============================================================================
// Scheduling
// ============================================================================

fn schedule_uop(uop: i32, port_busy: &mut [[i32; 3]; CYCLE_MAP_SIZE], cycle: i32, commit: bool) -> i32 {
    let mut c = cycle as usize;
    while c < CYCLE_MAP_SIZE {
        if (uop & PORT_P5) != 0 && port_busy[c][2] == 0 {
            if commit { port_busy[c][2] = uop; }
            return c as i32;
        }
        if (uop & PORT_P0) != 0 && port_busy[c][0] == 0 {
            if commit { port_busy[c][0] = uop; }
            return c as i32;
        }
        if (uop & PORT_P1) != 0 && port_busy[c][1] == 0 {
            if commit { port_busy[c][1] = uop; }
            return c as i32;
        }
        c += 1;
    }
    -1
}

fn schedule_mop(mop: &MacroOp, port_busy: &mut [[i32; 3]; CYCLE_MAP_SIZE], cycle: i32, dep_cycle: i32, commit: bool) -> i32 {
    let mut c = cycle;
    if mop.dependent {
        c = c.max(dep_cycle);
    }
    if mop.is_eliminated() {
        return c;
    }
    if mop.is_simple() {
        return schedule_uop(mop.uop1, port_busy, c, commit);
    }
    // Two uops: must schedule in same cycle
    while (c as usize) < CYCLE_MAP_SIZE {
        let c1 = schedule_uop(mop.uop1, port_busy, c, false);
        let c2 = schedule_uop(mop.uop2, port_busy, c, false);
        if c1 >= 0 && c1 == c2 {
            if commit {
                schedule_uop(mop.uop1, port_busy, c1, true);
                schedule_uop(mop.uop2, port_busy, c2, true);
            }
            return c1;
        }
        c += 1;
    }
    -1
}

// ============================================================================
// Generate
// ============================================================================

/// Generate a SuperscalarHash program using the Blake2Generator.
pub fn generate_superscalar(generator: &mut Blake2Generator) -> SuperscalarProgram {
    let mut port_busy = [[0i32; 3]; CYCLE_MAP_SIZE];
    let mut registers = [
        RegisterInfo::new(), RegisterInfo::new(), RegisterInfo::new(), RegisterInfo::new(),
        RegisterInfo::new(), RegisterInfo::new(), RegisterInfo::new(), RegisterInfo::new(),
    ];

    let mut current = WorkingInstruction::null();
    let mut macro_op_index: i32 = 0;
    let mut _code_size: i32 = 0;
    let mut _macro_op_count: i32 = 0;
    let mut cycle: i32 = 0;
    let mut dep_cycle: i32 = 0;
    let mut retire_cycle: i32 = 0;
    let mut ports_saturated = false;
    let mut program_size: i32 = 0;
    let mut mul_count: i32 = 0;
    let mut throw_away_count: i32 = 0;

    let mut output_instructions: Vec<SuperscalarInstruction> = Vec::new();

    for decode_cycle in 0..RANDOMX_SUPERSCALAR_LATENCY {
        if ports_saturated || program_size >= SUPERSCALAR_MAX_SIZE {
            break;
        }

        let buffer = fetch_next(current.info.inst_type, decode_cycle, mul_count, generator);
        let mut buffer_index: usize = 0;

        while buffer_index < buffer.slots.len() {
            let top_cycle = cycle;

            // If we have issued all macro-ops for the current instruction, create a new one
            if macro_op_index >= current.info.ops.len() as i32 {
                if ports_saturated || program_size >= SUPERSCALAR_MAX_SIZE {
                    break;
                }
                let slot_size = buffer.slots[buffer_index];
                let is_last = buffer_index + 1 == buffer.slots.len();
                let info = select_for_slot(generator, slot_size, buffer.index, is_last);
                current = WorkingInstruction::create(info, generator);
                macro_op_index = 0;
            }

            let mop = current.info.ops[macro_op_index as usize];

            // Calculate earliest schedule cycle (without committing)
            let mut schedule_cycle = schedule_mop(&mop, &mut port_busy, cycle, dep_cycle, false);
            if schedule_cycle < 0 {
                ports_saturated = true;
                break;
            }

            // Select source register if this is the source macro-op
            if macro_op_index == current.info.src_op {
                let mut forward = 0;
                while forward < LOOK_FORWARD_CYCLES {
                    if current.select_source(schedule_cycle, &registers, generator) {
                        break;
                    }
                    schedule_cycle += 1;
                    cycle += 1;
                    forward += 1;
                }
                if forward == LOOK_FORWARD_CYCLES {
                    if throw_away_count < MAX_THROWAWAY_COUNT {
                        throw_away_count += 1;
                        macro_op_index = current.info.ops.len() as i32;
                        continue;
                    }
                    current = WorkingInstruction::null();
                    break;
                }
            }

            // Select destination register if this is the dst macro-op
            if macro_op_index == current.info.dst_op {
                let mut forward = 0;
                while forward < LOOK_FORWARD_CYCLES {
                    if current.select_destination(schedule_cycle, throw_away_count > 0, &registers, generator) {
                        break;
                    }
                    schedule_cycle += 1;
                    cycle += 1;
                    forward += 1;
                }
                if forward == LOOK_FORWARD_CYCLES {
                    if throw_away_count < MAX_THROWAWAY_COUNT {
                        throw_away_count += 1;
                        macro_op_index = current.info.ops.len() as i32;
                        continue;
                    }
                    current = WorkingInstruction::null();
                    break;
                }
            }

            throw_away_count = 0;

            // Commit the schedule
            schedule_cycle = schedule_mop(&mop, &mut port_busy, schedule_cycle, schedule_cycle, true);
            if schedule_cycle < 0 {
                ports_saturated = true;
                break;
            }

            dep_cycle = schedule_cycle + mop.latency;

            // Update register info if this is the result op
            if macro_op_index == current.info.result_op {
                let dst = current.dst as usize;
                retire_cycle = dep_cycle;
                registers[dst].latency = retire_cycle;
                registers[dst].last_op_group = current.op_group;
                registers[dst].last_op_par = current.op_group_par;
            }

            _code_size += mop.size;
            buffer_index += 1;
            macro_op_index += 1;
            _macro_op_count += 1;

            if schedule_cycle >= RANDOMX_SUPERSCALAR_LATENCY {
                ports_saturated = true;
            }
            cycle = top_cycle;

            // When all macro-ops issued, emit the instruction
            if macro_op_index >= current.info.ops.len() as i32 {
                output_instructions.push(current.to_output());
                program_size += 1;
                if is_multiplication(current.info.inst_type) {
                    mul_count += 1;
                }
            }
        }

        cycle += 1;
    }

    // Calculate ASIC latencies
    let mut asic_latencies = [0i32; 8];
    for inst in &output_instructions {
        let dst = inst.dst as usize;
        let src = inst.src as usize;
        let lat_dst = asic_latencies[dst] + 1;
        let lat_src = if dst != src { asic_latencies[src] + 1 } else { 0 };
        asic_latencies[dst] = lat_dst.max(lat_src);
    }

    // Find address register (highest ASIC latency)
    let mut address_register = 0usize;
    let mut max_latency = 0;
    for i in 0..8 {
        if asic_latencies[i] > max_latency {
            max_latency = asic_latencies[i];
            address_register = i;
        }
    }

    // Suppress unused variable warnings
    let _ = retire_cycle;

    SuperscalarProgram {
        instructions: output_instructions,
        address_register,
    }
}

/// Execute a SuperscalarHash program on 8 registers.
pub fn execute_superscalar(r: &mut [u64; 8], prog: &SuperscalarProgram) {
    for inst in &prog.instructions {
        let dst = inst.dst as usize;
        let src = inst.src as usize;
        match inst.opcode {
            0 /* ISUB_R */ => r[dst] = r[dst].wrapping_sub(r[src]),
            1 /* IXOR_R */ => r[dst] ^= r[src],
            2 /* IADD_RS */ => r[dst] = r[dst].wrapping_add(r[src] << ((inst.mod_ >> 2) % 4)),
            3 /* IMUL_R */ => r[dst] = r[dst].wrapping_mul(r[src]),
            4 /* IROR_C */ => r[dst] = r[dst].rotate_right(inst.imm32),
            5 | 7 | 9 /* IADD_Cx */ => r[dst] = r[dst].wrapping_add(inst.imm32 as i32 as i64 as u64),
            6 | 8 | 10 /* IXOR_Cx */ => r[dst] ^= inst.imm32 as i32 as i64 as u64,
            11 /* IMULH_R */ => r[dst] = ((r[dst] as u128).wrapping_mul(r[src] as u128) >> 64) as u64,
            12 /* ISMULH_R */ => r[dst] = ((r[dst] as i64 as i128).wrapping_mul(r[src] as i64 as i128) >> 64) as u64,
            13 /* IMUL_RCP */ => r[dst] = r[dst].wrapping_mul(randomx_reciprocal(inst.imm32)),
            _ => {}
        }
    }
}
