//! Opcode definitions and instruction encoding/decoding for the Lua 5.4 VM.
//!
//! Ports `src/lopcodes.c` (the `luaP_opmodes` table) and `src/lopcodes.h`
//! (the `OpCode`/`OpMode` enums, field-size constants, and instruction
//! accessor macros). Per PORTING.md §1, headers merge into their consuming
//! `.rs`.
//!
//! C source preserved inline as `// C:` comments for diff-time review.

// C: /* $Id: lopcodes.c $ */
// C: /* $Id: lopcodes.h $ */

// ─── Instruction format diagram ──────────────────────────────────────────────
//
// C: /*
// C:   3 3 2 2 2 2 2 2 2 2 2 2 1 1 1 1 1 1 1 1 1 1 0 0 0 0 0 0 0 0 0 0
// C:   1 0 9 8 7 6 5 4 3 2 1 0 9 8 7 6 5 4 3 2 1 0 9 8 7 6 5 4 3 2 1 0
// C: iABC    C(8)     |      B(8)     |k|     A(8)      |   Op(7)     |
// C: iABx          Bx(17)               |     A(8)      |   Op(7)     |
// C: iAsBx        sBx (signed)(17)      |     A(8)      |   Op(7)     |
// C: iAx                     Ax(25)                     |   Op(7)     |
// C: isJ                     sJ (signed)(25)            |   Op(7)     |
// C: */

// ─── OpMode ──────────────────────────────────────────────────────────────────

/// Instruction addressing mode.
///
/// C: `enum OpMode { iABC, iABx, iAsBx, iAx, isJ };`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpMode {
    /// C: `iABC`  — A(8), B(8), C(8), k(1)
    Abc = 0,
    /// C: `iABx`  — A(8), Bx(17)
    ABx = 1,
    /// C: `iAsBx` — A(8), sBx signed(17)
    AsBx = 2,
    /// C: `iAx`   — Ax(25)
    Ax = 3,
    /// C: `isJ`   — sJ signed(25)
    SJ = 4,
}

// ─── Field size constants ─────────────────────────────────────────────────────
//
// C: #define SIZE_C   8
// C: #define SIZE_B   8
// C: #define SIZE_Bx  (SIZE_C + SIZE_B + 1)
// C: #define SIZE_A   8
// C: #define SIZE_Ax  (SIZE_Bx + SIZE_A)
// C: #define SIZE_sJ  (SIZE_Bx + SIZE_A)
// C: #define SIZE_OP  7

pub const SIZE_C: u32 = 8;
pub const SIZE_B: u32 = 8;
pub const SIZE_BX: u32 = SIZE_C + SIZE_B + 1;
pub const SIZE_A: u32 = 8;
pub const SIZE_AX: u32 = SIZE_BX + SIZE_A;
pub const SIZE_S_J: u32 = SIZE_BX + SIZE_A;
pub const SIZE_OP: u32 = 7;

// ─── Field position constants ─────────────────────────────────────────────────
//
// C: #define POS_OP  0
// C: #define POS_A   (POS_OP + SIZE_OP)
// C: #define POS_k   (POS_A + SIZE_A)
// C: #define POS_B   (POS_k + 1)
// C: #define POS_C   (POS_B + SIZE_B)
// C: #define POS_Bx  POS_k
// C: #define POS_Ax  POS_A
// C: #define POS_sJ  POS_A

pub const POS_OP: u32 = 0;
pub const POS_A: u32 = POS_OP + SIZE_OP;
pub const POS_K: u32 = POS_A + SIZE_A;
pub const POS_B: u32 = POS_K + 1;
pub const POS_C: u32 = POS_B + SIZE_B;
pub const POS_BX: u32 = POS_K;
pub const POS_AX: u32 = POS_A;
pub const POS_S_J: u32 = POS_A;

// ─── Argument limit constants ─────────────────────────────────────────────────
//
// C: #define MAXARG_Bx     ((1<<SIZE_Bx)-1)
// C: #define OFFSET_sBx    (MAXARG_Bx>>1)
// C: #define MAXARG_Ax     ((1<<SIZE_Ax)-1)
// C: #define MAXARG_sJ     ((1<<SIZE_sJ)-1)
// C: #define OFFSET_sJ     (MAXARG_sJ>>1)
// C: #define MAXARG_A      ((1<<SIZE_A)-1)
// C: #define MAXARG_B      ((1<<SIZE_B)-1)
// C: #define MAXARG_C      ((1<<SIZE_C)-1)
// C: #define OFFSET_sC     (MAXARG_C>>1)

pub const MAXARG_BX: u32 = (1u32 << SIZE_BX) - 1;
pub const OFFSET_S_BX: i32 = (MAXARG_BX >> 1) as i32;
pub const MAXARG_AX: u32 = (1u32 << SIZE_AX) - 1;
pub const MAXARG_S_J: u32 = (1u32 << SIZE_S_J) - 1;
pub const OFFSET_S_J: i32 = (MAXARG_S_J >> 1) as i32;
pub const MAXARG_A: u32 = (1u32 << SIZE_A) - 1;
pub const MAXARG_B: u32 = (1u32 << SIZE_B) - 1;
pub const MAXARG_C: u32 = (1u32 << SIZE_C) - 1;
pub const OFFSET_S_C: i32 = (MAXARG_C >> 1) as i32;

