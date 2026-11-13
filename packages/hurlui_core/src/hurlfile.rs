/*
 * Hurl (https://hurl.dev)
 * Copyright (C) 2026 Orange
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *          http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */
//! Tolerant, line-oriented reader and writer for Hurl files.
//!
//! The GUI does not embed the Hurl parser: the `hurl` binary installed on the
//! machine stays the single source of truth for the language (its version may
//! differ from the one this application was built against). This module only
//! needs to *locate* the building blocks of each entry (request line, headers,
//! sections, body, response part) and keeps every value as raw source text, so
//! templates, escapes, filters or predicates unknown to the GUI survive a
//! round trip untouched.
//!
//! Editing is done by splicing: only the entry that was modified in the GUI is
//! regenerated, the rest of the file is preserved byte for byte.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct KeyValue {
    pub name: String,
    pub value: String,
}

/// An explicit assert: `<query and filters> <predicate> <value>`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Assert {
    pub query: String,
    pub predicate: String,
    pub value: String,
}

/// Request body.
///
/// `mode` is one of `none`, `json`, `xml`, `text`, `graphql`, `file`, `form`,
/// `multipart` or `hurl` (raw Hurl body syntax kept verbatim: one line
/// strings, `base64,...;`, `hex,...;`, unknown multiline flavours...).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Body {
    pub mode: String,
    pub content: String,
    /// For `json` and `xml`: the body is written inside a ```` ```json ```` block
    /// instead of bare.
    pub fenced: bool,
}

impl Default for Body {
    fn default() -> Self {
        Body {
            mode: "none".to_string(),
            content: String::new(),
            fenced: false,
        }
    }
}

/// A section unknown to the GUI, preserved verbatim.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct RawSection {
    pub name: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    /// 1-based position in the file, as used by `--from-entry`/`--to-entry`.
    pub index: usize,
    /// 0-based first line of the entry, leading comments included.
    pub start_line: usize,
    /// 0-based last line of the entry (inclusive).
    pub end_line: usize,
    /// Comments preceding the request, without the leading `#`.
    pub docs: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<KeyValue>,
    pub query: Vec<KeyValue>,
    pub form: Vec<KeyValue>,
    pub multipart: Vec<KeyValue>,
    pub cookies: Vec<KeyValue>,
    /// `name` is the user, `value` the password.
    pub basic_auth: Option<KeyValue>,
    pub options: Vec<KeyValue>,
    pub request_extra: Vec<RawSection>,
    pub body: Body,
    pub has_response: bool,
    pub version: String,
    pub status: String,
    pub response_headers: Vec<KeyValue>,
    pub captures: Vec<KeyValue>,
    pub asserts: Vec<Assert>,
    pub response_extra: Vec<RawSection>,
    /// Implicit body assert, raw Hurl syntax.
    pub response_body: String,
}

impl Default for Entry {
    fn default() -> Self {
        Entry {
            index: 0,
            start_line: 0,
            end_line: 0,
            docs: String::new(),
            method: "GET".to_string(),
            url: String::new(),
            headers: vec![],
            query: vec![],
            form: vec![],
            multipart: vec![],
            cookies: vec![],
            basic_auth: None,
            options: vec![],
            request_extra: vec![],
            body: Body::default(),
            has_response: false,
            version: "HTTP".to_string(),
            status: String::new(),
            response_headers: vec![],
            captures: vec![],
            asserts: vec![],
            response_extra: vec![],
            response_body: String::new(),
        }
    }
}

impl Entry {
    /// Display name: first documentation line, if any.
    pub fn name(&self) -> String {
        self.docs
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string()
    }
}

/// Predicates of an assert, as Hurl writes them. A test checks that these
/// are the ones of `hurl_core` (`PredicateFuncValue`): they are offered by
/// the editor of asserts. Any of them can follow `not`.
pub const PREDICATES: &[&str] = &[
    "==",
    "!=",
    ">",
    ">=",
    "<",
    "<=",
    "startsWith",
    "endsWith",
    "contains",
    "includes",
    "matches",
    "exists",
    "isEmpty",
    "isInteger",
    "isFloat",
    "isNumber",
    "isBoolean",
    "isString",
    "isCollection",
    "isList",
    "isObject",
    "isDate",
    "isIsoDate",
    "isIpv4",
    "isIpv6",
    "isUuid",
];

/// Legacy spellings, still found in old files: recognized, not offered.
const LEGACY_PREDICATES: &[&str] = &[
    "equals",
    "notEquals",
    "greaterThan",
    "greaterThanOrEquals",
    "lessThan",
    "lessThanOrEquals",
    "countEquals",
];

fn is_predicate(word: &str) -> bool {
    if PREDICATES.contains(&word) || LEGACY_PREDICATES.contains(&word) {
        return true;
    }
    // Future type predicates (`isXxx`).
    let mut chars = word.chars();
    word.len() > 2
        && chars.next() == Some('i')
        && chars.next() == Some('s')
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && word.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_trivia(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// Removes a trailing comment from an unquoted value (first `#` not escaped).
fn strip_comment(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'#' => return s[..i].trim_end(),
            _ => i += 1,
        }
    }
    s.trim_end()
}

/// Byte offset of a trailing comment in a line that may hold quoted strings,
/// backtick strings or `/regex/` literals.
fn comment_start_quoted(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if c == b'\\' {
            i += 2;
            continue;
        }
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'"' | b'`' => quote = Some(c),
                b'/' if i == 0 || b[i - 1].is_ascii_whitespace() => quote = Some(b'/'),
                b'#' => return Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

fn strip_comment_quoted(s: &str) -> &str {
    if s.contains("```") {
        return s.trim_end();
    }
    match comment_start_quoted(s) {
        Some(i) => s[..i].trim_end(),
        None => s.trim_end(),
    }
}

/// Whitespace separated tokens, quotes aware. Returns byte ranges.
fn tokens(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let start = i;
        let mut quote: Option<u8> = None;
        while i < b.len() {
            let c = b[i];
            if c == b'\\' {
                i += 2;
                continue;
            }
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                }
                None => {
                    if c.is_ascii_whitespace() {
                        break;
                    }
                    if c == b'"' || c == b'`' || (c == b'/' && i == start) {
                        quote = Some(c);
                    }
                }
            }
            i += 1;
        }
        let end = i.min(b.len());
        out.push((start, end));
    }
    out
}

pub fn parse_assert(text: &str) -> Assert {
    let text = text.trim();
    let toks = tokens(text);
    for (k, &(s, e)) in toks.iter().enumerate().skip(1) {
        let word = &text[s..e];
        if word == "not" {
            if let Some(&(ns, ne)) = toks.get(k + 1)
                && is_predicate(&text[ns..ne])
            {
                return Assert {
                    query: text[..s].trim().to_string(),
                    predicate: format!("not {}", &text[ns..ne]),
                    value: text[ne..].trim().to_string(),
                };
            }
        } else if is_predicate(word) {
            return Assert {
                query: text[..s].trim().to_string(),
                predicate: word.to_string(),
                value: text[e..].trim().to_string(),
            };
        }
    }
    Assert {
        query: text.to_string(),
        predicate: String::new(),
        value: String::new(),
    }
}

/// Splits `key: value`. The value is returned untouched (comment included).
fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' if b.get(i + 1) == Some(&b'{') => {
                let p = t[i..].find("}}")?;
                i += p + 2;
                continue;
            }
            b'\\' => {
                i += 2;
                continue;
            }
            b':' => break,
            b' ' | b'\t' => {
                let rest = t[i..].trim_start();
                if rest.starts_with(':') {
                    let name = &t[..i];
                    return (!name.is_empty()).then(|| (name, rest[1..].trim_start()));
                }
                return None;
            }
            b'#' | b'"' | b'`' => return None,
            _ => {}
        }
        i += 1;
    }
    if i == 0 || i >= b.len() {
        return None;
    }
    Some((&t[..i], t[i + 1..].trim_start()))
}

