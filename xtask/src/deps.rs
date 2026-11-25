//! `check-deps`: the AD-4 dependency matrix (architecture, Design Paradigm
//! table) checked against `cargo metadata`. Only normal and build edges count;
//! dev-dependencies are test scaffolding. Every workspace crate must be
//! classified here, so a new crate cannot slip past the matrix.
//!
//! Story 3.3 (AD-10, amended 2026-11-23): call-site rules beside the
//! matrix, over the shipped source under `--root` (every `.rs` of a crate
//! but its `tests/`, `benches/` and `examples/`, comments and literals left
//! out; a literal or comment that never ends fails closed):
//!
//! - `TrustedInput`'s two named constructors, `from_step_output` and
//!   `from_owner_note`, are named only in `crates/am-engine/src/` and, in
//!   am-domain, on their definition lines;
//! - `Instruction::authored` (review P14), which turns text into a trusted
//!   instruction, is named only where config or the owner's own words
//!   become one: [`INSTRUCTION_SITES`] (three am-app files), the
//!   `#[cfg(test)]` items of [`INSTRUCTION_TEST_SITES`], and am-domain's
//!   definition line.
//!
//! Story 3.5 (AD-10, the type wall), and what the rule can and cannot do.
//! `UntrustedData` has no conversion into an `Instruction`, but a string
//! can still be pulled out of `UntrustedContent::Text(s)`, `Part::Untrusted`
//! or a `TrustedInput` and handed to `Instruction::authored`. So a file in
//! [`INSTRUCTION_SITES`] may not name `UntrustedData`, `UntrustedContent`,
//! `into_content`, `Part::Untrusted`, `Part::Trusted` or `TrustedInput`, over
//! whole-file text with whitespace normalised (a glob or braced import of
//! `Part`'s variants and an alias of `Part` fail too), and no file in
//! `crates/` may alias the untrusted types or `TrustedInput`. **The limit:**
//! this bans names in a short list of am-app files that build only from
//! config or fixed text. It does not follow data: a `String` already in
//! scope in a site file (a step's output, say) can still be handed to
//! `authored`, so what a site passes in is a review check, not this rule's.
//! The rule only narrows what story 3.3's rule allows.
//!
//! Story 3.4 (AD-15, AD-28; review R12): the one Human-only resolve of an
//! Inbox item, `Writer::resolve_inbox_item`, and the Human actor's type,
//! `HumanActor`, are matched as tokens over whole files (so a split line, a
//! `use … as` or `type` alias or `<HumanActor>::new` is still seen): they
//! stand in shipped code only in am-domain (the definition and its
//! re-export), am-store's `src/lib.rs` (the resolve), the two command
//! handlers ([`RESOLVE_SITES`]) and `#[cfg(test)]` items of am-store.
//! Review S5: in am-domain only `actor.rs` and `lib.rs`'s plain re-export,
//! and in every file an `as` alias or a `pub use` of either fails.
//!
//! Story 3.6, the store slice (AD-21; the owner's condition of the third
//! re-slice and the manager's row 49, 2026-11-24): names unique in
//! `crates/`, matched as bare tokens over whole files, so a split line,
//! spacing, a path call, a function value, a turbofish, `r#` or a `use …
//! as` alias of the store's `Reader` is still seen (review round 1, R4):
//!
//! - the store's row command, `apply_row_write`, stands in shipped code
//!   only in `crates/am-store/src/` ([`ROW_HOME`]) and, since the ToolGate
//!   slice, the engine's implementation of the row-writer port
//!   ([`ROW_WRITER_IMPL`], its one caller);
//! - the readers of row content on the store's `Reader` ([`ROW_READERS`])
//!   stand there only too; the data-free receipt (`row_receipt`) is not
//!   one of them.
//!
//! Story 3.6, the ToolGate slice (AD-10 and AD-21 as amended 2026-11-24;
//! owner conditions Q1, Q4 and b; matrix rows 33, 37 and 43), over whole
//! files, so spacing around `.` and `::`, line breaks and qualified paths
//! are seen:
//!
//! - the port's method `write_row` stands only as the one method call in
//!   ToolGate ([`TOOLGATE`]), inside its `fn write`, and as the `fn
//!   write_row` of its trait
//!   ([`ROWS_PORT`]) and of the engine's implementation
//!   ([`ROW_WRITER_IMPL`]);
//! - `EngineResult::row_written`, the engine result's one constructor, is
//!   called by path only in ToolGate (its definition, `fn row_written`, is
//!   am-domain's);
//! - no file in `crates/` makes an alias (`use … as`, `type`) of
//!   `RowWriter` or `EngineResult`, and no file outside am-domain imports
//!   `am_domain::rows::*` or `am_domain::content::*`;
//! - the records that carry a row's data ([`ROW_RECORDS`]) stand only in
//!   am-store's source, and only am-store depends on `rusqlite` (normal,
//!   build or target-specific edges; dev-dependencies are tests), so no
//!   other crate reads a row and no engine path can pass row content to a
//!   model call but the one read path below.
//!
//! Story 3.22, "Rows reaching later steps" (AD-10, AD-43; matrix row 46):
//! the one read path widens both rules above by one name each and one
//! file. The store's snapshot reader, `row_snapshot` ([`SNAPSHOT_READER`]),
//! and its record, `RowSnapshot` ([`SNAPSHOT_RECORD`]), stand in shipped
//! code only in am-store's source and in am-engine's `row_inputs` module
//! ([`ROW_READ_SITE`]), which hands a step the rows it reads as fenced
//! untrusted data. The other readers and records stay am-store's alone,
//! there too, and an alias of `RowSnapshot` (`use … as`, `type`) fails in
//! every file. Review triage round 1, R1: that module calls the snapshot
//! reader exactly once, and its own items, `read_rows` and `RowRead`
//! ([`READ_ITEMS`]), stand only there (their definitions) and in the
//! engine's driver ([`ROW_DRIVER`]), where `read_rows` is only ever called;
//! neither is renamed in any file. Review triage round 2: the module is
//! `pub(crate)`, its snapshot call stands inside `fn read_rows`, the
//! driver names both items only inside `fn inputs_of` (T2), and `fn
//! inputs_of` names no way to take a part's text out of its fence (T3).