/// Sentinel "no register" value that fits in 8 bits.
///
/// C: `#define NO_REG  MAXARG_A`
pub const NO_REG: u32 = MAXARG_A;

/// Maximum RK index (for debugging only).
///
/// C: `#define MAXINDEXRK  MAXARG_B`
pub const MAXINDEXRK: u32 = MAXARG_B;

/// Number of list items to accumulate before a SETLIST instruction.
///
/// C: `#define LFIELDS_PER_FLUSH  50`
pub const LFIELDS_PER_FLUSH: u32 = 50;

// ─── OpCode enum ─────────────────────────────────────────────────────────────
//
// C: /* Grep "ORDER OP" if you change these enums. */
// C: typedef enum { OP_MOVE, ..., OP_EXTRAARG } OpCode;
//
// ORDER OP — variant discriminants must match `lopcodes.h` exactly.
// The VM casts the raw opcode field directly to this enum.

/// All opcodes for the Lua 5.4 virtual machine.
///
/// ORDER OP — must match `lopcodes.h` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum OpCode {
    // C: OP_MOVE,/*  A B    R[A] := R[B]  */
    Move = 0,
    // C: OP_LOADI,/* A sBx  R[A] := sBx  */
    LoadI,
    // C: OP_LOADF,/* A sBx  R[A] := (lua_Number)sBx  */
    LoadF,
    // C: OP_LOADK,/* A Bx   R[A] := K[Bx]  */
    LoadK,
    // C: OP_LOADKX,/* A     R[A] := K[extra arg]  */
    LoadKX,
    // C: OP_LOADFALSE,/* A  R[A] := false  */
    LoadFalse,
    // C: OP_LFALSESKIP,/* A R[A] := false; pc++  */
    LFalseSkip,
    // C: OP_LOADTRUE,/* A   R[A] := true  */
    LoadTrue,
    // C: OP_LOADNIL,/* A B  R[A], R[A+1], ..., R[A+B] := nil  */
    LoadNil,
    // C: OP_GETUPVAL,/* A B R[A] := UpValue[B]  */
    GetUpVal,
    // C: OP_SETUPVAL,/* A B UpValue[B] := R[A]  */
    SetUpVal,

    // C: OP_GETTABUP,/* A B C  R[A] := UpValue[B][K[C]:shortstring]  */
    GetTabUp,
    // C: OP_GETTABLE,/* A B C  R[A] := R[B][R[C]]  */
    GetTable,
    // C: OP_GETI,/*    A B C  R[A] := R[B][C]  */
    GetI,
    // C: OP_GETFIELD,/* A B C  R[A] := R[B][K[C]:shortstring]  */
    GetField,

    // C: OP_SETTABUP,/* A B C  UpValue[A][K[B]:shortstring] := RK(C)  */
    SetTabUp,
    // C: OP_SETTABLE,/* A B C  R[A][R[B]] := RK(C)  */
    SetTable,
    // C: OP_SETI,/*    A B C  R[A][B] := RK(C)  */
    SetI,
    // C: OP_SETFIELD,/* A B C  R[A][K[B]:shortstring] := RK(C)  */
    SetField,

    // C: OP_NEWTABLE,/* A B C k  R[A] := {}  */
    NewTable,

    // C: OP_SELF,/* A B C  R[A+1] := R[B]; R[A] := R[B][RK(C):string]  */
    // PORT NOTE: `self` is a Rust keyword; renamed to `Self_`.
    Self_,

    // C: OP_ADDI,/* A B sC  R[A] := R[B] + sC  */
    AddI,

    // C: OP_ADDK,/* A B C  R[A] := R[B] + K[C]:number  */
    AddK,
    // C: OP_SUBK,/* A B C  R[A] := R[B] - K[C]:number  */
    SubK,
    // C: OP_MULK,/* A B C  R[A] := R[B] * K[C]:number  */
    MulK,
    // C: OP_MODK,/* A B C  R[A] := R[B] % K[C]:number  */
    ModK,
    // C: OP_POWK,/* A B C  R[A] := R[B] ^ K[C]:number  */
    PowK,
    // C: OP_DIVK,/* A B C  R[A] := R[B] / K[C]:number  */
    DivK,
    // C: OP_IDIVK,/* A B C  R[A] := R[B] // K[C]:number  */
    IDivK,

    // C: OP_BANDK,/* A B C  R[A] := R[B] & K[C]:integer  */
    BAndK,
    // C: OP_BORK,/*  A B C  R[A] := R[B] | K[C]:integer  */
    BOrK,
    // C: OP_BXORK,/* A B C  R[A] := R[B] ~ K[C]:integer  */
    BXorK,

    // C: OP_SHRI,/* A B sC  R[A] := R[B] >> sC  */
    ShrI,
    // C: OP_SHLI,/* A B sC  R[A] := sC << R[B]  */
    ShlI,

    // C: OP_ADD,/*  A B C  R[A] := R[B] + R[C]  */
    Add,
    // C: OP_SUB,/*  A B C  R[A] := R[B] - R[C]  */
    Sub,
    // C: OP_MUL,/*  A B C  R[A] := R[B] * R[C]  */
    Mul,
    // C: OP_MOD,/*  A B C  R[A] := R[B] % R[C]  */
    Mod,
    // C: OP_POW,/*  A B C  R[A] := R[B] ^ R[C]  */
    Pow,
    // C: OP_DIV,/*  A B C  R[A] := R[B] / R[C]  */
    Div,
    // C: OP_IDIV,/* A B C  R[A] := R[B] // R[C]  */
    IDiv,

    // C: OP_BAND,/* A B C  R[A] := R[B] & R[C]  */
    BAnd,
    // C: OP_BOR,/*  A B C  R[A] := R[B] | R[C]  */
    BOr,
    // C: OP_BXOR,/* A B C  R[A] := R[B] ~ R[C]  */
    BXor,
    // C: OP_SHL,/*  A B C  R[A] := R[B] << R[C]  */
    Shl,
    // C: OP_SHR,/*  A B C  R[A] := R[B] >> R[C]  */
    Shr,

    // C: OP_MMBIN,/*  A B C    call C metamethod over R[A] and R[B]  */
    MmBin,
    // C: OP_MMBINI,/* A sB C k call C metamethod over R[A] and sB  */
    MmBinI,
    // C: OP_MMBINK,/* A B C k  call C metamethod over R[A] and K[B]  */
    MmBinK,

    // C: OP_UNM,/*    A B  R[A] := -R[B]  */
    Unm,
    // C: OP_BNOT,/*   A B  R[A] := ~R[B]  */
    BNot,
    // C: OP_NOT,/*    A B  R[A] := not R[B]  */
    Not,
    // C: OP_LEN,/*    A B  R[A] := #R[B]  */
    Len,

    // C: OP_CONCAT,/* A B  R[A] := R[A].. ... ..R[A + B - 1]  */
    Concat,

    // C: OP_CLOSE,/* A  close all upvalues >= R[A]  */
    Close,
    // C: OP_TBC,/*   A  mark variable A "to be closed"  */
    Tbc,
    // C: OP_JMP,/*   sJ  pc += sJ  */
    Jmp,

    // C: OP_EQ,/* A B k  if ((R[A] == R[B]) ~= k) then pc++  */
    Eq,
    // C: OP_LT,/* A B k  if ((R[A] <  R[B]) ~= k) then pc++  */
    Lt,
    // C: OP_LE,/* A B k  if ((R[A] <= R[B]) ~= k) then pc++  */
    Le,

    // C: OP_EQK,/* A B k   if ((R[A] == K[B]) ~= k) then pc++  */
    EqK,
    // C: OP_EQI,/* A sB k  if ((R[A] == sB) ~= k) then pc++  */
    EqI,
    // C: OP_LTI,/* A sB k  if ((R[A] < sB) ~= k) then pc++  */
    LtI,
    // C: OP_LEI,/* A sB k  if ((R[A] <= sB) ~= k) then pc++  */
    LeI,
    // C: OP_GTI,/* A sB k  if ((R[A] > sB) ~= k) then pc++  */
    GtI,
    // C: OP_GEI,/* A sB k  if ((R[A] >= sB) ~= k) then pc++  */
    GeI,

    // C: OP_TEST,/*    A k    if (not R[A] == k) then pc++  */
    Test,
    // C: OP_TESTSET,/* A B k  if (not R[B] == k) then pc++ else R[A] := R[B]  */
    TestSet,

    // C: OP_CALL,/*     A B C    R[A], ... ,R[A+C-2] := R[A](R[A+1], ... ,R[A+B-1])  */
    Call,
    // C: OP_TAILCALL,/* A B C k  return R[A](R[A+1], ... ,R[A+B-1])  */
    TailCall,

    // C: OP_RETURN,/*  A B C k  return R[A], ... ,R[A+B-2]  */
    Return,
    // C: OP_RETURN0,/*           return  */
    Return0,
    // C: OP_RETURN1,/* A         return R[A]  */
    Return1,

    // C: OP_FORLOOP,/* A Bx  update counters; if loop continues then pc-=Bx  */
    ForLoop,
    // C: OP_FORPREP,/* A Bx  check values and prepare; if not to run then pc+=Bx+1  */
    ForPrep,

    // C: OP_TFORPREP,/* A Bx  create upvalue for R[A+3]; pc+=Bx  */
    TForPrep,
    // C: OP_TFORCALL,/* A C   R[A+4], ... ,R[A+3+C] := R[A](R[A+1], R[A+2])  */
    TForCall,
    // C: OP_TFORLOOP,/* A Bx  if R[A+2] ~= nil then { R[A]=R[A+2]; pc -= Bx }  */
    TForLoop,

    // C: OP_SETLIST,/* A B C k  R[A][C+i] := R[A+i], 1 <= i <= B  */
    SetList,

    // C: OP_CLOSURE,/* A Bx  R[A] := closure(KPROTO[Bx])  */
    Closure,

    // C: OP_VARARG,/* A C  R[A], R[A+1], ..., R[A+C-2] = vararg  */
    VarArg,

    // C: OP_VARARGPREP,/* A  (adjust vararg parameters)  */
    VarArgPrep,

    // C: OP_EXTRAARG/* Ax  extra (larger) argument for previous opcode  */
    ExtraArg,
}

