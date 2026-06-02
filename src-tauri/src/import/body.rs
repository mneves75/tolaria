//! Decode an Apple Notes note body (`ZICNOTEDATA.ZDATA`) into markdown.
//!
//! The blob is gzip-framed protobuf. This module gunzips it, decodes the
//! `NoteStoreProto` with the vendored schema, maps the protobuf formatting runs
//! onto the [`convert`] domain types, and renders markdown. Keeping the mapping
//! here means [`convert`] stays free of any protobuf dependency.

use std::cmp::Ordering;
use std::io::Read;

use flate2::read::GzDecoder;
use prost::Message;

use crate::import::convert::{self, AttributeRun, Baseline, BlockStyle, InlineStyle};

mod proto {
    include!(concat!(env!("OUT_DIR"), "/ciofecaforensics.rs"));
}

// Apple `style_type` values (see ParagraphStyle).
const STYLE_TITLE: i32 = 0;
const STYLE_HEADING: i32 = 1;
const STYLE_SUBHEADING: i32 = 2;
const STYLE_MONOSPACED: i32 = 4;
const STYLE_BULLETED_LIST: i32 = 100;
const STYLE_DASHED_LIST: i32 = 101;
const STYLE_NUMBERED_LIST: i32 = 102;
const STYLE_CHECKLIST: i32 = 103;

// Apple `font_weight` values.
const WEIGHT_BOLD: i32 = 1;
const WEIGHT_ITALIC: i32 = 2;
const WEIGHT_BOLD_ITALIC: i32 = 3;

/// Decode a gzipped note-body blob into markdown.
pub fn note_body_to_markdown(gzipped_blob: &[u8]) -> Result<String, String> {
    let bytes = gunzip(gzipped_blob)?;
    let note = decode_note(&bytes)?;
    let runs: Vec<AttributeRun> = note.attribute_run.iter().map(map_run).collect();
    Ok(convert::note_to_markdown(&note.note_text, &runs))
}

fn gunzip(blob: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = GzDecoder::new(blob);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|err| format!("note body gunzip failed: {err}"))?;
    Ok(out)
}

fn decode_note(bytes: &[u8]) -> Result<proto::Note, String> {
    let store = proto::NoteStoreProto::decode(bytes)
        .map_err(|err| format!("note body protobuf decode failed: {err}"))?;
    Ok(store.document.note)
}

fn map_run(run: &proto::AttributeRun) -> AttributeRun {
    let style = run.paragraph_style.as_ref();
    AttributeRun {
        length: run.length.max(0) as usize,
        block: map_block(style),
        inline: map_inline(run),
        blockquote: style.and_then(|s| s.block_quote).unwrap_or(0) != 0,
        indent: style.and_then(|s| s.indent_amount).unwrap_or(0).max(0) as u32,
    }
}

fn map_block(style: Option<&proto::ParagraphStyle>) -> BlockStyle {
    let Some(style) = style else {
        return BlockStyle::Body;
    };
    match style.style_type.unwrap_or(-1) {
        STYLE_TITLE => BlockStyle::Title,
        STYLE_HEADING => BlockStyle::Heading,
        STYLE_SUBHEADING => BlockStyle::Subheading,
        STYLE_MONOSPACED => BlockStyle::Monospaced,
        STYLE_BULLETED_LIST => BlockStyle::BulletedList,
        STYLE_DASHED_LIST => BlockStyle::DashedList,
        STYLE_NUMBERED_LIST => BlockStyle::NumberedList,
        STYLE_CHECKLIST => BlockStyle::Checklist {
            done: style.checklist.as_ref().is_some_and(|c| c.done != 0),
        },
        _ => BlockStyle::Body,
    }
}

fn map_inline(run: &proto::AttributeRun) -> InlineStyle {
    let weight = run.font_weight.unwrap_or(0);
    InlineStyle {
        bold: weight == WEIGHT_BOLD || weight == WEIGHT_BOLD_ITALIC,
        italic: weight == WEIGHT_ITALIC || weight == WEIGHT_BOLD_ITALIC,
        strikethrough: run.strikethrough.unwrap_or(0) != 0,
        underline: run.underlined.unwrap_or(0) != 0,
        baseline: map_baseline(run.superscript.unwrap_or(0)),
        link: run.link.clone().filter(|link| !link.is_empty()),
    }
}

