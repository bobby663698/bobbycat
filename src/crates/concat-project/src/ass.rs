// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Advanced SubStation Alpha (`.ass`) subtitles: the data layer of an
//! import, beside [`crate::srt`].
//!
//! Only the three sections a caption import needs are read:
//! `[Script Info]` for the play resolution, `[V4+ Styles]` for the named
//! styles, and `[Events]` for the dialogue lines. A style keeps the
//! fields a title can use (face, size, colours, weight, stroke, shadow,
//! alignment and margins). Override tags in the dialogue text are
//! discarded except `\an`, which chooses one of the nine alignments;
//! `\N` and `\n` become line breaks and `\h` a space.
//!
//! The host does the file dialog and turns the cues into commands, the
//! same way it does for SubRip. This module reads text and nothing else.

use std::collections::BTreeMap;

use thiserror::Error;

/// One of ASS's nine alignments, written `\an1` through `\an9`.
/// They run left to right, bottom row first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// `\an1`.
    BottomLeft,
    /// `\an2`.
    BottomCenter,
    /// `\an3`.
    BottomRight,
    /// `\an4`.
    MiddleLeft,
    /// `\an5`.
    MiddleCenter,
    /// `\an6`.
    MiddleRight,
    /// `\an7`.
    TopLeft,
    /// `\an8`.
    TopCenter,
    /// `\an9`.
    TopRight,
}

impl Alignment {
    /// The alignment numbered 1..=9, or `None` for anything else.
    pub fn from_number(number: u32) -> Option<Self> {
        Some(match number {
            1 => Self::BottomLeft,
            2 => Self::BottomCenter,
            3 => Self::BottomRight,
            4 => Self::MiddleLeft,
            5 => Self::MiddleCenter,
            6 => Self::MiddleRight,
            7 => Self::TopLeft,
            8 => Self::TopCenter,
            9 => Self::TopRight,
            _ => return None,
        })
    }

    /// Its ASS number, 1..=9.
    pub fn number(self) -> u32 {
        match self {
            Self::BottomLeft => 1,
            Self::BottomCenter => 2,
            Self::BottomRight => 3,
            Self::MiddleLeft => 4,
            Self::MiddleCenter => 5,
            Self::MiddleRight => 6,
            Self::TopLeft => 7,
            Self::TopCenter => 8,
            Self::TopRight => 9,
        }
    }

    /// Horizontal place, 0 left, 0.5 centre, 1 right.
    pub fn horizontal(self) -> f64 {
        match self {
            Self::BottomLeft | Self::MiddleLeft | Self::TopLeft => 0.0,
            Self::BottomCenter | Self::MiddleCenter | Self::TopCenter => 0.5,
            Self::BottomRight | Self::MiddleRight | Self::TopRight => 1.0,
        }
    }

    /// Vertical place, 0 top, 0.5 middle, 1 bottom.
    pub fn vertical(self) -> f64 {
        match self {
            Self::TopLeft | Self::TopCenter | Self::TopRight => 0.0,
            Self::MiddleLeft | Self::MiddleCenter | Self::MiddleRight => 0.5,
            Self::BottomLeft | Self::BottomCenter | Self::BottomRight => 1.0,
        }
    }
}

/// An ASS colour field. ASS writes `&HAABBGGRR` (or `&HBBGGRR` with no
/// alpha); its alpha is inverted, 0 opaque and 255 transparent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    /// Red, 0..255.
    pub r: u8,
    /// Green, 0..255.
    pub g: u8,
    /// Blue, 0..255.
    pub b: u8,
    /// ASS alpha as written: 0 opaque, 255 transparent.
    pub a: u8,
}