fn key_value(line: &str) -> Option<KeyValue> {
    split_key_value(line).map(|(name, value)| KeyValue {
        name: name.to_string(),
        value: strip_comment(value).to_string(),
    })
}

fn key_value_quoted(text: &str) -> Option<KeyValue> {
    split_key_value(text).map(|(name, value)| KeyValue {
        name: name.to_string(),
        value: strip_comment_quoted(value).to_string(),
    })
}

fn response_line(line: &str) -> Option<(String, String)> {
    let t = strip_comment(line.trim());
    let mut it = t.split_whitespace();
    let version = it.next()?;
    let status = it.next()?;
    if it.next().is_some() {
        return None;
    }
    let version_ok = version == "HTTP"
        || version.strip_prefix("HTTP/").is_some_and(|r| {
            r == "*" || (!r.is_empty() && r.chars().all(|c| c.is_ascii_digit() || c == '.'))
        });
    let status_ok = status == "*" || status.chars().all(|c| c.is_ascii_digit());
    (version_ok && status_ok).then(|| (version.to_string(), status.to_string()))
}

fn request_line(line: &str) -> Option<(String, String)> {
    let t = line.trim_start();
    let end = t.find(|c: char| !c.is_ascii_uppercase())?;
    if end == 0 {
        return None;
    }
    let rest = &t[end..];
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let url = strip_comment(rest.trim_start());
    if url.is_empty() || url.starts_with(':') || url.starts_with('=') {
        return None;
    }
    Some((t[..end].to_string(), url.to_string()))
}

fn section_header(line: &str) -> Option<String> {
    let t = strip_comment(line.trim());
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    let first = inner.chars().next()?;
    (first.is_ascii_uppercase() && inner.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| inner.to_string())
}

fn canonical_section(name: &str) -> &'static str {
    match name {
        "Query" | "QueryStringParams" => "query",
        "Form" | "FormParams" => "form",
        "Multipart" | "MultipartFormData" => "multipart",
        "Cookies" => "cookies",
        "BasicAuth" => "basicauth",
        "Options" => "options",
        "Captures" => "captures",
        "Asserts" => "asserts",
        _ => "",
    }
}

fn is_json_number(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    if s.is_empty() || !s.as_bytes()[0].is_ascii_digit() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
}

fn is_plain_key(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.[]@$".contains(c))
}

/// Does this (non trivia) line start a body?
fn body_start(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with('`') || t.starts_with('<') || t.starts_with('"') {
        return true;
    }
    if t.starts_with('{') && !t.starts_with("{{") {
        return true;
    }
    if t.starts_with('[') {
        // `[0]: value` is a legal key, `[1, 2]` is a JSON array.
        return !split_key_value(t).is_some_and(|(name, _)| is_plain_key(name));
    }
    if t.starts_with("base64,") || t.starts_with("hex,") || t.starts_with("file,") {
        return true;
    }
    let v = strip_comment(t);
    matches!(v, "true" | "false" | "null") || is_json_number(v)
}

fn entry_boundary(line: &str) -> bool {
    response_line(line).is_some() || request_line(line).is_some()
}

/// Skips what `line` holds of the XML prolog: `<?...?>`, comments and
/// `<!...>` declarations. `until` is the terminator of the construct left
/// open by the previous lines, if any. Returns the text which follows the
/// prolog, `None` when the line ends before the root element.
fn skip_xml_prolog<'a>(line: &'a str, until: &mut Option<&'static str>) -> Option<&'a str> {
    let mut rest = line;
    loop {
        if let Some(end) = *until {
            let p = rest.find(end)?;
            rest = &rest[p + end.len()..];
            *until = None;
        }
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        *until = Some(if rest.starts_with("<?") {
            "?>"
        } else if rest.starts_with("<!--") {
            "-->"
        } else if rest.starts_with("<!") {
            ">"
        } else {
            return Some(rest);
        });
    }
}

/// Lines which may be read, over a whole file, by the searches for the end of
/// XML bodies which end up finding none (see [`xml_end`]).
const XML_SEARCH_BUDGET: usize = 1_000_000;

/// Last line of the bare XML body starting at line `i`: the line where the
/// root element closes, `None` when it never does.
///
/// The root element follows the prolog. It closes with its start tag when
/// that tag is self-closing, otherwise on the first line where as many
/// `</name` as `<name` were seen (each followed by a whitespace, `>` or `/`).
/// Every line is looked at once: a body of any size costs a single pass.
///
/// A document which is never closed is searched up to the end of the file
/// while its body stops at the next entry: a file made of thousands of them
/// would be read thousands of times. Such fruitless searches draw on
/// `budget`, a number of lines for the whole file. It is far above what a
/// file which could be read in a reasonable time before this limit existed
/// can use; once it is spent, documents are simply not searched any more
/// (they end at the next entry).
fn xml_end(lines: &[&str], i: usize, budget: &mut usize) -> Option<usize> {
    let within = &lines[..lines.len().min(i.saturating_add(*budget))];
    match xml_end_within(within, i) {
        Ok(end) => Some(end),
        Err(read) => {
            *budget = budget.saturating_sub(read);
            None
        }
    }
}

/// See [`xml_end`]. The error is the number of lines read in vain.
fn xml_end_within(lines: &[&str], i: usize) -> Result<usize, usize> {
    let ends_name = |c: char| c.is_whitespace() || c == '>' || c == '/';
    let unclosed = lines.len().saturating_sub(i);
    let mut until = None;
    let (first, start) = lines
        .iter()
        .enumerate()
        .skip(i)
        .find_map(|(j, line)| Some((j, skip_xml_prolog(line, &mut until)?)))
        .ok_or(unclosed)?;
    // Anything but an element after the prolog: not an XML document.
    let name = start.strip_prefix('<').ok_or(first + 1 - i)?;
    let name = &name[..name.find(ends_name).unwrap_or(name.len())];
    if name.is_empty() {
        return Ok(first);
    }
    let (open, close) = (format!("<{name}"), format!("</{name}"));
    let mut start_tag_closed = false;
    // Elements named like the root which are still open.
    let mut depth = 0isize;
    let root_lines = std::iter::once(start).chain(lines[first + 1..].iter().copied());
    for (rest, j) in root_lines.zip(first..) {
        if !start_tag_closed && let Some(gt) = rest.find('>') {
            start_tag_closed = true;
            if rest[..gt].ends_with('/') {
                return Ok(j);
            }
        }
        // A tag never spans two lines: the end of the line ends a name too.
        let count = |tag: &str| {
            rest.match_indices(tag)
                .filter(|(k, _)| rest[k + tag.len()..].chars().next().is_none_or(ends_name))
                .count() as isize
        };
        depth += count(&open) - count(&close);
        if start_tag_closed && depth == 0 {
            return Ok(j);
        }
    }
    Err(unclosed)
}

