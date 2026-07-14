//! Opcode name table for debug/disassembly output.
//!
//! Order must match the `OpCode` enum (`lopcodes.h` in the reference C
//! source): the `ORDER OP` invariant.

//
// Drops the trailing NULL sentinel C uses to mark the end of the array:
// length is `OP_COUNT`, known at compile time, and a Rust slice with
// bounds-checking serves the same role.

/// Total number of opcodes. Must equal `OpCode::Count as usize` once the
/// enum lands.
pub const OP_COUNT: usize = 86;

/// Opcode names, indexed by `OpCode as usize`. ORDER OP — must match the
/// `OpCode` enum order in `lopcodes.h` exactly.
pub const OPNAMES: [&str; OP_COUNT] = [
    "MOVE",
    "LOADI",
    "LOADF",
    "LOADK",
    "LOADKX",
    "LOADFALSE",
    "LFALSESKIP",
    "LOADTRUE",
    "LOADNIL",
    "GETUPVAL",
    "SETUPVAL",
    "GETTABUP",
    "GETTABLE",
    "GETI",
    "GETFIELD",
    "SETTABUP",
    "SETTABLE",
    "SETI",
    "SETFIELD",
    "NEWTABLE",
    "SELF",
    "ADDI",
    "ADDK",
    "SUBK",
    "MULK",
    "MODK",
    "POWK",
    "DIVK",
    "IDIVK",
    "BANDK",
    "BORK",
    "BXORK",
    "SHRI",
    "SHLI",
    "ADD",
    "SUB",
    "MUL",
    "MOD",
    "POW",
    "DIV",
    "IDIV",
    "BAND",
    "BOR",
    "BXOR",
    "SHL",
    "SHR",
    "MMBIN",
    "MMBINI",
    "MMBINK",
    "UNM",
    "BNOT",
    "NOT",
    "LEN",
    "CONCAT",
    "CLOSE",
    "TBC",
    "JMP",
    "EQ",
    "LT",
    "LE",
    "EQK",
    "EQI",
    "LTI",
    "LEI",
    "GTI",
    "GEI",
    "TEST",
    "TESTSET",
    "CALL",
    "TAILCALL",
    "RETURN",
    "RETURN0",
    "RETURN1",
    "FORLOOP",
    "FORPREP",
    "TFORPREP",
    "TFORCALL",
    "TFORLOOP",
    "SETLIST",
    "CLOSURE",
    "VARARG",
    "VARARGPREP",
    "EXTRAARG",
    "ERRNNIL",
    "VARARGPACK",
    "GETVARG",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_count_matches_table() {
        assert_eq!(OPNAMES.len(), OP_COUNT);
    }

    #[test]
    fn first_and_last_opcodes() {
        assert_eq!(OPNAMES[0], "MOVE");
        assert_eq!(OPNAMES[OP_COUNT - 1], "GETVARG");
    }
}