use crate::Failure;
use crate::forbid::{string_literals, strip_comments_and_strings, unterminated_finding};
use crate::seam_guard::names;
use crate::shipped::{in_ranges, sources, test_ranges};
use crate::walk::is_word;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// `TrustedInput`'s two named constructors (AD-10).
const TRUSTED_CONSTRUCTORS: [&str; 2] = ["from_step_output", "from_owner_note"];

/// The one crate that calls them.
const TRUSTED_CALLER: &str = "crates/am-engine/src/";

/// The crate that defines them.
const TRUSTED_HOME: &str = "crates/am-domain/src/";

/// The shipped files that may call `Instruction::authored` (review P14):
/// am-app building a run's plan from the config snapshot (a seat's
/// instructions, a step's instruction) and the spike harness's and the
/// Account test's fixed prompts. Only am-app files: am-engine authors no
/// `Instruction` (its spike driver takes the fixed prompts from the caller).
pub const INSTRUCTION_SITES: [&str; 3] = [
    "crates/am-app/src/run.rs",
    "crates/am-app/src/spike.rs",
    "crates/am-app/src/accounts.rs",
];

/// What a file in [`INSTRUCTION_SITES`] may not name in code (story 3.5,
/// AD-10): the untrusted types, and the accessor that hands their content
/// out.
/// Review R3 adds `Part::Untrusted`, `Part::Trusted` and `TrustedInput`,
/// through which untrusted or journaled text could still be handed on.
const UNTRUSTED_NAMES: [&str; 6] = [
    "UntrustedData",
    "UntrustedContent",
    "into_content",
    "Part::Untrusted",
    "Part::Trusted",
    "TrustedInput",
];

/// The types no file in `crates/` may rename (review R3): an alias would
/// carry them past the site rule above.
const ALIASED_TYPES: [&str; 3] = ["UntrustedData", "UntrustedContent", "TrustedInput"];

/// The shipped files whose `#[cfg(test)]` items (only) may call it: the
/// engine driver's own unit tests build plans with it.
pub const INSTRUCTION_TEST_SITES: [&str; 1] = ["crates/am-engine/src/run.rs"];

/// The Human-only resolve of an Inbox item and the Human actor's type
/// (AD-15, story 3.4), matched as tokens.
const RESOLVE_TOKENS: [&str; 2] = ["resolve_inbox_item", "HumanActor"];

/// The two command handlers that may name them: the API's resolve route
/// and `agentic gate`.
pub const RESOLVE_SITES: [&str; 2] = ["crates/am-server/src/inbox.rs", "crates/am-app/src/gate.rs"];

/// Where the type is defined (review S5: only there in am-domain).
const HUMAN_HOME: &str = "crates/am-domain/src/actor.rs";

/// Where the type is re-exported, by a plain `pub use`.
const HUMAN_REEXPORT: &str = "crates/am-domain/src/lib.rs";

/// The file that defines the resolve.
const RESOLVE_HOME: &str = "crates/am-store/src/lib.rs";

/// The crate whose `#[cfg(test)]` items may name both (its own tests of
/// the resolve).
const RESOLVE_TEST_CRATE: &str = "crates/am-store/src/";

/// The store's row command (story 3.6, the store slice; AD-21).
const ROW_COMMAND: &str = "apply_row_write";

/// The store `Reader`'s methods that read row content (story 3.6, row 49),
/// named uniquely in `crates/` (review round 1, R4).
const ROW_READERS: [&str; 4] = [
    "table_rows_of",
    "table_row_by_id",
    "row_history_of",
    "row_written_at",
];

/// The one place in shipped code that names them: am-store's own source.
const ROW_HOME: &str = "crates/am-store/src/";

/// The engine's implementation of the row-writer port: the one shipped
/// file outside am-store that names the row command (story 3.6, the
/// ToolGate slice), and the second home of `fn write_row`.
const ROW_WRITER_IMPL: &str = "crates/am-engine/src/row_writer.rs";

/// The one caller of the port's method and builder of an engine result.
const TOOLGATE: &str = "crates/am-engine/src/toolgate.rs";

/// The port's definition.
const ROWS_PORT: &str = "crates/am-domain/src/rows.rs";

/// The port's method and the engine result's constructor.
const PORT_METHOD: &str = "write_row";
const ENGINE_CONSTRUCTOR: &str = "row_written";

/// The types no file in `crates/` may rename (story 3.6): an alias would
/// carry them past the two rules above.
const PORT_TYPES: [&str; 2] = ["RowWriter", "EngineResult"];

/// The am-domain modules no other crate may glob-import.
const GLOBBED_MODULES: [&str; 2] = ["rows", "content"];

/// The records that carry a row's data (story 3.6, row 43).
const ROW_RECORDS: [&str; 2] = ["RowRecord", "RowHistoryRecord"];

/// Story 3.22 (row 46): the store's snapshot reader, a row's current
/// after-image, and the record it returns.
const SNAPSHOT_READER: &str = "row_snapshot";
const SNAPSHOT_RECORD: &str = "RowSnapshot";

/// The one shipped file outside am-store that names them: the engine's
/// read site of the rows a step reads at its start.
const ROW_READ_SITE: &str = "crates/am-engine/src/row_inputs.rs";

/// Story 3.22, review triage round 1, R1: the read site's own items (a
/// read's `part` holds a row's text), and the one file besides the read
/// site that names them, the engine's driver.
const READ_ITEMS: [&str; 2] = ["read_rows", "RowRead"];
const ROW_DRIVER: &str = "crates/am-engine/src/run.rs";