/// Total number of opcodes.
///
/// C: `#define NUM_OPCODES ((int)(OP_EXTRAARG) + 1)`
pub const NUM_OPCODES: usize = OpCode::ExtraArg as usize + 1;

impl OpCode {
    /// Convert a raw `u32` opcode field value to an `OpCode`.
    ///
    /// Returns `None` if `v >= NUM_OPCODES`.
    ///
    /// C: `GET_OPCODE(i)` — `cast(OpCode, ((i)>>POS_OP) & MASK1(SIZE_OP,0))`
    ///
    /// TODO(port): replace explicit match with a safe transmute or `num_enum`
    /// crate derive once Phase B settles the dependency policy. The match is
    /// correct but mechanical; 83 arms is noise at compile-time and runtime.
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Move),
            1 => Some(Self::LoadI),
            2 => Some(Self::LoadF),
            3 => Some(Self::LoadK),
            4 => Some(Self::LoadKX),
            5 => Some(Self::LoadFalse),
            6 => Some(Self::LFalseSkip),
            7 => Some(Self::LoadTrue),
            8 => Some(Self::LoadNil),
            9 => Some(Self::GetUpVal),
            10 => Some(Self::SetUpVal),
            11 => Some(Self::GetTabUp),
            12 => Some(Self::GetTable),
            13 => Some(Self::GetI),
            14 => Some(Self::GetField),
            15 => Some(Self::SetTabUp),
            16 => Some(Self::SetTable),
            17 => Some(Self::SetI),
            18 => Some(Self::SetField),
            19 => Some(Self::NewTable),
            20 => Some(Self::Self_),
            21 => Some(Self::AddI),
            22 => Some(Self::AddK),
            23 => Some(Self::SubK),
            24 => Some(Self::MulK),
            25 => Some(Self::ModK),
            26 => Some(Self::PowK),
            27 => Some(Self::DivK),
            28 => Some(Self::IDivK),
            29 => Some(Self::BAndK),
            30 => Some(Self::BOrK),
            31 => Some(Self::BXorK),
            32 => Some(Self::ShrI),
            33 => Some(Self::ShlI),
            34 => Some(Self::Add),
            35 => Some(Self::Sub),
            36 => Some(Self::Mul),
            37 => Some(Self::Mod),
            38 => Some(Self::Pow),
            39 => Some(Self::Div),
            40 => Some(Self::IDiv),
            41 => Some(Self::BAnd),
            42 => Some(Self::BOr),
            43 => Some(Self::BXor),
            44 => Some(Self::Shl),
            45 => Some(Self::Shr),
            46 => Some(Self::MmBin),
            47 => Some(Self::MmBinI),
            48 => Some(Self::MmBinK),
            49 => Some(Self::Unm),
            50 => Some(Self::BNot),
            51 => Some(Self::Not),
            52 => Some(Self::Len),
            53 => Some(Self::Concat),
            54 => Some(Self::Close),
            55 => Some(Self::Tbc),
            56 => Some(Self::Jmp),
            57 => Some(Self::Eq),
            58 => Some(Self::Lt),
            59 => Some(Self::Le),
            60 => Some(Self::EqK),
            61 => Some(Self::EqI),
            62 => Some(Self::LtI),
            63 => Some(Self::LeI),
            64 => Some(Self::GtI),
            65 => Some(Self::GeI),
            66 => Some(Self::Test),
            67 => Some(Self::TestSet),
            68 => Some(Self::Call),
            69 => Some(Self::TailCall),
            70 => Some(Self::Return),
            71 => Some(Self::Return0),
            72 => Some(Self::Return1),
            73 => Some(Self::ForLoop),
            74 => Some(Self::ForPrep),
            75 => Some(Self::TForPrep),
            76 => Some(Self::TForCall),
            77 => Some(Self::TForLoop),
            78 => Some(Self::SetList),
            79 => Some(Self::Closure),
            80 => Some(Self::VarArg),
            81 => Some(Self::VarArgPrep),
            82 => Some(Self::ExtraArg),
            _ => None,
        }
    }
}

