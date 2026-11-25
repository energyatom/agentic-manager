//! The rows a step reads at its start (epic 3 story 22, "Rows reaching
//! later steps"; AD-10 as amended and restated 2026-11-24, AD-43; owner
//! Q4's reader condition).
//!
//! The one shipped file outside am-store that reads a row's content, and
//! only through the store's snapshot reader, `Reader::row_snapshot` (`cargo
//! xtask check-deps`). For each row a step's plan names, in the order
//! named, it reads the row's current after-image and gives:
//!
//! - one `row` input edge, labelled `row:<table>/<row id>@<version>`, that
//!   names the after-image it read (`from_row`, its `row_history.seq`) and
//!   carries the image's taint, the floor of the edge's (the store raises a
//!   lower one when it journals the edge with the attempt's start);
//! - one fenced [`UntrustedData`] part under the same label, holding the
//!   row's data as stored, with one pass of JSON unescaping of printable
//!   ASCII (review triage round 2, T1): a `\uXXXX` escape of a character
//!   from 0x20 to 0x7E, and `\/`, become the character, so a fence's
//!   closing tag escaped letter by letter reaches the providers' neutraliser
//!   as the literal tag. Everything else, number text included, is kept
//!   exactly as stored; the row itself is never changed.
//!
//! A row's text is untrusted data whatever the row's mark: nothing here
//! turns it into anything else, and the part taints the step that reads
//! it. A row the table does not hold, or a row named twice, is refused in
//! words naming the table and the row id (store ids), never any row's
//! data. The module is the crate's own; only the engine's driver calls
//! [`read_rows`], inside its `fn inputs_of`, and this file calls the
//! snapshot reader once, inside `fn read_rows` (`cargo xtask check-deps`).

use std::collections::BTreeSet;
use std::fmt;

use am_domain::UntrustedData;
use am_domain::rows::RowInput;
use am_store::{Reader, StepInput, StoreError};

/// One row read: the edge to journal with the attempt's start, and the
/// fenced part that carries the row's data.
#[derive(Clone, PartialEq, Eq)]
pub struct RowRead {
    pub input: StepInput,
    pub part: UntrustedData,
}

/// The row's data is never printed (the store slice's review T6): the edge
/// and the part's label only.
impl fmt::Debug for RowRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowRead")
            .field("input", &self.input)
            .field("part_label", &self.part.label())
            .finish()
    }
}

/// Reads each of `rows` at an attempt's start, in the order named, from
/// the committed after-images: one [`RowRead`] each. `Ok(Err(words))` when
/// a row is named twice (review triage round 1, R4), or a table does not
/// hold the row named (an unknown id, or a row of another table): the
/// words name the table asked and the row id, never any row's data, and
/// the attempt must not start. Reading writes nothing.
pub fn read_rows(
    reader: &Reader,
    rows: &[RowInput],
) -> Result<Result<Vec<RowRead>, String>, StoreError> {
    let mut named = BTreeSet::new();
    for row in rows {
        if !named.insert((row.table_id.as_str(), row.row_id)) {
            return Ok(Err(format!(
                "it reads row {} of table `{}` twice, and a step reads each row once",
                row.row_id, row.table_id
            )));
        }
    }
    let mut reads = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row.row_id.to_string();
        let Some(image) = reader.row_snapshot(&row.table_id, &id)? else {
            return Ok(Err(format!(
                "it reads row {id} of table `{}`, which the table does not hold",
                row.table_id
            )));
        };
        let label = format!("row:{}/{id}@{}", row.table_id, image.version);
        reads.push(RowRead {
            input: StepInput {
                kind: "row".into(),
                from_step_run: None,
                from_item: None,
                from_row: Some(image.seq),
                label: label.clone(),
                taint: image.taint,
            },
            part: UntrustedData::text(label, printable_unescaped(&image.data)),
        });
    }
    Ok(Ok(reads))
}

/// `text` with one pass of JSON unescaping (review triage round 2, T1;
/// the manager's D3: one pass, never to a fixpoint): `\uXXXX` (four hex
/// digits, either case) standing for a printable ASCII character (0x20 to
/// 0x7E) and `\/` become that character. Every other escape (`\\`, `\"`,
/// `\n`, a `\uXXXX` of a control or non-ASCII character) is kept as
/// written, two characters at a time, so an escaped backslash never starts
/// another escape; all other text is kept exactly.
fn printable_unescaped(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' || i + 1 == chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        match chars[i + 1] {
            '/' => {
                out.push('/');
                i += 2;
            }
            'u' if i + 6 <= chars.len() => {
                let printable = chars[i + 2..i + 6]
                    .iter()
                    .try_fold(0u32, |n, c| c.to_digit(16).map(|d| n * 16 + d))
                    .and_then(char::from_u32)
                    .filter(|c| (' '..='~').contains(c));
                match printable {
                    Some(c) => out.push(c),
                    None => out.extend(&chars[i..i + 6]),
                }
                i += 6;
            }
            other => {
                out.push('\\');
                out.push(other);
                i += 2;
            }
        }
    }
    out
}

