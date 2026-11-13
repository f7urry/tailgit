//! Small token-based highlighter for the merge window (IntelliJ "Light" / "Dark" colours).
//!
//! It is deliberately simple (no multi-line state): keywords, strings, numbers, comments,
//! annotations, method names, constants and field declarations are enough to read a diff.

use eframe::egui::{
    text::{LayoutJob, TextFormat},
    Color32, FontId,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Java,
    Js,
    Rust,
    Python,
    Plain,
}

impl Lang {
    pub fn from_path(path: &str) -> Lang {
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "java" | "kt" | "kts" | "scala" | "groovy" | "gradle" | "cs" | "c" | "h" | "cpp" | "hpp" | "cc" | "go"
            | "swift" | "dart" => Lang::Java,
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Lang::Js,
            "rs" => Lang::Rust,
            "py" => Lang::Python,
            _ => Lang::Plain,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    Str,
    Number,
    Comment,
    Annotation,
    Method,
    Field,
}

/// Every colour the merge window uses.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub text: Color32,
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub annotation: Color32,
    pub method: Color32,
    pub field: Color32,
    pub lineno: Color32,
    pub sep: Color32,
    pub icon: Color32,
    pub icon_dim: Color32,
    pub link: Color32,
    pub hover: Color32,
    pub fold: Color32,
    pub btn_border: Color32,
    pub primary: Color32,
    pub side_fill: Color32,
    pub side_border: Color32,
    pub result_fill: Color32,
    pub applied_fill: Color32,
    pub applied_border: Color32,
    pub conflict_side_fill: Color32,
    pub conflict_border: Color32,
    pub conflict_fill: Color32,
    pub conflict_mark: Color32,
    pub thumb: Color32,
}

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: c(30, 31, 34),
            text: c(188, 190, 196),
            keyword: c(207, 142, 109),
            string: c(106, 171, 115),
            number: c(42, 172, 184),
            comment: c(122, 126, 133),
            annotation: c(179, 174, 96),
            method: c(86, 168, 245),
            field: c(199, 125, 187),
            lineno: c(110, 114, 122),
            sep: c(57, 59, 63),
            icon: c(157, 160, 170),
            icon_dim: c(92, 95, 102),
            link: c(84, 138, 247),
            hover: c(57, 59, 63),
            fold: c(43, 45, 48),
            btn_border: c(87, 89, 94),
            primary: c(53, 116, 240),
            side_fill: c(43, 45, 48),
            side_border: c(88, 90, 95),
            result_fill: c(66, 68, 72),
            applied_fill: c(54, 56, 60),
            applied_border: c(78, 80, 85),
            conflict_side_fill: c(74, 50, 52),
            conflict_border: c(140, 82, 84),
            conflict_fill: c(94, 56, 58),
            conflict_mark: c(150, 84, 86),
            thumb: Color32::from_white_alpha(70),
        }
    } else {
        Palette {
            bg: Color32::WHITE,
            text: c(8, 8, 8),
            keyword: c(0, 51, 179),
            string: c(6, 125, 23),
            number: c(23, 80, 235),
            comment: c(140, 140, 140),
            annotation: c(158, 136, 13),
            method: c(0, 98, 122),
            field: c(135, 16, 148),
            lineno: c(153, 153, 153),
            sep: c(235, 236, 240),
            icon: c(108, 112, 126),
            icon_dim: c(190, 192, 198),
            link: c(53, 116, 240),
            hover: c(235, 236, 240),
            fold: c(247, 248, 250),
            btn_border: c(201, 204, 214),
            primary: c(53, 116, 240),
            side_fill: c(242, 242, 242),
            side_border: c(196, 196, 196),
            result_fill: c(216, 216, 216),
            applied_fill: c(228, 228, 228),
            applied_border: c(205, 205, 205),
            conflict_side_fill: c(255, 228, 224),
            conflict_border: c(240, 170, 160),
            conflict_fill: c(255, 206, 201),
            conflict_mark: c(255, 190, 184),
            thumb: Color32::from_black_alpha(60),
        }
    }
}

impl Palette {
    fn color(&self, k: Kind) -> Color32 {
        match k {
            Kind::Plain => self.text,
            Kind::Keyword => self.keyword,
            Kind::Str => self.string,
            Kind::Number => self.number,
            Kind::Comment => self.comment,
            Kind::Annotation => self.annotation,
            Kind::Method => self.method,
            Kind::Field => self.field,
        }
    }
}

const JAVA_KW: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue", "default",
    "do", "double", "else", "enum", "extends", "final", "finally", "float", "for", "goto", "if", "implements",
    "import", "instanceof", "int", "interface", "long", "native", "new", "package", "private", "protected", "public",
    "return", "short", "static", "strictfp", "super", "switch", "synchronized", "this", "throw", "throws",
    "transient", "try", "void", "volatile", "while", "var", "record", "true", "false", "null", "fun", "val",
    "object", "when", "is", "in", "override", "data", "sealed", "interface", "namespace", "using", "func", "defer",
];
const JS_KW: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do",
    "else", "enum", "export", "extends", "false", "finally", "for", "from", "function", "if", "implements", "import",
    "in", "instanceof", "interface", "let", "new", "null", "of", "private", "protected", "public", "readonly",
    "return", "static", "super", "switch", "this", "throw", "true", "try", "type", "typeof", "undefined", "var",
    "void", "while", "with", "yield",
];
const RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
];
const PY_KW: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not",
    "or", "pass", "raise", "return", "True", "try", "while", "with", "yield",
];