/// Consumes a body starting at line `i`. Returns the index of the first line
/// after the body, the structured body and its raw source text.
/// `xml_budget` is what is left of [`XML_SEARCH_BUDGET`] for the file.
fn consume_body(lines: &[&str], i: usize, xml_budget: &mut usize) -> (usize, Body, String) {
    let n = lines.len();
    let first = lines[i].trim();
    let raw_of = |j: usize| lines[i..=j].join("\n").trim().to_string();
    let hurl = |j: usize| {
        let raw = raw_of(j);
        (
            j + 1,
            Body {
                mode: "hurl".into(),
                content: raw.clone(),
                fenced: false,
            },
            raw,
        )
    };
    // Lines up to the next entry boundary, trailing trivia excluded.
    let until_boundary = |from: usize| {
        let mut j = from;
        while j + 1 < n && !entry_boundary(lines[j + 1]) {
            j += 1;
        }
        while j > i && is_trivia(lines[j]) {
            j -= 1;
        }
        j
    };

    if let Some(after) = first.strip_prefix("```") {
        if after.contains("```") {
            return hurl(i);
        }
        let Some(j) = (i + 1..n).find(|&j| lines[j].contains("```")) else {
            return hurl(n - 1);
        };
        let lang = after.trim();
        let clean_end = strip_comment(lines[j].trim()) == "```";
        let mode = match lang {
            "" => "text",
            "json" => "json",
            "xml" => "xml",
            "graphql" => "graphql",
            _ => "",
        };
        if !clean_end || mode.is_empty() {
            return hurl(j);
        }
        // The content is the exact payload: every line of a multiline string
        // ends with a newline, which tells an empty body from a blank line.
        let content: String = lines[i + 1..j].iter().flat_map(|l| [*l, "\n"]).collect();
        let body = Body {
            mode: mode.into(),
            content,
            fenced: true,
        };
        return (j + 1, body, raw_of(j));
    }

    if first.starts_with('{') || first.starts_with('[') {
        let mut depth = 0i32;
        let mut j = i;
        let mut balanced = false;
        loop {
            if j > i && entry_boundary(lines[j]) {
                j -= 1;
                break;
            }
            let b = lines[j].as_bytes();
            let mut in_str = false;
            let mut k = 0;
            while k < b.len() {
                let c = b[k];
                if in_str {
                    if c == b'\\' {
                        k += 1;
                    } else if c == b'"' {
                        in_str = false;
                    }
                } else {
                    match c {
                        b'"' => in_str = true,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => depth -= 1,
                        b'#' if depth <= 0 => break,
                        _ => {}
                    }
                }
                k += 1;
            }
            if depth <= 0 {
                balanced = true;
                break;
            }
            if j + 1 >= n {
                break;
            }
            j += 1;
        }
        if !balanced {
            while j > i && is_trivia(lines[j]) {
                j -= 1;
            }
        }
        let raw = raw_of(j);
        let body = Body {
            mode: "json".into(),
            content: raw.clone(),
            fenced: false,
        };
        return (j + 1, body, raw);
    }

    if first.starts_with('<') {
        // The body ends with the closing tag of the root element. Text nodes
        // may look like a request or a response line, so entry boundaries are
        // only used as a fallback when the document is never closed.
        let j = xml_end(lines, i, xml_budget).unwrap_or_else(|| until_boundary(i));
        let raw = raw_of(j);
        let body = Body {
            mode: "xml".into(),
            content: raw.clone(),
            fenced: false,
        };
        return (j + 1, body, raw);
    }

    if first.starts_with("base64,") || first.starts_with("hex,") || first.starts_with("file,") {
        let mut j = i;
        while !lines[j].contains(';') && j + 1 < n && !entry_boundary(lines[j + 1]) {
            j += 1;
        }
        if j == i
            && let Some(name) = strip_comment(first)
                .strip_prefix("file,")
                .and_then(|r| r.strip_suffix(';'))
        {
            let body = Body {
                mode: "file".into(),
                content: name.trim().to_string(),
                fenced: false,
            };
            return (i + 1, body, raw_of(i));
        }
        return hurl(j);
    }

    if first.starts_with('`') {
        return hurl(i);
    }

    let scalar = strip_comment(first);
    if first.starts_with('"')
        || matches!(scalar, "true" | "false" | "null")
        || is_json_number(scalar)
    {
        let raw = raw_of(i);
        let body = Body {
            mode: "json".into(),
            content: raw.clone(),
            fenced: false,
        };
        return (i + 1, body, raw);
    }

    hurl(until_boundary(i))
}

/// A capture or an assert may embed a multiline string: gathers the lines up
/// to the closing fence. Returns the text and the index of the last line.
///
/// The searches for a closing fence cost a single pass over a file, whatever
/// it holds: a search stops at the first line holding a fence, and only a
/// line holding one starts a search. A search which finds nothing is
/// therefore the last one of the file.
fn gather_multiline(lines: &[&str], i: usize) -> (String, usize) {
    if lines[i].matches("```").count() % 2 == 1
        && let Some(j) = (i + 1..lines.len()).find(|&j| lines[j].contains("```"))
    {
        let mut text = lines[i].trim_start().to_string();
        for l in &lines[i + 1..=j] {
            text.push('\n');
            text.push_str(l);
        }
        return (text, j);
    }
    (lines[i].trim().to_string(), i)
}

/// Unexpected content after a request body: the body becomes raw text, made
/// of what would be written for it followed by `raw`.
///
/// A body which is already raw text is extended in place. Writing it again
/// for every piece of content would make a request followed by thousands of
/// bodies quadratic.
fn append_to_body(body: &mut Body, raw: &str) {
    let mut content = if body.mode == "hurl" {
        // What `serialize_body` writes for raw text: its trimmed content.
        let mut content = std::mem::take(&mut body.content);
        content.truncate(content.trim_end().len());
        let leading = content.len() - content.trim_start().len();
        // Only when what came first wrote nothing: once at most in a row,
        // `raw` being trimmed.
        if leading > 0 {
            content.drain(..leading);
        }
        content
    } else {
        serialize_body(body)
    };
    content.push('\n');
    content.push_str(raw);
    *body = Body {
        mode: "hurl".into(),
        content,
        fenced: false,
    };
}

#[derive(PartialEq, Clone, Copy)]
enum Phase {
    ReqHeaders,
    ReqSection,
    ReqBody,
    RespHeaders,
    RespSection,
    RespBody,
}