/// Story 3.22, review triage round 2: the engine's crate root, which keeps
/// the read site `pub(crate)` (T2), and the accessors that take a part's
/// text out of its `UntrustedData`, which `fn inputs_of` never names (T3).
const ENGINE_LIB: &str = "crates/am-engine/src/lib.rs";
const FENCE_EXITS: [&str; 2] = ["content", "into_content"];

/// The one SQLite library, and the one crate that may depend on it.
const SQLITE: &str = "rusqlite";
const SQLITE_HOME: &str = "am-store";

/// Every finding of the call-site rules on the shipped source under
/// `root`.
fn call_site_findings(root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    for source in sources(root) {
        if let Some(finding) = unterminated_finding(&source.name, &source.text) {
            findings.push(finding);
            continue;
        }
        let code = strip_comments_and_strings(&source.text);
        let tests = test_ranges(&source.text);
        for (n, line) in code.lines().enumerate() {
            let flat = line.split_whitespace().collect::<Vec<_>>().join(" ");
            let definition = |name: &str| {
                source.name.starts_with(TRUSTED_HOME)
                    && flat.contains(&format!("fn {name}"))
                    && flat.matches(name).count() == 1
            };
            for name in TRUSTED_CONSTRUCTORS {
                if !names(line, name) || source.name.starts_with(TRUSTED_CALLER) || definition(name)
                {
                    continue;
                }
                findings.push(format!(
                    "{}:{}: names TrustedInput::{name}; only am-engine builds a TrustedInput, from a clean step's journaled output or an owner's note (AD-10: its two constructors are called only in {TRUSTED_CALLER})",
                    source.name,
                    n + 1
                ));
            }
            let allowed = INSTRUCTION_SITES.contains(&source.name.as_str())
                || (INSTRUCTION_TEST_SITES.contains(&source.name.as_str())
                    && in_ranges(&tests, n + 1))
                || definition("authored");
            if INSTRUCTION_SITES.contains(&source.name.as_str()) {
                for name in UNTRUSTED_NAMES {
                    if names(line, name) {
                        findings.push(format!(
                            "{}:{}: names {name}, and this file may call Instruction::authored; a file that turns config or the owner's words into an Instruction may not name untrusted content, so no text of it can become one (AD-10: no conversion from UntrustedData to Instruction)",
                            source.name,
                            n + 1
                        ));
                    }
                }
            }
            if names(line, "authored") && !allowed {
                findings.push(format!(
                    "{}:{}: names Instruction::authored; text becomes a trusted Instruction only where config or the owner's own words do, at the sites xtask/src/deps.rs lists (AD-10; model, tool and file text is never an Instruction)",
                    source.name,
                    n + 1
                ));
            }
        }
        if INSTRUCTION_SITES.contains(&source.name.as_str()) {
            findings.extend(site_findings(&source.name, &code, &findings));
        }
        findings.extend(resolve_findings(&source.name, &code, &tests));
        findings.extend(row_findings(&source.name, &code));
        findings.extend(alias_findings(&source.name, &code, &ALIASED_TYPES));
        findings.extend(port_findings(&source.name, &code));
        findings.extend(record_findings(&source.name, &code));
        findings.extend(read_path_findings(&source.name, &code, &source.text));
    }
    findings
}

/// Story 3.22, review triage round 1, R1 (AD-10, AD-43): [`READ_ITEMS`]
/// stand only in [`ROW_READ_SITE`] and [`ROW_DRIVER`], where `read_rows`
/// is only called (never a function value or an import); neither is
/// renamed in any file; and the read site calls the snapshot reader
/// exactly once. Review triage round 2, T2: that call stands inside `fn
/// read_rows`, both items stand in the driver only inside `fn inputs_of`,
/// and the engine's `lib.rs` keeps the module the crate's own. T3 (as the
/// manager narrowed it): `fn inputs_of` names no way to take a part's text
/// out of its `UntrustedData` ([`FENCE_EXITS`], `write!`, or a `Debug`
/// placeholder in any string there). `text` is the file as written (its
/// string literals); each finding names its line and the exact name.
fn read_path_findings(name: &str, code: &str, text: &str) -> Vec<String> {
    let mut findings: Vec<String> = alias_lines(code, &READ_ITEMS)
        .into_iter()
        .map(|(line, token)| {
            format!(
                "{name}:{line}: makes an alias of {token} (`type` or `use … as`); the read site's items are named as themselves, so check-deps sees every use of them (story 3.22: one read path; AD-10)"
            )
        })
        .collect();
    let called = |at: usize, token: &str| code[at + token.len()..].trim_start().starts_with('(');
    let within = |span: Option<(usize, usize)>, at: usize| {
        span.is_some_and(|(from, to)| (from..to).contains(&at))
    };
    if name == ROW_READ_SITE {
        let offsets = token_offsets(code, SNAPSHOT_READER);
        let read = fn_body(code, "read_rows");
        let calls = offsets
            .iter()
            .filter(|at| {
                before(code, **at) == Before::Dot
                    && called(**at, SNAPSHOT_READER)
                    && within(read, **at)
            })
            .count();
        if offsets.len() > 1 || calls != offsets.len() {
            let lines: BTreeSet<usize> = offsets.iter().map(|at| line_at(code, *at)).collect();
            for line in lines {
                findings.push(format!(
                    "{name}:{line}: names {SNAPSHOT_READER} again, or not as one method call inside fn read_rows; am-engine's row_inputs module reads a row's after-image with exactly one call of the store's snapshot reader, in fn read_rows itself (story 3.22, review triage rounds 1 and 2, R1 and T2; AD-10, AD-43)"
                ));
            }
        }
        return findings;
    }
    if name == ENGINE_LIB {
        for at in token_offsets(code, "row_inputs") {
            let head = code[..at].trim_end();
            let public = head.strip_suffix("mod").is_some_and(|rest| {
                !rest.chars().next_back().is_some_and(is_word)
                    && rest
                        .trim_end()
                        .strip_suffix("pub")
                        .is_some_and(|rest| !rest.chars().next_back().is_some_and(is_word))
            });
            if public {
                findings.push(format!(
                    "{name}:{}: declares row_inputs `pub`; the read site is the crate's own, `pub(crate) mod row_inputs`, so no other crate reads a row through it (story 3.22, review triage round 2, T2; AD-10)",
                    line_at(code, at)
                ));
            }
        }
    }
    let inputs = if name == ROW_DRIVER {
        fn_body(code, "inputs_of")
    } else {
        None
    };
    let mut named: BTreeSet<(usize, &str)> = BTreeSet::new();
    for token in READ_ITEMS {
        for at in token_offsets(code, token) {
            let allowed = within(inputs, at) && (token != "read_rows" || called(at, token));
            if !allowed {
                named.insert((line_at(code, at), token));
            }
        }
    }
    for (line, token) in named {
        findings.push(format!(
            "{name}:{line}: names {token}, an item of am-engine's row_inputs module, whose reads carry a row's text; it stands in shipped code only in that module and in the engine's driver, its run module, inside fn inputs_of, where read_rows is only called (story 3.22, review triage rounds 1 and 2, R1 and T2: one read path, fenced and tainting; AD-10, AD-43)"
        ));
    }
    let Some((from, to)) = inputs else {
        return findings;
    };
    let mut exits: BTreeSet<(usize, &str)> = BTreeSet::new();
    for token in FENCE_EXITS {
        for at in token_offsets(code, token)
            .into_iter()
            .filter(|at| (from..to).contains(at))
        {
            exits.insert((line_at(code, at), token));
        }
    }
    for at in token_offsets(code, "write")
        .into_iter()
        .filter(|at| (from..to).contains(at) && code[at + 5..].trim_start().starts_with('!'))
    {
        exits.insert((line_at(code, at), "write!"));
    }
    let lines = line_at(code, from)..=line_at(code, to);
    for (line, literal) in string_literals(text) {
        if lines.contains(&line) && (literal.contains(":?}") || literal.contains(":#?}")) {
            exits.insert((line, "{:?}"));
        }
    }
    for (line, token) in exits {
        findings.push(format!(
            "{name}:{line}: names {token} inside fn inputs_of, a way to take a part's text out of its UntrustedData; there a row's text stays fenced, so fn inputs_of names no content, into_content, write! or Debug placeholder (story 3.22, review triage round 2, T3; AD-10)"
        ));
    }
    findings
}

