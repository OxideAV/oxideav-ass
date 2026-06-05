//! Typed `[Fonts]` / `[Graphics]` attachment accessor.
//!
//! The base [`parse`](crate::parse) entry point round-trips the
//! `[Fonts]` and `[Graphics]` SSA section bodies verbatim through
//! [`SubtitleTrack::extradata`](oxideav_subtitle::ir::SubtitleTrack::extradata)
//! so the writer can re-emit them unchanged. That is enough for a
//! parse → write loop but does not expose the embedded font / picture
//! payloads as decoded bytes — downstream tools that want to inspect or
//! materialise the attached glyph files have to walk the printable
//! body lines themselves and reverse the SSA Appendix-B character
//! encoding.
//!
//! This module fills that gap with a side-channel reader:
//! [`parse_attachments`] re-walks the script header, groups consecutive
//! body lines under each `fontname: <name>` / `filename: <name>`
//! marker, and decodes the printable run back into the original binary
//! payload.
//!
//! The encoding rules are taken from the SSA v4 spec appendix:
//! three input bytes are packed into a 24-bit integer and split into
//! four 6-bit fields, each offset by 33 and emitted as an ASCII
//! character. Lengths that are not a multiple of three are padded:
//! a one-byte tail multiplies by `0x100` and emits the top 12 bits as
//! two characters, a two-byte tail multiplies by `0x10000` and emits
//! the top 18 bits as three characters. Lines are 80 characters wide
//! except possibly the last, and the offset of 33 means lowercase
//! letters never appear in the body — so `fontname:` and `filename:`
//! always survive case-folding into a section boundary marker.
//!
//! Decoding ends at the next section header, the next `fontname:` /
//! `filename:` line, or end-of-file.

use oxideav_core::{Error, Result};

/// Which SSA attachment section a payload came from.
///
/// The two sections share the same printable encoding scheme; they
/// differ only in the keyword that introduces each file and in what
/// the consumer is expected to do with the bytes (install as a font
/// vs. show as a picture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    /// `[Fonts]` — the bytes are a font file the script wants to use.
    Font,
    /// `[Graphics]` — the bytes are a picture file referenced by the
    /// script (legacy SSA backdrop / overlay).
    Graphics,
}

/// One decoded `[Fonts]` / `[Graphics]` attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// `Font` for `fontname:`, `Graphics` for `filename:`.
    pub kind: AttachmentKind,
    /// File name as written on the introducing `fontname:` /
    /// `filename:` line, trimmed of leading and trailing whitespace.
    pub name: String,
    /// Decoded binary payload — the bytes that, when re-encoded with
    /// the SSA Appendix-B scheme, would produce the printable body
    /// the parser consumed.
    pub data: Vec<u8>,
}

/// Walk the header sections of an SSA / ASS script and collect every
/// `[Fonts]` / `[Graphics]` attachment as typed bytes.
///
/// Returns an empty vector when the script has no attachment sections
/// or when every present attachment has an empty body. The order of
/// the returned vector follows the source order of the introducing
/// `fontname:` / `filename:` lines.
///
/// Lines whose contents contain characters outside the SSA printable
/// alphabet (anything below `!` = 33 or above `~` = 126) inside an
/// attachment body are skipped — the SSA spec defines the alphabet as
/// "the ascii character for each number" produced by `value + 33`, so
/// values outside `33..=126` could not have been emitted by an
/// encoder. This makes the reader robust against stray editor edits
/// (e.g. a blank line inside the body) without rejecting the whole
/// attachment.
pub fn parse_attachments(bytes: &[u8]) -> Result<Vec<Attachment>> {
    let text = strip_utf8_bom(bytes);
    let mut out: Vec<Attachment> = Vec::new();
    let mut current_section = String::new();
    let mut current: Option<Attachment> = None;

    for line_raw in text.split('\n') {
        let line = line_raw.trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some(att) = current.take() {
                out.push(att);
            }
            current_section = trimmed[1..trimmed.len() - 1].to_ascii_lowercase();
            continue;
        }

        match current_section.as_str() {
            "fonts" => {
                if let Some(rest) = trimmed.strip_prefix("fontname:") {
                    if let Some(att) = current.take() {
                        out.push(att);
                    }
                    current = Some(Attachment {
                        kind: AttachmentKind::Font,
                        name: rest.trim().to_string(),
                        data: Vec::new(),
                    });
                } else if let Some(att) = current.as_mut() {
                    decode_body_line_into(trimmed, &mut att.data)?;
                }
            }
            "graphics" => {
                if let Some(rest) = trimmed.strip_prefix("filename:") {
                    if let Some(att) = current.take() {
                        out.push(att);
                    }
                    current = Some(Attachment {
                        kind: AttachmentKind::Graphics,
                        name: rest.trim().to_string(),
                        data: Vec::new(),
                    });
                } else if let Some(att) = current.as_mut() {
                    decode_body_line_into(trimmed, &mut att.data)?;
                }
            }
            _ => {
                // Outside a recognised attachment section. Drop any
                // dangling attachment in case a body line straggled
                // out of its section (defensive).
                if let Some(att) = current.take() {
                    out.push(att);
                }
            }
        }
    }

    if let Some(att) = current.take() {
        out.push(att);
    }

    Ok(out)
}

