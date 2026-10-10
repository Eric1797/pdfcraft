//! Convert Word (.docx) documents to PDF: paragraphs, headings, lists, simple tables and
//! inline images, set in the Helvetica family (L4, Convert ▸ Word).
//!
//! A .docx file is a zip package; only `word/document.xml` is required, with
//! `word/numbering.xml` for lists and `word/_rels/document.xml.rels` plus `word/media/*`
//! for images. Everything read from the file is capped, and anything the converter does
//! not understand is skipped, never fatal: headers, footers, footnotes, text boxes,
//! shapes, charts, equations, tracked deletions, strike-through, small caps and column
//! layouts are left out; nested tables are flattened to text; merged cells are treated
//! as separate cells; right-to-left paragraphs keep their logical order; fonts always
//! map to the Helvetica family (sizes, bold, italic, underline and colours are kept).

use std::collections::HashMap;
use std::io::Read;

use pdfcraft_cos::{Dict, Document, ObjRef, Object};
use pdfcraft_fonts::{helvetica_width, literal, win_ansi};

use super::{CreateError, MAX_SIDE, add_page, set_title};

/// Caps for hostile packages and documents.
const MAX_ENTRIES: usize = 4096;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MEDIA_FILES: usize = 500;
const MAX_PARAS: usize = 200_000;
const MAX_INLINES: usize = 2_000_000;
const MAX_TABLE_ROWS: usize = 20_000;
const MAX_TABLE_COLS: usize = 128;
const MAX_PAGES: usize = 10_000;

/// A paragraph's alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// One styled text run.
#[derive(Clone, Debug, Default)]
struct Run {
    text: String,
    bold: bool,
    italic: bool,
    underline: bool,
    size: f64,
    color: (f64, f64, f64),
    /// Baseline shift in points (superscript up, subscript down).
    dy: f64,
}

/// Inline content of a paragraph.
#[derive(Clone, Debug)]
enum Inline {
    Text(Run),
    Break,
    PageBreak,
    Image(usize),
}

/// A list marker: which numbering series and at what level.
#[derive(Clone, Copy, Debug)]
struct ListRef {
    num: u32,
    level: u8,
}

/// One paragraph.
#[derive(Clone, Debug)]
struct Para {
    size: f64,
    align: Align,
    before: f64,
    after: f64,
    indent: f64,
    page_break: bool,
    list: Option<ListRef>,
    content: Vec<Inline>,
}

/// One table cell: its own paragraphs.
#[derive(Clone, Debug, Default)]
struct Cell {
    paras: Vec<Para>,
}

/// A simple table: column widths in points and rows of cells.
#[derive(Clone, Debug)]
struct Table {
    widths: Vec<f64>,
    rows: Vec<Vec<Cell>>,
}

#[derive(Clone, Debug)]
enum Block {
    Para(Para),
    Table(Table),
}

/// An image referenced by the document.
#[derive(Clone, Debug)]
struct Image {
    name: String,
    bytes: Vec<u8>,
    w_emu: f64,
    h_emu: f64,
}

/// A parsed document: blocks, images and the page setup.
#[derive(Clone, Debug, Default)]
struct WordDoc {
    blocks: Vec<Block>,
    images: Vec<Image>,
    page: (f64, f64),
    margins: (f64, f64, f64, f64),
}

fn bad(name: &str, why: &str) -> CreateError {
    CreateError::Invalid(format!("{name}: {why}"))
}

fn u16le(b: &[u8]) -> Option<u16> {
    b.get(..2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}

fn u32le(b: &[u8]) -> Option<u32> {
    b.get(..4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

// ── zip ───────────────────────────────────────────────────────────────────────────────

/// One file in the package: its name and where its data lives.
struct ZipEntry {
    name: String,
    method: u16,
    comp_size: u64,
    data_off: u64,
}

/// The central directory: names, methods and data offsets. Lengths and offsets are
/// bounds-checked; zip64 and data descriptors are refused rather than guessed.
fn zip_index(bytes: &[u8]) -> Result<Vec<ZipEntry>, String> {
    if bytes.len() < 22 || !bytes.starts_with(b"PK\x03\x04") && !bytes.starts_with(b"PK\x05\x06") && !bytes.starts_with(b"PK\x07\x08") {
        return Err("not a zip package".into());
    }
    // The end-of-central-directory record sits within the last 64 KiB + 22 bytes.
    let tail_from = bytes.len().saturating_sub(65535 + 22);
    let tail = bytes.get(tail_from..).ok_or("not a zip package")?;
    let eocd = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail.get(i..i.saturating_add(4)) == Some(b"PK\x05\x06".as_slice()))
        .ok_or("not a zip package")?;
    let eocd = tail.get(eocd..).ok_or("not a zip package")?;
    let count = u16le(eocd.get(10..12).unwrap_or_default()).unwrap_or(0) as usize;
    let dir_size = u32le(eocd.get(12..16).unwrap_or_default()).unwrap_or(0) as u64;
    let dir_off = u32le(eocd.get(16..20).unwrap_or_default()).unwrap_or(0) as u64;
    if count == 0 || count > MAX_ENTRIES || count == 0xFFFF {
        return Err("not a Word document".into());
    }
    let dir_end = dir_off.checked_add(dir_size).ok_or("not a zip package")?;
    if dir_end > bytes.len() as u64 {
        return Err("not a zip package".into());
    }
    let mut entries = Vec::new();
    let mut at = dir_off;
    for _ in 0..count {
        let hend = (at as usize).checked_add(46).ok_or("not a zip package")?;
        let h = bytes.get(at as usize..hend).filter(|h| h.starts_with(b"PK\x01\x02")).ok_or("not a zip package")?;
        let method = u16le(h.get(10..12).unwrap_or_default()).unwrap_or(0xFFFF);
        let comp_size = u32le(h.get(20..24).unwrap_or_default()).unwrap_or(0) as u64;
        let name_len = u16le(h.get(28..30).unwrap_or_default()).unwrap_or(0) as u64;
        let extra_len = u16le(h.get(30..32).unwrap_or_default()).unwrap_or(0) as u64;
        let comment_len = u16le(h.get(32..34).unwrap_or_default()).unwrap_or(0) as u64;
        let local_off = u32le(h.get(42..46).unwrap_or_default()).unwrap_or(0) as u64;
        if name_len == 0 || name_len > MAX_ENTRY_BYTES || comp_size > MAX_ENTRY_BYTES {
            return Err("not a Word document".into());
        }
        let name_from = (at as usize).checked_add(46).ok_or("not a zip package")?;
        let name_end = name_from.checked_add(name_len as usize).ok_or("not a zip package")?;
        let name = bytes.get(name_from..name_end).ok_or("not a zip package")?;
        let name = std::str::from_utf8(name).map_err(|_| "not a Word document".to_string())?.to_string();
        let lh_end = (local_off as usize).checked_add(30).ok_or("not a zip package")?;
        let lh = bytes.get(local_off as usize..lh_end).filter(|h| h.starts_with(b"PK\x03\x04")).ok_or("not a zip package")?;
        let lh_name = u16le(lh.get(26..28).unwrap_or_default()).unwrap_or(0) as u64;
        let lh_extra = u16le(lh.get(28..30).unwrap_or_default()).unwrap_or(0) as u64;
        let data_off =
            (local_off as usize).checked_add(30).and_then(|o| o.checked_add(lh_name as usize)).and_then(|o| o.checked_add(lh_extra as usize));
        let Some(data_off) = data_off else { return Err("not a zip package".into()) };
        if (data_off as u64).checked_add(comp_size).is_none_or(|end| end > bytes.len() as u64) {
            return Err("not a zip package".into());
        }
        if !name.ends_with('/') {
            entries.push(ZipEntry { name, method, comp_size, data_off: data_off as u64 });
        }
        at = (at as usize)
            .checked_add(46)
            .and_then(|o| o.checked_add(name_len as usize))
            .and_then(|o| o.checked_add(extra_len as usize))
            .and_then(|o| o.checked_add(comment_len as usize))
            .ok_or("not a zip package")? as u64;
        if at > dir_end {
            return Err("not a zip package".into());
        }
    }
    Ok(entries)
}

/// The inflated bytes of one entry, capped.
fn zip_read(bytes: &[u8], entry: &ZipEntry, total: &mut u64) -> Result<Vec<u8>, String> {
    let from = entry.data_off as usize;
    let to = from.checked_add(entry.comp_size as usize).ok_or("not a zip package")?;
    let data = bytes.get(from..to).ok_or("not a zip package")?;
    let out = match entry.method {
        0 => data.to_vec(),
        8 => {
            let dec = flate2::read::DeflateDecoder::new(data);
            let mut out = Vec::new();
            dec.take(MAX_ENTRY_BYTES.saturating_add(1)).read_to_end(&mut out).map_err(|_| "not a Word document".to_string())?;
            if out.len() as u64 > MAX_ENTRY_BYTES {
                return Err("not a Word document".into());
            }
            out
        }
        _ => return Err("not a Word document".into()),
    };
    *total = total.checked_add(out.len() as u64).ok_or("not a Word document".to_string())?;
    if *total > MAX_TOTAL_BYTES {
        return Err("not a Word document".into());
    }
    Ok(out)
}

// ── xml ───────────────────────────────────────────────────────────────────────────────

/// The local part of a possibly prefixed tag or attribute name.
fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => name.get(i + 1..).unwrap_or(name),
        None => name,
    }
}