/// Review T5: the site ban over whole-file text with whitespace around `::`
/// normalised, so `Part ::Untrusted` or a path split over lines is seen, plus
/// the forms that reach `Part`'s variants without writing the path: a glob
/// or braced import (`Part::*`, `Part::{…}`) and an alias of `Part`. A name
/// already reported with its line by the per-line check is not repeated.
fn site_findings(name: &str, code: &str, earlier: &[String]) -> Vec<String> {
    let mut flat = String::new();
    let mut pending_space = false;
    for word in code.split_whitespace() {
        if !flat.is_empty() && !flat.ends_with("::") && !word.starts_with("::") {
            pending_space = true;
        }
        if pending_space {
            flat.push(' ');
            pending_space = false;
        }
        flat.push_str(word);
    }
    let mut findings = Vec::new();
    for token in UNTRUSTED_NAMES {
        let seen = earlier
            .iter()
            .any(|f| f.starts_with(&format!("{name}:")) && f.contains(&format!("names {token},")));
        if !seen && !token_offsets(&flat, token).is_empty() {
            findings.push(format!(
                "{name}: names {token} (across spaces or line breaks), and this file may call Instruction::authored; a file that turns config or the owner's words into an Instruction may not name untrusted content (AD-10: no conversion from UntrustedData to Instruction)"
            ));
        }
    }
    for form in ["Part::*", "Part::{"] {
        if flat.contains(form) {
            findings.push(format!(
                "{name}: imports Part's variants with `{form}`, and this file may call Instruction::authored; it may not reach Part::Untrusted or Part::Trusted without naming them (AD-10)"
            ));
        }
    }
    findings.extend(alias_findings(name, &flat, &["Part"]));
    findings
}