/// Parses the entries of a Hurl file. Never fails: lines that cannot be
/// understood are kept as raw text in the closest body or section.
pub fn parse(text: &str) -> Vec<Entry> {
    // Hurl ignores a leading byte order mark: so must the entry numbering.
    let (_, text) = split_bom(text);
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let n = lines.len();
    let mut entries: Vec<Entry> = vec![];
    let mut phase = Phase::ReqHeaders;
    let mut section = "";
    let mut next_start = 0usize;
    let mut xml_budget = XML_SEARCH_BUDGET;
    let mut i = 0usize;

    while i < n {
        let line = lines[i];
        if is_trivia(line) {
            i += 1;
            continue;
        }

        if let Some((version, status)) = response_line(line) {
            if let Some(e) = entries.last_mut().filter(|e| !e.has_response) {
                e.has_response = true;
                e.version = version;
                e.status = status;
                e.end_line = i;
                phase = Phase::RespHeaders;
            }
            next_start = i + 1;
            i += 1;
            continue;
        }

        if let Some((method, url)) = request_line(line) {
            let docs = lines[next_start..i]
                .iter()
                .filter_map(|l| {
                    let c = l.trim().strip_prefix('#')?;
                    Some(c.strip_prefix(' ').unwrap_or(c).trim_end().to_string())
                })
                .collect::<Vec<_>>()
                .join("\n");
            entries.push(Entry {
                index: entries.len() + 1,
                start_line: next_start,
                end_line: i,
                docs,
                method,
                url,
                ..Entry::default()
            });
            phase = Phase::ReqHeaders;
            next_start = i + 1;
            i += 1;
            continue;
        }

        let Some(e) = entries.last_mut() else {
            // Content before the first request: left outside of any entry.
            next_start = i + 1;
            i += 1;
            continue;
        };
        let in_request = matches!(
            phase,
            Phase::ReqHeaders | Phase::ReqSection | Phase::ReqBody
        );

        if let Some(name) = section_header(line) {
            section = canonical_section(&name);
            if in_request {
                phase = Phase::ReqSection;
                match section {
                    "form" | "multipart" if e.body.mode == "none" => e.body.mode = section.into(),
                    "query" | "form" | "multipart" | "cookies" | "basicauth" | "options" => {}
                    _ => {
                        section = "";
                        e.request_extra.push(RawSection {
                            name,
                            content: String::new(),
                        });
                    }
                }
            } else {
                phase = Phase::RespSection;
                if !matches!(section, "captures" | "asserts") {
                    section = "";
                    e.response_extra.push(RawSection {
                        name,
                        content: String::new(),
                    });
                }
            }
            e.end_line = i;
            next_start = i + 1;
            i += 1;
            continue;
        }

        // Key/values and section lines.
        let mut consumed: Option<usize> = None;
        if !body_start(line) {
            match phase {
                Phase::ReqHeaders | Phase::RespHeaders => {
                    if let Some(kv) = key_value(line) {
                        if phase == Phase::ReqHeaders {
                            e.headers.push(kv);
                        } else {
                            e.response_headers.push(kv);
                        }
                        consumed = Some(i);
                    }
                }
                Phase::ReqSection => match section {
                    "query" | "form" | "multipart" | "cookies" | "options" => {
                        if let Some(kv) = key_value(line) {
                            match section {
                                "query" => e.query.push(kv),
                                "form" => e.form.push(kv),
                                "multipart" => e.multipart.push(kv),
                                "cookies" => e.cookies.push(kv),
                                _ => e.options.push(kv),
                            }
                            consumed = Some(i);
                        }
                    }
                    "basicauth" => {
                        if let Some(kv) = key_value(line) {
                            if e.basic_auth.is_none() {
                                e.basic_auth = Some(kv);
                            }
                            consumed = Some(i);
                        }
                    }
                    _ => {
                        if let Some(s) = e.request_extra.last_mut() {
                            if !s.content.is_empty() {
                                s.content.push('\n');
                            }
                            s.content.push_str(line.trim_end());
                            consumed = Some(i);
                        }
                    }
                },
                Phase::RespSection => match section {
                    "captures" => {
                        let (text, j) = gather_multiline(&lines, i);
                        if let Some(kv) = key_value_quoted(&text) {
                            e.captures.push(kv);
                            consumed = Some(j);
                        }
                    }
                    "asserts" => {
                        let (text, j) = gather_multiline(&lines, i);
                        e.asserts.push(parse_assert(strip_comment_quoted(&text)));
                        consumed = Some(j);
                    }
                    _ => {
                        if let Some(s) = e.response_extra.last_mut() {
                            if !s.content.is_empty() {
                                s.content.push('\n');
                            }
                            s.content.push_str(line.trim_end());
                            consumed = Some(i);
                        }
                    }
                },
                _ => {}
            }
        }
        if let Some(j) = consumed {
            e.end_line = j;
            next_start = j + 1;
            i = j + 1;
            continue;
        }

        // Everything else is a body.
        let (next, body, raw) = consume_body(&lines, i, &mut xml_budget);
        if in_request {
            if phase == Phase::ReqBody {
                // Unexpected content after a body: keep it verbatim.
                append_to_body(&mut e.body, &raw);
            } else {
                e.body = body;
            }
            phase = Phase::ReqBody;
        } else {
            if phase == Phase::RespBody {
                e.response_body.push('\n');
            }
            e.response_body.push_str(&raw);
            phase = Phase::RespBody;
        }
        e.end_line = next - 1;
        next_start = next;
        i = next;
    }
    entries
}

/// Multiline string body. The content is written verbatim: only the newline
/// that the syntax requires before the closing fence is added when missing.
fn fenced(lang: &str, content: &str) -> String {
    let mut out = format!("```{lang}\n{content}");
    if !content.is_empty() && !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```");
    out
}

fn serialize_body(body: &Body) -> String {
    let content = body.content.trim_end();
    match body.mode.as_str() {
        "json" | "xml" => {
            if body.fenced {
                fenced(&body.mode, &body.content)
            } else {
                content.trim().to_string()
            }
        }
        "text" => fenced("", &body.content),
        "graphql" => fenced("graphql", &body.content),
        "file" => {
            if content.trim().is_empty() {
                String::new()
            } else {
                format!("file,{};", content.trim())
            }
        }
        "hurl" => content.trim().to_string(),
        _ => String::new(),
    }
}

/// `#` starts a comment in unquoted Hurl values (URL, headers, params...): a
/// `#` typed as is would silently truncate the value, so it is written `\#`.
/// Already escaped characters are left untouched.
fn escape_hash(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '#' => out.push_str("\\#"),
            _ => out.push(c),
        }
    }
    out
}

/// Writes `name: value` lines. `unquoted` values get their `#` escaped;
/// captures hold quoted queries and are written as they are.
fn push_key_values(out: &mut Vec<String>, items: &[KeyValue], unquoted: bool) {
    for kv in items {
        let name = kv.name.trim();
        if name.is_empty() {
            continue;
        }
        let value = if unquoted {
            escape_hash(kv.value.trim())
        } else {
            kv.value.trim().to_string()
        };
        if value.is_empty() {
            out.push(format!("{name}:"));
        } else {
            out.push(format!("{name}: {value}"));
        }
    }
}

fn has_named(items: &[KeyValue]) -> bool {
    items.iter().any(|kv| !kv.name.trim().is_empty())
}

/// Hurl source of an entry, as a list of lines.
pub fn entry_lines(e: &Entry) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for l in e.docs.lines() {
        let l = l.trim_end();
        out.push(if l.is_empty() {
            "#".to_string()
        } else {
            format!("# {l}")
        });
    }
    let method = if e.method.trim().is_empty() {
        "GET"
    } else {
        e.method.trim()
    };
    out.push(format!("{} {}", method, escape_hash(e.url.trim())));
    push_key_values(&mut out, &e.headers, true);
    if has_named(&e.query) {
        out.push("[Query]".into());
        push_key_values(&mut out, &e.query, true);
    }
    if e.body.mode == "form" || has_named(&e.form) {
        out.push("[Form]".into());
        push_key_values(&mut out, &e.form, true);
    }
    if e.body.mode == "multipart" || has_named(&e.multipart) {
        out.push("[Multipart]".into());
        push_key_values(&mut out, &e.multipart, true);
    }
    if has_named(&e.cookies) {
        out.push("[Cookies]".into());
        push_key_values(&mut out, &e.cookies, true);
    }
    if let Some(auth) = e.basic_auth.as_ref().filter(|a| !a.name.trim().is_empty()) {
        out.push("[BasicAuth]".into());
        push_key_values(&mut out, std::slice::from_ref(auth), true);
    }
    if has_named(&e.options) {
        out.push("[Options]".into());
        push_key_values(&mut out, &e.options, true);
    }
    for s in &e.request_extra {
        out.push(format!("[{}]", s.name));
        out.extend(s.content.lines().map(str::to_string));
    }
    let body = serialize_body(&e.body);
    if !body.is_empty() {
        out.extend(body.lines().map(str::to_string));
    }

    let asserts: Vec<&Assert> = e
        .asserts
        .iter()
        .filter(|a| !a.query.trim().is_empty())
        .collect();
    // A version other than the default `HTTP` is an expectation of its own.
    let version = match e.version.trim() {
        "" => "HTTP",
        version => version,
    };
    let response_needed = e.has_response
        || version != "HTTP"
        || !e.status.trim().is_empty()
        || has_named(&e.response_headers)
        || has_named(&e.captures)
        || !asserts.is_empty()
        || !e.response_extra.is_empty()
        || !e.response_body.trim().is_empty();
    if response_needed {
        let status = if e.status.trim().is_empty() {
            "*"
        } else {
            e.status.trim()
        };
        out.push(format!("{version} {status}"));
        push_key_values(&mut out, &e.response_headers, true);
        if has_named(&e.captures) {
            out.push("[Captures]".into());
            push_key_values(&mut out, &e.captures, false);
        }
        if !asserts.is_empty() {
            out.push("[Asserts]".into());
            for a in asserts {
                let line = format!(
                    "{} {} {}",
                    a.query.trim(),
                    a.predicate.trim(),
                    a.value.trim()
                );
                out.extend(line.trim_end().lines().map(str::to_string));
            }
        }
        for s in &e.response_extra {
            out.push(format!("[{}]", s.name));
            out.extend(s.content.lines().map(str::to_string));
        }
        if !e.response_body.trim().is_empty() {
            out.extend(e.response_body.trim().lines().map(str::to_string));
        }
    }
    out
}

