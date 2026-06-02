//! Assemble a decoded [`RawNote`] into something the import can write to disk.
//!
//! This is the integration seam between reading ([`store`]) and writing: it
//! produces the destination filename stem and the markdown body, and converts
//! Apple's Core Data timestamps to Unix time. Imported notes are written like
//! notes created by hand in Tolaria: the title is the `# H1` the converter
//! already emits plus the slug filename, with no extra frontmatter. The
//! source-id ↔ path mapping lives in the import manifest, not in frontmatter.
//!
//! Dates are returned (not written) so the orchestration step can apply them as
//! the file's modified time, matching how Tolaria sources note dates from the
//! filesystem rather than frontmatter.

use std::collections::HashSet;

use crate::import::materialize;
use crate::import::store::RawNote;

/// Seconds between the Unix epoch (1970-01-01) and the Core Data epoch
/// (2001-01-01). Apple `ZCREATIONDATE` / `ZMODIFIEDDATE1` count from 2001.
const CORETIME_EPOCH_OFFSET: f64 = 978_307_200.0;

/// A note ready to be written to the vault.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembledNote {
    /// Source-store UUID, carried through for the import manifest.
    pub source_id: String,
    /// Unique filename stem (no extension) within this import run.
    pub stem: String,
    /// The markdown file body.
    pub markdown: String,
    /// Creation time as Unix seconds, if known.
    pub created_unix: Option<f64>,
    /// Modification time as Unix seconds, if known.
    pub modified_unix: Option<f64>,
}

/// Assemble one note, reserving its filename stem in `taken`.
///
/// `taken` accumulates stems already used in this run so two notes never map to
/// the same file. Errors only if the note body fails to decode.
pub fn assemble_note(raw: &RawNote, taken: &mut HashSet<String>) -> Result<AssembledNote, String> {
    let markdown = raw.to_markdown()?;
    let stem = materialize::unique_stem(&raw.title, taken);
    Ok(AssembledNote {
        source_id: raw.source_id.clone(),
        stem,
        markdown,
        created_unix: raw.created.map(coretime_to_unix),
        modified_unix: raw.modified.map(coretime_to_unix),
    })
}

fn coretime_to_unix(coretime: f64) -> f64 {
    coretime + CORETIME_EPOCH_OFFSET
}

#[cfg(test)]
mod tests {
    use super::{assemble_note, AssembledNote, CORETIME_EPOCH_OFFSET};
    use crate::import::body::encode_plain_note_gzip;
    use crate::import::store::RawNote;
    use std::collections::HashSet;

    fn raw(source_id: &str, title: &str, body: &str) -> RawNote {
        RawNote {
            source_id: source_id.to_string(),
            title: title.to_string(),
            created: Some(700_000_000.0),
            modified: Some(700_000_100.0),
            password_protected: false,
            body_gzip: encode_plain_note_gzip(body),
        }
    }

    #[test]
    fn assembles_stem_markdown_and_unix_dates() {
        let mut taken = HashSet::new();
        let note = assemble_note(&raw("uuid-1", "Quarterly Plan: Q3", "the body"), &mut taken).unwrap();
        assert_eq!(
            note,
            AssembledNote {
                source_id: "uuid-1".to_string(),
                stem: "quarterly-plan-q3".to_string(),
                markdown: "the body".to_string(),
                created_unix: Some(700_000_000.0 + CORETIME_EPOCH_OFFSET),
                modified_unix: Some(700_000_100.0 + CORETIME_EPOCH_OFFSET),
            }
        );
    }

    #[test]
    fn two_notes_with_same_title_get_distinct_stems() {
        let mut taken = HashSet::new();
        let first = assemble_note(&raw("a", "Groceries", "x"), &mut taken).unwrap();
        let second = assemble_note(&raw("b", "Groceries", "y"), &mut taken).unwrap();
        assert_eq!(first.stem, "groceries");
        assert_eq!(second.stem, "groceries-2");
    }

    #[test]
    fn empty_title_assembles_untitled() {
        let mut taken = HashSet::new();
        let note = assemble_note(&raw("a", "", "orphan body"), &mut taken).unwrap();
        assert_eq!(note.stem, "untitled");
        assert_eq!(note.markdown, "orphan body");
    }

    #[test]
    fn missing_dates_stay_none() {
        let mut taken = HashSet::new();
        let mut input = raw("a", "Note", "body");
        input.created = None;
        input.modified = None;
        let note = assemble_note(&input, &mut taken).unwrap();
        assert_eq!(note.created_unix, None);
        assert_eq!(note.modified_unix, None);
    }

    #[test]
    fn undecodable_body_is_an_error() {
        let mut taken = HashSet::new();
        let mut input = raw("a", "Note", "body");
        input.body_gzip = b"not gzip".to_vec();
        assert!(assemble_note(&input, &mut taken).is_err());
    }
}
