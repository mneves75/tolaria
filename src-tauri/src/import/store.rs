//! Read notes from an Apple Notes `NoteStore.sqlite` store.
//!
//! Two hazards drive the design (see ADR-0139): the Notes app holds the
//! database open in WAL mode, so recent notes can live only in the `-wal`
//! sidecar; and the schema is reverse-engineered. [`copy_database`] copies the
//! database plus its sidecars, [`open_checkpointed`] opens the *copy* read-write
//! so the WAL is merged, and [`enumerate_notes`] reads the note rows.
//!
//! The schema (table/column names and the note-to-body join) is derived from
//! the threeplanetssoftware / Obsidian reference implementations and validated
//! against a real macOS database (2,274 notes, 99.9% body decode). That database
//! used `ZCREATIONDATE3` / `ZMODIFICATIONDATE1` rather than the older
//! `ZCREATIONDATE` / `ZMODIFIEDDATE1`, so the date columns are resolved at
//! runtime ([`build_enumerate_sql`]); other macOS versions should be checked the
//! same way.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::import::body;

const NOTE_STORE_FILENAME: &str = "NoteStore.sqlite";
const WAL_SIDECARS: [&str; 2] = ["-wal", "-shm"];

/// A single note as read from the store, before conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct RawNote {
    /// `ZIDENTIFIER` — the stable per-note UUID.
    pub source_id: String,
    /// `ZTITLE1` — the title (may be empty).
    pub title: String,
    /// `ZCREATIONDATE` — Core Data time (seconds since 2001-01-01).
    pub created: Option<f64>,
    /// `ZMODIFIEDDATE1` — Core Data time.
    pub modified: Option<f64>,
    /// `ZISPASSWORDPROTECTED` — locked notes are skipped+reported, not decoded.
    pub password_protected: bool,
    /// `ZICNOTEDATA.ZDATA` — the gzipped protobuf note body.
    pub body_gzip: Vec<u8>,
}

impl RawNote {
    /// Convert this note's body to markdown.
    pub fn to_markdown(&self) -> Result<String, String> {
        body::note_body_to_markdown(&self.body_gzip)
    }
}

/// Copy `NoteStore.sqlite` and its WAL sidecars from `notes_dir` into
/// `work_dir`, returning the path to the copied database.
///
/// Copying the `-wal` and `-shm` sidecars is load-bearing: the most recently
/// edited notes can live only in the write-ahead log, so a copy of the main
/// file alone would silently miss them.
pub fn copy_database(notes_dir: &Path, work_dir: &Path) -> Result<PathBuf, String> {
    let dst = work_dir.join(NOTE_STORE_FILENAME);
    copy_file(&notes_dir.join(NOTE_STORE_FILENAME), &dst)?;
    for suffix in WAL_SIDECARS {
        copy_optional_sidecar(notes_dir, work_dir, suffix)?;
    }
    Ok(dst)
}

fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|err| format!("failed to copy {}: {err}", src.display()))
}

fn copy_optional_sidecar(notes_dir: &Path, work_dir: &Path, suffix: &str) -> Result<(), String> {
    let name = format!("{NOTE_STORE_FILENAME}{suffix}");
    let src = notes_dir.join(&name);
    if src.exists() {
        copy_file(&src, &work_dir.join(&name))?;
    }
    Ok(())
}

/// Open a copied database read-write and checkpoint its WAL so the latest notes
/// are merged before reading.
///
/// This must be the throwaway copy, never the user's live database. A read-only
/// connection cannot replay the WAL, which is why this opens read-write.
pub fn open_checkpointed(db_copy: &Path) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(db_copy, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|err| format!("failed to open {}: {err}", db_copy.display()))?;
    // Best-effort: merge the WAL into the main file. Reading already sees WAL
    // data on a read-write connection; this just makes it explicit.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(conn)
}

// The creation/modification date columns are version-volatile: real databases
// have been seen using ZCREATIONDATE3 + ZMODIFICATIONDATE1 where older
// references used ZCREATIONDATE + ZMODIFIEDDATE1, and a missing column is a hard
// SQL error. So the date columns are chosen at runtime from the first candidate
// that actually exists, most-recent-naming first.
const CREATED_CANDIDATES: [&str; 4] = [
    "ZCREATIONDATE3",
    "ZCREATIONDATE2",
    "ZCREATIONDATE1",
    "ZCREATIONDATE",
];
const MODIFIED_CANDIDATES: [&str; 4] = [
    "ZMODIFICATIONDATE1",
    "ZMODIFICATIONDATE",
    "ZMODIFIEDDATE1",
    "ZMODIFIEDDATE",
];

/// Read every note that has a body from the store.
pub fn enumerate_notes(conn: &Connection) -> Result<Vec<RawNote>, String> {
    let sql = build_enumerate_sql(conn)?;
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| format!("failed to prepare note query: {err}"))?;
    let rows = stmt
        .query_map([], row_to_note)
        .map_err(|err| format!("failed to query notes: {err}"))?;
    let mut notes = Vec::new();
    for row in rows {
        notes.push(row.map_err(|err| format!("failed to read note row: {err}"))?);
    }
    Ok(notes)
}