const BOM: &str = "\u{feff}";

/// Separates a leading byte order mark from the text.
fn split_bom(text: &str) -> (&str, &str) {
    match text.strip_prefix(BOM) {
        Some(rest) => (BOM, rest),
        None => ("", text),
    }
}

fn detect_eol(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

pub fn serialize_entry(e: &Entry, eol: &str) -> String {
    let mut s = entry_lines(e).join(eol);
    s.push_str(eol);
    s
}

/// Hurl source of `e`, refused when it would not read back as this single
/// entry: writing it would merge it with its neighbours or split it, and
/// renumber the rest of the file.
pub fn checked_entry(e: &Entry, eol: &str) -> Result<String, String> {
    let method = e.method.trim();
    if !method.is_empty() && !method.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(format!("invalid method {method:?}: uppercase letters only"));
    }
    if e.url.trim().is_empty() {
        return Err("the URL of the request is empty".into());
    }
    let status = e.status.trim();
    if !(status.is_empty() || status == "*" || status.chars().all(|c| c.is_ascii_digit())) {
        return Err(format!(
            "invalid expected status {status:?}: a number, or * for any"
        ));
    }
    let text = serialize_entry(e, eol);
    let parsed = parse(&text);
    if parsed.len() != 1 || parsed[0].method != if method.is_empty() { "GET" } else { method } {
        return Err(format!(
            "this request cannot be written as one Hurl entry (it reads back as {}): check its bodies and raw sections",
            parsed.len()
        ));
    }
    Ok(text)
}

fn is_blank(seg: &str) -> bool {
    seg.trim().is_empty()
}

/// Appends `entry_text` to `out`, separated from what precedes and from what
/// follows (`rest`) by one blank line.
fn push_entry(out: &mut String, entry_text: &str, rest: &[&str], eol: &str) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    let last_blank = out.split_inclusive('\n').next_back().is_none_or(is_blank);
    if !out.is_empty() && !last_blank {
        out.push_str(eol);
    }
    out.push_str(entry_text);
    if rest.first().is_some_and(|s| !is_blank(s)) {
        out.push_str(eol);
    }
    for s in rest {
        out.push_str(s);
    }
}

/// Replaces entry `index` (1-based) by `entry`; the rest of the file is kept
/// byte for byte.
pub fn replace_entry(text: &str, index: usize, entry: &Entry) -> Result<String, String> {
    let entries = parse(text);
    let old = entries.get(index.wrapping_sub(1)).ok_or_else(|| {
        format!(
            "entry {index} not found (the file has {} entries)",
            entries.len()
        )
    })?;
    let eol = detect_eol(text);
    let entry_text = checked_entry(entry, eol)?;
    let (bom, text) = split_bom(text);
    let segs: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out: String = segs[..old.start_line.min(segs.len())].concat();
    let rest = &segs[(old.end_line + 1).min(segs.len())..];
    push_entry(&mut out, &entry_text, rest, eol);
    Ok(format!("{bom}{out}"))
}

/// Inserts `entry` after entry `after` (1-based), or after the last entry.
pub fn insert_entry(text: &str, after: Option<usize>, entry: &Entry) -> Result<String, String> {
    let entries = parse(text);
    let eol = detect_eol(text);
    let entry_text = checked_entry(entry, eol)?;
    let (bom, text) = split_bom(text);
    let segs: Vec<&str> = text.split_inclusive('\n').collect();
    let anchor = after
        .and_then(|a| entries.get(a.wrapping_sub(1)))
        .or(entries.last());
    let pos = match anchor {
        Some(e) => (e.end_line + 1).min(segs.len()),
        None => segs.len(),
    };
    let mut out: String = segs[..pos].concat();
    push_entry(&mut out, &entry_text, &segs[pos..], eol);
    Ok(format!("{bom}{out}"))
}