/// Predefined and numeric character references; anything else is kept as written.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let Some(semi) = tail.find(';') else {
            out.push('&');
            rest = tail;
            continue;
        };
        let (ent, after) = (&tail[..semi], &tail[semi + 1..]);
        if ent.len() > 32 {
            out.push('&');
            rest = tail;
            continue;
        }
        match ent {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if ent.starts_with('#') => match parse_char_ref(&ent[1..]) {
                Some(c) => out.push(c),
                None => {
                    out.push('&');
                    out.push_str(ent);
                    out.push(';');
                }
            },
            _ => {
                out.push('&');
                out.push_str(ent);
                out.push(';');
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn parse_char_ref(ent: &str) -> Option<char> {
    let n = if let Some(hex) = ent.strip_prefix('x').or_else(|| ent.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        ent.parse::<u32>().ok()?
    };
    if n == 0 || (0xD800..0xE000).contains(&n) || n > 0x10FFFF {
        return None;
    }
    char::from_u32(n)
}

/// A streaming reader over one XML part.
struct Xml<'a> {
    reader: quick_xml::Reader<&'a [u8]>,
    buf: Vec<u8>,
}

impl<'a> Xml<'a> {
    fn new(mut xml: &'a [u8]) -> Self {
        if let Some(stripped) = xml.strip_prefix(b"\xef\xbb\xbf".as_slice()) {
            xml = stripped;
        }
        Self { reader: quick_xml::Reader::from_str(std::str::from_utf8(xml).unwrap_or_default()), buf: Vec::new() }
    }

    fn next(&mut self) -> Result<Option<quick_xml::events::Event<'_>>, String> {
        self.buf.clear();
        match self.reader.read_event_into(&mut self.buf) {
            Ok(quick_xml::events::Event::Eof) => Ok(None),
            Ok(e) => Ok(Some(e)),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The value of attribute `name` (matched by local name), unescaped.
fn attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    for a in e.attributes().filter_map(|a| a.ok()) {
        if local(a.key.0) == name {
            let v = std::str::from_utf8(a.value.as_ref()).ok()?;
            return Some(unescape(v));
        }
    }
    None
}

fn num_attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<f64> {
    attr(e, name)?.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Twentieths of a point to points.
fn twips(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<f64> {
    num_attr(e, name).map(|v| v / 20.0).filter(|v| v.is_finite() && *v >= 0.0)
}

/// Half-points to points, clamped to a sane font size.
fn half_pt(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<f64> {
    num_attr(e, name).map(|v| (v / 2.0).clamp(4.0, 72.0))
}

/// A 6-digit hex colour to 0–1 components; `auto` and anything else is default (black).
fn hex_color(v: &str) -> Option<(f64, f64, f64)> {
    let v = v.trim();
    let d = v.as_bytes();
    if d.len() != 6 || v.eq_ignore_ascii_case("auto") {
        return None;
    }
    let hex = |i: usize| u32::from_str_radix(std::str::from_utf8(d.get(i..i.saturating_add(2))?).ok()?, 16).ok()? as f64 / 255.0;
    Some((hex(0), hex(2), hex(4)))
}

/// An on/off run property: present means on unless `val` says false/0/off.
fn flag(e: &quick_xml::events::BytesStart<'_>) -> bool {
    match attr(e, b"val") {
        None => true,
        Some(v) => !matches!(v.to_ascii_lowercase().as_str(), "false" | "0" | "off" | "none"),
    }
}

// ── document model ────────────────────────────────────────────────────────────────────

/// Numbering series: numId to its top level's format (`bullet`, `decimal`, …).
#[derive(Default)]
struct Numbering {
    fmt: HashMap<u32, String>,
}

/// `w:numbering.xml`: numId to abstractNumId, abstractNumId to the top level's format.
fn parse_numbering(xml: &[u8]) -> Numbering {
    let mut numbering = Numbering::default();
    let mut abstracts: HashMap<u32, String> = HashMap::new();
    let mut nums: Vec<(u32, u32)> = Vec::new();
    let mut xml = Xml::new(xml);
    let mut current_abstract: Option<u32> = None;
    let mut current_level: Option<u32> = None;
    let mut current_fmt: Option<String> = None;
    while let Ok(Some(e)) = xml.next() {
        match e {
            quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => match local(e.name().0) {
                b"abstractNum" => {
                    current_abstract = attr(&e, b"abstractNumId").and_then(|v| v.parse::<u32>().ok());
                    current_fmt = None;
                }
                b"lvl" => {
                    if current_level.is_none() {
                        current_level = attr(&e, b"ilvl").and_then(|v| v.parse::<u32>().ok());
                    }
                }
                b"numFmt" => {
                    if current_level.is_some_and(|l| l == 0) && current_fmt.is_none() {
                        current_fmt = attr(&e, b"val");
                    }
                }
                b"num" => {
                    if let (Some(id), Some(ab)) =
                        (attr(&e, b"numId").and_then(|v| v.parse::<u32>().ok()), attr(&e, b"abstractNumId").and_then(|v| v.parse::<u32>().ok()))
                    {
                        nums.push((id, ab));
                    }
                }
                _ => {}
            },
            quick_xml::events::Event::End(end) => match local(end.name().0) {
                b"abstractNum" => {
                    if let (Some(id), Some(fmt)) = (current_abstract, current_fmt) {
                        abstracts.insert(id, fmt);
                    }
                    current_abstract = None;
                    current_level = None;
                    current_fmt = None;
                }
                b"lvl" => current_level = None,
                _ => {}
            },
            _ => {}
        }
        if abstracts.len() + nums.len() > 20_000 {
            break;
        }
    }
    for (id, ab) in nums {
        numbering.fmt.insert(id, abstracts.get(&ab).cloned().unwrap_or_else(|| "decimal".to_string()));
        if numbering.fmt.len() > 20_000 {
            break;
        }
    }
    numbering
}

/// Run properties being gathered inside `w:rPr`.
#[derive(Clone, Debug, Default)]
struct RunStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    size: Option<f64>,
    color: Option<(f64, f64, f64)>,
    superscript: bool,
    subscript: bool,
}

/// Paragraph properties being gathered inside `w:pPr`.
#[derive(Clone, Debug)]
struct ParaStyle {
    size: f64,
    bold: bool,
    align: Align,
    before: f64,
    after: f64,
    indent: f64,
    page_break: bool,
    list: Option<ListRef>,
}

impl Default for ParaStyle {
    fn default() -> Self {
        Self { size: 11.0, bold: false, align: Align::Left, before: 0.0, after: 0.0, indent: 0.0, page_break: false, list: None }
    }
}

/// Heading sizes by level (Word's defaults, in points).
fn heading_size(level: u32) -> Option<f64> {
    match level {
        1 => Some(16.0),
        2 => Some(14.0),
        3 => Some(12.0),
        4..=9 => Some(11.0),
        _ => None,
    }
}

/// A style id like `Heading1` or `heading 2` to (size, bold).
fn named_style(name: &str) -> Option<(f64, bool)> {
    let flat: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    if flat == "title" {
        return Some((26.0, true));
    }
    let level = flat.strip_prefix("heading")?.parse::<u32>().ok()?;
    heading_size(level).map(|size| (size, true))
}

struct Parser<'a> {
    name: &'a str,
    xml: Xml<'a>,
    numbering: Numbering,
    /// numId to the next ordered number.
    counters: HashMap<u32, usize>,
    rels: HashMap<String, String>,
    media: HashMap<String, Vec<u8>>,
    images: Vec<Image>,
    inlines: usize,
    paras: usize,
}

/// Push buffered run text as one inline, resolving the run's size and baseline.
fn flush_run(current: &mut String, out: &mut Vec<Inline>, style: &RunStyle, base_size: f64, base_bold: bool) {
    if current.is_empty() {
        return;
    }
    let size = style.size.unwrap_or(base_size);
    let (dy, size) = if style.superscript {
        (size * 0.35, size * 0.65)
    } else if style.subscript {
        (-size * 0.2, size * 0.65)
    } else {
        (0.0, size)
    };
    out.push(Inline::Text(Run {
        text: std::mem::take(current),
        bold: style.bold || base_bold,
        italic: style.italic,
        underline: style.underline,
        size,
        color: style.color.unwrap_or((0.0, 0.0, 0.0)),
        dy,
    }));
}

impl<'a> Parser<'a> {
    fn err(&self, why: &str) -> CreateError {
        bad(self.name, why)
    }

    /// The next event, with XML errors labelled by file. (A `map_err` closure here would
    /// borrow `self` while the returned event borrows the buffer.)
    fn next(&mut self) -> Result<Option<quick_xml::events::Event<'_>>, CreateError> {
        let name = self.name;
        self.xml.next().map_err(|e| bad(name, &e.to_string()))
    }

    /// Skip the rest of the current element (the opening tag was just read).
    fn skip(&mut self) -> Result<(), CreateError> {
        let mut depth = 1usize;
        while let Some(e) = self.next()? {
            match e {
                quick_xml::events::Event::Start(_) => depth = depth.saturating_add(1),
                quick_xml::events::Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {}
            }
            if depth > 10_000 {
                return Err(self.err("the Word document is too deeply nested"));
            }
        }
        Err(self.err("the Word document ends mid-element"))
    }

    fn text_of(&mut self, preserve: bool) -> Result<String, CreateError> {
        let name = self.name;
        let mut out = String::new();
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            match e {
                quick_xml::events::Event::Text(t) => {
                    let bytes = t.into_inner();
                    let s = std::str::from_utf8(&bytes).map_err(|_| bad(name, "the Word document is not valid text"))?;
                    out.push_str(&unescape(s));
                }
                quick_xml::events::Event::End(end) if local(end.name().0) == b"t" => break,
                quick_xml::events::Event::Start(_) => self.skip()?,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        if preserve { Ok(out) } else { Ok(out.trim().to_string()) }
    }

    fn run_style(&mut self) -> Result<RunStyle, CreateError> {
        let mut style = RunStyle::default();
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => match local(e.name().0) {
                    b"b" => style.bold = flag(&e),
                    b"bCs" | b"iCs" => {}
                    b"i" => style.italic = flag(&e),
                    b"u" => style.underline = flag(&e),
                    b"sz" => {
                        if let Some(size) = half_pt(&e, b"val") {
                            style.size = Some(size);
                        }
                    }
                    b"color" => {
                        if let Some(v) = attr(&e, b"val") {
                            style.color = hex_color(&v);
                        }
                    }
                    b"vertAlign" => match attr(&e, b"val").as_deref() {
                        Some("superscript") => style.superscript = true,
                        Some("subscript") => style.subscript = true,
                        _ => {}
                    },
                    _ => {
                        if start {
                            // Unknown run property with children: skip it.
                            self.skip()?;
                        }
                    }
                },
                quick_xml::events::Event::End(end) if local(end.name().0) == b"rPr" => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(style)
    }

    fn check_inline(&mut self) -> Result<(), CreateError> {
        self.inlines = self.inlines.saturating_add(1);
        if self.inlines > MAX_INLINES {
            return Err(self.err("the Word document has too much text"));
        }
        Ok(())
    }

    fn check_para(&mut self) -> Result<(), CreateError> {
        self.paras = self.paras.saturating_add(1);
        if self.paras > MAX_PARAS {
            return Err(self.err("the Word document has too many paragraphs"));
        }
        Ok(())
    }

    /// One `w:r` run: its text (and breaks, tabs, images) with the run's style.
    fn parse_run(&mut self, base_size: f64, base_bold: bool) -> Result<Vec<Inline>, CreateError> {
        let mut style = RunStyle::default();
        let mut out = Vec::new();
        let mut current = String::new();
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                    let tag = local(e.name().0);
                    // `w:t` is the only element with text; everything else is handled by tag.
                    if tag == b"t" {
                        let preserve = attr(&e, b"space").is_some_and(|v| v == "preserve");
                        let text = self.text_of(preserve)?;
                        self.check_inline()?;
                        current.push_str(&text);
                    } else if tag == b"rPr" {
                        if start {
                            style = self.run_style()?;
                        } else {
                            style = RunStyle::default();
                        }
                    } else if tag == b"br" {
                        let page = attr(&e, b"type").as_deref() == Some("page");
                        flush_run(&mut current, &mut out, &style, base_size, base_bold);
                        self.check_inline()?;
                        out.push(if page { Inline::PageBreak } else { Inline::Break });
                    } else if tag == b"tab" {
                        current.push_str("    ");
                    } else if tag == b"drawing" || tag == b"pict" {
                        flush_run(&mut current, &mut out, &style, base_size, base_bold);
                        if start {
                            if let Some(image) = self.parse_drawing()? {
                                self.check_inline()?;
                                out.push(Inline::Image(image));
                            }
                        }
                    } else if start {
                        // Field markers, proofing, deleted text: skipped with their subtree.
                        self.skip()?;
                    }
                }
                quick_xml::events::Event::End(end) if local(end.name().0) == b"r" => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        flush_run(&mut current, &mut out, &style, base_size, base_bold);
        Ok(out)
    }

    /// The runs inside `w:hyperlink` or `w:ins` (same content as a paragraph body).
    fn parse_rich(&mut self, end: &[u8], base_size: f64, base_bold: bool) -> Result<Vec<Inline>, CreateError> {
        let mut out = Vec::new();
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                    let tag = local(e.name().0);
                    if tag == b"r" {
                        if start {
                            out.extend(self.parse_run(base_size, base_bold)?);
                        }
                    } else if start {
                        self.skip()?;
                    }
                }
                quick_xml::events::Event::End(e) if local(e.name().0) == end => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(out)
    }

    fn para_style(&mut self) -> Result<ParaStyle, CreateError> {
        let mut style = ParaStyle::default();
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => match local(e.name().0) {
                    b"pStyle" => {
                        if let Some(v) = attr(&e, b"val") {
                            if let Some((size, bold)) = named_style(&v) {
                                style.size = size;
                                style.bold = bold;
                                style.before = 12.0;
                                style.after = 4.0;
                            }
                        }
                    }
                    b"jc" => {
                        style.align = match attr(&e, b"val").as_deref() {
                            Some("center") => Align::Center,
                            Some("right") | Some("end") => Align::Right,
                            _ => Align::Left,
                        };
                    }
                    b"spacing" => {
                        if let Some(v) = twips(&e, b"before") {
                            style.before = v.min(144.0);
                        }
                        if let Some(v) = twips(&e, b"after") {
                            style.after = v.min(144.0);
                        }
                    }
                    b"ind" => {
                        if let Some(v) = twips(&e, b"left") {
                            style.indent = v.min(432.0);
                        }
                    }
                    b"numPr" => {
                        if start {
                            style.list = self.parse_num_pr()?;
                        }
                    }
                    b"pageBreakBefore" => style.page_break = flag(&e),
                    _ => {
                        if start {
                            self.skip()?;
                        }
                    }
                },
                quick_xml::events::Event::End(end) if local(end.name().0) == b"pPr" => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(style)
    }

    fn parse_num_pr(&mut self) -> Result<Option<ListRef>, CreateError> {
        let (mut num, mut level) = (None, 0u8);
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => match local(e.name().0) {
                    b"numId" => num = attr(&e, b"val").and_then(|v| v.parse::<u32>().ok()),
                    b"ilvl" => level = attr(&e, b"val").and_then(|v| v.parse::<u8>().ok()).unwrap_or(0).min(8),
                    _ => {
                        if start {
                            self.skip()?;
                        }
                    }
                },
                quick_xml::events::Event::End(end) if local(end.name().0) == b"numPr" => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(num.map(|num| ListRef { num, level }))
    }

    /// One `w:p` paragraph (the opening tag was just read; empty for `<w:p/>`).
    fn parse_para(&mut self, empty: bool) -> Result<Para, CreateError> {
        self.check_para()?;
        let mut style = ParaStyle::default();
        let mut content = Vec::new();
        if !empty {
            loop {
                let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
                let start = matches!(e, quick_xml::events::Event::Start(_));
                match e {
                    quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                        let tag = local(e.name().0);
                        if tag == b"pPr" {
                            if start {
                                style = self.para_style()?;
                            }
                        } else if tag == b"r" {
                            if start {
                                content.extend(self.parse_run(style.size, style.bold)?);
                            }
                        } else if tag == b"hyperlink" || tag == b"ins" || tag == b"sdtContent" {
                            if start {
                                // `tag` borrows the event buffer, so match to a static end tag.
                                let end: &[u8] = if tag == b"hyperlink" {
                                    b"hyperlink"
                                } else if tag == b"ins" {
                                    b"ins"
                                } else {
                                    b"sdtContent"
                                };
                                content.extend(self.parse_rich(end, style.size, style.bold)?);
                            }
                        } else if tag == b"br" {
                            self.check_inline()?;
                            content.push(Inline::Break);
                        } else if start {
                            // Deletions, bookmarks, proofing and section marks: skipped.
                            self.skip()?;
                        }
                    }
                    quick_xml::events::Event::End(end) if local(end.name().0) == b"p" => break,
                    quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                    _ => {}
                }
            }
        }
        Ok(Para {
            size: style.size,
            align: style.align,
            before: style.before,
            after: style.after,
            indent: style.indent,
            page_break: style.page_break,
            list: style.list,
            content,
        })
    }

    /// An inline or anchored picture: the embedded image id plus its extent in EMU.
    /// The opening `w:drawing`/`w:pict` tag was just read.
    fn parse_drawing(&mut self) -> Result<Option<usize>, CreateError> {
        let (mut embed, mut cx, mut cy) = (None, None, None);
        let mut depth = 1usize;
        while let Some(e) = self.next()? {
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                    if start {
                        depth = depth.saturating_add(1);
                    }
                    match local(e.name().0) {
                        b"blip" => {
                            if embed.is_none() {
                                embed = attr(&e, b"embed");
                            }
                        }
                        // Legacy VML pictures (`w:pict`) reference the image by `r:id`.
                        b"imagedata" => {
                            if embed.is_none() {
                                embed = attr(&e, b"id");
                            }
                        }
                        b"extent" => {
                            cx = cx.or_else(|| attr(&e, b"cx").and_then(|v| v.parse::<f64>().ok()));
                            cy = cy.or_else(|| attr(&e, b"cy").and_then(|v| v.parse::<f64>().ok()));
                        }
                        _ => {}
                    }
                    if depth > 10_000 {
                        return Err(self.err("the Word document is too deeply nested"));
                    }
                }
                quick_xml::events::Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        let (Some(id), Some(cx), Some(cy)) = (embed, cx, cy) else { return Ok(None) };
        if !(cx.is_finite() && cy.is_finite() && cx > 0.0 && cy > 0.0) {
            return Ok(None);
        }
        let Some(target) = self.rels.get(&id).cloned() else { return Ok(None) };
        let Some(data) = self.media.get(&target).cloned() else { return Ok(None) };
        if self.images.len() >= MAX_MEDIA_FILES {
            return Ok(None);
        }
        self.images.push(Image { name: target, bytes: data, w_emu: cx, h_emu: cy });
        Ok(Some(self.images.len().saturating_sub(1)))
    }

    /// One table row: cells of paragraphs. Nested tables are flattened to text.
    fn parse_row(&mut self) -> Result<Vec<Cell>, CreateError> {
        let mut row = Vec::new();
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                    let tag = local(e.name().0);
                    if tag == b"tc" {
                        if start {
                            row.push(self.parse_cell()?);
                        } else {
                            row.push(Cell::default());
                        }
                    } else if start {
                        self.skip()?;
                    }
                    if row.len() > MAX_TABLE_COLS {
                        return Err(self.err("the Word table has too many columns"));
                    }
                }
                quick_xml::events::Event::End(end) if local(end.name().0) == b"tr" => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(row)
    }

    /// One table cell: paragraphs, with nested tables flattened between them.
    fn parse_cell(&mut self) -> Result<Cell, CreateError> {
        let mut cell = Cell::default();
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                    let tag = local(e.name().0);
                    if tag == b"p" {
                        cell.paras.push(self.parse_para(!start)?);
                    } else if tag == b"tbl" {
                        if start {
                            // Nested table: its text joins the cell, separated from the rest.
                            for para in self.flatten_table()? {
                                cell.paras.push(para);
                            }
                        }
                    } else if start {
                        self.skip()?;
                    }
                }
                quick_xml::events::Event::End(end) if local(end.name().0) == b"tc" => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(cell)
    }

    /// A nested table as plain paragraphs (one per row, cells joined with " | ").
    fn flatten_table(&mut self) -> Result<Vec<Para>, CreateError> {
        let table = self.parse_table()?;
        let mut paras = Vec::new();
        for row in table.rows {
            let mut texts = Vec::new();
            for cell in row {
                let mut cell_text = String::new();
                for para in cell.paras {
                    for inline in para.content {
                        if let Inline::Text(run) = inline {
                            if !cell_text.is_empty() && !cell_text.ends_with(' ') {
                                cell_text.push(' ');
                            }
                            cell_text.push_str(&run.text);
                        }
                    }
                }
                texts.push(cell_text.trim().to_string());
            }
            let line = texts.join(" | ");
            if !line.is_empty() {
                self.check_para()?;
                paras.push(Para {
                    size: 11.0,
                    align: Align::Left,
                    before: 0.0,
                    after: 0.0,
                    indent: 0.0,
                    page_break: false,
                    list: None,
                    content: vec![Inline::Text(Run { text: line, size: 11.0, ..Run::default() })],
                });
            }
        }
        Ok(paras)
    }

    /// One `w:tbl` table (the opening tag was just read).
    fn parse_table(&mut self) -> Result<Table, CreateError> {
        let mut widths: Vec<f64> = Vec::new();
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        let mut in_grid = false;
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                    let tag = local(e.name().0);
                    if tag == b"tblGrid" {
                        in_grid = start;
                    } else if tag == b"gridCol" && in_grid {
                        if let Some(w) = attr(&e, b"w").and_then(|v| v.parse::<f64>().ok()).map(|v| v / 20.0) {
                            if w.is_finite() && w > 0.0 && widths.len() < MAX_TABLE_COLS {
                                widths.push(w);
                            }
                        }
                    } else if tag == b"tr" {
                        if start {
                            rows.push(self.parse_row()?);
                        }
                        if rows.len() > MAX_TABLE_ROWS {
                            return Err(self.err("the Word table has too many rows"));
                        }
                    } else if start {
                        self.skip()?;
                    }
                }
                quick_xml::events::Event::End(end) => match local(end.name().0) {
                    b"tbl" => break,
                    b"tblGrid" => in_grid = false,
                    _ => {}
                },
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(Table { widths, rows })
    }

    /// Relationships: drawing id to `word/`-relative target (best-effort; pictures
    /// simply drop out when the part is missing or malformed).
    fn parse_rels(xml: &[u8]) -> HashMap<String, String> {
        let mut rels = HashMap::new();
        let mut x = Xml::new(xml);
        let mut skip = 0usize;
        while let Ok(Some(e)) = x.next() {
            if skip > 0 {
                match e {
                    quick_xml::events::Event::Start(_) => skip = skip.saturating_add(1),
                    quick_xml::events::Event::End(_) => skip = skip.saturating_sub(1),
                    _ => {}
                }
                if skip > 10_000 {
                    break;
                }
                continue;
            }
            let start = matches!(e, quick_xml::events::Event::Start(_));
            if let quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) = e {
                if local(e.name().0) == b"Relationship" {
                    if let (Some(id), Some(target)) = (attr(&e, b"Id"), attr(&e, b"Target")) {
                        if target.starts_with("media/") && rels.len() < MAX_MEDIA_FILES {
                            rels.insert(id, target);
                        }
                    }
                }
                if start {
                    skip = 1;
                }
            }
        }
        rels
    }

    /// A section's page size and margins in points; the last section seen wins.
    fn parse_sect_pr(&mut self, doc: &mut WordDoc) -> Result<(), CreateError> {
        loop {
            let Some(e) = self.next()? else { return Err(self.err("the Word document ends mid-element")) };
            let start = matches!(e, quick_xml::events::Event::Start(_));
            match e {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => match local(e.name().0) {
                    b"pgSz" => {
                        let w = attr(&e, b"w").and_then(|v| v.parse::<f64>().ok()).map(|v| v / 20.0);
                        let h = attr(&e, b"h").and_then(|v| v.parse::<f64>().ok()).map(|v| v / 20.0);
                        if let (Some(w), Some(h)) = (w, h) {
                            if w.is_finite() && h.is_finite() && (72.0..=MAX_SIDE).contains(&w) && (72.0..=MAX_SIDE).contains(&h) {
                                doc.page = (w, h);
                            }
                        }
                    }
                    b"pgMar" => {
                        let m = |n: &[u8]| attr(&e, n).and_then(|v| v.parse::<f64>().ok()).map(|v| (v / 20.0).clamp(0.0, 288.0));
                        if let (Some(t), Some(b), Some(l), Some(r)) = (m(b"top"), m(b"bottom"), m(b"left"), m(b"right")) {
                            doc.margins = (t, b, l, r);
                        }
                    }
                    _ => {
                        if start {
                            self.skip()?;
                        }
                    }
                },
                quick_xml::events::Event::End(end) if local(end.name().0) == b"sectPr" => break,
                quick_xml::events::Event::Eof => return Err(self.err("the Word document ends mid-element")),
                _ => {}
            }
        }
        Ok(())
    }
}

