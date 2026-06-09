//! Typed accessor for the per-event `Name` column of a `Dialogue:`
//! line.
//!
//! The base [`parse`](crate::parse) entry point reads the dialogue
//! `Format:` row, splits each `Dialogue:` line on commas, and drops the
//! `Name` column on the floor — the shared `SubtitleCue` IR has no
//! slot for the per-event character / actor name the column carries.
//! The round-trip writer fills the column with an empty string.
//!
//! The SSA v4.x specification defines the column as:
//!
//! > *Field 5: Name — Character name. This is the name of the
//! > character who speaks the dialogue. It is for information only, to
//! > make the script easier to follow when editing/timing.*
//!
//! Three semantic facts fall out of that wording:
//!
//! * The column is **informational only** — renderers ignore it.
//!   Editors surface it as the per-line "speaker" / "actor" label.
//! * The column is a CSV cell on a `Dialogue:` line, so embedded
//!   commas are not representable; trailing CSV columns rely on the
//!   per-line delimiter. The parser treats any unescaped comma as the
//!   column terminator and never sees one here.
//! * The column **may be empty**. The dominant export format (every
//!   well-known tool) emits the literal empty cell `,,` when no actor
//!   name is set. The typed surface distinguishes the empty cell from
//!   an explicit non-empty name so the round-trip writer can re-emit
//!   either form unchanged.
//!
//! [`parse_name_field`] resolves the column into a typed
//! [`NameOverride`] enum:
//!
//! * [`NameOverride::Unset`] — column was empty or whitespace-only.
//!   The dominant case in real scripts; equivalent to "no per-event
//!   speaker label".
//! * [`NameOverride::Name(s)`] — column carried a non-empty, trimmed
//!   string. The value is exposed as an owned `String`.
//!
//! The parser is total — it never panics and never returns an error.
//! Surrounding whitespace inside the column is trimmed before the
//! emptiness check; the spec does not pin whether leading / trailing
//! spaces are part of the name or column padding, so the typed
//! accessor follows the same trimming convention as the layer / margin
//! / effect accessors. Authoring tools that use spaces around the
//! actual name (`" Bob "` for visual alignment in the source `.ass`)
//! see the inner name only — the renderer never reads this column, so
//! the surface is informational and the trimming is consistent with
//! the rest of the dialogue-column accessors in this crate.

/// Typed view of the per-event `Name` column on a `Dialogue:` line.
///
/// Produced by [`parse_name_field`]. The two variants encode the
/// spec's two semantic states: "no per-event speaker label" and "this
/// is the character / actor name for this dialogue line". The
/// [`Default`](NameOverride::Unset) impl is `Unset`, matching the
/// dominant case in real scripts (most export tools leave the column
/// empty).
///
/// The variant carries an owned `String` because the spec's wording is
/// "Character name" with no length or character-set bound beyond the
/// CSV row's own no-comma constraint; the typed surface preserves the
/// exact captured bytes inside the trimmed body so a round-trip writer
/// can re-emit the same name unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NameOverride {
    /// Column was empty or whitespace-only. Equivalent to "no
    /// per-event speaker label". This is the dominant case in real
    /// scripts; every well-known export tool emits the empty cell when
    /// no actor name is set.
    #[default]
    Unset,
    /// Column carried a non-empty character / actor name (whitespace
    /// trimmed). Renderers ignore this value per the spec's
    /// "information only" wording; editors surface it as the per-line
    /// speaker label.
    Name(String),
}

impl NameOverride {
    /// Returns the character name, if explicitly set. The
    /// [`Unset`](NameOverride::Unset) variant returns `None` so the
    /// caller can substitute a fallback in an
    /// `event.name.or(override.as_name())` chain.
    #[inline]
    pub fn as_name(&self) -> Option<&str> {
        match self {
            NameOverride::Unset => None,
            NameOverride::Name(s) => Some(s.as_str()),
        }
    }

    /// Consume the override and return the owned name string, if
    /// explicitly set. The [`Unset`](NameOverride::Unset) variant
    /// returns `None`.
    #[inline]
    pub fn into_name(self) -> Option<String> {
        match self {
            NameOverride::Unset => None,
            NameOverride::Name(s) => Some(s),
        }
    }

    /// Resolve this override to the effective character-name string the
    /// editor should display. [`Unset`](NameOverride::Unset) maps to
    /// the empty string (the spec's "no per-event speaker label" base);
    /// [`Name(s)`](NameOverride::Name) maps to `s`. This is the
    /// convenience accessor for an editor's actor-column rendering
    /// loop.
    #[inline]
    pub fn resolve(&self) -> &str {
        match self {
            NameOverride::Unset => "",
            NameOverride::Name(s) => s.as_str(),
        }
    }