pub fn delete_entry(text: &str, index: usize) -> Result<String, String> {
    let entries = parse(text);
    let old = entries.get(index.wrapping_sub(1)).ok_or_else(|| {
        format!(
            "entry {index} not found (the file has {} entries)",
            entries.len()
        )
    })?;
    let (bom, text) = split_bom(text);
    let segs: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out: String = segs[..old.start_line.min(segs.len())].concat();
    let mut rest = &segs[(old.end_line + 1).min(segs.len())..];
    // Avoid leaving two blank separators behind.
    let last_blank = out.split_inclusive('\n').next_back().is_none_or(is_blank);
    while last_blank && rest.first().is_some_and(|s| is_blank(s)) {
        rest = &rest[1..];
    }
    for s in rest {
        out.push_str(s);
    }
    Ok(format!("{bom}{out}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The predicates offered by the editor of asserts are the ones of
    /// Hurl: a predicate added to, renamed in or removed from `hurl_core`
    /// shows here.
    #[test]
    fn predicates_are_the_ones_of_hurl() {
        let source = include_str!("../../hurl_core/src/ast/section.rs");
        let mut identifiers: Vec<&str> = source
            .lines()
            .filter_map(|line| line.trim().strip_prefix("PredicateFuncValue::"))
            .filter_map(|line| line.split_once("=> \""))
            .filter_map(|(_, identifier)| identifier.strip_suffix("\","))
            .collect();
        assert!(identifiers.len() > 20, "{identifiers:?}");
        let mut predicates = PREDICATES.to_vec();
        identifiers.sort_unstable();
        predicates.sort_unstable();
        assert_eq!(predicates, identifiers);
        assert!(PREDICATES.iter().all(|p| !LEGACY_PREDICATES.contains(p)));
    }

    const SAMPLE: &str = r#"# Login
# Gets a token
POST {{base_url}}/login?x=1 # trailing
Content-Type: application/json
X-Hash: a\#b
[Query]
lang: fr
[BasicAuth]
bob: secret
[Options]
location: true
{
  "user": "bob",
  "tags": ["a", "b"],
  "note": "HTTP 200 # not a comment"
}
HTTP/1.1 200
Content-Type: application/json
[Captures]
token: jsonpath "$.token"
[Asserts]
jsonpath "$.count" >= 1 # comment
jsonpath "$.name" not contains "x # y"
header "Location" matches /a#b/
jsonpath "$.list" count == 2
body == ```
line1
GET http://not-a-request
```
status == 200


# Second
GET {{base_url}}/users
HTTP 200
[Asserts]
xpath "string(//h1)" exists
`Hello`
"#;

    #[test]
    fn parses_structure() {
        let entries = parse(SAMPLE);
        assert_eq!(entries.len(), 2);
        let e = &entries[0];
        assert_eq!(e.docs, "Login\nGets a token");
        assert_eq!(e.name(), "Login");
        assert_eq!(
            (e.method.as_str(), e.url.as_str()),
            ("POST", "{{base_url}}/login?x=1")
        );
        assert_eq!((e.start_line, e.end_line), (0, 29));
        assert_eq!(e.headers.len(), 2);
        assert_eq!(e.headers[1].value, "a\\#b");
        assert_eq!(
            e.query,
            vec![KeyValue {
                name: "lang".into(),
                value: "fr".into()
            }]
        );
        assert_eq!(e.basic_auth.as_ref().unwrap().name, "bob");
        assert_eq!(e.options[0].name, "location");
        assert_eq!(e.body.mode, "json");
        assert!(e.body.content.starts_with('{') && e.body.content.ends_with('}'));
        assert!(e.has_response);
        assert_eq!((e.version.as_str(), e.status.as_str()), ("HTTP/1.1", "200"));
        assert_eq!(e.response_headers.len(), 1);
        assert_eq!(e.captures[0].value, "jsonpath \"$.token\"");
        assert_eq!(e.asserts.len(), 6);
        assert_eq!(
            e.asserts[0],
            Assert {
                query: "jsonpath \"$.count\"".into(),
                predicate: ">=".into(),
                value: "1".into()
            }
        );
        assert_eq!(e.asserts[1].predicate, "not contains");
        assert_eq!(e.asserts[1].value, "\"x # y\"");
        assert_eq!(e.asserts[2].value, "/a#b/");
        assert_eq!(e.asserts[3].query, "jsonpath \"$.list\" count");
        assert!(e.asserts[4].value.contains("GET http://not-a-request"));
        assert_eq!(e.asserts[5].query, "status");

        let e = &entries[1];
        assert_eq!(e.docs, "Second");
        assert_eq!(e.asserts[0].predicate, "exists");
        assert_eq!(e.response_body, "`Hello`");
    }

    #[test]
    fn bodies() {
        let text = "POST http://a\n```json\n{\"a\": 1}\n```\n\nPOST http://b\n<a>\n  <b/>\nHTTP 200\n</a>\nHTTP 200\n\nPOST http://c\nfile,data.bin;\n\nPOST http://d\n[Form]\na: 1\n\nPOST http://e\n```\nplain\n```\n\nPOST http://f\nbase64,AAAA\nBBBB;\n";
        let e = parse(text);
        assert_eq!(e.len(), 6);
        assert_eq!((e[0].body.mode.as_str(), e[0].body.fenced), ("json", true));
        assert_eq!(e[0].body.content, "{\"a\": 1}\n");
        assert_eq!(e[1].body.mode, "xml");
        assert!(e[1].body.content.ends_with("</a>"));
        assert!(e[1].has_response);
        assert_eq!(
            (e[2].body.mode.as_str(), e[2].body.content.as_str()),
            ("file", "data.bin")
        );
        assert_eq!(e[3].body.mode, "form");
        assert_eq!(
            (e[4].body.mode.as_str(), e[4].body.content.as_str()),
            ("text", "plain\n")
        );
        assert_eq!(e[5].body.mode, "hurl");
        assert_eq!(e[5].body.content, "base64,AAAA\nBBBB;");
    }

    #[test]
    fn version_alone_is_an_expectation() {
        let entry = Entry {
            url: "http://a".into(),
            version: "HTTP/2".into(),
            ..Entry::default()
        };
        assert!(!entry.has_response && entry.status.is_empty());
        let text = serialize_entry(&entry, "\n");
        assert_eq!(text, "GET http://a\nHTTP/2 *\n");
        let again = &parse(&text)[0];
        assert!(again.has_response);
        assert_eq!(
            (again.version.as_str(), again.status.as_str()),
            ("HTTP/2", "*")
        );
        assert_eq!(checked_entry(&entry, "\n").unwrap(), text);
        // The default version expects nothing.
        for version in ["HTTP", "", " "] {
            let entry = Entry {
                url: "http://a".into(),
                version: version.into(),
                ..Entry::default()
            };
            assert_eq!(
                serialize_entry(&entry, "\n"),
                "GET http://a\n",
                "{version:?}"
            );
        }
    }

    #[test]
    fn unknown_fields_of_an_entry_are_ignored() {
        // Older frontends still send the `line` the model used to hold.
        let entry: Entry = serde_json::from_str(
            r#"{ "index": 2, "line": 7, "method": "PUT", "url": "http://a" }"#,
        )
        .unwrap();
        assert_eq!(
            (entry.index, entry.method.as_str(), entry.url.as_str()),
            (2, "PUT", "http://a")
        );
        assert!(serde_json::to_value(&entry).unwrap().get("line").is_none());
    }

    /// Reference for [`xml_end`]: is the text a complete XML document? Simple
    /// but scans the whole text, which made the detection of the end of a
    /// body quadratic when it was called for every line.
    fn xml_complete(s: &str) -> bool {
        let mut t = s.trim_start();
        loop {
            if t.starts_with("<?") {
                match t.find("?>") {
                    Some(p) => t = t[p + 2..].trim_start(),
                    None => return false,
                }
            } else if t.starts_with("<!--") {
                match t.find("-->") {
                    Some(p) => t = t[p + 3..].trim_start(),
                    None => return false,
                }
            } else if t.starts_with("<!") {
                match t.find('>') {
                    Some(p) => t = t[p + 1..].trim_start(),
                    None => return false,
                }
            } else {
                break;
            }
        }
        if !t.starts_with('<') {
            return false;
        }
        let name: String = t[1..]
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/')
            .collect();
        if name.is_empty() {
            return true;
        }
        let Some(gt) = t.find('>') else {
            return false;
        };
        if t[..gt].ends_with('/') {
            return true;
        }
        let count = |pat: &str| -> usize {
            t.match_indices(pat)
                .filter(|(i, _)| {
                    t[i + pat.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| c.is_whitespace() || c == '>' || c == '/')
                })
                .count()
        };
        let opens = count(&format!("<{name}"));
        let closes = count(&format!("</{name}"));
        opens > 0 && opens == closes
    }

    fn xml_end_reference(lines: &[&str], i: usize) -> Option<usize> {
        let mut acc = String::new();
        (i..lines.len()).find(|&j| {
            acc.push_str(lines[j]);
            acc.push('\n');
            xml_complete(&acc)
        })
    }

    #[test]
    fn xml_end_agrees_with_the_reference() {
        const FRAGMENTS: &[&str] = &[
            "",
            "  ",
            "text",
            "HTTP 200",
            ">",
            "/>",
            "/",
            "<",
            "<>",
            "< a>",
            "</a",
            "</a>",
            "</a></a>",
            "<a",
            "<a>",
            " <a> ",
            "<a/>",
            "<a />",
            "<a b=\"1\">",
            "<a></a>",
            "<a><a>",
            "<a>x</a><a>",
            "<ab>",
            "</ab>",
            "<a\u{a0}b>",
            "<a<a",
            "<a<a<a<a>",
            "</a<a>",
            "<é>",
            "</é>",
            "<?",
            "?>",
            "<?>",
            "<?xml version=\"1.0\"?>",
            "<?xml?><a>",
            "<?xml?> text",
            "<!--",
            "-->",
            "<!-->",
            "<!--->",
            "<!-- c --> <a>x</a>",
            "<!-- <a> -->",
            "<!",
            "<!DOCTYPE a>",
            "<!DOCTYPE a [",
            "]><a/>",
        ];
        let xml_end = |lines: &[&str], i: usize| {
            let mut unlimited = usize::MAX;
            xml_end(lines, i, &mut unlimited)
        };
        let check = |lines: &[&str]| {
            assert_eq!(xml_end(lines, 0), xml_end_reference(lines, 0), "{lines:?}");
        };
        for a in FRAGMENTS {
            check(&[a]);
            for b in FRAGMENTS {
                check(&[a, b]);
                for c in FRAGMENTS {
                    check(&[a, b, c]);
                }
            }
        }
        // Longer documents, from a start line which is not the first one.
        let doc = "GET http://x\n<?xml version=\"1.0\"?>\n<!-- <list> -->\n<list\n  size=\"2\">\n<list>\n</list>\n<listing/>\n</list>\nHTTP 200\n<list>\n";
        let lines: Vec<&str> = doc.lines().collect();
        for i in 0..lines.len() {
            assert_eq!(
                xml_end(&lines, i),
                xml_end_reference(&lines, i),
                "from line {i}"
            );
        }
        assert_eq!(xml_end(&lines, 1), Some(8));
        assert_eq!(xml_end(&lines, 10), None);
    }

    /// A document of `items` lines between `<root>` and `</root>`.
    fn large_xml(items: usize) -> String {
        let mut xml = String::from("<?xml version=\"1.0\"?>\n<root>\n");
        for k in 0..items {
            xml.push_str(&format!(
                "  <item id=\"{k}\">GET http://not-a-request/{k}</item>\n"
            ));
        }
        xml.push_str("</root>");
        xml
    }

    /// Generous bound: the point is to catch a quadratic scan (tens of
    /// seconds on such inputs), not to measure anything.
    const FAST: std::time::Duration = std::time::Duration::from_secs(5);

    #[test]
    fn large_xml_body_is_read_in_linear_time() {
        let xml = large_xml(20_000);
        let text = format!("POST http://a\n{xml}\nHTTP 200\n\nGET http://b\n");
        let started = std::time::Instant::now();
        let entries = parse(&text);
        let elapsed = started.elapsed();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].body.mode, "xml");
        assert_eq!(entries[0].body.content, xml);
        assert_eq!(
            (entries[0].has_response, entries[0].status.as_str()),
            (true, "200")
        );
        assert_eq!(entries[1].url, "http://b");
        assert!(elapsed < FAST, "{elapsed:?}");
    }

    #[test]
    fn unclosed_xml_body_is_read_in_linear_time() {
        // Never closed: the body stops at the next entry, and every later
        // body is searched for its end up to the end of the file.
        let lines: String = (0..20_000)
            .map(|k| format!("  <item id=\"{k}\"/>\n"))
            .collect();
        for open in ["<root>", "<!-- never closed", "<?xml never closed"] {
            let text =
                format!("POST http://a\n{open}\n{lines}\nGET http://b\nHTTP 200\n<root>\n{lines}");
            let started = std::time::Instant::now();
            let entries = parse(&text);
            let elapsed = started.elapsed();
            assert_eq!(entries.len(), 2, "{open}");
            assert_eq!(entries[0].body.mode, "xml", "{open}");
            assert_eq!(
                entries[0].body.content,
                format!("{open}\n{}", lines.trim_end()),
                "{open}"
            );
            assert!(!entries[0].has_response, "{open}");
            assert_eq!(
                entries[1].response_body,
                format!("<root>\n{}", lines.trim_end()),
                "{open}"
            );
            assert!(elapsed < FAST, "{open}: {elapsed:?}");
        }
    }

    #[test]
    fn fruitless_xml_searches_are_bounded() {
        // Each document is searched to the end of the file, in vain.
        let lines = ["<a>", "<b>", "x", "<!-- c", "<?d", "</e>"];
        let mut budget = 100;
        assert_eq!(xml_end(&lines, 0, &mut budget), None);
        assert_eq!(budget, 94);
        assert_eq!(xml_end(&lines, 3, &mut budget), None);
        assert_eq!(budget, 91);
        // Not a document: given up on its first line.
        assert_eq!(xml_end(&lines, 2, &mut budget), None);
        assert_eq!(budget, 90);
        // A search which succeeds costs nothing: its lines are consumed.
        let closed = ["<a>", "<a>", "</a>", "</a>", "<b>"];
        assert_eq!(xml_end(&closed, 0, &mut budget), Some(3));
        assert_eq!(budget, 90);
        // Not searched further than what is left.
        let mut budget = 3;
        assert_eq!(xml_end(&closed, 0, &mut budget), None);
        assert_eq!(budget, 0);
        assert_eq!(xml_end(&closed, 1, &mut budget), None);

        // Thousands of documents which are never closed: without the bound,
        // the file would be read once for each of them.
        let count = 30_000;
        let text: String = (0..count)
            .map(|k| format!("POST http://a/{k}\n<a>\n"))
            .collect();
        let started = std::time::Instant::now();
        let entries = parse(&text);
        let elapsed = started.elapsed();
        assert_eq!(entries.len(), count);
        assert!(
            entries
                .iter()
                .all(|e| e.body.mode == "xml" && e.body.content == "<a>")
        );
        assert!(elapsed < FAST, "{elapsed:?}");
    }

    #[test]
    fn unclosed_multiline_strings_are_read_in_linear_time() {
        // Every line opens a multiline string: each one is closed by the
        // fence of the next line, the last one by nothing.
        let count = 40_001;
        for (section, line) in [
            ("Asserts", "body == ```text"),
            ("Captures", "name: body ```text"),
        ] {
            let lines: String = (0..count).map(|k| format!("{line} {k}\n")).collect();
            let text = format!("GET http://a\nHTTP 200\n[{section}]\n{lines}");
            let started = std::time::Instant::now();
            let entries = parse(&text);
            let elapsed = started.elapsed();
            assert_eq!(entries.len(), 1, "{section}");
            let e = &entries[0];
            let values: Vec<&str> = match section {
                "Asserts" => e.asserts.iter().map(|a| a.value.as_str()).collect(),
                _ => e.captures.iter().map(|c| c.value.as_str()).collect(),
            };
            let value = line.split_once(' ').unwrap().1.trim_start_matches("== ");
            assert_eq!(values.len(), count / 2 + 1, "{section}");
            assert_eq!(values[0], format!("{value} 0\n{line} 1"), "{section}");
            assert_eq!(
                values[count / 2],
                format!("{value} {}", count - 1),
                "{section}"
            );
            assert_eq!(e.end_line, count + 2, "{section}");
            assert!(elapsed < FAST, "{section}: {elapsed:?}");
        }

        // A single fence, then thousands of lines which are not closed by any.
        let lines: String = (0..count)
            .map(|k| format!("header \"X-{k}\" == \"{k}\"\n"))
            .collect();
        let text = format!("GET http://a\nHTTP 200\n[Asserts]\nbody == ```\n{lines}");
        let started = std::time::Instant::now();
        let entries = parse(&text);
        let elapsed = started.elapsed();
        assert_eq!(entries[0].asserts.len(), count + 1);
        assert_eq!(entries[0].asserts[0].value, "```");
        assert_eq!(
            entries[0].asserts[count].query,
            format!("header \"X-{}\"", count - 1)
        );
        assert!(elapsed < FAST, "{elapsed:?}");
    }

    #[test]
    fn bodies_after_a_body_are_read_in_linear_time() {
        let count = 60_000;
        let bodies: Vec<String> = (0..count)
            .map(|k| format!("`body number {k} of the request, kept verbatim`"))
            .collect();
        let text = format!(
            "POST http://a\n{{\"first\": true}}\n{}\nHTTP 200\n",
            bodies.join("\n")
        );
        let started = std::time::Instant::now();
        let entries = parse(&text);
        let elapsed = started.elapsed();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!((e.body.mode.as_str(), e.body.fenced), ("hurl", false));
        assert_eq!(
            e.body.content,
            format!("{{\"first\": true}}\n{}", bodies.join("\n"))
        );
        assert_eq!((e.has_response, e.end_line), (true, count + 2));
        assert!(elapsed < FAST, "{elapsed:?}");
    }

    #[test]
    fn content_after_a_body_is_appended_as_if_the_body_was_rewritten() {
        // Reference: what is written for the body so far, then the content.
        // Simple, but copies the whole body every time.
        let reference = |body: &Body, raw: &str| Body {
            mode: "hurl".into(),
            content: format!("{}\n{raw}", serialize_body(body)),
            fenced: false,
        };
        let contents = [
            "",
            " ",
            "\n",
            "a",
            " a \n",
            "\n\n`a`\n`b`",
            "{\"a\": 1}\n",
            "\u{a0}data.bin\u{2003}",
        ];
        let modes = [
            "none",
            "json",
            "xml",
            "text",
            "graphql",
            "file",
            "hurl",
            "form",
            "multipart",
            "?",
        ];
        for mode in modes {
            for content in contents {
                for fenced in [false, true] {
                    let mut body = Body {
                        mode: mode.into(),
                        content: content.into(),
                        fenced,
                    };
                    for raw in ["`x`", "", " y ", "{\n}", "\n"] {
                        let expected = reference(&body, raw);
                        append_to_body(&mut body, raw);
                        assert_eq!(body, expected, "{mode} {content:?} {fenced} {raw:?}");
                    }
                }
            }
        }
        // Through the reader, after a first body for which nothing is written.
        assert_eq!(
            parse("POST http://a\nfile,;\n`a`\n")[0].body.content,
            "\n`a`"
        );
        assert_eq!(
            parse("POST http://a\nfile,;\n`a`\n`b`\n")[0].body.content,
            "`a`\n`b`"
        );
        let fenced = &parse("POST http://a\n```json\n{}\n```\n1\n<a/>\n")[0].body;
        assert_eq!(
            (fenced.mode.as_str(), fenced.content.as_str()),
            ("hurl", "```json\n{}\n```\n1\n<a/>")
        );
    }

    #[test]
    fn serialization_is_stable() {
        let entries = parse(SAMPLE);
        let text: String = entries
            .iter()
            .map(|e| serialize_entry(e, "\n"))
            .collect::<Vec<_>>()
            .join("\n");
        let again = parse(&text);
        assert_eq!(entries.len(), again.len());
        for (a, b) in entries.iter().zip(&again) {
            let mut a = a.clone();
            let mut b = b.clone();
            for e in [&mut a, &mut b] {
                e.start_line = 0;
                e.end_line = 0;
            }
            assert_eq!(a, b);
        }
    }

    #[test]
    fn replace_keeps_other_entries_verbatim() {
        let mut entry = parse(SAMPLE)[1].clone();
        entry.url = "{{base_url}}/people".into();
        let out = replace_entry(SAMPLE, 2, &entry).unwrap();
        let cut = SAMPLE.find("status == 200\n").unwrap() + "status == 200\n".len();
        assert_eq!(&out[..cut], &SAMPLE[..cut]);
        assert!(out.ends_with("# Second\nGET {{base_url}}/people\nHTTP 200\n[Asserts]\nxpath \"string(//h1)\" exists\n`Hello`\n"));
        assert!(replace_entry(SAMPLE, 3, &entry).is_err());
    }

    #[test]
    fn insert_and_delete() {
        let entry = Entry {
            method: "DELETE".into(),
            url: "http://x/1".into(),
            ..Entry::default()
        };
        let out = insert_entry("", None, &entry).unwrap();
        assert_eq!(out, "DELETE http://x/1\n");
        let out = insert_entry("GET http://a\nHTTP 200\n\n# tail\n", None, &entry).unwrap();
        assert_eq!(
            out,
            "GET http://a\nHTTP 200\n\nDELETE http://x/1\n\n# tail\n"
        );
        let out = insert_entry("GET http://a\n\nGET http://b\n", Some(1), &entry).unwrap();
        assert_eq!(out, "GET http://a\n\nDELETE http://x/1\n\nGET http://b\n");
        assert_eq!(parse(&out).len(), 3);
        let out = delete_entry(&out, 2).unwrap();
        assert_eq!(out, "GET http://a\n\nGET http://b\n");
        let out = delete_entry(&out, 1).unwrap();
        assert_eq!(out, "GET http://b\n");
    }

    #[test]
    fn multiline_payload_is_kept_verbatim() {
        // Trailing blank lines, an empty body and a single blank line are
        // three different payloads.
        for source in [
            "```\na\n\n```",
            "```\n```",
            "```\n\n```",
            "```json\n{}\n```",
        ] {
            let text = format!("POST http://a\n{source}\n");
            let entry = &parse(&text)[0];
            assert_eq!(serialize_entry(entry, "\n"), text, "{source:?}");
        }
        assert_eq!(
            parse("POST http://a\n```\na\n\n```\n")[0].body.content,
            "a\n\n"
        );
        assert_eq!(parse("POST http://a\n```\n```\n")[0].body.content, "");
        // Content typed without a final newline gets the one the syntax needs.
        let mut entry = Entry {
            method: "POST".into(),
            url: "http://a".into(),
            ..Entry::default()
        };
        entry.body = Body {
            mode: "text".into(),
            content: "typed".into(),
            fenced: false,
        };
        assert_eq!(
            serialize_entry(&entry, "\n"),
            "POST http://a\n```\ntyped\n```\n"
        );
    }

    #[test]
    fn hash_is_escaped_in_unquoted_values() {
        let mut entry = Entry {
            url: "http://a/page#top".into(),
            ..Entry::default()
        };
        entry.headers.push(KeyValue {
            name: "X-Key".into(),
            value: "a#b \\# c".into(),
        });
        entry.basic_auth = Some(KeyValue {
            name: "bob".into(),
            value: "pa#ss".into(),
        });
        entry.captures.push(KeyValue {
            name: "id".into(),
            value: "jsonpath \"$.a#b\"".into(),
        });
        entry.has_response = true;
        let text = serialize_entry(&entry, "\n");
        assert!(text.contains("GET http://a/page\\#top\n"), "{text}");
        assert!(text.contains("X-Key: a\\#b \\# c\n"), "{text}");
        assert!(text.contains("bob: pa\\#ss\n"), "{text}");
        assert!(text.contains("id: jsonpath \"$.a#b\"\n"), "{text}");
        // Nothing is truncated when the text is read back, and writing is stable.
        let again = &parse(&text)[0];
        assert_eq!(again.url, "http://a/page\\#top");
        assert_eq!(again.basic_auth.as_ref().unwrap().value, "pa\\#ss");
        assert_eq!(serialize_entry(again, "\n"), text);
    }

    #[test]
    fn refuses_entries_that_would_break_the_file() {
        let file = "GET http://a\nHTTP 200\n\nGET http://b\n";
        let ok = parse(file)[1].clone();
        for (what, broken) in [
            (
                "empty url",
                Entry {
                    url: "  ".into(),
                    ..ok.clone()
                },
            ),
            (
                "status",
                Entry {
                    status: "2xx".into(),
                    ..ok.clone()
                },
            ),
            (
                "method",
                Entry {
                    method: "get it".into(),
                    ..ok.clone()
                },
            ),
            (
                "second request in a raw body",
                Entry {
                    body: Body {
                        mode: "hurl".into(),
                        content: "`x`\nGET http://c".into(),
                        fenced: false,
                    },
                    ..ok.clone()
                },
            ),
        ] {
            assert!(replace_entry(file, 2, &broken).is_err(), "{what}");
            assert!(insert_entry(file, None, &broken).is_err(), "{what}");
        }
        assert_eq!(replace_entry(file, 2, &ok).unwrap(), file);
    }

    #[test]
    fn byte_order_mark_is_ignored_and_preserved() {
        let file = "\u{feff}GET http://a\nHTTP 200\n\nGET http://b\n";
        let entries = parse(file);
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].index, entries[0].url.as_str()), (1, "http://a"));
        let mut first = entries[0].clone();
        first.status = "201".into();
        let out = replace_entry(file, 1, &first).unwrap();
        assert_eq!(out, "\u{feff}GET http://a\nHTTP 201\n\nGET http://b\n");
        assert_eq!(delete_entry(file, 1).unwrap(), "\u{feff}GET http://b\n");
        let added = insert_entry(file, None, &entries[1]).unwrap();
        assert!(added.starts_with('\u{feff}') && parse(&added).len() == 3);
    }

    #[test]
    fn crlf_is_preserved() {
        let text = "GET http://a\r\nHTTP 200\r\n\r\nGET http://b\r\n";
        let mut entry = parse(text)[0].clone();
        assert_eq!(entry.status, "200");
        entry.status = "201".into();
        let out = replace_entry(text, 1, &entry).unwrap();
        assert_eq!(out, "GET http://a\r\nHTTP 201\r\n\r\nGET http://b\r\n");
    }
}