// ─── opmode_byte helper ───────────────────────────────────────────────────────
//
// C: #define opmode(mm,ot,it,t,a,m)
//        (((mm)<<7) | ((ot)<<6) | ((it)<<5) | ((t)<<4) | ((a)<<3) | (m))
//
// Bit layout for each entry in OP_MODES:
//   bits 0-2: OpMode value (Abc=0 ABx=1 AsBx=2 Ax=3 SJ=4)
//   bit 3:    instruction sets register A
//   bit 4:    is a test (next instruction must be a jump)
//   bit 5:    instruction uses L->top from previous (IT mode)
//   bit 6:    instruction sets L->top for next (OT mode)
//   bit 7:    is a metamethod instruction (MM)

const fn opmode_byte(mm: u8, ot: u8, it: u8, t: u8, a: u8, m: u8) -> u8 {
    (mm << 7) | (ot << 6) | (it << 5) | (t << 4) | (a << 3) | m
}

// Shorthand mode constants for the OP_MODES table below.
const M_ABC: u8 = OpMode::Abc as u8;
const M_ABX: u8 = OpMode::ABx as u8;
const M_ASBX: u8 = OpMode::AsBx as u8;
const M_AX: u8 = OpMode::Ax as u8;
const M_SJ: u8 = OpMode::SJ as u8;

// ─── OP_MODES table ───────────────────────────────────────────────────────────
//
// C: /* ORDER OP */
// C: LUAI_DDEF const lu_byte luaP_opmodes[NUM_OPCODES] = {
// C:   opmode(mm, ot, it, t, a, mode)  /* OP_XXX */
// C:   ...
// C: };
//
// Per macros.tsv: LUAI_DDEF → drop (definition site, no modifier needed in Rust).
// Per macros.tsv: LUAI_DDEC → `pub(crate) static` at the declaration site.