/// The document body: paragraphs and tables in order.
fn parse_body(parser: &mut Parser<'_>, doc: &mut WordDoc) -> Result<(), CreateError> {
    parse_blocks(parser, doc, None)
}

/// Blocks until end-of-input (`None`, the body) or the matching end tag: structured
/// document tags (`w:sdt`) and their content wrappers are entered, not skipped.
fn parse_blocks(parser: &mut Parser<'_>, doc: &mut WordDoc, end: Option<&[u8]>) -> Result<(), CreateError> {
    loop {
        let Some(e) = parser.next()? else {
            if end.is_none() {
                break;
            }
            return Err(parser.err("the Word document ends mid-element"));
        };
        let start = matches!(e, quick_xml::events::Event::Start(_));
        match e {
            quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => {
                let tag = local(e.name().0);
                if tag == b"p" {
                    doc.blocks.push(Block::Para(parser.parse_para(!start)?));
                } else if tag == b"tbl" {
                    if start {
                        doc.blocks.push(Block::Table(parser.parse_table()?));
                    }
                } else if tag == b"sectPr" {
                    if start {
                        parser.parse_sect_pr(doc)?;
                    }
                } else if tag == b"sdt" {
                    if start {
                        parse_blocks(parser, doc, Some(b"sdt"))?;
                    }
                } else if start && tag != b"body" && tag != b"document" && tag != b"sdtContent" {
                    parser.skip()?;
                }
            }
            quick_xml::events::Event::End(e) if end.is_some_and(|t| local(e.name().0) == t) => break,
            _ => {}
        }
    }
    Ok(())
}