/// Story 3.5, review R3 (AD-10): a `type X = …Token…;` or a `use … Token
/// as X` of an untrusted type or `TrustedInput`, anywhere in shipped code.
fn alias_findings(name: &str, code: &str, types: &[&'static str]) -> Vec<String> {
    alias_lines(code, types)
        .into_iter()
        .map(|(line, token)| {
            format!(
                "{name}:{line}: makes an alias of {token} (`type` or `use … as`); the types are named as themselves so the call-site rules see every use of them (AD-10: no path from untrusted text to an Instruction)"
            )
        })
        .collect()
}

/// The lines (and the type) of every `type X = …Token…;` or `use … Token as
/// X` of one of `types` in `code`.
fn alias_lines(code: &str, types: &[&'static str]) -> BTreeSet<(usize, &'static str)> {
    let mut lines: BTreeSet<(usize, &'static str)> = BTreeSet::new();
    for token in types.iter().copied() {
        for at in token_offsets(code, token) {
            let after = code[at + token.len()..].trim_start();
            let renamed =
                after.starts_with("as") && !after[2..].chars().next().is_some_and(is_word);
            // The statement the token stands in: back to the last `;`, `{`
            // or `}` that is not a use group's brace is close enough to see
            // a leading `type`.
            let start = code[..at].rfind([';', '}']).map_or(0, |i| i + 1);
            let statement = &code[start..at];
            let type_alias = statement.contains('=')
                && statement
                    .split(|c: char| !is_word(c))
                    .any(|word| word == "type");
            if renamed || type_alias {
                lines.insert((line_at(code, at), token));
            }
        }
    }
    lines
}

/// What stands right before the byte offset `at` in `code`, past
/// whitespace and line breaks: `.` (a method call), `::` (a path) or the
/// keyword `fn` (a definition).
#[derive(PartialEq, Eq)]
enum Before {
    Dot,
    Path,
    Fn,
    Other,
}

fn before(code: &str, at: usize) -> Before {
    let head = code[..at].trim_end();
    if head.ends_with("::") {
        Before::Path
    } else if head.ends_with('.') {
        Before::Dot
    } else if head.ends_with("fn")
        && !head[..head.len() - 2]
            .chars()
            .next_back()
            .is_some_and(is_word)
    {
        Before::Fn
    } else {
        Before::Other
    }
}

/// Story 3.6, the ToolGate slice (AD-10, AD-21 as amended; rows 33 and
/// 37): the row-writer port's method and the engine result's constructor
/// at their sites only, no alias of either type, and no glob import of the
/// am-domain modules that define them outside am-domain.
fn port_findings(name: &str, code: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let mut lines: BTreeSet<usize> = BTreeSet::new();
    // Review round 1, R2, and round 2, T1: ToolGate calls the port once,
    // inside its `fn write`.
    let offsets = token_offsets(code, PORT_METHOD);
    let calls = offsets
        .iter()
        .filter(|at| before(code, **at) == Before::Dot)
        .count();
    let write_path = fn_body(code, "write");
    for at in offsets {
        let allowed = match before(code, at) {
            Before::Dot => {
                name == TOOLGATE
                    && calls == 1
                    && write_path.is_some_and(|(from, to)| (from..to).contains(&at))
            }
            Before::Fn => name == ROWS_PORT || name == ROW_WRITER_IMPL,
            Before::Path | Before::Other => false,
        };
        if !allowed {
            lines.insert(line_at(code, at));
        }
    }
    for line in lines {
        findings.push(format!(
            "{name}:{line}: names {PORT_METHOD}, the row-writer port's method; only ToolGate (am-engine's toolgate module) calls it, once, inside its `fn write`, and it is defined only by the port in am-domain's rows module and implemented only in am-engine's row_writer module (AD-21 as amended 2026-11-24, owner Q1: one path writes a row)"
        ));
    }
    let mut lines: BTreeSet<usize> = BTreeSet::new();
    for at in token_offsets(code, ENGINE_CONSTRUCTOR) {
        if before(code, at) == Before::Path && name != TOOLGATE {
            lines.insert(line_at(code, at));
        }
    }
    for line in lines {
        findings.push(format!(
            "{name}:{line}: names EngineResult::{ENGINE_CONSTRUCTOR}, the engine result's one constructor; only ToolGate (am-engine's toolgate module) builds an EngineResult, from engine values only, and am-domain's content module defines it (AD-10 as amended 2026-11-24, owner Q4)"
        ));
    }
    for (line, token) in alias_lines(code, &PORT_TYPES) {
        findings.push(format!(
            "{name}:{line}: makes an alias of {token} (`type` or `use … as`); the row-writer port and the engine result are named as themselves so check-deps sees every use of them (AD-21, AD-10)"
        ));
    }
    if name == ROW_WRITER_IMPL {
        findings.extend(public_items(name, code));
    }
    if !name.starts_with("crates/am-domain/") {
        let flat = flat_paths(code);
        for module in GLOBBED_MODULES {
            if globs(&flat, module) {
                findings.push(format!(
                    "{name}: imports am_domain::{module}::* (a glob); outside am-domain the row-writer port, its types and the engine result are imported by name, so check-deps sees every use of them (AD-21, AD-10)"
                ));
            }
        }
    }
    findings
}

/// The byte span of the body of `fn <function>` in `code` (its braces
/// matched; comments and literals are blanked already).
fn fn_body(code: &str, function: &str) -> Option<(usize, usize)> {
    let at = token_offsets(code, function)
        .into_iter()
        .find(|at| before(code, *at) == Before::Fn)?;
    let open = at + code[at..].find('{')?;
    let mut depth = 0usize;
    for (i, c) in code[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, open + i));
                }
            }
            _ => {}
        }
    }
    None
}

/// Review round 1, R2: the port's implementation has two public items,
/// `pub struct StoreRowWriter` and its `pub fn new`; any other (a method,
/// a field, a `pub(crate)` item) would hand the row command on.
fn public_items(name: &str, code: &str) -> Vec<String> {
    let mut lines: BTreeSet<usize> = BTreeSet::new();
    for at in token_offsets(code, "pub") {
        let item = code[at + 3..]
            .split(['(', '{', ';', ':', '<'])
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if item != "struct StoreRowWriter" && item != "fn new" {
            lines.insert(line_at(code, at));
        }
    }
    lines
        .into_iter()
        .map(|line| {
            format!(
                "{name}:{line}: makes a public item on StoreRowWriter other than the struct and its `new`; the row-writer port's implementation hands the store's row command to no one but through the port (story 3.6, review round 1, R2; AD-21)"
            )
        })
        .collect()
}

/// `code` on one line, whitespace dropped around `::`, `{`, `}` and `,`.
fn flat_paths(code: &str) -> String {
    let mut flat = String::new();
    for word in code.split_whitespace() {
        let joins = flat.ends_with("::")
            || flat.ends_with(['{', ',', '}'])
            || word.starts_with("::")
            || word.starts_with(['{', '}', ',']);
        if !flat.is_empty() && !joins {
            flat.push(' ');
        }
        flat.push_str(word);
    }
    flat
}

/// Whether the flattened `flat` glob-imports `am_domain::<module>::*`,
/// directly or inside an `am_domain::{…}` group.
fn globs(flat: &str, module: &str) -> bool {
    let glob = format!("{module}::*");
    if flat.contains(&format!("am_domain::{glob}")) {
        return true;
    }
    flat.match_indices("am_domain::{").any(|(at, open)| {
        let inner = &flat[at + open.len()..];
        let end = inner.find('}').unwrap_or(inner.len());
        inner[..end].split(',').any(|item| item.trim() == glob)
    })
}