/// Opcode properties table, indexed by `OpCode as usize`.
///
/// C: `const lu_byte luaP_opmodes[NUM_OPCODES]`
///
/// Use `get_op_mode`, `test_a_mode`, etc. to query individual properties
/// rather than indexing this array directly.
pub(crate) const OP_MODES: [u8; NUM_OPCODES] = [
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_MOVE */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iAsBx) /* OP_LOADI */
    opmode_byte(0, 0, 0, 0, 1, M_ASBX),
    // C: opmode(0, 0, 0, 0, 1, iAsBx) /* OP_LOADF */
    opmode_byte(0, 0, 0, 0, 1, M_ASBX),
    // C: opmode(0, 0, 0, 0, 1, iABx)  /* OP_LOADK */
    opmode_byte(0, 0, 0, 0, 1, M_ABX),
    // C: opmode(0, 0, 0, 0, 1, iABx)  /* OP_LOADKX */
    opmode_byte(0, 0, 0, 0, 1, M_ABX),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_LOADFALSE */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_LFALSESKIP */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_LOADTRUE */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_LOADNIL */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_GETUPVAL */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_SETUPVAL */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_GETTABUP */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_GETTABLE */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_GETI */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_GETFIELD */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_SETTABUP */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_SETTABLE */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_SETI */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_SETFIELD */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_NEWTABLE */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_SELF */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_ADDI */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_ADDK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_SUBK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_MULK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_MODK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_POWK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_DIVK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_IDIVK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_BANDK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_BORK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_BXORK */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_SHRI */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_SHLI */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_ADD */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_SUB */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_MUL */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_MOD */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_POW */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_DIV */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_IDIV */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_BAND */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_BOR */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_BXOR */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_SHL */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_SHR */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(1, 0, 0, 0, 0, iABC)  /* OP_MMBIN */
    opmode_byte(1, 0, 0, 0, 0, M_ABC),
    // C: opmode(1, 0, 0, 0, 0, iABC)  /* OP_MMBINI */
    opmode_byte(1, 0, 0, 0, 0, M_ABC),
    // C: opmode(1, 0, 0, 0, 0, iABC)  /* OP_MMBINK */
    opmode_byte(1, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_UNM */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_BNOT */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_NOT */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_LEN */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABC)  /* OP_CONCAT */
    opmode_byte(0, 0, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_CLOSE */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_TBC */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, isJ)   /* OP_JMP */
    opmode_byte(0, 0, 0, 0, 0, M_SJ),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_EQ */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_LT */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_LE */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_EQK */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_EQI */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_LTI */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_LEI */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_GTI */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_GEI */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 0, iABC)  /* OP_TEST */
    opmode_byte(0, 0, 0, 1, 0, M_ABC),
    // C: opmode(0, 0, 0, 1, 1, iABC)  /* OP_TESTSET */
    opmode_byte(0, 0, 0, 1, 1, M_ABC),
    // C: opmode(0, 1, 1, 0, 1, iABC)  /* OP_CALL */
    opmode_byte(0, 1, 1, 0, 1, M_ABC),
    // C: opmode(0, 1, 1, 0, 1, iABC)  /* OP_TAILCALL */
    opmode_byte(0, 1, 1, 0, 1, M_ABC),
    // C: opmode(0, 0, 1, 0, 0, iABC)  /* OP_RETURN */
    opmode_byte(0, 0, 1, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_RETURN0 */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_RETURN1 */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABx)  /* OP_FORLOOP */
    opmode_byte(0, 0, 0, 0, 1, M_ABX),
    // C: opmode(0, 0, 0, 0, 1, iABx)  /* OP_FORPREP */
    opmode_byte(0, 0, 0, 0, 1, M_ABX),
    // C: opmode(0, 0, 0, 0, 0, iABx)  /* OP_TFORPREP */
    opmode_byte(0, 0, 0, 0, 0, M_ABX),
    // C: opmode(0, 0, 0, 0, 0, iABC)  /* OP_TFORCALL */
    opmode_byte(0, 0, 0, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABx)  /* OP_TFORLOOP */
    opmode_byte(0, 0, 0, 0, 1, M_ABX),
    // C: opmode(0, 0, 1, 0, 0, iABC)  /* OP_SETLIST */
    opmode_byte(0, 0, 1, 0, 0, M_ABC),
    // C: opmode(0, 0, 0, 0, 1, iABx)  /* OP_CLOSURE */
    opmode_byte(0, 0, 0, 0, 1, M_ABX),
    // C: opmode(0, 1, 0, 0, 1, iABC)  /* OP_VARARG */
    opmode_byte(0, 1, 0, 0, 1, M_ABC),
    // C: opmode(0, 0, 1, 0, 1, iABC)  /* OP_VARARGPREP */
    opmode_byte(0, 0, 1, 0, 1, M_ABC),
    // C: opmode(0, 0, 0, 0, 0, iAx)   /* OP_EXTRAARG */
    opmode_byte(0, 0, 0, 0, 0, M_AX),
];

