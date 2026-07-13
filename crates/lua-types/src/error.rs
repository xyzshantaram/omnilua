//! `LuaError` and its canonical constructors. PORT_STRATEGY §3.7, PORTING.md §6.

use crate::status::LuaStatus;
use crate::value::LuaValue;
use std::fmt;

/// Internal control-flow payload used by the standalone CLI to implement
/// `os.exit` without making Lua protected calls catch it as an ordinary error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LuaExit(pub i32);

/// Internal control-flow payload for Lua 5.5 `coroutine.close()` self-close.
/// It is caught at the coroutine resume boundary; panic hooks should suppress it
/// like [`LuaExit`] because it is not a Rust runtime panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LuaThreadClose(pub LuaStatus);

/// The Lua error type. Carries a `LuaValue` payload because Lua errors can
/// be any value (typically a string).
///
/// The `*Msg` variants carry the message as owned bytes instead of a GC
/// string: host-side code builds errors with no VM (and no active heap) in
/// scope, and allocating the payload eagerly either required an active
/// `HeapGuard` or fell back to a detached, never-freed box (the issue #249
/// leak class — see #253). The bytes are materialized into a real GC string
/// by [`into_value`](LuaError::into_value) at the moment the error actually
/// enters a VM, where a heap is active by construction.
#[derive(Debug, Clone)]
pub enum LuaError {
    Runtime(LuaValue),
    Syntax(LuaValue),
    RuntimeMsg(Box<[u8]>),
    SyntaxMsg(Box<[u8]>),
    Memory,
    Error,
    Yield,
    File,
    Gc,
}

impl LuaError {
    // ── Generic message constructors ─────────────────────────────────────

    /// Build a runtime error whose message is owned bytes — no GC
    /// allocation happens here; [`into_value`](LuaError::into_value)
    /// materializes the string when the error enters a VM (#253).
    pub fn runtime(args: fmt::Arguments<'_>) -> Self {
        LuaError::RuntimeMsg(format!("{}", args).into_bytes().into_boxed_slice())
    }
    pub fn syntax(args: fmt::Arguments<'_>) -> Self {
        LuaError::SyntaxMsg(format!("{}", args).into_bytes().into_boxed_slice())
    }
    pub fn syntax_at(args: fmt::Arguments<'_>, source: &[u8], line: i32) -> Self {
        LuaError::SyntaxMsg(
            format!("{}:{}: {}", String::from_utf8_lossy(source), line, args)
                .into_bytes()
                .into_boxed_slice(),
        )
    }
    pub fn syntax_raw(msg: &[u8]) -> Self {
        LuaError::SyntaxMsg(msg.to_vec().into_boxed_slice())
    }

    // ── Standard-shape constructors ──────────────────────────────────────
    pub fn type_error(v: &LuaValue, op: &str) -> Self {
        LuaError::runtime(format_args!("attempt to {} a {} value", op, v.type_name()))
    }
    pub fn call_error(v: &LuaValue) -> Self {
        Self::type_error(v, "call")
    }
    pub fn concat_error(p1: &LuaValue, p2: &LuaValue) -> Self {
        let bad = if matches!(p1, LuaValue::Str(_) | LuaValue::Int(_) | LuaValue::Float(_)) {
            p2
        } else {
            p1
        };
        LuaError::runtime(format_args!(
            "attempt to concatenate a {} value",
            bad.type_name()
        ))
    }
    pub fn arith_error(p1: &LuaValue, p2: &LuaValue, _msg: &str) -> Self {
        let bad = if matches!(p1, LuaValue::Int(_) | LuaValue::Float(_)) {
            p2
        } else {
            p1
        };
        LuaError::runtime(format_args!(
            "attempt to perform arithmetic on a {} value",
            bad.type_name()
        ))
    }
    pub fn int_overflow(_p1: &LuaValue, _p2: &LuaValue) -> Self {
        LuaError::runtime(format_args!("number has no integer representation"))
    }
    pub fn order_error(p1: &LuaValue, p2: &LuaValue) -> Self {
        LuaError::runtime(format_args!(
            "attempt to compare {} with {}",
            p1.type_name(),
            p2.type_name()
        ))
    }
    pub fn for_error(v: &LuaValue, what: &str) -> Self {
        LuaError::runtime(format_args!(
            "bad 'for' {} (number expected, got {})",
            what,
            v.type_name()
        ))
    }
    pub fn arg_error(narg: i32, msg: &str) -> Self {
        LuaError::runtime(format_args!("bad argument #{} ({})", narg, msg))
    }
    pub fn type_arg_error(narg: i32, expected: &str, got: &LuaValue) -> Self {
        LuaError::runtime(format_args!(
            "bad argument #{} ({} expected, got {})",
            narg,
            expected,
            got.type_name()
        ))
    }