/// A named `[V4+ Styles]` entry, kept to the fields a title can use.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// `Fontname`.
    pub font: String,
    /// `Fontsize`.
    pub size: f64,
    /// `PrimaryColour`, the text's fill.
    pub primary: Color,
    /// `OutlineColour`, the stroke's colour.
    pub outline_color: Color,
    /// `Bold`: ASS writes -1 for on.
    pub bold: bool,
    /// `Italic`: ASS writes -1 for on.
    pub italic: bool,
    /// `Outline`, the stroke's width.
    pub outline: f64,
    /// `Shadow`, the shadow's distance.
    pub shadow: f64,
    /// `Alignment` from the style, before any `\an` override.
    pub alignment: Alignment,
    /// `MarginL`, in script pixels.
    pub margin_l: f64,
    /// `MarginR`, in script pixels.
    pub margin_r: f64,
    /// `MarginV`, in script pixels.
    pub margin_v: f64,
}

/// One dialogue line.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// Seconds from the start.
    pub start: f64,
    /// Seconds from the start; never before `start`.
    pub end: f64,
    /// The words, with `\N` and `\n` as line breaks.
    pub text: String,
    /// The name of the style in [`Document::styles`], when it names one.
    pub style: Option<String>,
    /// A `\an` override in the line, which wins over the style's
    /// alignment.
    pub alignment: Option<Alignment>,
}

/// What was read out of a file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    /// `PlayResX`, when the Script Info section gives one.
    pub play_res_x: Option<u32>,
    /// `PlayResY`, when the Script Info section gives one.
    pub play_res_y: Option<u32>,
    /// Styles by their `Name`, in file order for equal names.
    pub styles: BTreeMap<String, Style>,
    /// Dialogue lines, in file order.
    pub cues: Vec<Cue>,
}

/// Why a file could not be read, and the line it stopped on, from 1.
#[derive(Clone, Debug, Error, PartialEq)]
#[error("line {line}: {reason}")]
pub struct AssError {
    /// The line, counted from 1.
    pub line: usize,
    /// What was wrong with it.
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Section {
    Other,
    ScriptInfo,
    Styles,
    Events,
}

/// Reads an ASS file. Unknown sections, fields and comment lines are
/// ignored; a malformed time or colour falls back rather than failing
/// the whole import, but a dialogue line without a usable time is an
/// error, as it would place a caption at the wrong point on the timeline.
pub fn parse(text: &str) -> Result<Document, AssError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut document = Document::default();
    let mut section = Section::Other;
    let mut style_format: Vec<String> = Vec::new();
    let mut event_format: Vec<String> = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        let at = number + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            section = match name.trim().to_ascii_lowercase().as_str() {
                "script info" => Section::ScriptInfo,
                "v4+ styles" => Section::Styles,
                "events" => Section::Events,
                _ => Section::Other,
            };
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        match section {
            Section::ScriptInfo => match key.as_str() {
                "playresx" => document.play_res_x = rest.trim().parse().ok(),
                "playresy" => document.play_res_y = rest.trim().parse().ok(),
                _ => {}
            },
            Section::Styles => match key.as_str() {
                "format" => style_format = format_of(rest),
                "style" => {
                    if let Some((name, style)) = parse_style(&style_format, rest) {
                        document.styles.insert(name, style);
                    }
                }
                _ => {}
            },
            Section::Events => match key.as_str() {
                "format" => event_format = format_of(rest),
                "dialogue" => {
                    document.cues.push(parse_dialogue(&event_format, rest, at)?);
                }
                _ => {}
            },
            Section::Other => {}
        }
    }
    Ok(document)
}

fn error(line: usize, reason: &str) -> AssError {
    AssError {
        line,
        reason: reason.to_owned(),
    }
}

/// The field names of a `Format:` line.
fn format_of(rest: &str) -> Vec<String> {
    rest.split(',')
        .map(|field| field.trim().to_owned())
        .collect()
}

/// A record's fields. The last field may hold commas (dialogue text
/// does), so split into at most as many pieces as the format names.
fn fields_of(rest: &str, format: &[String]) -> Vec<&str> {
    let count = format.len().max(1);
    rest.splitn(count, ',').map(str::trim).collect()
}

/// A named field of a record, by the `Format:` line's spelling.
fn field<'a>(format: &[String], values: &[&'a str], name: &str) -> Option<&'a str> {
    let index = format.iter().position(|field| field.eq_ignore_ascii_case(name))?;
    values.get(index).copied()
}

fn number_field(format: &[String], values: &[&str], name: &str) -> Option<f64> {
    field(format, values, name)?.parse().ok()
}