    /// Returns `true` if the column carried an explicit non-empty
    /// name. The inverse of `matches!(self, NameOverride::Unset)`.
    #[inline]
    pub fn is_set(&self) -> bool {
        matches!(self, NameOverride::Name(_))
    }
}

/// Resolve the `Name` column into a typed [`NameOverride`].
///
/// The input is the raw bytes between two adjacent commas on a
/// `Dialogue:` line at the column position the `Format:` row labels
/// `Name` (sometimes called `Actor` in editor UI; the on-disk column
/// header is always `Name` per the SSA v4.x spec). Empty /
/// whitespace-only columns map to [`NameOverride::Unset`]; anything
/// else is captured into [`NameOverride::Name`] after surrounding
/// whitespace is trimmed.
///
/// The parser is total — it never panics and never returns an error.
/// The spec describes the column as "Character name" with no length or
/// character-set bound beyond the CSV row's own no-comma constraint;
/// the typed surface preserves the inner trimmed text byte-for-byte.
///
/// Per the spec, this column is **informational only**. Renderers
/// ignore it. The accessor exists so editors and downstream tools that
/// surface a "speaker" column do not need to re-implement the
/// dialogue-row split themselves.
///
/// # Examples
///
/// ```
/// use oxideav_ass::dialogue_name::{parse_name_field, NameOverride};
///
/// // Empty / whitespace columns — no per-event override.
/// assert_eq!(parse_name_field(""), NameOverride::Unset);
/// assert_eq!(parse_name_field("   "), NameOverride::Unset);
///
/// // Explicit character names round-trip after trimming.
/// assert_eq!(
///     parse_name_field("Bob"),
///     NameOverride::Name("Bob".to_string()),
/// );
/// assert_eq!(
///     parse_name_field("  Alice  "),
///     NameOverride::Name("Alice".to_string()),
/// );
///
/// // Resolve to the effective speaker label.
/// assert_eq!(parse_name_field("").resolve(), "");
/// assert_eq!(parse_name_field("Narrator").resolve(), "Narrator");
/// ```
pub fn parse_name_field(field: &str) -> NameOverride {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        NameOverride::Unset
    } else {
        NameOverride::Name(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_column_is_unset() {
        assert_eq!(parse_name_field(""), NameOverride::Unset);
    }

    #[test]
    fn whitespace_only_column_is_unset() {
        assert_eq!(parse_name_field("   "), NameOverride::Unset);
        assert_eq!(parse_name_field("\t"), NameOverride::Unset);
        assert_eq!(parse_name_field("\t  \t"), NameOverride::Unset);
    }

    #[test]
    fn explicit_ascii_name_is_captured() {
        assert_eq!(
            parse_name_field("Bob"),
            NameOverride::Name("Bob".to_string())
        );
        assert_eq!(
            parse_name_field("Narrator"),
            NameOverride::Name("Narrator".to_string())
        );
    }

    #[test]
    fn explicit_name_with_spaces_inside_is_preserved() {
        // The spec does not forbid spaces inside the name; only commas
        // are unrepresentable (they would terminate the CSV column
        // before the parser ever sees them here).
        assert_eq!(
            parse_name_field("Captain Smith"),
            NameOverride::Name("Captain Smith".to_string())
        );
        assert_eq!(
            parse_name_field("Side Character A"),
            NameOverride::Name("Side Character A".to_string())
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        // Real-world authoring tools sometimes pad CSV columns with a
        // leading or trailing space; the inner name is the load-bearing
        // payload.
        assert_eq!(
            parse_name_field("  Alice  "),
            NameOverride::Name("Alice".to_string())
        );
        assert_eq!(
            parse_name_field("\tBob"),
            NameOverride::Name("Bob".to_string())
        );
        assert_eq!(
            parse_name_field("Carol\t"),
            NameOverride::Name("Carol".to_string())
        );
    }

    #[test]
    fn inner_whitespace_is_not_collapsed() {
        // Trimming touches the surrounding whitespace only; inner
        // multi-space sequences (e.g. an editor that uses spaces for
        // visual alignment inside a multi-word name) are preserved
        // verbatim.
        assert_eq!(
            parse_name_field("Bob   the   Builder"),
            NameOverride::Name("Bob   the   Builder".to_string())
        );
    }

    #[test]
    fn non_ascii_name_round_trips() {
        // ASS files are UTF-8 (the parser strips a leading BOM at the
        // top of the file); non-ASCII character names are common in
        // fansub material.
        assert_eq!(
            parse_name_field("田中"),
            NameOverride::Name("田中".to_string())
        );
        assert_eq!(
            parse_name_field("María"),
            NameOverride::Name("María".to_string())
        );
        assert_eq!(
            parse_name_field("Σωκράτης"),
            NameOverride::Name("Σωκράτης".to_string())
        );
    }

    #[test]
    fn punctuation_inside_name_is_preserved() {
        // The spec only forbids commas (a CSV-row constraint, not a
        // name-payload constraint). Quotes, apostrophes, parentheses
        // are all fair game.
        assert_eq!(
            parse_name_field("O'Brien"),
            NameOverride::Name("O'Brien".to_string())
        );
        assert_eq!(
            parse_name_field("Dr. Watson"),
            NameOverride::Name("Dr. Watson".to_string())
        );
        assert_eq!(
            parse_name_field("Character (off-screen)"),
            NameOverride::Name("Character (off-screen)".to_string())
        );
    }

    #[test]
    fn single_character_name_is_captured() {
        // The shortest non-empty name — one character — is still a
        // valid name.
        assert_eq!(parse_name_field("A"), NameOverride::Name("A".to_string()));
        assert_eq!(parse_name_field("?"), NameOverride::Name("?".to_string()));
    }

    #[test]
    fn as_name_accessor_returns_none_for_unset() {
        assert_eq!(parse_name_field("").as_name(), None);
        assert_eq!(parse_name_field("   ").as_name(), None);
    }

    #[test]
    fn as_name_accessor_returns_some_for_explicit_value() {
        assert_eq!(parse_name_field("Bob").as_name(), Some("Bob"));
        assert_eq!(parse_name_field("  Alice  ").as_name(), Some("Alice"));
    }

    #[test]
    fn into_name_accessor_consumes_and_returns_owned_string() {
        // The owning variant of `as_name` for callers that want to
        // store / forward the captured name.
        assert_eq!(parse_name_field("").into_name(), None);
        assert_eq!(parse_name_field("Bob").into_name(), Some("Bob".to_string()));
    }

    #[test]
    fn resolve_unset_maps_to_empty_string() {
        // The spec's "no per-event speaker label" base is an empty
        // string for editor display.
        assert_eq!(parse_name_field("").resolve(), "");
        assert_eq!(parse_name_field("   ").resolve(), "");
    }

    #[test]
    fn resolve_explicit_value_maps_through() {
        assert_eq!(parse_name_field("Bob").resolve(), "Bob");
        assert_eq!(parse_name_field("  Alice  ").resolve(), "Alice");
    }

    #[test]
    fn is_set_distinguishes_the_two_variants() {
        assert!(!parse_name_field("").is_set());
        assert!(!parse_name_field("   ").is_set());
        assert!(parse_name_field("Bob").is_set());
        assert!(parse_name_field("田中").is_set());
    }

    #[test]
    fn default_trait_matches_empty_column() {
        // `NameOverride::default()` is the same as an empty column.
        assert_eq!(NameOverride::default(), NameOverride::Unset);
        assert_eq!(NameOverride::default(), parse_name_field(""));
    }

    #[test]
    fn eq_traits_are_usable() {
        // The type is `Eq` — values can be compared / matched freely.
        let a = parse_name_field("Bob");
        let b = parse_name_field("Bob");
        let c = parse_name_field("Alice");
        assert_eq!(a, b);
        assert_ne!(a, c);
        match a {
            NameOverride::Name(s) => assert_eq!(s, "Bob"),
            NameOverride::Unset => panic!("expected explicit name"),
        }
    }

    #[test]
    fn clone_round_trips_through_a_pair() {
        // The variant is `Clone`; document the expected ergonomic.
        let a = parse_name_field("Captain Smith");
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(b.as_name(), Some("Captain Smith"));
    }

    #[test]
    fn whitespace_padded_value_compares_equal_to_unpadded() {
        // Trimming ensures the two forms hash / compare the same; this
        // is the property an editor needs when grouping per-actor.
        assert_eq!(parse_name_field("Bob"), parse_name_field("  Bob  "));
        assert_eq!(parse_name_field("Bob"), parse_name_field("\tBob\t"));
    }

    #[test]
    fn unset_is_distinct_from_explicit_empty_string() {
        // The constructor `NameOverride::Name(String::new())` is not
        // reachable from the parser (the only path goes through the
        // trim-emptiness check), but the spec distinguishes "no per-
        // event speaker" from "explicit blank speaker", and the
        // accessor surface preserves that distinction. Document the
        // expected behaviour at the constructor boundary.
        let unset = NameOverride::Unset;
        let explicit_empty = NameOverride::Name(String::new());
        assert_ne!(unset, explicit_empty);
        assert!(!unset.is_set());
        // An explicitly empty name still counts as "set" — the typed
        // accessor distinguishes the spec states.
        assert!(explicit_empty.is_set());
    }

    #[test]
    fn long_name_round_trips() {
        // The spec does not pin an upper length bound; a long name
        // (e.g. an editor that used the speaker column as a free-form
        // note slot) round-trips byte-for-byte through the typed
        // surface.
        let long = "A".repeat(1024);
        let parsed = parse_name_field(&long);
        assert_eq!(parsed, NameOverride::Name(long.clone()));
        assert_eq!(parsed.resolve(), long.as_str());
    }
}