/// Story 3.6, row 43 (owner condition b): the records that carry a row's
/// data stand in shipped code only in am-store's source. Story 3.22 (row
/// 46): the snapshot record there and in [`ROW_READ_SITE`], and never
/// under another name, in any file.
fn record_findings(name: &str, code: &str) -> Vec<String> {
    let mut findings: Vec<String> = alias_lines(code, &[SNAPSHOT_RECORD])
        .into_iter()
        .map(|(line, token)| {
            format!(
                "{name}:{line}: makes an alias of {token} (`type` or `use … as`); the record of a row's current after-image is named as itself, so check-deps sees every use of it (story 3.22: one read site; AD-10)"
            )
        })
        .collect();
    if name.starts_with(ROW_HOME) {
        return findings;
    }
    if name != ROW_READ_SITE {
        let lines: BTreeSet<usize> = token_offsets(code, SNAPSHOT_RECORD)
            .into_iter()
            .map(|at| line_at(code, at))
            .collect();
        for line in lines {
            findings.push(format!(
                "{name}:{line}: names {SNAPSHOT_RECORD}, the record of a row's current after-image; it stands in shipped code only in {ROW_HOME} and in am-engine's row_inputs module, the one site that reads a row for a step (story 3.22, owner Q4: a row reaches a model only fenced and tainting; AD-10, AD-43)"
            ));
        }
    }
    let mut named: BTreeSet<(usize, &str)> = BTreeSet::new();
    for record in ROW_RECORDS {
        for at in token_offsets(code, record) {
            named.insert((line_at(code, at), record));
        }
    }
    findings.extend(named.into_iter().map(|(line, record)| {
        format!(
            "{name}:{line}: names {record}, a record that carries a row's data; it stands in shipped code only in {ROW_HOME} (story 3.6, owner condition b; story 3.22: a step reads a row only through {SNAPSHOT_RECORD} in am-engine's row_inputs module; AD-21)"
        )
    }));
    findings
}

