use std::fmt;

/// Ergonomic code builder that replaces raw `Vec<String>` + `push` patterns
/// in code generators. Provides shorthand methods for common operations.
///
/// # Example
///
/// ```no_run
/// let mut cb = CodeBuf::new();
/// cb.l("fn main() {");
/// cb.l("    println!(\"hello\");");
/// cb.l("}");
/// assert_eq!(cb.build(), "fn main() {\n    println!(\"hello\");\n}");
/// ```
#[allow(dead_code)]
pub(crate) struct CodeBuf {
    lines: Vec<String>,
}

#[allow(dead_code)]
impl CodeBuf {
    /// Creates an empty code buffer.
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Creates an empty code buffer with the given capacity hint.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            lines: Vec::with_capacity(cap),
        }
    }

    /// Adds a line from an owned `String`.
    ///
    /// This is the direct equivalent of `lines.push(s)` on a `Vec<String>`,
    /// making it easy to migrate `lines.push(format!(...))` calls.
    #[inline]
    pub fn push(&mut self, s: String) {
        self.lines.push(s);
    }

    /// Adds a literal line.
    #[inline]
    pub fn l(&mut self, s: &str) -> &mut Self {
        self.lines.push(s.to_string());
        self
    }

    /// Adds a formatted line.
    ///
    /// Prefer this over `l(&format!(...))` to avoid an extra allocation
    /// when the buffer is empty.
    #[inline]
    pub fn f(&mut self, args: fmt::Arguments<'_>) -> &mut Self {
        self.lines.push(args.to_string());
        self
    }

    /// Adds a blank line.
    #[inline]
    pub fn b(&mut self) -> &mut Self {
        self.lines.push(String::new());
        self
    }

    /// Appends all lines from another `CodeBuf`.
    #[inline]
    pub fn extend(&mut self, other: CodeBuf) -> &mut Self {
        self.lines.extend(other.lines);
        self
    }

    /// Appends all strings from a `Vec<String>`.
    #[inline]
    pub fn extend_vec(&mut self, v: Vec<String>) {
        self.lines.extend(v);
    }

    /// Returns `true` if no lines have been added.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Returns a mutable reference to the inner `Vec<String>`.
    ///
    /// Useful for passing to legacy functions that still accept
    /// `&mut Vec<String>` during incremental migration.
    #[inline]
    pub fn lines_mut(&mut self) -> &mut Vec<String> {
        &mut self.lines
    }

    /// Returns a mutable reference to the last line, if any.
    #[inline]
    pub fn last_mut(&mut self) -> Option<&mut String> {
        self.lines.last_mut()
    }

    /// Returns the number of lines.
    #[inline]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Joins all lines with `\n` and returns the final string.
    pub fn build(self) -> String {
        self.lines.join("\n")
    }
}

/// Returns `true` if `name` matches the wildcard `pattern`.
///
/// Supports `*` as a wildcard that matches any sequence of characters.
/// - `"user*"` matches `"user"`, `"user_admin"`, `"user_list"`, etc.
/// - `"*admin"` matches `"admin"`, `"user_admin"`, etc.
/// - `"user*admin"` matches `"user_admin"`, `"user_super_admin"`, etc.
/// - `"user"` (no wildcard) matches only `"user"` exactly.
#[cfg(any(
    feature = "ts",
    feature = "js",
    feature = "kt",
    feature = "rs",
    feature = "cs"
))]
pub(crate) fn matches_wildcard(name: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        name.starts_with(prefix)
    } else if let Some(suffix) = pattern.strip_prefix('*') {
        name.ends_with(suffix)
    } else if let Some((before, after)) = pattern.split_once('*') {
        name.starts_with(before)
            && name.ends_with(after)
            && name.len() >= before.len() + after.len()
    } else {
        name == pattern
    }
}

/// Convenience macro for adding a formatted line to a `CodeBuf`.
///
/// Equivalent to `cb.f(format_args!(...))` but shorter.
///
/// # Example
///
/// ```no_run
/// let mut cb = CodeBuf::new();
/// cbl!(cb, "let x = {};", 42);
/// ```
#[allow(unused_macros)]
macro_rules! cbl {
    ($cb:expr, $($arg:tt)*) => {
        $cb.f(format_args!($($arg)*))
    };
}