// ─── OP_MODES accessors ───────────────────────────────────────────────────────
//
// C: #define getOpMode(m)    (cast(enum OpMode, luaP_opmodes[m] & 7))
// C: #define testAMode(m)    (luaP_opmodes[m] & (1 << 3))
// C: #define testTMode(m)    (luaP_opmodes[m] & (1 << 4))
// C: #define testITMode(m)   (luaP_opmodes[m] & (1 << 5))
// C: #define testOTMode(m)   (luaP_opmodes[m] & (1 << 6))
// C: #define testMMMode(m)   (luaP_opmodes[m] & (1 << 7))

/// Extract the `OpMode` for an opcode.
///
/// C: `getOpMode(m)` — `cast(enum OpMode, luaP_opmodes[m] & 7)`
pub fn get_op_mode(op: OpCode) -> OpMode {
    match OP_MODES[op as usize] & 7 {
        0 => OpMode::Abc,
        1 => OpMode::ABx,
        2 => OpMode::AsBx,
        3 => OpMode::Ax,
        4 => OpMode::SJ,
        // PERF(port): unreachable branch — values 5-7 are unused; profile in Phase B
        _ => OpMode::Abc,
    }
}

/// True if this opcode writes to register A.
///
/// C: `testAMode(m)` — `luaP_opmodes[m] & (1 << 3)`
#[inline]
pub fn test_a_mode(op: OpCode) -> bool {
    (OP_MODES[op as usize] & (1 << 3)) != 0
}

/// True if this opcode is a test (the next instruction must be a jump).
///
/// C: `testTMode(m)` — `luaP_opmodes[m] & (1 << 4)`
#[inline]
pub fn test_t_mode(op: OpCode) -> bool {
    (OP_MODES[op as usize] & (1 << 4)) != 0
}

/// True if this opcode uses `L->top` as set by the previous instruction (B == 0 case).
///
/// C: `testITMode(m)` — `luaP_opmodes[m] & (1 << 5)`
#[inline]
pub fn test_it_mode(op: OpCode) -> bool {
    (OP_MODES[op as usize] & (1 << 5)) != 0
}

/// True if this opcode sets `L->top` for the next instruction (C == 0 case).
///
/// C: `testOTMode(m)` — `luaP_opmodes[m] & (1 << 6)`
#[inline]
pub fn test_ot_mode(op: OpCode) -> bool {
    (OP_MODES[op as usize] & (1 << 6)) != 0
}

/// True if this opcode is a metamethod call.
///
/// C: `testMMMode(m)` — `luaP_opmodes[m] & (1 << 7)`
#[inline]
pub fn test_mm_mode(op: OpCode) -> bool {
    (OP_MODES[op as usize] & (1 << 7)) != 0
}

// ─── Instruction newtype ──────────────────────────────────────────────────────
//
// Per types.tsv: `Instruction` is a `u32` newtype; bytecode word.
// All accessor/builder macros from lopcodes.h become methods here.

/// A single Lua bytecode instruction (unsigned 32-bit word).
///
/// C: `typedef unsigned int Instruction;` (see llimits.h)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct Instruction(pub u32);

impl Instruction {
    // ── Low-level field accessors ─────────────────────────────────────────

    /// Extract a bit-field of `size` bits at position `pos`.
    ///
    /// C: `getarg(i, pos, size)` — `cast_int(((i)>>(pos)) & MASK1(size,0))`
    #[inline]
    pub const fn get_arg(self, pos: u32, size: u32) -> u32 {
        (self.0 >> pos) & ((1u32 << size) - 1)
    }

    /// Set a bit-field of `size` bits at position `pos` to `v`.
    ///
    /// C: `setarg(i, v, pos, size)`
    #[inline]
    pub fn set_arg(&mut self, v: u32, pos: u32, size: u32) {
        let mask = ((1u32 << size) - 1) << pos;
        self.0 = (self.0 & !mask) | ((v << pos) & mask);
    }

    // ── Opcode field ──────────────────────────────────────────────────────

    /// Extract the opcode.
    ///
    /// C: `GET_OPCODE(i)` — `cast(OpCode, ((i)>>POS_OP) & MASK1(SIZE_OP,0))`
    #[inline]
    pub fn opcode(self) -> Option<OpCode> {
        OpCode::from_u32(self.get_arg(POS_OP, SIZE_OP))
    }

    /// Replace the opcode field.
    ///
    /// C: `SET_OPCODE(i, o)`
    #[inline]
    pub fn set_opcode(&mut self, op: OpCode) {
        self.set_arg(op as u32, POS_OP, SIZE_OP);
    }

    // ── A field ───────────────────────────────────────────────────────────

    /// C: `GETARG_A(i)`
    #[inline]
    pub const fn arg_a(self) -> u32 {
        self.get_arg(POS_A, SIZE_A)
    }

    /// C: `SETARG_A(i, v)`
    #[inline]
    pub fn set_arg_a(&mut self, v: u32) {
        self.set_arg(v, POS_A, SIZE_A);
    }

    // ── k bit ─────────────────────────────────────────────────────────────

    /// C: `GETARG_k(i)` — returns 0 or 1.
    #[inline]
    pub const fn arg_k(self) -> u32 {
        self.get_arg(POS_K, 1)
    }

    /// C: `TESTARG_k(i)` — boolean form of `GETARG_k`.
    #[inline]
    pub const fn test_k(self) -> bool {
        self.arg_k() != 0
    }

    /// C: `SETARG_k(i, v)`
    #[inline]
    pub fn set_arg_k(&mut self, v: u32) {
        self.set_arg(v, POS_K, 1);
    }