/// The read site's tests (story 3.22; AD-10 as amended and restated
/// 2026-11-24, AD-43; plan matrix rows 38 and 47). Review triage round 2,
/// T2: the module is `pub(crate)`, so they moved here from
/// `tests/row_inputs.rs`. They name none of the store's other row readers,
/// its row command or its row records (`cargo xtask check-deps` reads this
/// file whole): the rows are made, and the store read back, by raw SQL on
/// the store's own database (rusqlite is a dev-dependency). On a real
/// store in a temporary folder; no network, no model.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use am_domain::rows::RowInput;
    use am_domain::{Taint, UntrustedData};
    use am_store::{StepInput, Store};
    use rusqlite::{Connection, params};
    use serde_json::json;
    use uuid::Uuid;

    use super::{RowRead, read_rows};

    const TABLE: &str = "entries";
    const OTHER_TABLE: &str = "others";

    /// In the tainted row's data and the clean row's.
    const DIRTY_CANARY: &str = "canary-engine-row-dirty-7a1";
    const CLEAN_CANARY: &str = "canary-engine-row-clean-7a2";

    /// A person's actor, as `row_history.actor` holds one with no seat
    /// provenance.
    const PERSON: &str = r#"{"kind":"human","person":"owner","hat":"Reviewer"}"#;

    /// A store holding two rows of `entries`, each at version 1: `dirty`
    /// (taint 1) and `fine` (taint 0).
    struct Rig {
        _dir: tempfile::TempDir,
        db: PathBuf,
        store: Store,
        dirty: String,
        fine: String,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let store = Store::open(&db).unwrap();
        let r = Rig {
            _dir: dir,
            db,
            store,
            dirty: Uuid::now_v7().to_string(),
            fine: Uuid::now_v7().to_string(),
        };
        r.put(
            &r.dirty,
            &json!({ "ref": "collect", "title": DIRTY_CANARY }).to_string(),
            1,
        );
        r.put(
            &r.fine,
            &json!({ "ref": "check", "title": CLEAN_CANARY }).to_string(),
            0,
        );
        r
    }

    impl Rig {
        /// Writes `row` of `entries` at version 1, its data text exactly
        /// `data`, by hand: its after-image first, then the row.
        fn put(&self, row: &str, data: &str, taint: i64) {
            let at = "2026-11-25T10:00:00.000Z";
            let c = Connection::open(&self.db).unwrap();
            c.execute(
                "INSERT INTO row_history (table_id, row_id, version, data, taint, actor, op, created_at) VALUES (?1, ?2, 1, ?3, ?4, ?5, 'insert', ?6)",
                params![TABLE, row, data, taint, PERSON, at],
            )
            .unwrap();
            c.execute(
                "INSERT INTO table_rows (table_id, row_id, data, version, taint, created_at, updated_at) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?5)",
                params![TABLE, row, data, taint, at],
            )
            .unwrap();
        }

        /// `row`'s newest after-image, read back by hand: its `seq`,
        /// version, data and taint.
        fn image(&self, row: &str) -> (i64, i64, String, Taint) {
            Connection::open(&self.db)
                .unwrap()
                .query_row(
                    "SELECT seq, version, data, taint FROM row_history WHERE table_id = ?1 AND row_id = ?2 ORDER BY version DESC LIMIT 1",
                    params![TABLE, row],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            Taint::of_flag(r.get::<_, i64>(3)? == 1),
                        ))
                    },
                )
                .unwrap()
        }

        /// Every row and after-image, raw, in rowid order.
        fn dump(&self) -> Vec<String> {
            let c = Connection::open(&self.db).unwrap();
            let mut out = Vec::new();
            for sql in [
                "SELECT table_id || '|' || row_id || '|' || data || '|' || version || '|' || taint FROM table_rows ORDER BY rowid",
                "SELECT seq || '|' || row_id || '|' || data || '|' || version || '|' || taint FROM row_history ORDER BY seq",
            ] {
                let mut st = c.prepare(sql).unwrap();
                let rows: Vec<String> = st
                    .query_map([], |r| r.get(0))
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap();
                out.extend(rows);
            }
            out
        }
    }

    fn input(table: &str, row: &str) -> RowInput {
        RowInput {
            table_id: table.into(),
            row_id: Uuid::parse_str(row).unwrap(),
        }
    }

    /// Rows 38 and 47: each row input becomes one `row` edge and one fenced
    /// part, in the order named. The edge names the current after-image
    /// (`from_row` = its `seq`) under `row:entries/<id>@1` and carries its
    /// taint: 1 for the tainted row, 0 for the clean one (lineage tells
    /// them apart). The part is `UntrustedData` under the same label
    /// holding the row's data as stored, the clean row's too (no conversion
    /// to a trusted input exists). Reading writes nothing.
    #[test]
    fn a_row_input_becomes_one_fenced_part_and_one_edge() {
        let r = rig();
        let reader = r.store.reader().unwrap();
        let before = r.dump();

        let reads: Vec<RowRead> =
            read_rows(&reader, &[input(TABLE, &r.dirty), input(TABLE, &r.fine)])
                .unwrap()
                .expect("both rows are held");

        let label = |row: &str| format!("row:{TABLE}/{row}@1");
        assert_eq!(reads.len(), 2, "one read per row input");
        for (read, row) in reads.iter().zip([&r.dirty, &r.fine]) {
            let (seq, version, data, taint) = r.image(row);
            assert_eq!(version, 1);
            assert_eq!(
                read.input,
                StepInput {
                    kind: "row".into(),
                    from_step_run: None,
                    from_item: None,
                    from_row: Some(seq),
                    label: label(row),
                    taint,
                },
                "the edge names the after-image it read (AD-43)"
            );
            assert_eq!(
                read.part,
                UntrustedData::text(label(row), data),
                "the row's data, fenced under the edge's label"
            );
        }
        assert_eq!(
            reads[0].input.taint,
            Taint::Tainted,
            "the tainted row's mark"
        );
        assert_eq!(reads[1].input.taint, Taint::Clean, "the clean row's mark");
        assert_eq!(r.dump(), before, "reading writes nothing");
    }

    /// Row 47 (Design Notes: a row missing at the start fails the start in
    /// words): an id `entries` does not hold, and a row of `entries` asked
    /// under `others`, are each refused in words that name the table asked
    /// and the row id (store ids), never any row's data.
    #[test]
    fn a_row_the_table_does_not_hold_is_refused_in_store_words() {
        let r = rig();
        let reader = r.store.reader().unwrap();
        let unknown = Uuid::now_v7().to_string();

        for (what, table, row) in [
            ("an unknown id", TABLE, unknown.as_str()),
            ("a row of another table", OTHER_TABLE, r.dirty.as_str()),
        ] {
            let words = read_rows(&reader, &[input(TABLE, &r.fine), input(table, row)])
                .unwrap()
                .expect_err(what);
            assert!(words.contains(table), "{what}: names the table: {words}");
            assert!(words.contains(row), "{what}: names the row id: {words}");
            for canary in [DIRTY_CANARY, CLEAN_CANARY] {
                assert!(!words.contains(canary), "{what}: no row data: {words}");
            }
        }
    }

    /// Review triage round 1, R4 (C3): a row named twice is refused in
    /// words (`Ok(Err(words))`), whichever rows stand between: the words
    /// name the table and the row id, never any row's data.
    #[test]
    fn a_row_named_twice_is_refused_in_store_words() {
        let r = rig();
        let reader = r.store.reader().unwrap();

        let words = read_rows(
            &reader,
            &[
                input(TABLE, &r.fine),
                input(TABLE, &r.dirty),
                input(TABLE, &r.fine),
            ],
        )
        .unwrap()
        .expect_err("the same row twice");

        assert!(words.contains(TABLE), "names the table: {words}");
        assert!(words.contains(&r.fine), "names the row id: {words}");
        for canary in [DIRTY_CANARY, CLEAN_CANARY] {
            assert!(!words.contains(canary), "no row data: {words}");
        }
    }

    /// Review triage round 2, T1: the part's text is the row's data
    /// with one pass of JSON unescaping: a `\uXXXX` escape of a printable
    /// ASCII character (0x20 to 0x7E, either hex case) and `\/` become the
    /// character. Every other escape (an escaped backslash, which also
    /// keeps the `u0041` after it as text; an escaped quote; a `\uXXXX` of
    /// a control or non-ASCII character) and all other text, number text
    /// included, are kept exactly as stored. The edge and its label are as
    /// for any row; the row itself is not changed.
    #[test]
    fn printable_escapes_are_decoded_once_and_the_rest_is_kept() {
        let r = rig();
        let row = Uuid::now_v7().to_string();
        let stored = r#"{"title":"a\u0041b\u004ac\u004Ad<\/e\u00e9f\u001fg\\u0041h\"i","n":12345678901234567890123,"x":1e2}"#;
        r.put(&row, stored, 0);
        let before = r.dump();

        let reads = read_rows(&r.store.reader().unwrap(), &[input(TABLE, &row)])
            .unwrap()
            .expect("the row is held");

        let label = format!("row:{TABLE}/{row}@1");
        assert_eq!(
            reads[0].part,
            UntrustedData::text(
                label.clone(),
                r#"{"title":"aAbJcJd</e\u00e9f\u001fg\\u0041h\"i","n":12345678901234567890123,"x":1e2}"#
            )
        );
        assert_eq!(reads[0].input.label, label);
        assert_eq!(r.image(&row).2, stored, "the row keeps its text");
        assert_eq!(r.dump(), before, "reading writes nothing");
    }
}