/// The byte offsets of `token` in `code`, as a whole token.
fn token_offsets(code: &str, token: &str) -> Vec<usize> {
    code.match_indices(token)
        .filter(|(at, _)| {
            let before = code[..*at].chars().next_back();
            let after = code[at + token.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        })
        .map(|(at, _)| at)
        .collect()
}

/// The 1-based line of the byte offset `at` in `code`.
fn line_at(code: &str, at: usize) -> usize {
    code[..at].matches('\n').count() + 1
}

/// The byte spans of the `pub use …;` statements of `code` (also
/// `pub(crate) use` and the like), from `pub` to the `;`.
fn pub_use_spans(code: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for start in token_offsets(code, "pub") {
        let mut rest = code[start + 3..].trim_start();
        if let Some(scoped) = rest.strip_prefix('(') {
            let Some(close) = scoped.find(')') else {
                continue;
            };
            rest = scoped[close + 1..].trim_start();
        }
        let is_use = rest.starts_with("use") && !rest[3..].chars().next().is_some_and(is_word);
        if !is_use {
            continue;
        }
        let from = code.len() - rest.len();
        let end = code[from..].find(';').map_or(code.len(), |e| from + e + 1);
        spans.push((start, end));
    }
    spans
}

/// Story 3.4 (AD-15; reviews R12, S5): the Human-only resolve
/// (`resolve_inbox_item`) and the Human actor's type (`HumanActor`),
/// matched as tokens over the whole of one shipped file's code.
///
/// - They stand only in am-domain's `actor.rs` (the definition) and
///   `lib.rs` (its plain re-export), am-store's `lib.rs` (the resolve),
///   the two command handlers ([`RESOLVE_SITES`]), and `#[cfg(test)]`
///   items of am-store.
/// - In every file, the allowed ones included, an `as` alias of either, or
///   a `pub use` of either, fails, but am-domain `lib.rs`'s plain
///   re-export: another name, or another crate's re-export, would carry
///   them past this rule.
fn resolve_findings(name: &str, code: &str, tests: &[(usize, usize)]) -> Vec<String> {
    let pub_uses = pub_use_spans(code);
    let mut renamed: BTreeSet<usize> = BTreeSet::new();
    let mut misplaced: BTreeSet<usize> = BTreeSet::new();
    for token in RESOLVE_TOKENS {
        for at in token_offsets(code, token) {
            let line = line_at(code, at);
            let after = code[at + token.len()..].trim_start();
            let aliased =
                after.starts_with("as") && !after[2..].chars().next().is_some_and(is_word);
            let in_pub_use = pub_uses.iter().any(|(from, to)| (*from..*to).contains(&at));
            if aliased || (in_pub_use && name != HUMAN_REEXPORT) {
                renamed.insert(line);
                continue;
            }
            let allowed = name == HUMAN_HOME
                || (name == HUMAN_REEXPORT && in_pub_use)
                || name == RESOLVE_HOME
                || RESOLVE_SITES.contains(&name)
                || (name.starts_with(RESOLVE_TEST_CRATE) && in_ranges(tests, line));
            if !allowed {
                misplaced.insert(line);
            }
        }
    }
    let mut findings = Vec::new();
    for line in renamed {
        findings.push(format!(
            "{name}:{line}: renames or re-exports HumanActor or resolve_inbox_item (`as` or `pub use`); they are named only as themselves, so this rule sees every use of them (AD-15: only a person resolves an Inbox item)"
        ));
    }
    for line in misplaced {
        findings.push(format!(
            "{name}:{line}: names HumanActor or resolve_inbox_item; they (and HumanActor::new, which builds the one resolver) stand in shipped code only in am-domain's definition and re-export, am-store's lib.rs and the two command handlers, the API's Inbox route and `agentic gate` (AD-15: no engine, tool, MCP or other code path resolves an item)"
        ));
    }
    findings
}

/// Story 3.6, the store slice (AD-21; rows 48 and 49): outside
/// [`ROW_HOME`], the token `apply_row_write` (but in [`ROW_WRITER_IMPL`],
/// the ToolGate slice's one caller) or a [`ROW_READERS`] name anywhere in a
/// file's code (comments and literals left out); since story 3.22,
/// [`SNAPSHOT_READER`] outside it and [`ROW_READ_SITE`]. Each finding names
/// its line and the exact name.
fn row_findings(name: &str, code: &str) -> Vec<String> {
    if name.starts_with(ROW_HOME) {
        return Vec::new();
    }
    let mut findings = Vec::new();
    // Review round 1, R2: in the port's implementation, one call only,
    // inside its `fn write_row`.
    let calls = token_offsets(code, ROW_COMMAND);
    let the_one = name == ROW_WRITER_IMPL
        && calls.len() == 1
        && before(code, calls[0]) == Before::Dot
        && fn_body(code, PORT_METHOD).is_some_and(|(from, to)| (from..to).contains(&calls[0]));
    let lines: BTreeSet<usize> = calls
        .into_iter()
        .filter(|_| !the_one)
        .map(|at| line_at(code, at))
        .collect();
    for line in lines {
        findings.push(format!(
            "{name}:{line}: names {ROW_COMMAND}, the store's row command; it stands in shipped code only in {ROW_HOME} and in am-engine's row_writer module, the row-writer port's implementation and its one caller (the owner's condition for story 3.6: no other engine, app, server or tool path writes a row; AD-21)"
        ));
    }
    let mut named: BTreeSet<(usize, &str)> = BTreeSet::new();
    for reader in ROW_READERS {
        for at in token_offsets(code, reader) {
            named.insert((line_at(code, at), reader));
        }
    }
    for (line, reader) in named {
        findings.push(format!(
            "{name}:{line}: names {reader}, a reader of row content on the store's Reader; no shipped code outside {ROW_HOME} names it (story 3.6, manager 2026-11-24; story 3.22: a step reads a row only through {SNAPSHOT_READER} in am-engine's row_inputs module; the data-free row_receipt is allowed; AD-21)"
        ));
    }
    if name != ROW_READ_SITE {
        let lines: BTreeSet<usize> = token_offsets(code, SNAPSHOT_READER)
            .into_iter()
            .map(|at| line_at(code, at))
            .collect();
        for line in lines {
            findings.push(format!(
                "{name}:{line}: names {SNAPSHOT_READER}, the store's reader of a row's current after-image; it stands in shipped code only in {ROW_HOME} and in am-engine's row_inputs module, the one site that reads a row for a step (story 3.22, owner Q4: a row reaches a model only fenced and tainting; AD-10, AD-43)"
            ));
        }
    }
    findings
}

/// What `am-domain` may use besides std (AD-4: no IO, no async runtime).
const DOMAIN_EXTERNALS: &[&str] = &["serde", "thiserror", "jiff", "uuid"];

/// The only external crates `am-glob` may use (architecture Design
/// Paradigm table, amended 2026-11-20).
const GLOB_EXTERNALS: &[&str] = &["globset", "unicode-normalization"];

/// The only crates that may depend on `am-glob`.
const GLOB_USERS: &[&str] = &["am-config", "am-engine", "am-tools", "am-connectors"];

const GLOB: &str = "am-glob";

const ADAPTERS: &[&str] = &[
    "am-net",
    "am-providers",
    "am-tools",
    "am-connectors",
    "am-render",
    "am-services",
    "am-mcp",
    "am-extract",
    "am-secrets",
];

#[derive(Clone, Copy)]
enum Layer {
    Domain,
    /// am-glob: the one glob matcher, a leaf.
    Glob,
    Engine,
    /// am-store and am-config.
    Record,
    Adapter,
    Api,
    Server,
    /// am-app and am-desktop: anything.
    Root,
    /// am-testkit, am-e2e and xtask: exempt from the matrix.
    Exempt,
}

fn layer(name: &str) -> Option<Layer> {
    Some(match name {
        "am-domain" => Layer::Domain,
        GLOB => Layer::Glob,
        "am-engine" => Layer::Engine,
        "am-store" | "am-config" => Layer::Record,
        n if ADAPTERS.contains(&n) => Layer::Adapter,
        "am-api" => Layer::Api,
        "am-server" => Layer::Server,
        "am-app" | "am-desktop" => Layer::Root,
        "am-testkit" | "am-e2e" | "xtask" => Layer::Exempt,
        _ => return None,
    })
}

/// The workspace crates `krate` (of `layer`) may depend on, and the rule's
/// wording. `am-glob` is added for its four users only.
fn allowed(layer: Layer, krate: &str) -> Option<(Vec<&'static str>, String)> {
    let user = GLOB_USERS.contains(&krate);
    let (mut list, rule): (Vec<&'static str>, String) = match layer {
        Layer::Domain => (
            vec![],
            "am-domain may depend only on std, serde, thiserror, jiff and uuid (AD-4)".into(),
        ),
        Layer::Glob => (
            vec![],
            "am-glob may depend on no workspace crate, and on no external crate but globset and unicode-normalization (AD-4)".into(),
        ),
        Layer::Engine => (
            vec!["am-domain", "am-store"],
            "am-engine may depend only on am-domain, am-store and am-glob (AD-4)".into(),
        ),
        Layer::Record if user => (
            vec!["am-domain"],
            format!("{krate} may depend only on am-domain and am-glob (AD-4)"),
        ),
        Layer::Record => (
            vec!["am-domain"],
            format!("{krate} may depend only on am-domain (AD-4)"),
        ),
        Layer::Adapter if user => (
            vec!["am-domain", "am-net"],
            format!("{krate} may depend only on am-domain, am-net and am-glob (AD-4)"),
        ),
        Layer::Adapter => (
            vec!["am-domain", "am-net"],
            "adapters may depend only on am-domain and am-net (AD-4)".into(),
        ),
        Layer::Api => (
            vec!["am-domain"],
            "am-api may depend only on am-domain (AD-4)".into(),
        ),
        Layer::Server => (
            vec!["am-domain", "am-engine", "am-api", "am-store"],
            "am-server may depend only on am-domain, am-engine, am-api and am-store (AD-4)".into(),
        ),
        Layer::Root | Layer::Exempt => return None,
    };
    if user {
        list.push(GLOB);
    }
    Some((list, rule))
}

/// The one glob library, which only am-glob may name (Consistency
/// Conventions: Globs).
const GLOBSET: &str = "globset";

/// The rule for an edge into globset from any crate but am-glob.
fn globset_rule() -> String {
    "only am-glob may depend on globset, the one glob matcher's engine; match globs through am_glob::Glob (AD-4, Conventions: Globs)".into()
}

/// The rule for an edge into am-glob from a crate that is not one of its
/// users.
fn glob_user_rule() -> String {
    "only am-config, am-engine, am-tools and am-connectors may depend on am-glob (AD-4)".into()
}

pub fn run(metadata: Option<&Path>, root: &Path) -> Result<String, Failure> {
    let json = match metadata {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| Failure::Setup(format!("cannot read {}: {e}", path.display())))?,
        None => cargo_metadata(root)?,
    };
    let doc: Value = serde_json::from_str(&json)
        .map_err(|e| Failure::Setup(format!("cargo metadata is not JSON: {e}")))?;
    let calls = call_site_findings(root);
    match check(&doc) {
        Ok(summary) if calls.is_empty() => Ok(format!(
            "{summary}; only am-engine builds a TrustedInput, and Instruction::authored is called only at its listed sites (AD-10); only the two command handlers name the Inbox resolve and HumanActor outside am-domain and am-store (AD-15); only ToolGate calls the row-writer port and builds an engine result, only the engine's port implementation names the store's row command, and outside am-store only the engine's row_inputs module reads a row, through one row_snapshot call in read_rows, for the engine's driver's inputs_of alone (AD-10, AD-21, AD-43)"
        )),
        Ok(_) => Err(Failure::Findings(calls)),
        Err(Failure::Findings(mut findings)) => {
            findings.extend(calls);
            Err(Failure::Findings(findings))
        }
        Err(e) => Err(e),
    }
}