    // ── B field (iABC only) ───────────────────────────────────────────────

    /// C: `GETARG_B(i)` — debug-asserts iABC mode in C; here we trust the caller.
    #[inline]
    pub const fn arg_b(self) -> u32 {
        self.get_arg(POS_B, SIZE_B)
    }

    /// C: `GETARG_sB(i)` — signed B (subtracts `OFFSET_S_C`).
    #[inline]
    pub const fn arg_s_b(self) -> i32 {
        self.arg_b() as i32 - OFFSET_S_C
    }

    /// C: `SETARG_B(i, v)`
    #[inline]
    pub fn set_arg_b(&mut self, v: u32) {
        self.set_arg(v, POS_B, SIZE_B);
    }

    // ── C field (iABC only) ───────────────────────────────────────────────

    /// C: `GETARG_C(i)`
    #[inline]
    pub const fn arg_c(self) -> u32 {
        self.get_arg(POS_C, SIZE_C)
    }

    /// C: `GETARG_sC(i)` — signed C (subtracts `OFFSET_S_C`).
    #[inline]
    pub const fn arg_s_c(self) -> i32 {
        self.arg_c() as i32 - OFFSET_S_C
    }

    /// C: `SETARG_C(i, v)`
    #[inline]
    pub fn set_arg_c(&mut self, v: u32) {
        self.set_arg(v, POS_C, SIZE_C);
    }

    // ── Bx field (iABx / iAsBx) ──────────────────────────────────────────

    /// C: `GETARG_Bx(i)` — unsigned Bx.
    #[inline]
    pub const fn arg_bx(self) -> u32 {
        self.get_arg(POS_BX, SIZE_BX)
    }

    /// C: `SETARG_Bx(i, v)`
    #[inline]
    pub fn set_arg_bx(&mut self, v: u32) {
        self.set_arg(v, POS_BX, SIZE_BX);
    }

    /// C: `GETARG_sBx(i)` — signed Bx (subtracts `OFFSET_S_BX`).
    #[inline]
    pub const fn arg_s_bx(self) -> i32 {
        self.arg_bx() as i32 - OFFSET_S_BX
    }

    /// C: `SETARG_sBx(i, b)` — stores `b + OFFSET_S_BX` in the Bx field.
    #[inline]
    pub fn set_arg_s_bx(&mut self, b: i32) {
        self.set_arg_bx((b + OFFSET_S_BX) as u32);
    }

    // ── Ax field (iAx) ────────────────────────────────────────────────────

    /// C: `GETARG_Ax(i)`
    #[inline]
    pub const fn arg_ax(self) -> u32 {
        self.get_arg(POS_AX, SIZE_AX)
    }

    /// C: `SETARG_Ax(i, v)`
    #[inline]
    pub fn set_arg_ax(&mut self, v: u32) {
        self.set_arg(v, POS_AX, SIZE_AX);
    }

    // ── sJ field (isJ) ────────────────────────────────────────────────────

    /// C: `GETARG_sJ(i)` — signed J (subtracts `OFFSET_S_J`).
    #[inline]
    pub const fn arg_s_j(self) -> i32 {
        self.get_arg(POS_S_J, SIZE_S_J) as i32 - OFFSET_S_J
    }

    /// C: `SETARG_sJ(i, j)` — stores `j + OFFSET_S_J` in the sJ field.
    #[inline]
    pub fn set_arg_s_j(&mut self, j: i32) {
        self.set_arg((j + OFFSET_S_J) as u32, POS_S_J, SIZE_S_J);
    }

    // ── Instruction builders ──────────────────────────────────────────────

    /// Build an `iABC` instruction.
    ///
    /// C: `CREATE_ABCk(o, a, b, c, k)`
    #[inline]
    pub fn abck(op: OpCode, a: u32, b: u32, c: u32, k: u32) -> Self {
        Self(
            ((op as u32) << POS_OP)
                | (a << POS_A)
                | (b << POS_B)
                | (c << POS_C)
                | (k << POS_K),
        )
    }

    /// Build an `iABx` instruction.
    ///
    /// C: `CREATE_ABx(o, a, bc)`
    #[inline]
    pub fn abx(op: OpCode, a: u32, bc: u32) -> Self {
        Self(((op as u32) << POS_OP) | (a << POS_A) | (bc << POS_BX))
    }

    /// Build an `iAx` instruction.
    ///
    /// C: `CREATE_Ax(o, a)`
    #[inline]
    pub fn ax(op: OpCode, a: u32) -> Self {
        Self(((op as u32) << POS_OP) | (a << POS_AX))
    }

    /// Build an `isJ` instruction.
    ///
    /// C: `CREATE_sJ(o, j, k)`
    #[inline]
    pub fn sj(op: OpCode, j: u32, k: u32) -> Self {
        Self(((op as u32) << POS_OP) | (j << POS_S_J) | (k << POS_K))
    }

    // ── Mode query helpers (isOT / isIT) ──────────────────────────────────

    /// True if this instruction sets `L->top` for the next instruction.
    ///
    /// C: `isOT(i)` —
    /// `(testOTMode(GET_OPCODE(i)) && GETARG_C(i) == 0) || GET_OPCODE(i) == OP_TAILCALL`
    pub fn is_out_top(self) -> bool {
        match self.opcode() {
            Some(op) => (test_ot_mode(op) && self.arg_c() == 0) || op == OpCode::TailCall,
            None => false,
        }
    }