fn letters(mut n: usize, upper: bool) -> String {
    let mut out = String::new();
    while n > 0 {
        let r = (n.saturating_sub(1)) % 26;
        let c = if upper { b'A' + r as u8 } else { b'a' + r as u8 };
        out.insert(0, c as char);
        n = (n.saturating_sub(1)) / 26;
    }
    out
}

fn roman(mut n: usize, upper: bool) -> String {
    const TABLE: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v && out.len() < 32 {
            out.push_str(s);
            n -= v;
        }
    }
    if upper { out.to_ascii_uppercase() } else { out }
}

// ── layout ────────────────────────────────────────────────────────────────────────────

/// One laid-out fragment: text in one font, size and colour.
struct Frag {
    text: String,
    bold: bool,
    italic: bool,
    underline: bool,
    size: f64,
    color: (f64, f64, f64),
    dy: f64,
    width: f64,
}

impl Frag {
    fn from_run(run: &Run) -> Self {
        Self {
            text: run.text.clone(),
            bold: run.bold,
            italic: run.italic,
            underline: run.underline,
            size: run.size,
            color: run.color,
            dy: run.dy,
            width: helvetica_width(&run.text, run.size),
        }
    }
}

/// One line: fragments, an optional image (which always stands alone), or a page break.
struct Line {
    frags: Vec<Frag>,
    width: f64,
    height: f64,
    image: Option<usize>,
    page_break: bool,
}