fn cargo_metadata(root: &Path) -> Result<String, Failure> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| Failure::Setup(format!("cannot run cargo metadata: {e}")))?;
    if !out.status.success() {
        return Err(Failure::Setup(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    String::from_utf8(out.stdout)
        .map_err(|e| Failure::Setup(format!("cargo metadata output is not UTF-8: {e}")))
}

fn check(doc: &Value) -> Result<String, Failure> {
    let members: BTreeSet<&str> = doc["workspace_members"]
        .as_array()
        .ok_or_else(|| Failure::Setup("metadata has no workspace_members".into()))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let packages: Vec<&Value> = doc["packages"]
        .as_array()
        .ok_or_else(|| Failure::Setup("metadata has no packages".into()))?
        .iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(id)))
        .collect();
    let names: BTreeSet<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();

    let mut findings = Vec::new();
    for pkg in &packages {
        let name = pkg["name"].as_str().unwrap_or("?");
        // Story 3.6 (row 43): only am-store opens the database, whatever
        // layer a crate is in; a dev-dependency is test scaffolding.
        let sqlite = pkg["dependencies"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .any(|d| d["kind"].as_str() != Some("dev") && d["name"].as_str() == Some(SQLITE));
        if sqlite && name != SQLITE_HOME {
            findings.push(format!(
                "{name} -> {SQLITE}: only am-store may depend on rusqlite, the one SQLite library, so no other crate opens the database or reads a row (AD-4, AD-21; story 3.6, owner condition b)"
            ));
        }
        let Some(layer) = layer(name) else {
            findings.push(format!(
                "{name}: not in the AD-4 matrix; classify it in xtask/src/deps.rs before it lands"
            ));
            continue;
        };
        let deps = pkg["dependencies"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let Some((list, rule)) = allowed(layer, name) else {
            // Composition roots may use anything but am-glob, which only
            // its four users may, and globset, which only am-glob may
            // (exempt crates are test scaffolding).
            if matches!(layer, Layer::Root) {
                for dep in deps.iter().filter(|d| d["kind"].as_str() != Some("dev")) {
                    match dep["name"].as_str() {
                        Some(GLOB) => {
                            findings.push(format!("{name} -> {GLOB}: {}", glob_user_rule()))
                        }
                        Some(GLOBSET) => {
                            findings.push(format!("{name} -> {GLOBSET}: {}", globset_rule()))
                        }
                        _ => {}
                    }
                }
            }
            continue;
        };
        for dep in deps {
            // Normal (null) and build edges count; dev-dependencies do not.
            if dep["kind"].as_str() == Some("dev") {
                continue;
            }
            let dep_name = dep["name"].as_str().unwrap_or("?");
            if dep_name == SQLITE {
                continue; // reported above
            }
            let internal = names.contains(dep_name);
            let ok = if internal {
                list.contains(&dep_name)
            } else {
                match layer {
                    Layer::Domain => DOMAIN_EXTERNALS.contains(&dep_name),
                    Layer::Glob => GLOB_EXTERNALS.contains(&dep_name),
                    _ => true,
                }
            };
            if dep_name == GLOBSET && !matches!(layer, Layer::Glob) {
                findings.push(format!("{name} -> {dep_name}: {}", globset_rule()));
                continue;
            }
            if !ok {
                let rule = if dep_name == GLOB && !matches!(layer, Layer::Glob) {
                    glob_user_rule()
                } else {
                    rule.clone()
                };
                findings.push(format!("{name} -> {dep_name}: {rule}"));
            }
        }
    }
    if findings.is_empty() {
        Ok(format!(
            "{} workspace crates follow the AD-4 matrix",
            packages.len()
        ))
    } else {
        Err(Failure::Findings(findings))
    }
}