/// Decode a single printable attachment body line, append the binary
/// bytes onto `out`, and return `Ok(())`.
///
/// A line that mixes printable characters with anything outside the
/// `33..=126` ASCII range is silently skipped — see the
/// [`parse_attachments`] doc-comment for the rationale.
fn decode_body_line_into(line: &str, out: &mut Vec<u8>) -> Result<()> {
    let bytes = line.as_bytes();
    for &b in bytes {
        if !(33..=126).contains(&b) {
            return Ok(());
        }
    }
    decode_chunk_into(bytes, out)
}

/// Decode a printable-character chunk back into binary bytes.
///
/// `chunk.len() % 4` falls into one of three cases per the spec:
///
/// * `0` — every quartet packs back into three full output bytes.
/// * `3` — the trailing triplet decodes the top 18 bits of a
///   `payload * 0x10000` value into two output bytes (the low byte
///   was the multiplier and is discarded).
/// * `2` — the trailing pair decodes the top 12 bits of a
///   `payload * 0x100` value into one output byte.
///
/// Any other residue (one stray character, which the spec does not
/// describe) is a malformed body and returns
/// [`Error::InvalidData`].
fn decode_chunk_into(chunk: &[u8], out: &mut Vec<u8>) -> Result<()> {
    let mut i = 0;
    while i + 4 <= chunk.len() {
        let v = chars_to_value(&chunk[i..i + 4]);
        out.push((v >> 16) as u8);
        out.push((v >> 8) as u8);
        out.push(v as u8);
        i += 4;
    }
    match chunk.len() - i {
        0 => Ok(()),
        3 => {
            // 18 top bits of (n0 << 16): 3 input chars → 2 output bytes.
            let mut v: u32 = 0;
            for &c in &chunk[i..i + 3] {
                v = (v << 6) | (c - 33) as u32;
            }
            // We have 18 packed bits in the low 18 of `v`; those
            // represent the top 18 bits of (payload << 16). Shifting
            // right by 16 reverses the encoder's multiplication.
            let payload = v << (24 - 18); // align high
            out.push((payload >> 16) as u8);
            out.push((payload >> 8) as u8);
            Ok(())
        }
        2 => {
            // 12 top bits of (n0 << 8): 2 input chars → 1 output byte.
            let mut v: u32 = 0;
            for &c in &chunk[i..i + 2] {
                v = (v << 6) | (c - 33) as u32;
            }
            let payload = v << (24 - 12); // align high
            out.push((payload >> 16) as u8);
            Ok(())
        }
        _ => Err(Error::InvalidData(
            "attachment body line has stray printable character".to_string(),
        )),
    }
}

/// Pack four printable characters (each in `33..=126`) back into the
/// 24-bit integer the encoder packed them out of.
fn chars_to_value(quad: &[u8]) -> u32 {
    let a = (quad[0] - 33) as u32;
    let b = (quad[1] - 33) as u32;
    let c = (quad[2] - 33) as u32;
    let d = (quad[3] - 33) as u32;
    (a << 18) | (b << 12) | (c << 6) | d
}

/// UTF-8 BOM strip + lossy text decode mirroring the main parser. Kept
/// local so this module stays standalone.
fn strip_utf8_bom(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    let trimmed = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    String::from_utf8_lossy(trimmed)
}

// ---------------------------------------------------------------------------
// In-crate fixture encoder used by tests only — never exported.