fn flag_field(format: &[String], values: &[&str], name: &str) -> bool {
    matches!(field(format, values, name).map(str::trim), Some("-1") | Some("1"))
}

/// `&HAABBGGRR`, or `&HBBGGRR`. An unparsable colour is white, the
/// least surprising fill for a title that would otherwise be invisible.
fn color_of(text: &str) -> Color {
    let digits = text.trim().trim_start_matches('&').trim_start_matches('H');
    if !digits.is_ascii() || digits.len() < 6 {
        return Color {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        };
    }
    let tail = &digits[digits.len() - 6..];
    let rgb = u32::from_str_radix(tail, 16).unwrap_or(0xffffff);
    let alpha = if digits.len() >= 8 {
        u8::from_str_radix(&digits[digits.len() - 8..digits.len() - 6], 16).unwrap_or(0)
    } else {
        0
    };
    Color {
        r: (rgb & 0xff) as u8,
        g: ((rgb >> 8) & 0xff) as u8,
        b: ((rgb >> 16) & 0xff) as u8,
        a: alpha,
    }
}

fn parse_style(format: &[String], rest: &str) -> Option<(String, Style)> {
    let values = fields_of(rest, format);
    let name = field(format, &values, "Name")?.to_owned();
    let alignment = field(format, &values, "Alignment")
        .and_then(|value| value.trim().parse::<u32>().ok())
        .and_then(Alignment::from_number)
        .unwrap_or(Alignment::BottomCenter);
    let style = Style {
        font: field(format, &values, "Fontname").unwrap_or("").to_owned(),
        size: number_field(format, &values, "Fontsize").unwrap_or(20.0),
        primary: color_of(field(format, &values, "PrimaryColour").unwrap_or("")),
        outline_color: color_of(field(format, &values, "OutlineColour").unwrap_or("")),
        bold: flag_field(format, &values, "Bold"),
        italic: flag_field(format, &values, "Italic"),
        outline: number_field(format, &values, "Outline").unwrap_or(0.0),
        shadow: number_field(format, &values, "Shadow").unwrap_or(0.0),
        alignment,
        margin_l: number_field(format, &values, "MarginL").unwrap_or(0.0),
        margin_r: number_field(format, &values, "MarginR").unwrap_or(0.0),
        margin_v: number_field(format, &values, "MarginV").unwrap_or(0.0),
    };
    Some((name, style))
}

/// `H:MM:SS.cc` in seconds. The fraction is hundredths in ASS, but
/// parsing it as a plain fraction works for any number of digits.
fn timestamp(text: &str) -> Option<f64> {
    let mut parts = text.trim().split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn parse_dialogue(format: &[String], rest: &str, at: usize) -> Result<Cue, AssError> {
    let values = fields_of(rest, format);
    let start = field(format, &values, "Start")
        .and_then(timestamp)
        .ok_or_else(|| error(at, "a dialogue line without a usable start time"))?;
    let end = field(format, &values, "End")
        .and_then(timestamp)
        .ok_or_else(|| error(at, "a dialogue line without a usable end time"))?;
    if end < start {
        return Err(error(at, "the dialogue ends before it starts"));
    }
    let (text, alignment) = clean_text(field(format, &values, "Text").unwrap_or(""));
    let style = field(format, &values, "Style")
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    Ok(Cue {
        start,
        end,
        text,
        style,
        alignment,
    })
}

/// Drops `{...}` override groups, keeping the last `\anN` in them,
/// and turns ASS's explicit breaks into newlines.
fn clean_text(raw: &str) -> (String, Option<Alignment>) {
    let mut alignment = None;
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' => {
                let mut tag = String::new();
                for inside in chars.by_ref() {
                    if inside == '}' {
                        break;
                    }
                    tag.push(inside);
                }
                if let Some(at) = tag.find("\\an") {
                    let digits: String = tag[at + 3..]
                        .chars()
                        .take_while(|digit| digit.is_ascii_digit())
                        .collect();
                    if let Ok(number) = digits.parse::<u32>() {
                        alignment = Alignment::from_number(number);
                    }
                }
            }
            '\\' => match chars.next() {
                Some('N') | Some('n') => out.push('\n'),
                Some('h') => out.push(' '),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            other => out.push(other),
        }
    }
    (out, alignment)
}

