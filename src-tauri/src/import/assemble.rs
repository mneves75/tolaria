//! Assemble a decoded [`RawNote`] into the content the import will write.
//!
//! This is the content seam: it produces the markdown body and converts Apple's
//! Core Data timestamps to Unix time. It deliberately does *not* decide the
//! note's filename or path — that is the orchestration step's job, because a
//! re-import must keep a note at the path the manifest already recorded.
//!
//! Imported notes are shaped like notes created by hand in Tolaria: the title is
//! the `# H1` the converter already emits plus the slug filename, with no extra
//! frontmatter. Dates are returned (not written) so orchestration can apply them
//! as the file's modified time, matching how Tolaria sources note dates from the
//! filesystem rather than frontmatter.

use crate::import::store::RawNote;

/// Seconds between the Unix epoch (1970-01-01) and the Core Data epoch
/// (2001-01-01). Apple `ZCREATIONDATE` / `ZMODIFIEDDATE1` count from 2001.
const CORETIME_EPOCH_OFFSET: f64 = 978_307_200.0;

/// A note's assembled content, ready for the orchestration step to place.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembledNote {
    /// Source-store UUID, carried through for the import manifest.
    pub source_id: String,
    /// The note's title (used to derive a filename when it is first imported).
    pub title: String,
    /// The markdown file body.
    pub markdown: String,
    /// Creation time as Unix seconds, if known.
    pub created_unix: Option<f64>,
    /// Modification time as Unix seconds, if known.
    pub modified_unix: Option<f64>,
}

/// Assemble one note's content. Errors only if the note body fails to decode.
pub fn assemble_note(raw: &RawNote) -> Result<AssembledNote, String> {
    Ok(AssembledNote {
        source_id: raw.source_id.clone(),
        title: raw.title.clone(),
        markdown: raw.to_markdown()?,
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
    fn assembles_markdown_and_unix_dates() {
        let note = assemble_note(&raw("uuid-1", "Quarterly Plan: Q3", "the body")).unwrap();
        assert_eq!(
            note,
            AssembledNote {
                source_id: "uuid-1".to_string(),
                title: "Quarterly Plan: Q3".to_string(),
                markdown: "the body".to_string(),
                created_unix: Some(700_000_000.0 + CORETIME_EPOCH_OFFSET),
                modified_unix: Some(700_000_100.0 + CORETIME_EPOCH_OFFSET),
            }
        );
    }

    #[test]
    fn missing_dates_stay_none() {
        let mut input = raw("a", "Note", "body");
        input.created = None;
        input.modified = None;
        let note = assemble_note(&input).unwrap();
        assert_eq!(note.created_unix, None);
        assert_eq!(note.modified_unix, None);
    }

    #[test]
    fn undecodable_body_is_an_error() {
        let mut input = raw("a", "Note", "body");
        input.body_gzip = b"not gzip".to_vec();
        assert!(assemble_note(&input).is_err());
    }
}