fn text_line(frags: Vec<Frag>, width: f64) -> Line {
    let height = frags.iter().map(|f: &Frag| f.size * 1.25 + f.dy.max(0.0)).fold(0.0f64, f64::max);
    Line { frags, width, height: height.max(1.0), image: None, page_break: false }
}

/// Split paragraph inlines into lines within `avail` points.
fn wrap_para(para: &Para, avail: f64, list_prefix: Option<&str>) -> Vec<Line> {
    let mut state = Wrap { lines: Vec::new(), frags: Vec::new(), width: 0.0, avail };
    if let Some(prefix) = list_prefix {
        let pre = Run { text: prefix.to_string(), size: para.size, ..Run::default() };
        for piece in prefix.split_inclusive(' ') {
            if !piece.is_empty() {
                state.word(piece, &pre);
            }
        }
    }
    for inline in &para.content {
        match inline {
            Inline::Text(run) => {
                // Tabs were expanded to spaces while parsing; newlines cannot appear here.
                for piece in run.text.split_inclusive(' ') {
                    if !piece.is_empty() {
                        state.word(piece, run);
                    }
                }
            }
            Inline::Break => state.push_line(),
            Inline::PageBreak => {
                state.push_line();
                state.lines.push(Line { frags: Vec::new(), width: 0.0, height: 0.0, image: None, page_break: true });
            }
            Inline::Image(idx) => {
                state.push_line();
                state.lines.push(Line { frags: Vec::new(), width: 0.0, height: 0.0, image: Some(*idx), page_break: false });
            }
        }
    }
    state.push_line();
    state.lines
}