#[cfg(test)]
pub(crate) fn encode_for_test(data: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let v = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push((((v >> 18) & 0x3F) as u8 + 33) as char);
        out.push((((v >> 12) & 0x3F) as u8 + 33) as char);
        out.push((((v >> 6) & 0x3F) as u8 + 33) as char);
        out.push(((v & 0x3F) as u8 + 33) as char);
        i += 3;
    }
    match data.len() - i {
        0 => {}
        1 => {
            // payload = data[i] << 16 → top 12 bits become 2 chars.
            let payload: u32 = (data[i] as u32) << 16;
            let bits = payload >> (24 - 12);
            out.push((((bits >> 6) & 0x3F) as u8 + 33) as char);
            out.push(((bits & 0x3F) as u8 + 33) as char);
        }
        2 => {
            // payload = (data[i] << 16) | (data[i+1] << 8) → top 18 bits → 3 chars.
            let payload: u32 = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
            let bits = payload >> (24 - 18);
            out.push((((bits >> 12) & 0x3F) as u8 + 33) as char);
            out.push((((bits >> 6) & 0x3F) as u8 + 33) as char);
            out.push(((bits & 0x3F) as u8 + 33) as char);
        }
        _ => unreachable!(),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script_with(section: &str, intro_key: &str, file_name: &str, body: &str) -> String {
        format!(
            "[Script Info]\nScriptType: v4.00+\n\n[{section}]\n{intro_key}: {file_name}\n{body}\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,x\n"
        )
    }

    #[test]
    fn fonts_section_decodes_three_byte_aligned_payload() {
        // Payload length 3 → exactly one printable quartet, no tail.
        let raw: &[u8] = &[0x00, 0x10, 0x80];
        let body = encode_for_test(raw);
        let script = script_with("Fonts", "fontname", "demo.ttf", &body);
        let atts = parse_attachments(script.as_bytes()).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].kind, AttachmentKind::Font);
        assert_eq!(atts[0].name, "demo.ttf");
        assert_eq!(atts[0].data, raw);
    }

    #[test]
    fn graphics_section_decodes_one_byte_tail() {
        // 4 bytes → one full quartet (3 chars worth) + a 1-byte tail
        // that the encoder emits as 2 characters per the spec. Walks
        // both the regular path and the odd-length padding path.
        let raw: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF];
        let body = encode_for_test(raw);
        // Sanity: encoder must produce 4 + 2 = 6 chars for a 4-byte input.
        assert_eq!(body.len(), 6);
        let script = script_with("Graphics", "filename", "splash.png", &body);
        let atts = parse_attachments(script.as_bytes()).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].kind, AttachmentKind::Graphics);
        assert_eq!(atts[0].name, "splash.png");
        assert_eq!(atts[0].data, raw);
    }

    #[test]
    fn multiple_fonts_split_on_repeated_fontname_marker() {
        // Two consecutive `fontname:` blocks in the same `[Fonts]`
        // section must surface as two `Attachment` entries with
        // independent payloads, in source order. Also covers
        // multi-line bodies — the spec mandates 80-char lines, so we
        // emit two short lines and confirm they concatenate.
        let raw_a: &[u8] = &[0x01, 0x02, 0x03, 0x04, 0x05]; // 5 bytes → quartet + 3-char tail
        let raw_b: &[u8] = &[0xFF, 0xEE, 0xDD]; // 3 bytes → one quartet
        let body_a = encode_for_test(raw_a);
        let body_b = encode_for_test(raw_b);
        // Split body_a across two physical lines to exercise the
        // line-joining path.
        let (head_a, tail_a) = body_a.split_at(4);
        let script = format!(
            "[Script Info]\nScriptType: v4.00+\n\n[Fonts]\nfontname: a.ttf\n{head_a}\n{tail_a}\nfontname: b.ttf\n{body_b}\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,x\n"
        );
        let atts = parse_attachments(script.as_bytes()).unwrap();
        assert_eq!(atts.len(), 2);
        assert_eq!(atts[0].name, "a.ttf");
        assert_eq!(atts[0].data, raw_a);
        assert_eq!(atts[1].name, "b.ttf");
        assert_eq!(atts[1].data, raw_b);
    }

    #[test]
    fn no_attachment_sections_yields_empty_vec() {
        let script = "[Script Info]\nScriptType: v4.00+\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,hi\n";
        let atts = parse_attachments(script.as_bytes()).unwrap();
        assert!(atts.is_empty());
    }

    #[test]
    fn empty_body_yields_attachment_with_empty_payload() {
        // `fontname:` with no body lines should still surface as a
        // typed entry — the caller may want to know the script
        // *referenced* an embedded font even if the body is missing
        // (e.g. the editor stripped it). Re-parsing then writing the
        // script keeps the marker, so we mirror that behaviour.
        let script = "[Script Info]\nScriptType: v4.00+\n\n[Fonts]\nfontname: only_a_name.ttf\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,x\n";
        let atts = parse_attachments(script.as_bytes()).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].name, "only_a_name.ttf");
        assert!(atts[0].data.is_empty());
    }
}