    /// True if this instruction uses `L->top` from the previous instruction.
    ///
    /// C: `isIT(i)` — `testITMode(GET_OPCODE(i)) && GETARG_B(i) == 0`
    pub fn is_in_top(self) -> bool {
        match self.opcode() {
            Some(op) => test_it_mode(op) && self.arg_b() == 0,
            None => false,
        }
    }

    /// Return the `OpMode` for this instruction.
    ///
    /// C: `getOpMode(GET_OPCODE(i))`
    pub fn op_mode(self) -> Option<OpMode> {
        self.opcode().map(get_op_mode)
    }
}

// ─── Signed-argument encode/decode helpers ────────────────────────────────────
//
// C: #define int2sC(i)  ((i) + OFFSET_sC)
// C: #define sC2int(i)  ((i) - OFFSET_sC)
//
// These are inline helpers used at call sites; the Instruction methods above
// incorporate them, but standalone functions are provided for codegen use.

/// Encode a signed integer into an unsigned C-field value.
///
/// C: `int2sC(i)`
#[inline]
pub const fn int_to_s_c(i: i32) -> u32 {
    (i + OFFSET_S_C) as u32
}

/// Decode a C-field unsigned value to a signed integer.
///
/// C: `sC2int(i)`
#[inline]
pub const fn s_c_to_int(i: u32) -> i32 {
    i as i32 - OFFSET_S_C
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_opcodes_matches_enum() {
        assert_eq!(NUM_OPCODES, 83);
        assert_eq!(OpCode::ExtraArg as usize, 82);
    }

    #[test]
    fn op_modes_table_length() {
        assert_eq!(OP_MODES.len(), NUM_OPCODES);
    }

    #[test]
    fn opmode_byte_values() {
        assert_eq!(OP_MODES[OpCode::Move as usize], 0b00001000); // a=1, mode=iABC=0 → 8
        assert_eq!(OP_MODES[OpCode::LoadI as usize], 0b00001010); // a=1, mode=iAsBx=2 → 10
        assert_eq!(OP_MODES[OpCode::Jmp as usize], 0b00000100); // a=0, mode=isJ=4 → 4
        assert_eq!(OP_MODES[OpCode::MmBin as usize], 0b10000000); // mm=1, a=0, mode=iABC=0 → 128
        assert_eq!(OP_MODES[OpCode::Call as usize], 0b01101000); // ot=1,it=1,a=1,mode=0 → 104
        assert_eq!(OP_MODES[OpCode::ExtraArg as usize], 0b00000011); // mode=iAx=3 → 3
    }

    #[test]
    fn from_u32_round_trip() {
        for i in 0..NUM_OPCODES {
            let op = OpCode::from_u32(i as u32).expect("valid opcode");
            assert_eq!(op as usize, i);
        }
        assert!(OpCode::from_u32(83).is_none());
    }

    #[test]
    fn instruction_arg_a() {
        let i = Instruction::abck(OpCode::Move, 5, 3, 0, 0);
        assert_eq!(i.arg_a(), 5);
        assert_eq!(i.arg_b(), 3);
    }

    #[test]
    fn instruction_s_bx_round_trip() {
        let mut i = Instruction::abx(OpCode::LoadK, 0, 0);
        i.set_arg_s_bx(-10);
        assert_eq!(i.arg_s_bx(), -10);
        i.set_arg_s_bx(0);
        assert_eq!(i.arg_s_bx(), 0);
        i.set_arg_s_bx(100);
        assert_eq!(i.arg_s_bx(), 100);
    }

    #[test]
    fn instruction_s_j_round_trip() {
        let mut i = Instruction::sj(OpCode::Jmp, (OFFSET_S_J) as u32, 0);
        assert_eq!(i.arg_s_j(), 0);
        i.set_arg_s_j(42);
        assert_eq!(i.arg_s_j(), 42);
        i.set_arg_s_j(-1);
        assert_eq!(i.arg_s_j(), -1);
    }

    #[test]
    fn get_op_mode_smoke() {
        assert_eq!(get_op_mode(OpCode::Move), OpMode::Abc);
        assert_eq!(get_op_mode(OpCode::LoadI), OpMode::AsBx);
        assert_eq!(get_op_mode(OpCode::LoadK), OpMode::ABx);
        assert_eq!(get_op_mode(OpCode::Jmp), OpMode::SJ);
        assert_eq!(get_op_mode(OpCode::ExtraArg), OpMode::Ax);
    }

    #[test]
    fn int_to_s_c_round_trip() {
        assert_eq!(s_c_to_int(int_to_s_c(0)), 0);
        assert_eq!(s_c_to_int(int_to_s_c(100)), 100);
        assert_eq!(s_c_to_int(int_to_s_c(-50)), -50);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// PORT STATUS
//   source:        src/lopcodes.c  (104 lines, 0 functions — data only)
//                  src/lopcodes.h  (406 lines, merged per PORTING.md §1)
//   target_crate:  lua-code
//   confidence:    high
//   todos:         1
//   port_notes:    1
//   unsafe_blocks: 0
//   notes:         Pure data/encoding translation; OpCode::from_u32 needs
//                  Phase B review for transmute vs. num_enum. Self_ rename
//                  is permanent (Rust keyword conflict).
// ──────────────────────────────────────────────────────────────────────────