/// Greedy word wrapper over styled runs.
struct Wrap {
    lines: Vec<Line>,
    frags: Vec<Frag>,
    width: f64,
    avail: f64,
}

impl Wrap {
    fn push_line(&mut self) {
        if self.frags.is_empty() {
            return;
        }
        self.lines.push(text_line(std::mem::take(&mut self.frags), self.width));
        self.width = 0.0;
    }

    fn word(&mut self, text: &str, run: &Run) {
        let mut frag = Frag::from_run(run);
        frag.text = text.to_string();
        frag.width = helvetica_width(text, run.size);
        if self.width + frag.width <= self.avail || self.frags.is_empty() {
            self.width += frag.width;
            self.frags.push(frag);
            return;
        }
        self.push_line();
        // An overlong word breaks by character.
        if frag.width > self.avail {
            let mut part = String::new();
            let mut part_w = 0.0;
            for c in text.chars() {
                let mut cbuf = [0u8; 4];
                let cw = helvetica_width(c.encode_utf8(&mut cbuf), run.size);
                if part_w + cw > self.avail && !part.is_empty() {
                    self.char_part(run, std::mem::take(&mut part), part_w);
                    part_w = 0.0;
                }
                part_w += cw;
                part.push(c);
            }
            if !part.is_empty() {
                self.char_part(run, part, part_w);
            }
            return;
        }
        self.width = frag.width;
        self.frags.push(frag);
    }

    fn char_part(&mut self, run: &Run, text: String, width: f64) {
        let mut frag = Frag::from_run(run);
        frag.text = text;
        frag.width = width;
        self.width = width;
        self.frags.push(frag);
        self.push_line();
    }
}

/// Page assembly: blocks to finished content streams with their resources.
struct Pages {
    fonts: ObjRef,
    page: (f64, f64),
    margins: (f64, f64, f64, f64),
    current: Vec<u8>,
    images: Vec<(String, ObjRef)>,
    done: Vec<(Vec<u8>, Vec<(String, ObjRef)>)>,
    y: f64,
}

impl Pages {
    fn new(doc: &mut Document, page: (f64, f64), margins: (f64, f64, f64, f64)) -> Self {
        let mut fonts = Dict::new();
        for (key, face) in [("F1", "Helvetica"), ("F2", "Helvetica-Bold"), ("F3", "Helvetica-Oblique"), ("F4", "Helvetica-BoldOblique")] {
            let mut font = Dict::new();
            font.set(b"Type".to_vec(), Object::name("Font"));
            font.set(b"Subtype".to_vec(), Object::name("Type1"));
            font.set(b"BaseFont".to_vec(), Object::name(face));
            font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
            let r = doc.add(font);
            fonts.set(key.as_bytes().to_vec(), Object::Ref(r));
        }
        let fr = doc.add(fonts);
        let mut fonts = Dict::new();
        fonts.set(b"Font".to_vec(), Object::Ref(fr));
        let fonts = doc.add(fonts);
        Self { fonts, page, margins, current: Vec::new(), images: Vec::new(), done: Vec::new(), y: page.1 - margins.0 }
    }

    fn bottom(&self) -> f64 {
        self.margins.1
    }

    fn avail_width(&self) -> f64 {
        (self.page.0 - self.margins.2 - self.margins.3).max(36.0)
    }

    fn finish_page(&mut self) {
        self.done.push((std::mem::take(&mut self.current), std::mem::take(&mut self.images)));
        self.y = self.page.1 - self.margins.0;
    }

    fn need(&mut self, height: f64) {
        if self.y - height < self.bottom() && self.y > self.bottom() {
            self.finish_page();
        }
    }

    fn image_key(&mut self, doc: &mut Document, name: &str, bytes: &[u8]) -> Option<String> {
        if let Some(i) = self.images.iter().position(|(n, _)| n == name) {
            return Some(format!("Im{i}"));
        }
        match super::image_xobject(doc, name, bytes) {
            Ok((r, _)) => {
                self.images.push((name.to_string(), r));
                Some(format!("Im{}", self.images.len().saturating_sub(1)))
            }
            Err(_) => None,
        }
    }
}

fn font_key(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "F4",
        (true, false) => "F2",
        (false, true) => "F3",
        (false, false) => "F1",
    }
}

fn push_text(out: &mut Vec<u8>, text: &str) {
    out.extend(literal(&win_ansi(text)));
    out.extend_from_slice(b" Tj");
}

/// Draw one laid-out line at `(x, top)`; returns the baseline used.
fn draw_line(out: &mut Vec<u8>, line: &Line, x: f64, top: f64) -> f64 {
    let base = top - line.frags.iter().map(|f| f.size).fold(0.0f64, f64::max);
    out.extend_from_slice(b"BT ");
    let mut fx = x;
    for frag in &line.frags {
        let key = font_key(frag.bold, frag.italic);
        out.extend_from_slice(format!("/{key} {:.3} Tf {:.3} {:.3} {:.3} rg ", frag.size, frag.color.0, frag.color.1, frag.color.2).as_bytes());
        out.extend_from_slice(format!("1 0 0 1 {:.3} {:.3} Tm ", fx, base + frag.dy).as_bytes());
        push_text(out, &frag.text);
        fx += frag.width;
    }
    out.extend_from_slice(b" ET\n");
    // Underlines: one stroke per underlined fragment.
    let mut ux = x;
    for frag in &line.frags {
        if frag.underline {
            let y = base + frag.dy - frag.size * 0.08;
            out.extend_from_slice(
                format!(
                    "{:.3} {:.3} {:.3} RG {:.3} w {:.3} {:.3} m {:.3} {:.3} l S\n",
                    frag.color.0,
                    frag.color.1,
                    frag.color.2,
                    (frag.size / 12.0).max(0.4),
                    ux,
                    y,
                    ux + frag.width,
                    y
                )
                .as_bytes(),
            );
        }
        ux += frag.width;
    }
    base
}

/// The list prefix for one paragraph, advancing the series counter.
fn list_prefix(counters: &mut HashMap<u32, usize>, numbering: &Numbering, list: ListRef) -> String {
    let fmt = numbering.fmt.get(&list.num).map(String::as_str).unwrap_or("decimal");
    if fmt == "bullet" {
        return "• ".to_string();
    }
    let n = counters.get(&list.num).copied().unwrap_or(0).saturating_add(1).max(1);
    counters.insert(list.num, n);
    let body = match fmt {
        "upperLetter" => letters(n, true),
        "lowerLetter" => letters(n, false),
        "upperRoman" => roman(n, true),
        "lowerRoman" => roman(n, false),
        _ => n.to_string(),
    };
    format!("{body}. ")
}

struct Placer<'a> {
    pages: Pages,
    images: &'a [Image],
    numbering: &'a Numbering,
    counters: HashMap<u32, usize>,
}

