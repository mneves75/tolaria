//! Derive on-disk filenames for imported notes.
//!
//! Imported notes must be named exactly like notes created inside Tolaria, so
//! wikilinks resolve and title-sync does not fight the import. Tolaria filenames
//! are slugs, so this module reuses the app's canonical [`title_to_slug`] rather
//! than inventing its own sanitizer, then adds the two import-specific concerns:
//! bounding very long slugs (Apple titles can be whole paragraphs) and
//! de-duplicating collisions deterministically by import order.

use std::collections::HashSet;

use crate::vault::title_to_slug;

/// Maximum slug length in characters. Filesystem components cap at 255 bytes
/// (fewer for multibyte); this leaves headroom for the `.md` suffix and any
/// collision suffix. The durable identity is the `_apple_notes_id` frontmatter,
/// never the filename.
const MAX_STEM_CHARS: usize = 120;

/// Fallback stem for notes whose title slugifies to nothing.
const UNTITLED_STEM: &str = "untitled";

/// Derive a unique slug-style filename stem for an imported note and reserve it.
///
/// `taken` accumulates every stem already used in the destination; the returned
/// stem is inserted into it so the next call cannot reuse it.
pub fn unique_stem(title: &str, taken: &mut HashSet<String>) -> String {
    let base = bounded_slug(title);
    let unique = disambiguate(&base, taken);
    taken.insert(unique.clone());
    unique
}

/// Slugify a title and bound its length, falling back to `untitled`.
fn bounded_slug(title: &str) -> String {
    let slug = title_to_slug(title);
    let bounded = truncate_on_char(&slug);
    if bounded.is_empty() {
        UNTITLED_STEM.to_string()
    } else {
        bounded
    }
}

/// Truncate to `MAX_STEM_CHARS` characters without splitting a char and without
/// leaving a trailing hyphen.
fn truncate_on_char(slug: &str) -> String {
    if slug.chars().count() <= MAX_STEM_CHARS {
        return slug.to_string();
    }
    let truncated: String = slug.chars().take(MAX_STEM_CHARS).collect();
    truncated.trim_end_matches('-').to_string()
}

/// Append `-2`, `-3`, ... until the stem is unused. Hyphenated to stay
/// slug-consistent with the rest of the filename.
fn disambiguate(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{unique_stem, MAX_STEM_CHARS, UNTITLED_STEM};
    use std::collections::HashSet;

    fn fresh() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn slugifies_like_native_notes() {
        let mut taken = fresh();
        assert_eq!(
            unique_stem("Quarterly Plan: Q3", &mut taken),
            "quarterly-plan-q3"
        );
    }

    #[test]
    fn empty_title_falls_back_to_untitled() {
        let mut taken = fresh();
        assert_eq!(unique_stem("", &mut taken), UNTITLED_STEM);
    }

    #[test]
    fn symbol_only_title_falls_back_to_untitled() {
        let mut taken = fresh();
        assert_eq!(unique_stem("!!! ???", &mut taken), UNTITLED_STEM);
    }

    #[test]
    fn collisions_get_hyphenated_suffixes_in_order() {
        let mut taken = fresh();
        assert_eq!(unique_stem("Groceries", &mut taken), "groceries");
        assert_eq!(unique_stem("Groceries", &mut taken), "groceries-2");
        assert_eq!(unique_stem("groceries", &mut taken), "groceries-3");
    }

    #[test]
    fn untitled_notes_also_disambiguate() {
        let mut taken = fresh();
        assert_eq!(unique_stem("", &mut taken), "untitled");
        assert_eq!(unique_stem("", &mut taken), "untitled-2");
    }

    #[test]
    fn long_title_is_truncated_without_trailing_hyphen() {
        let mut taken = fresh();
        let title = "word ".repeat(200); // slug far exceeds the cap
        let stem = unique_stem(&title, &mut taken);
        assert!(stem.chars().count() <= MAX_STEM_CHARS);
        assert!(!stem.ends_with('-'));
        assert!(stem.starts_with("word-word"));
    }

    #[test]
    fn preserves_unicode_letters() {
        let mut taken = fresh();
        // title_to_slug keeps Unicode alphanumerics; only separators become hyphens.
        assert_eq!(unique_stem("Café Notes", &mut taken), "café-notes");
    }

    #[test]
    fn reserves_each_returned_stem() {
        let mut taken = fresh();
        let first = unique_stem("Plan", &mut taken);
        assert!(taken.contains(&first));
        let second = unique_stem("Plan", &mut taken);
        assert_ne!(first, second);
        assert!(taken.contains(&second));
    }
}