fn map_baseline(superscript: i32) -> Baseline {
    match superscript.cmp(&0) {
        Ordering::Greater => Baseline::Super,
        Ordering::Less => Baseline::Sub,
        Ordering::Equal => Baseline::Normal,
    }
}

/// Build a gzipped note-body blob holding `text` as a single body paragraph.
/// Test-only helper shared with the `store` module's tests so they can insert a
/// real `ZICNOTEDATA.ZDATA` value.
#[cfg(test)]
pub(crate) fn encode_plain_note_gzip(text: &str) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let store = proto::NoteStoreProto {
        document: proto::Document {
            version: 1,
            note: proto::Note {
                note_text: text.to_string(),
                attribute_run: vec![proto::AttributeRun {
                    length: text.encode_utf16().count() as i32,
                    ..Default::default()
                }],
            },
        },
    };
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&store.encode_to_vec())
        .expect("gzip write");
    encoder.finish().expect("gzip finish")
}

#[cfg(test)]
mod tests {
    use super::{note_body_to_markdown, proto};
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use prost::Message;
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).expect("gzip write");
        encoder.finish().expect("gzip finish")
    }

    fn note_blob(text: &str, runs: Vec<proto::AttributeRun>) -> Vec<u8> {
        let store = proto::NoteStoreProto {
            document: proto::Document {
                version: 1,
                note: proto::Note {
                    note_text: text.to_string(),
                    attribute_run: runs,
                },
            },
        };
        gzip(&store.encode_to_vec())
    }

    fn styled_run(length: i32, style_type: i32) -> proto::AttributeRun {
        proto::AttributeRun {
            length,
            paragraph_style: Some(proto::ParagraphStyle {
                style_type: Some(style_type),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn utf16_len(text: &str) -> i32 {
        text.encode_utf16().count() as i32
    }

    #[test]
    fn decodes_title_and_body() {
        let blob = note_blob(
            "My Note\nbody text",
            vec![
                styled_run(utf16_len("My Note\n"), 0),
                styled_run(utf16_len("body text"), -1),
            ],
        );
        assert_eq!(
            note_body_to_markdown(&blob).unwrap(),
            "# My Note\nbody text"
        );
    }

    #[test]
    fn decodes_bold_run() {
        let mut bold = styled_run(utf16_len("bold"), -1);
        bold.font_weight = Some(1);
        let blob = note_blob("bold", vec![bold]);
        assert_eq!(note_body_to_markdown(&blob).unwrap(), "**bold**");
    }

    #[test]
    fn decodes_heading() {
        let blob = note_blob("Section", vec![styled_run(utf16_len("Section"), 1)]);
        assert_eq!(note_body_to_markdown(&blob).unwrap(), "## Section");
    }

    #[test]
    fn decodes_checklist_done_state() {
        let mut done = styled_run(utf16_len("packed\n"), 103);
        done.paragraph_style.as_mut().unwrap().checklist = Some(proto::Checklist {
            uuid: Vec::new(),
            done: 1,
        });
        let mut todo = styled_run(utf16_len("pack"), 103);
        todo.paragraph_style.as_mut().unwrap().checklist = Some(proto::Checklist {
            uuid: Vec::new(),
            done: 0,
        });
        let blob = note_blob("packed\npack", vec![done, todo]);
        assert_eq!(
            note_body_to_markdown(&blob).unwrap(),
            "- [x] packed\n- [ ] pack"
        );
    }

    #[test]
    fn decodes_external_link() {
        let mut link = styled_run(utf16_len("site"), -1);
        link.link = Some("https://example.com".to_string());
        let blob = note_blob("site", vec![link]);
        assert_eq!(
            note_body_to_markdown(&blob).unwrap(),
            "[site](https://example.com)"
        );
    }

    #[test]
    fn rejects_non_gzip_input() {
        let err = note_body_to_markdown(b"not gzip at all").unwrap_err();
        assert!(err.contains("gunzip"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_gzipped_non_protobuf() {
        let blob = gzip(b"\xff\xff not a valid protobuf \x00");
        let err = note_body_to_markdown(&blob).unwrap_err();
        assert!(err.contains("decode"), "unexpected error: {err}");
    }
}