impl Placer<'_> {
    fn left(&self) -> f64 {
        self.pages.margins.2
    }

    fn place_para(&mut self, doc: &mut Document, para: &Para) -> Result<(), CreateError> {
        if para.page_break {
            self.pages.finish_page();
        }
        // Lists hang their prefix in an 18-point zone per level.
        let indent = para.indent + para.list.map_or(0.0, |l| 18.0 * f64::from(l.level.saturating_add(1)));
        let prefix = para.list.map(|list| list_prefix(&mut self.counters, self.numbering, list));
        let lines = wrap_para(para, (self.pages.avail_width() - indent).max(36.0), prefix.as_deref());
        let x0 = self.left() + indent;
        if lines.is_empty() {
            // An empty paragraph still takes a line, as in Word.
            let height = (para.size * 1.25).max(1.0);
            self.pages.need(para.before + height + para.after);
            self.pages.y -= para.before + height + para.after;
            return Ok(());
        }
        if para.before > 0.0 {
            self.pages.need(para.before);
            self.pages.y -= para.before;
        }
        for line in &lines {
            if line.page_break {
                self.pages.finish_page();
                continue;
            }
            if let Some(idx) = line.image {
                self.place_image(doc, idx, x0, para.align);
                continue;
            }
            if line.frags.is_empty() {
                self.pages.need(line.height);
                self.pages.y -= line.height;
                continue;
            }
            self.pages.need(line.height);
            let x = match para.align {
                Align::Left => x0,
                Align::Center => x0 + (self.pages.avail_width() - indent - line.width).max(0.0) / 2.0,
                Align::Right => x0 + (self.pages.avail_width() - indent - line.width).max(0.0),
            };
            let top = self.pages.y;
            draw_line(&mut self.pages.current, line, x, top);
            self.pages.y -= line.height;
        }
        if para.after > 0.0 {
            self.pages.y -= para.after;
        }
        Ok(())
    }

    fn place_image(&mut self, doc: &mut Document, idx: usize, x0: f64, align: Align) {
        let Some(image) = self.images.get(idx) else { return };
        let (mut w, mut h) = (image.w_emu / 12700.0, image.h_emu / 12700.0);
        if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
            return;
        }
        let avail = self.pages.avail_width();
        let max_h = (self.pages.page.1 - self.pages.margins.0 - self.pages.bottom()).max(36.0);
        let k = (avail / w).min(max_h / h).min(1.0);
        if !k.is_finite() || k <= 0.0 {
            return;
        }
        w *= k;
        h *= k;
        self.pages.need(h);
        let x = match align {
            Align::Left => x0,
            Align::Center => x0 + (avail - w).max(0.0) / 2.0,
            Align::Right => x0 + (avail - w).max(0.0),
        };
        let Some(key) = self.pages.image_key(doc, &image.name.clone(), &image.bytes) else { return };
        let y = self.pages.y - h;
        self.pages.current.extend_from_slice(format!("q {w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm /{key} Do Q\n").as_bytes());
        self.pages.y -= h;
    }

    fn place_table(&mut self, table: &Table) -> Result<(), CreateError> {
        if table.rows.is_empty() {
            return Ok(());
        }
        let avail = self.pages.avail_width();
        let cols = table.rows.iter().map(Vec::len).fold(0, usize::max).clamp(1, MAX_TABLE_COLS);
        let mut widths = table.widths.clone();
        widths.resize(cols, 0.0);
        let specified: f64 = widths.iter().sum();
        if specified <= 0.0 {
            widths = vec![avail / cols as f64; cols];
        } else if specified > avail {
            let k = avail / specified;
            for w in &mut widths {
                *w *= k;
            }
        }
        const PAD: f64 = 5.0;
        for row in &table.rows {
            // Every cell's lines first: the row is as tall as its tallest cell.
            let mut cells: Vec<Vec<Line>> = Vec::new();
            let mut row_h = 0.0f64;
            for c in 0..cols {
                let mut cell_lines = Vec::new();
                if let Some(cell) = row.get(c) {
                    for para in &cell.paras {
                        cell_lines.extend(wrap_para(para, (widths[c] - 2.0 * PAD).max(36.0), None));
                    }
                }
                row_h = row_h.max(cell_lines.iter().map(|l: &Line| l.height).fold(0.0f64, f64::max) + 2.0 * PAD);
                cells.push(cell_lines);
            }
            row_h = row_h.max(16.0);
            self.pages.need(row_h);
            let top = self.pages.y;
            let mut x = self.left();
            for (c, cell_lines) in cells.iter().enumerate() {
                let w = widths[c];
                self.pages
                    .current
                    .extend_from_slice(format!("0.55 0.55 0.55 RG 0.5 w {:.3} {:.3} {:.3} {:.3} re S\n", x, top - row_h, w, row_h).as_bytes());
                let mut ly = top - PAD;
                for line in cell_lines {
                    if line.frags.is_empty() {
                        ly -= line.height;
                        continue;
                    }
                    draw_line(&mut self.pages.current, line, x + PAD, ly);
                    ly -= line.height;
                }
                x += w;
            }
            self.pages.y -= row_h;
        }
        Ok(())
    }
}

/// Convert a Word (.docx) file to a PDF document.
pub fn from_office(name: &str, bytes: &[u8]) -> Result<Document, CreateError> {
    let title = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let entries = zip_index(bytes).map_err(|e| bad(name, &e))?;
    let find = |suffix: &str| entries.iter().find(|e| e.name == suffix);
    let mut total = 0u64;
    let mut read = |entry: &ZipEntry| zip_read(bytes, entry, &mut total).map_err(|e| bad(name, &e));
    let Some(doc_entry) = find("word/document.xml") else { return Err(bad(name, "not a Word document: word/document.xml is missing")) };
    let document = read(doc_entry)?;
    let numbering = find("word/numbering.xml").and_then(|e| read(e).ok()).map(|xml| parse_numbering(&xml)).unwrap_or_default();
    // Relationships and media are best-effort: pictures simply drop out when unreadable.
    let mut rels = HashMap::new();
    let mut media: HashMap<String, Vec<u8>> = HashMap::new();
    if let Some(entry) = find("word/_rels/document.xml.rels") {
        if let Ok(xml) = read(entry) {
            rels = Parser::parse_rels(&xml);
            let media_names: Vec<String> =
                entries.iter().filter(|e| e.name.starts_with("word/media/")).take(MAX_MEDIA_FILES).map(|e| e.name.clone()).collect();
            for m in media_names {
                if let Some(entry) = entries.iter().find(|e| e.name == m) {
                    if let Ok(data) = read(entry) {
                        media.insert(m.trim_start_matches("word/").to_string(), data);
                    }
                }
            }
        }
    }
    let mut parser =
        Parser { name, xml: Xml::new(&document), numbering, counters: HashMap::new(), rels, media, images: Vec::new(), inlines: 0, paras: 0 };
    let mut doc = WordDoc { page: super::LETTER, margins: (72.0, 72.0, 72.0, 72.0), ..WordDoc::default() };
    parse_body(&mut parser, &mut doc).map_err(|e| match e {
        CreateError::Invalid(m) if m.starts_with(name) => CreateError::Invalid(m),
        CreateError::Invalid(m) => CreateError::Invalid(format!("{name}: {m}")),
        e => e,
    })?;
    if doc.blocks.is_empty() {
        return Err(bad(name, "the Word document has no readable text"));
    }
    let mut out = Document::new_empty();
    let (page, margins) = (doc.page, doc.margins);
    let mut placer =
        Placer { pages: Pages::new(&mut out, page, margins), images: &parser.images, numbering: &parser.numbering, counters: HashMap::new() };
    for block in &doc.blocks {
        match block {
            Block::Para(para) => placer.place_para(&mut out, para)?,
            Block::Table(table) => placer.place_table(table)?,
        }
    }
    let pages = placer.pages;
    if pages.done.len() + 1 > MAX_PAGES {
        return Err(bad(name, "the Word document has too many pages"));
    }
    emit_pages(&mut out, pages)?;
    set_title(&mut out, title);
    Ok(out)
}