    // ── Pass-through constructors ────────────────────────────────────────
    pub fn from_value(v: LuaValue) -> Self {
        // Special-case: the global "not enough memory" string becomes Memory.
        // The real impl checks ptr equality against G(L)->memerrmsg.
        LuaError::Runtime(v)
    }
    pub fn with_status(status: LuaStatus) -> Self {
        match status {
            LuaStatus::Ok => LuaError::Error,
            LuaStatus::Yield => LuaError::Yield,
            LuaStatus::ErrRun => LuaError::Runtime(LuaValue::Nil),
            LuaStatus::ErrSyntax => LuaError::Syntax(LuaValue::Nil),
            LuaStatus::ErrMem => LuaError::Memory,
            LuaStatus::ErrErr => LuaError::Error,
            LuaStatus::ErrFile => LuaError::File,
            LuaStatus::ErrGc => LuaError::Gc,
        }
    }

    pub fn to_status(&self) -> LuaStatus {
        match self {
            LuaError::Runtime(_) | LuaError::RuntimeMsg(_) => LuaStatus::ErrRun,
            LuaError::Syntax(_) | LuaError::SyntaxMsg(_) => LuaStatus::ErrSyntax,
            LuaError::Memory => LuaStatus::ErrMem,
            LuaError::Error => LuaStatus::ErrErr,
            LuaError::Yield => LuaStatus::Yield,
            LuaError::File => LuaStatus::ErrFile,
            LuaError::Gc => LuaStatus::ErrGc,
        }
    }

    /// The message bytes, when this error carries a textual message —
    /// either a GC string payload or the owned bytes of a not-yet-
    /// materialized `*Msg` variant. The canonical accessor for host-side
    /// message extraction; callers needing a fallback keep their own.
    /// Never allocates, never panics.
    pub fn message_bytes(&self) -> Option<&[u8]> {
        match self {
            LuaError::Runtime(LuaValue::Str(s)) | LuaError::Syntax(LuaValue::Str(s)) => {
                Some(s.as_bytes())
            }
            LuaError::RuntimeMsg(b) | LuaError::SyntaxMsg(b) => Some(b),
            _ => None,
        }
    }

    /// Convert the error into the Lua value that gets pushed/propagated.
    ///
    /// This is the VM-entry boundary for the `*Msg` variants: the owned
    /// bytes become a real GC string here, via `GcRef::new`, which requires
    /// an active `HeapGuard`. Every in-tree call site that pushes an error
    /// object runs inside a guarded region (`pcall_k`, `api::load`,
    /// `with_state`, `lua_resume`, `reset_thread`), so the allocation is
    /// always collector-owned — this is what lets the guard-less detached
    /// allocation arm be removed entirely (#253).
    ///
    /// # Panics
    ///
    /// Panics for `RuntimeMsg`/`SyntaxMsg` payloads when no `HeapGuard` is
    /// active. Host code that only needs the message text should use
    /// [`message_bytes`](LuaError::message_bytes) or
    /// [`message_lossy`](LuaError::message_lossy) instead — those never
    /// allocate and never panic.
    pub fn into_value(self) -> LuaValue {
        match self {
            LuaError::Runtime(v) | LuaError::Syntax(v) => v,
            LuaError::RuntimeMsg(b) | LuaError::SyntaxMsg(b) => LuaValue::Str(
                crate::gc::GcRef::new(crate::string::LuaString::from_bytes(b.into_vec())),
            ),
            _ => LuaValue::Nil,
        }
    }

    /// Human-readable error payload for embedders.
    ///
    /// Lua errors can carry any Lua value. When the payload is a byte string,
    /// this returns it using lossy UTF-8 conversion; other payloads fall back to
    /// the Lua type name so host integrations do not have to parse `Debug`.
    pub fn message_lossy(&self) -> String {
        match self {
            LuaError::Runtime(v) | LuaError::Syntax(v) => lua_value_message_lossy(v),
            LuaError::RuntimeMsg(b) | LuaError::SyntaxMsg(b) => {
                String::from_utf8_lossy(b).into_owned()
            }
            LuaError::Memory => "not enough memory".to_string(),
            LuaError::Error => "error in error handling".to_string(),
            LuaError::Yield => "attempt to yield across a C-call boundary".to_string(),
            LuaError::File => "file error".to_string(),
            LuaError::Gc => "garbage collector error".to_string(),
        }
    }
}

fn lua_value_message_lossy(value: &LuaValue) -> String {
    match value {
        LuaValue::Str(s) => String::from_utf8_lossy(s.as_bytes()).into_owned(),
        LuaValue::Nil => "nil".to_string(),
        LuaValue::Bool(v) => v.to_string(),
        LuaValue::Int(v) => v.to_string(),
        LuaValue::Float(v) => v.to_string(),
        other => format!("{} error", other.type_name()),
    }
}

impl fmt::Display for LuaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LuaError::RuntimeMsg(b) | LuaError::SyntaxMsg(b) => {
                write!(f, "{}", String::from_utf8_lossy(b))
            }
            other => write!(f, "{:?}", other),
        }
    }
}
impl std::error::Error for LuaError {}