/// Build the enumeration query, resolving the volatile date columns against the
/// columns this database actually has.
fn build_enumerate_sql(conn: &Connection) -> Result<String, String> {
    let columns = table_columns(conn, "ZICCLOUDSYNCINGOBJECT")?;
    let created = date_expr(&columns, &CREATED_CANDIDATES);
    let modified = date_expr(&columns, &MODIFIED_CANDIDATES);
    Ok(format!(
        "SELECT o.ZIDENTIFIER, o.ZTITLE1, {created}, {modified}, o.ZISPASSWORDPROTECTED, d.ZDATA \
         FROM ZICCLOUDSYNCINGOBJECT o \
         JOIN ZICNOTEDATA d ON d.ZNOTE = o.Z_PK \
         WHERE d.ZDATA IS NOT NULL \
         ORDER BY o.Z_PK"
    ))
}

/// The `o.<column>` expression for the first candidate that exists, or `NULL`
/// when none do (so the note still imports, just without that date).
fn date_expr(columns: &std::collections::HashSet<String>, candidates: &[&str]) -> String {
    candidates
        .iter()
        .find(|name| columns.contains(**name))
        .map(|name| format!("o.{name}"))
        .unwrap_or_else(|| "NULL".to_string())
}

fn table_columns(
    conn: &Connection,
    table: &str,
) -> Result<std::collections::HashSet<String>, String> {
    // PRAGMA does not accept a bound parameter for the table name; the value is a
    // hardcoded constant, never user input.
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info('{table}')"))
        .map_err(|err| format!("failed to read schema for {table}: {err}"))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| format!("failed to list columns for {table}: {err}"))?;
    let mut set = std::collections::HashSet::new();
    for name in names {
        set.insert(name.map_err(|err| format!("failed to read column name: {err}"))?);
    }
    Ok(set)
}

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<RawNote> {
    Ok(RawNote {
        source_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
        title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        created: row.get::<_, Option<f64>>(2)?,
        modified: row.get::<_, Option<f64>>(3)?,
        password_protected: row.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,
        body_gzip: row.get::<_, Option<Vec<u8>>>(5)?.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::{copy_database, enumerate_notes, open_checkpointed};
    use crate::import::body::encode_plain_note_gzip;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn create_notes_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE ZICCLOUDSYNCINGOBJECT (
                 Z_PK INTEGER PRIMARY KEY,
                 ZIDENTIFIER TEXT,
                 ZTITLE1 TEXT,
                 ZCREATIONDATE REAL,
                 ZMODIFIEDDATE1 REAL,
                 ZISPASSWORDPROTECTED INTEGER,
                 ZNOTE INTEGER
             );
             CREATE TABLE ZICNOTEDATA (
                 Z_PK INTEGER PRIMARY KEY,
                 ZNOTE INTEGER,
                 ZDATA BLOB
             );",
        )
        .expect("create schema");
    }

    #[test]
    fn wal_resident_note_survives_copy_and_open() {
        let src_dir = TempDir::new().unwrap();
        let db_path = src_dir.path().join("NoteStore.sqlite");

        // Writer holds the database open in WAL mode with autocheckpoint off, so
        // the row lives only in the -wal sidecar, never the main file.
        let writer = Connection::open(&db_path).unwrap();
        writer.pragma_update(None, "journal_mode", "WAL").unwrap();
        writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        writer
            .execute("CREATE TABLE t (id INTEGER, v TEXT)", [])
            .unwrap();
        writer
            .execute("INSERT INTO t VALUES (1, 'wal-only')", [])
            .unwrap();

        let work = TempDir::new().unwrap();
        let copy = copy_database(src_dir.path(), work.path()).unwrap();
        let conn = open_checkpointed(&copy).unwrap();
        let value: String = conn
            .query_row("SELECT v FROM t WHERE id = 1", [], |r| r.get(0))
            .expect("WAL-resident row must be visible after read-write open");
        assert_eq!(value, "wal-only");

        drop(writer);
    }

    #[test]
    fn enumerates_and_decodes_a_note() {
        let src_dir = TempDir::new().unwrap();
        let db_path = src_dir.path().join("NoteStore.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        create_notes_schema(&conn);

        let blob = encode_plain_note_gzip("hello from notes");
        conn.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT
                (Z_PK, ZIDENTIFIER, ZTITLE1, ZCREATIONDATE, ZMODIFIEDDATE1, ZISPASSWORDPROTECTED)
             VALUES (1, 'uuid-1', 'My Note', 700000000.0, 700000100.0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ZICNOTEDATA (Z_PK, ZNOTE, ZDATA) VALUES (1, 1, ?1)",
            [&blob],
        )
        .unwrap();
        drop(conn);

        let work = TempDir::new().unwrap();
        let copy = copy_database(src_dir.path(), work.path()).unwrap();
        let conn = open_checkpointed(&copy).unwrap();
        let notes = enumerate_notes(&conn).unwrap();

        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.source_id, "uuid-1");
        assert_eq!(note.title, "My Note");
        assert_eq!(note.created, Some(700_000_000.0));
        assert!(!note.password_protected);
        assert_eq!(note.to_markdown().unwrap(), "hello from notes");
    }

    #[test]
    fn skips_rows_without_a_body() {
        let src_dir = TempDir::new().unwrap();
        let db_path = src_dir.path().join("NoteStore.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        create_notes_schema(&conn);
        conn.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT (Z_PK, ZIDENTIFIER, ZTITLE1) VALUES (1, 'uuid-1', 'No Body')",
            [],
        )
        .unwrap();
        drop(conn);

        let work = TempDir::new().unwrap();
        let copy = copy_database(src_dir.path(), work.path()).unwrap();
        let conn = open_checkpointed(&copy).unwrap();
        assert!(enumerate_notes(&conn).unwrap().is_empty());
    }

    #[test]
    fn flags_password_protected_notes() {
        let src_dir = TempDir::new().unwrap();
        let db_path = src_dir.path().join("NoteStore.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        create_notes_schema(&conn);
        let blob = encode_plain_note_gzip("locked body still present");
        conn.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT
                (Z_PK, ZIDENTIFIER, ZTITLE1, ZISPASSWORDPROTECTED)
             VALUES (1, 'uuid-1', 'Secret', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ZICNOTEDATA (Z_PK, ZNOTE, ZDATA) VALUES (1, 1, ?1)",
            [&blob],
        )
        .unwrap();
        drop(conn);

        let work = TempDir::new().unwrap();
        let copy = copy_database(src_dir.path(), work.path()).unwrap();
        let conn = open_checkpointed(&copy).unwrap();
        let notes = enumerate_notes(&conn).unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].password_protected);
    }

    #[test]
    fn missing_database_is_an_error() {
        let src_dir = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let err = copy_database(src_dir.path(), work.path()).unwrap_err();
        assert!(err.contains("failed to copy"), "unexpected error: {err}");
    }

    // Mirrors the column names a current macOS NoteStore.sqlite actually uses
    // (ZCREATIONDATE3 / ZMODIFICATIONDATE1), verified by the real-database spike.
    #[test]
    fn resolves_modern_date_columns() {
        let src_dir = TempDir::new().unwrap();
        let conn = Connection::open(src_dir.path().join("NoteStore.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZICCLOUDSYNCINGOBJECT (
                 Z_PK INTEGER PRIMARY KEY, ZIDENTIFIER TEXT, ZTITLE1 TEXT,
                 ZCREATIONDATE3 REAL, ZMODIFICATIONDATE1 REAL,
                 ZISPASSWORDPROTECTED INTEGER, ZNOTE INTEGER);
             CREATE TABLE ZICNOTEDATA (Z_PK INTEGER PRIMARY KEY, ZNOTE INTEGER, ZDATA BLOB);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT
                (Z_PK, ZIDENTIFIER, ZTITLE1, ZCREATIONDATE3, ZMODIFICATIONDATE1)
             VALUES (1, 'uuid-1', 'Note', 615178251.0, 791878249.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ZICNOTEDATA (Z_PK, ZNOTE, ZDATA) VALUES (1, 1, ?1)",
            [&encode_plain_note_gzip("hi")],
        )
        .unwrap();
        drop(conn);

        let work = TempDir::new().unwrap();
        let copy = copy_database(src_dir.path(), work.path()).unwrap();
        let conn = open_checkpointed(&copy).unwrap();
        let notes = enumerate_notes(&conn).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].created, Some(615_178_251.0));
        assert_eq!(notes[0].modified, Some(791_878_249.0));
    }

    #[test]
    fn enumerates_when_no_date_columns_exist() {
        let src_dir = TempDir::new().unwrap();
        let conn = Connection::open(src_dir.path().join("NoteStore.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZICCLOUDSYNCINGOBJECT (
                 Z_PK INTEGER PRIMARY KEY, ZIDENTIFIER TEXT, ZTITLE1 TEXT,
                 ZISPASSWORDPROTECTED INTEGER, ZNOTE INTEGER);
             CREATE TABLE ZICNOTEDATA (Z_PK INTEGER PRIMARY KEY, ZNOTE INTEGER, ZDATA BLOB);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT (Z_PK, ZIDENTIFIER, ZTITLE1) VALUES (1, 'uuid-1', 'Note')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ZICNOTEDATA (Z_PK, ZNOTE, ZDATA) VALUES (1, 1, ?1)",
            [&encode_plain_note_gzip("hi")],
        )
        .unwrap();
        drop(conn);

        let work = TempDir::new().unwrap();
        let copy = copy_database(src_dir.path(), work.path()).unwrap();
        let conn = open_checkpointed(&copy).unwrap();
        let notes = enumerate_notes(&conn).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].created, None);
        assert_eq!(notes[0].modified, None);
        assert_eq!(notes[0].to_markdown().unwrap(), "hi");
    }
}