/// Content streams and resources to finished pages, then the page tree entries.
fn emit_pages(out: &mut Document, mut pages: Pages) -> Result<(), CreateError> {
    if !pages.current.is_empty() || pages.done.is_empty() {
        pages.finish_page();
    }
    let (w, h) = pages.page;
    for (content, images) in std::mem::take(&mut pages.done) {
        let mut res = Dict::new();
        res.set(b"Font".to_vec(), Object::Ref(pages.fonts));
        if !images.is_empty() {
            let mut xo = Dict::new();
            for (i, (_, r)) in images.iter().enumerate() {
                xo.set(format!("Im{i}").into_bytes(), Object::Ref(*r));
            }
            res.set(b"XObject".to_vec(), Object::Dict(xo));
        }
        let content = if content.is_empty() { None } else { Some(content) };
        add_page(out, w, h, res, content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_cos::{SaveOptions, write_full};

    /// A minimal .docx package with stored (uncompressed) entries.
    fn docx(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in parts {
            let off = out.len() as u32;
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(parts.len() as u16).to_le_bytes());
        out.extend_from_slice(&(parts.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn document(body: &str) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:right=\"1440\" w:header=\"720\" w:footer=\"720\"/></w:sectPr></w:body></w:document>"
        )
        .into_bytes()
    }

    fn reopen(doc: &Document) -> (Document, Vec<Vec<u8>>) {
        let bytes = write_full(doc, &SaveOptions::default()).unwrap();
        let doc = Document::open(std::sync::Arc::new(bytes)).unwrap();
        let pages = doc.get(doc.root().unwrap()).as_dict().unwrap().reference(b"Pages").unwrap();
        let kids = doc.get(pages).as_dict().unwrap().get(b"Kids").unwrap().as_array().unwrap().clone();
        let contents = kids
            .iter()
            .map(|k| {
                let page = doc.resolve(k).as_dict().cloned().unwrap();
                let c = doc.resolve(page.get(b"Contents").unwrap());
                match &*c {
                    Object::Stream(s) => s.decoded().unwrap(),
                    _ => panic!("page without content"),
                }
            })
            .collect();
        (doc, contents)
    }

    #[test]
    fn headings_bold_and_text_convert() {
        let body = "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Report</w:t></w:r></w:p>\
            <w:p><w:r><w:t xml:space=\"preserve\">Hello </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">brave </w:t></w:r>\
            <w:r><w:rPr><w:i/><w:sz w:val=\"28\"/></w:rPr><w:t>world &amp; friends</w:t></w:r></w:p>";
        let pkg = docx(&[("word/document.xml", document(body))]);
        let doc = from_office("report.docx", &pkg).unwrap();
        let (doc, contents) = reopen(&doc);
        assert_eq!(contents.len(), 1);
        let text = String::from_utf8_lossy(&contents[0]);
        assert!(text.contains("(Report) Tj"), "{text}");
        assert!(text.contains("(Hello ) Tj"), "{text}");
        assert!(text.contains("(brave ) Tj"), "{text}");
        assert!(text.contains("world & friends"), "{text}");
        assert!(text.contains("/F1") && text.contains("/F2") && text.contains("/F3"), "regular, bold and italic faces: {text}");
        let pages = doc.get(doc.root().unwrap()).as_dict().unwrap().reference(b"Pages").unwrap();
        let kids = doc.get(pages).as_dict().unwrap().get(b"Kids").unwrap().as_array().unwrap();
        let page = doc.resolve(&kids[0]).as_dict().cloned().unwrap();
        let media = page.get(b"MediaBox").unwrap().as_array().unwrap();
        let size: Vec<f64> = media.iter().map(|o| o.as_f64().unwrap()).collect();
        assert_eq!(size, [0.0, 0.0, 612.0, 792.0]);
    }

    #[test]
    fn deflated_parts_convert() {
        let body = "<w:p><w:r><w:t>Squeezed</w:t></w:r></w:p>";
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut enc, &document(body)).unwrap();
        let deflated = enc.finish().unwrap();
        // A hand-packed deflated entry: local header, central directory, EOCD.
        let name = "word/document.xml";
        let mut out = Vec::new();
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(deflated.len() as u32).to_le_bytes());
        out.extend_from_slice(&(document(body).len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        let off = 0u32;
        out.extend_from_slice(&deflated);
        let cd_off = out.len() as u32;
        out.extend_from_slice(b"PK\x01\x02");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(deflated.len() as u32).to_le_bytes());
        out.extend_from_slice(&(document(body).len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        let cd_len = out.len() as u32 - cd_off;
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&cd_len.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        let (_, contents) = reopen(&from_office("s.docx", &out).unwrap());
        assert!(String::from_utf8_lossy(&contents[0]).contains("(Squeezed) Tj"));
    }

    #[test]
    fn lists_tables_and_images_convert() {
        let numbering = "<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
            <w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
            <w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum>\
            <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
            <w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>";
        let body = "<w:p><w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Dot one</w:t></w:r></w:p>\
            <w:p><w:pPr><w:numPr><w:numId w:val=\"2\"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>\
            <w:p><w:pPr><w:numPr><w:numId w:val=\"2\"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>\
            <w:tbl><w:tblGrid><w:gridCol w:w=\"4000\"/><w:gridCol w:w=\"4000\"/></w:tblGrid>\
            <w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr>\
            <w:tr><w:tc><w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
            <w:p><w:r><w:drawing><wp:inline xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
            <wp:extent cx=\"914400\" cy=\"457200\"/><wp:docPr id=\"1\" name=\"pic\"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed=\"rId5\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>\
            </wp:inline></w:drawing></w:r></w:p>";
        let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
            <Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/pic.png\"/></Relationships>";
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, 8, 4);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[120u8; 8 * 4 * 3]).unwrap();
        }
        let pkg = docx(&[
            ("word/document.xml", document(body)),
            ("word/numbering.xml", numbering.as_bytes().to_vec()),
            ("word/_rels/document.xml.rels", rels.as_bytes().to_vec()),
            ("word/media/pic.png", png),
        ]);
        let (doc, contents) = reopen(&from_office("mixed.docx", &pkg).unwrap());
        let text = String::from_utf8_lossy(&contents[0]);
        assert!(text.contains("(1. ) Tj") || text.contains("(1.)"), "decimal prefix: {text}");
        assert!(text.contains("Dot one") && text.contains("First") && text.contains("Second"), "{text}");
        assert!(text.contains("(A1)") && text.contains("(B2)"), "table cells: {text}");
        assert!(text.contains("re S"), "table borders: {text}");
        assert!(contents[0].windows(2).any(|w| w == [0x95, b' ']), "bullet prefix");
        assert!(text.contains("Do"), "the picture is drawn: {text}");
        let pages = doc.get(doc.root().unwrap()).as_dict().unwrap().reference(b"Pages").unwrap();
        let kids = doc.get(pages).as_dict().unwrap().get(b"Kids").unwrap().as_array().unwrap().clone();
        let page = doc.resolve(&kids[0]).as_dict().cloned().unwrap();
        let res = doc.resolve(page.get(b"Resources").unwrap()).as_dict().cloned().unwrap();
        let xo = doc.resolve(res.get(b"XObject").unwrap()).as_dict().cloned().unwrap();
        assert!(xo.get(b"Im0").is_some(), "the picture is an XObject");
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(from_office("a.docx", b"PK\x03\x04").is_err(), "truncated zip");
        assert!(from_office("a.docx", b"").is_err(), "empty");
        assert!(from_office("a.docx", b"hello world").is_err(), "not a zip");
        let no_doc = docx(&[("word/styles.xml", b"<w:styles/>".to_vec())]);
        let err = from_office("a.docx", &no_doc).unwrap_err().to_string();
        assert!(err.contains("a.docx") && err.contains("word/document.xml"), "{err}");
        let empty_count = {
            let mut eo = Vec::new();
            eo.extend_from_slice(b"PK\x05\x06");
            eo.extend_from_slice(&[0u8; 18]);
            eo
        };
        assert!(from_office("a.docx", &empty_count).is_err(), "empty directory");
        let no_text = docx(&[("word/document.xml", document("<w:p><w:r></w:r></w:p>"))]);
        let (_, contents) = reopen(&from_office("a.docx", &no_text).unwrap());
        assert_eq!(contents.len(), 1, "an empty paragraph still takes a line");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(unescape("a &amp; b &lt;c&gt; &#65;&#x42; &bogus;"), "a & b <c> AB &bogus;");
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(letters(1, false), "a");
        assert_eq!(letters(27, false), "aa");
        assert_eq!(letters(3, true), "C");
        assert_eq!(roman(4, false), "iv");
        assert_eq!(roman(2026, true), "MMXXVI");
    }
}