#[cfg(test)]
mod tests {
    use super::{Alignment, Color, Document, parse};

    const FILE: &str = "[Script Info]\n\
        Title: sample\n\
        PlayResX: 1280\n\
        PlayResY: 720\n\
        \n\
        [V4+ Styles]\n\
        Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
        Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1\n\
        \n\
        [Events]\n\
        Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
        Dialogue: 0,0:00:01.23,0:00:03.45,Default,,0,0,0,,{\\an8}First\\NSecond\n\
        Dialogue: 0,0:00:04.00,0:00:05.00,Default,,0,0,0,,Last line\n";

    #[test]
    fn all_nine_alignments_round_trip_to_a_grid() {
        for number in 1..=9 {
            let alignment = Alignment::from_number(number).expect("a real alignment");
            assert_eq!(alignment.number(), number);
        }
        assert_eq!(Alignment::from_number(0), None);
        assert_eq!(Alignment::from_number(10), None);
        assert_eq!(Alignment::BottomLeft.horizontal(), 0.0);
        assert_eq!(Alignment::BottomCenter.horizontal(), 0.5);
        assert_eq!(Alignment::BottomRight.horizontal(), 1.0);
        assert_eq!(Alignment::TopLeft.vertical(), 0.0);
        assert_eq!(Alignment::MiddleCenter.vertical(), 0.5);
        assert_eq!(Alignment::BottomRight.vertical(), 1.0);
    }

    #[test]
    fn colours_are_read_as_bbgg_rr() {
        assert_eq!(super::color_of("&H00FFFFFF"), Color { r: 255, g: 255, b: 255, a: 0 });
        assert_eq!(super::color_of("&H000000FF"), Color { r: 255, g: 0, b: 0, a: 0 });
        assert_eq!(super::color_of("&H00FF0000"), Color { r: 0, g: 0, b: 255, a: 0 });
        assert_eq!(super::color_of("&H800000FF"), Color { r: 255, g: 0, b: 0, a: 0x80 });
    }

    #[test]
    fn reads_styles_and_dialogues() {
        let document = parse(FILE).expect("parses");
        assert_eq!(document.play_res_x, Some(1280));
        assert_eq!(document.play_res_y, Some(720));
        assert_eq!(document.styles.len(), 1);
        let style = &document.styles["Default"];
        assert_eq!(style.font, "Arial");
        assert_eq!(style.size, 20.0);
        assert_eq!(style.primary, Color { r: 255, g: 255, b: 255, a: 0 });
        assert!(style.bold);
        assert!(!style.italic);
        assert_eq!(style.outline, 2.0);
        assert_eq!(style.alignment, Alignment::BottomCenter);
        assert_eq!((style.margin_l, style.margin_r, style.margin_v), (10.0, 10.0, 10.0));

        assert_eq!(document.cues.len(), 2);
        let first = &document.cues[0];
        assert_eq!((first.start, first.end), (1.23, 3.45));
        assert_eq!(first.text, "First\nSecond");
        assert_eq!(first.style.as_deref(), Some("Default"));
        assert_eq!(first.alignment, Some(Alignment::TopCenter));
        let second = &document.cues[1];
        assert_eq!((second.start, second.end), (4.0, 5.0));
        assert_eq!(second.text, "Last line");
        assert_eq!(second.alignment, None);
    }

    #[test]
    fn an_empty_or_commented_file_has_nothing_to_import() {
        let document = parse("; a comment\n[Script Info]\nTitle: none\n").expect("parses");
        assert_eq!(document, Document::default());
    }

    #[test]
    fn a_dialogue_without_a_time_is_an_error() {
        let text = "[Events]\n\
            Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
            Dialogue: 0,nope,0:00:02.00,Default,,0,0,0,,words\n";
        let error = parse(text).expect_err("no start time");
        assert_eq!(error.line, 3);
        assert!(error.reason.contains("start time"));
    }
}