fn keywords(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Java => JAVA_KW,
        Lang::Js => JS_KW,
        Lang::Rust => RUST_KW,
        Lang::Python => PY_KW,
        Lang::Plain => &[],
    }
}

/// Byte range of the field name when the line is a Java-style field declaration.
fn field_decl(line: &str, lang: Lang) -> Option<(usize, usize)> {
    if lang != Lang::Java {
        return None;
    }
    let t = line.trim_start();
    let base = line.len() - t.len();
    const MODS: [&str; 6] = ["private ", "protected ", "public ", "static ", "final ", "volatile "];
    if !MODS.iter().any(|m| t.starts_with(m)) || !t.trim_end().ends_with(';') {
        return None;
    }
    let head = t.split(['=', ';']).next().unwrap_or("");
    if head.contains('(') {
        return None;
    }
    let head = head.trim_end();
    let start = head.rfind(char::is_whitespace).map(|p| p + 1).unwrap_or(0);
    (start < head.len()).then_some((base + start, base + head.len()))
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}
fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Contiguous `(start, end, kind)` byte ranges covering `line` (no trailing newline).
pub fn tokenize(line: &str, lang: Lang) -> Vec<(usize, usize, Kind)> {
    let n = line.len();
    if lang == Lang::Plain || n == 0 {
        return vec![(0, n, Kind::Plain)];
    }
    let b = line.as_bytes();
    let trimmed = line.trim_start();
    let whole_comment = if lang == Lang::Python {
        trimmed.starts_with('#')
    } else {
        trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("* ") || trimmed == "*" || trimmed.starts_with("*/")
    };
    if whole_comment {
        return vec![(0, n, Kind::Comment)];
    }
    let kw = keywords(lang);
    let field = field_decl(line, lang);
    let mut out: Vec<(usize, usize, Kind)> = Vec::new();
    let mut push = |s: usize, e: usize, k: Kind| {
        if let Some(last) = out.last_mut() {
            if last.2 == k && last.1 == s {
                last.1 = e;
                return;
            }
        }
        out.push((s, e, k));
    };
    let mut i = 0;
    while i < n {
        let ch = b[i];
        if ch.is_ascii_whitespace() {
            let mut j = i;
            while j < n && b[j].is_ascii_whitespace() {
                j += 1;
            }
            push(i, j, Kind::Plain);
            i = j;
        } else if (lang == Lang::Python && ch == b'#') || (lang != Lang::Python && line[i..].starts_with("//")) {
            push(i, n, Kind::Comment);
            i = n;
        } else if lang != Lang::Python && line[i..].starts_with("/*") {
            let end = line[i + 2..].find("*/").map(|p| i + 2 + p + 2).unwrap_or(n);
            push(i, end, Kind::Comment);
            i = end;
        } else if ch == b'"' || (ch == b'\'' && lang != Lang::Rust) || (ch == b'`' && lang == Lang::Js) {
            let q = ch;
            let mut j = i + 1;
            while j < n {
                if b[j] == b'\\' {
                    j += 2;
                } else if b[j] == q {
                    j += 1;
                    break;
                } else {
                    j += 1;
                }
            }
            let j = j.min(n);
            // Never split a multi-byte character.
            let j = (j..=n).find(|&k| line.is_char_boundary(k)).unwrap_or(n);
            push(i, j, Kind::Str);
            i = j;
        } else if ch == b'@' && matches!(lang, Lang::Java | Lang::Js) && i + 1 < n && is_ident_start(b[i + 1]) {
            let mut j = i + 1;
            while j < n && (is_ident(b[j]) || b[j] == b'.') {
                j += 1;
            }
            push(i, j, Kind::Annotation);
            i = j;
        } else if ch.is_ascii_digit() {
            let mut j = i;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || (b[j] == b'.' && j + 1 < n && b[j + 1].is_ascii_digit())) {
                j += 1;
            }
            push(i, j, Kind::Number);
            i = j;
        } else if is_ident_start(ch) {
            let mut j = i;
            while j < n && is_ident(b[j]) {
                j += 1;
            }
            let word = &line[i..j];
            let next = line[j..].trim_start().bytes().next();
            let kind = if kw.contains(&word) {
                Kind::Keyword
            } else if next == Some(b'(') && ch.is_ascii_lowercase() {
                Kind::Method
            } else if word.len() > 1 && word.bytes().all(|x| x.is_ascii_uppercase() || x.is_ascii_digit() || x == b'_') {
                Kind::Field
            } else if field == Some((i, j)) {
                Kind::Field
            } else {
                Kind::Plain
            };
            push(i, j, kind);
            i = j;
        } else {
            let len = line[i..].chars().next().map_or(1, char::len_utf8);
            push(i, i + len, Kind::Plain);
            i += len;
        }
    }
    out
}

/// A layout job for `text` (any number of lines) with every row `line_height` tall and no wrapping.
pub fn job(text: &str, lang: Lang, pal: &Palette, font: &FontId, line_height: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let fmt = |k: Kind| TextFormat {
        font_id: font.clone(),
        color: pal.color(k),
        italics: k == Kind::Comment,
        line_height: Some(line_height),
        ..Default::default()
    };
    for raw in text.split_inclusive('\n') {
        let (body, nl) = match raw.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (raw, ""),
        };
        for (s, e, k) in tokenize(body, lang) {
            job.append(&body[s..e], 0.0, fmt(k));
        }
        if !nl.is_empty() {
            job.append(nl, 0.0, fmt(Kind::Plain));
        }
    }
    if job.sections.is_empty() {
        job.append("", 0.0, fmt(Kind::Plain));
    }
    job
}
