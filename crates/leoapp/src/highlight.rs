//! Colouring the body, following the `@language` directives inside it.
//!
//! The language a node is written in comes from the model:
//! `Outline::get_language` implements Leo's four-pass rule over the node, its
//! ancestors and the nearest `@<file>` extension. That is the language the
//! body *starts* in. A body may then change it: Leo's `match_at_language`
//! matches `@language` only at column 0 and switches from that line onward, so
//! one node can hold Python and then C, and this follows it.
//!
//! Two engines. A dozen languages have a tree-sitter grammar compiled in and
//! go through `treesit`, which can tell a function from a field. The rest --
//! Leo knows comment delimiters for some 170 -- go through the line scanner
//! below, which classifies a word by looking it up in `keywords`.
//!
//! Both live here rather than in `leolib` for the reason Leo keeps
//! `leoColorizer` out of its model: colouring is a view's business.

use std::ops::Range;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering::SeqCst};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use leolib::outline::set_delims_from_language;
use leolib::Outline;

use crate::keywords;
use crate::treesit;

/// What a run of characters is, and therefore how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Plain,
    /// A Leo directive: `@language`, `@others`, `@tabwidth`.
    Directive,
    /// A section reference, `<< like this >>`.
    Section,
    Comment,
    Str,
    /// A literal value: a number, a boolean, a named constant.
    Number,
    Keyword,
    /// A builtin function: `print`, `strlen`.
    ///
    /// Also where the line scanner puts jEdit's `keyword2` to `keyword4`,
    /// which is one list with no way to tell a function from a type. Those
    /// lists are mostly library functions and variables -- 138 for Lua, 303
    /// for Tcl, 174 for Scheme -- against Objective-C's 19, which are types.
    BuiltinFunction,
    /// A builtin type: `int`, `u8`.
    BuiltinType,
    /// A builtin constant: `None`, `true`, `nil`.
    BuiltinConstant,
    /// A function or method, defined or called.
    Function,
    /// A type, or a constructor of one.
    Type,
    /// A field or attribute of an object.
    Property,
    /// A decorator or annotation: `@property`, `#[derive(Debug)]`.
    Attribute,
}

/// A run of one class, as byte offsets into its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub class: Class,
}

/// Leo's directives, which are coloured wherever the language is.
const DIRECTIVES: &[&str] = &[
    "@language",
    "@others",
    "@tabwidth",
    "@pagewidth",
    "@encoding",
    "@lineending",
    "@comment",
    "@delims",
    "@section-delims",
    "@first",
    "@last",
    "@path",
    "@nocolor-node",
    "@nocolor",
    "@killcolor",
    "@color",
    "@nowrap",
    "@wrap",
    "@nosearch",
    "@ignore",
    "@all",
    "@raw",
    "@end_raw",
    "@c",
    "@code",
    "@doc",
];

/// How a language writes strings and block comments.
///
/// The importers keep a `string_list` too, and this used to read it. They
/// answer a different question -- where does a block of code start -- and
/// Rust's is deliberately empty because its importer scans for itself. Read as
/// a colouring rule that says Rust has no strings, so `//` inside a literal
/// opened a comment that ran to the end of the line.
#[derive(Clone, Copy)]
struct Lex {
    /// String delimiters, longest first.
    strings: &'static [&'static str],
    /// A backslash escapes the next character inside a string.
    escapes: bool,
    /// Block comments nest, so the first close does not end the outermost.
    nested_comments: bool,
}

const DEFAULT_LEX: Lex = Lex {
    strings: &["\"", "'"],
    escapes: true,
    nested_comments: false,
};

/// `language`'s lexical shape, where it differs from `DEFAULT_LEX`.
fn lex_for(language: &str) -> Lex {
    /// `'x'` is a character literal, so a lone `'` must not open a string.
    const CHARS: &[&str] = &["\""];
    const TRIPLE: &[&str] = &["\"\"\"", "'''", "\"", "'"];
    match language {
        "python" | "cython" | "coffeescript" => Lex {
            strings: TRIPLE,
            ..DEFAULT_LEX
        },
        "c" | "cplusplus" | "csharp" | "java" | "objective_c" | "go" | "groovy" | "kotlin"
        | "swift" | "clojure" | "elisp" | "lisp" | "erlang" => Lex {
            strings: CHARS,
            ..DEFAULT_LEX
        },
        "rust" | "scala" | "d" | "dart" | "haskell" | "ocaml" | "scheme" => Lex {
            strings: CHARS,
            nested_comments: true,
            ..DEFAULT_LEX
        },
        // `''` is the escape, so a backslash is an ordinary character.
        "sql" | "pascal" | "fortran" | "fortran90" | "ada" | "vbscript" => Lex {
            escapes: false,
            ..DEFAULT_LEX
        },
        _ => DEFAULT_LEX,
    }
}

/// One language's lexical shape, from the model's tables and `lex_for`.
struct Rules {
    line_comment: String,
    block_start: String,
    block_end: String,
    lex: Lex,
    keywords: &'static [&'static str],
    builtins: &'static [&'static str],
}

impl Rules {
    fn for_language(language: &str) -> Self {
        let (line_comment, block_start, block_end) = set_delims_from_language(language);
        let (keywords, builtins) = keywords::for_language(language);
        Self {
            line_comment,
            block_start,
            block_end,
            lex: lex_for(language),
            keywords,
            builtins,
        }
    }

    /// The opening delimiter to count when a block comment nests.
    fn nesting_open(&self) -> &str {
        match self.lex.nested_comments {
            true => &self.block_start,
            false => "",
        }
    }
}

/// What carries over from one line to the next, inside one region.
#[derive(Default)]
struct State {
    /// The delimiter that closes an open string or block comment.
    target: String,
    /// True while the open target is a comment rather than a string.
    target_is_comment: bool,
    /// How many nested block comments are open, when the language nests them.
    depth: usize,
}

/// A run of lines in one language, as a half-open range.
struct Region {
    start: usize,
    end: usize,
    language: String,
}

/// The colourings of the bodies last shown, and the text each was made from.
///
/// `highlight` parses the whole body, and the body pane redraws on every key.
/// A node holding a function costs microseconds; an `@edit` node holds a whole
/// file in one body, where the parse costs tens of milliseconds: 35 ms a key
/// at 5,000 lines. So past `WHOLE_UNDER` lines an edit recolours only the
/// lines it changed, with `CONTEXT` lines either side, and keeps the rest;
/// the whole body is coloured again once typing has paused for `SETTLE`. A
/// body seen for the first time is coloured on screen first. The whole
/// colouring is made on the `worker` thread, and `poll` puts it in. `KEEP` bodies are
/// kept, so switching back to one is free. Only the shown body is coloured
/// whole: a hidden one keeps what it has, and is asked for again when shown.
#[derive(Default)]
pub struct Colouring {
    /// Most recently used first.
    entries: Vec<Entry>,
}

/// One body's colouring.
struct Entry {
    /// The node's gnx.
    node: String,
    spans: Rc<Vec<Vec<Span>>>,
    /// The text and language `spans` colour: the cache key, compared rather
    /// than hashed, and what an edit is measured against.
    lines: Arc<Vec<String>>,
    language: String,
    /// When a partial colouring is due to be made whole.
    due: Option<Instant>,
    /// The whole colouring of `lines`, asked of the worker.
    job: Option<Pending>,
}

/// A whole colouring asked of the worker. Hiding the body marks it
/// `UNWANTED`, so the worker skips it if it has not begun; one begun is
/// finished and kept for when the body is shown again.
struct Pending {
    receive: Receiver<Vec<Vec<Span>>>,
    state: Arc<AtomicU8>,
}

/// A job's state, shared by the entry and the worker.
const WANTED: u8 = 0;
const UNWANTED: u8 = 1;
const STARTED: u8 = 2;
const SKIPPED: u8 = 3;

impl Pending {
    /// Move the state from `from` to `to`, if it is still `from`.
    fn set(&self, from: u8, to: u8) {
        let _ = self.state.compare_exchange(from, to, SeqCst, SeqCst);
    }
}

impl Drop for Pending {
    /// Dropped after an edit or out of the cache: nobody can receive it.
    fn drop(&mut self) {
        self.set(WANTED, UNWANTED);
    }
}

/// A body for the worker to colour whole.
struct Job {
    lines: Arc<Vec<String>>,
    language: String,
    state: Arc<AtomicU8>,
    reply: Sender<Vec<Vec<Span>>>,
}

/// The one thread that colours bodies whole, for every outline open. Jobs
/// run in order, and one nobody wants any more is skipped.
fn worker() -> &'static Sender<Job> {
    static WORKER: OnceLock<Sender<Job>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (send, receive) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("colouring".into())
            .spawn(move || {
                for job in receive {
                    let start = job.state.compare_exchange(WANTED, STARTED, SeqCst, SeqCst);
                    if start.is_err() {
                        job.state.store(SKIPPED, SeqCst);
                        continue;
                    }
                    // A panic drops the reply, and the entry keeps its
                    // partial colouring; the worker lives on for the rest.
                    let spans =
                        catch_unwind(AssertUnwindSafe(|| highlight(&job.lines, &job.language)));
                    if let Ok(spans) = spans {
                        let _ = job.reply.send(spans);
                    }
                }
            })
            .expect("start the colouring thread");
        send
    })
}

/// A body this short is coloured whole on every change.
const WHOLE_UNDER: usize = 500;
/// Lines either side of an edit coloured with it, for a construct that spans
/// lines, such as a string, to come out right near the edit.
const CONTEXT: usize = 20;
/// How long typing must pause before a partial colouring is made whole.
pub const SETTLE: Duration = Duration::from_millis(300);
/// How many bodies' colourings are kept.
const KEEP: usize = 4;
/// How often to look for a whole colouring being made on a thread.
const JOB_POLL: Duration = Duration::from_millis(10);

impl Colouring {
    /// The colouring of `lines`, the body of `node`, recomputed only when they
    /// have changed. In a long body only the lines near a change, or the
    /// `visible` ones on a first visit, are coloured until the whole is ready.
    pub fn of(
        &mut self,
        node: &str,
        lines: &[String],
        language: &str,
        visible: Range<usize>,
    ) -> Rc<Vec<Vec<Span>>> {
        let now = Instant::now();
        let at = self.entries.iter().position(|e| e.node == node);
        let mut entry = match at {
            Some(i) => self.entries.remove(i),
            None => Entry {
                node: node.to_string(),
                spans: Rc::default(),
                lines: Arc::default(),
                language: String::new(),
                due: None,
                job: None,
            },
        };
        if let Some(job) = &entry.job {
            job.set(UNWANTED, WANTED);
        }
        entry.collect();
        if entry.language != language || entry.lines.as_slice() != lines {
            let long = lines.len() > WHOLE_UNDER;
            let (spans, due) = if long && entry.language == language && !entry.lines.is_empty() {
                let spans = patch(&entry.lines, &entry.spans, lines, language);
                (spans, Some(now + SETTLE))
            } else if long {
                let (a, b) = (visible.start.min(lines.len()), visible.end.min(lines.len()));
                (highlight_window(lines, language, a, b), Some(now))
            } else {
                (highlight(lines, language), None)
            };
            entry.spans = Rc::new(spans);
            entry.due = due;
            entry.job = None;
            entry.lines = Arc::new(lines.to_vec());
            entry.language = language.to_string();
        }
        entry.start_if_due(now);
        let spans = Rc::clone(&entry.spans);
        if let Some(hidden) = self.entries.first_mut() {
            hidden.collect();
            if let Some(job) = &hidden.job {
                job.set(WANTED, UNWANTED);
            }
        }
        self.entries.insert(0, entry);
        self.entries.truncate(KEEP);
        spans
    }

    /// Put in the shown body's whole colouring if the worker has made it, or
    /// ask for it if it is due. True if the colouring changed.
    pub fn poll(&mut self) -> bool {
        let Some(entry) = self.entries.first_mut() else {
            return false;
        };
        let changed = entry.collect();
        entry.start_if_due(Instant::now());
        changed
    }

    /// How long until `poll` has work for the shown body, if it will have any.
    pub fn due_in(&self) -> Option<Duration> {
        let entry = self.entries.first()?;
        match entry.job {
            Some(_) => Some(JOB_POLL),
            None => Some(entry.due?.saturating_duration_since(Instant::now())),
        }
    }

    /// Wait for the shown body's whole colouring, as a frame later would get it.
    #[cfg(test)]
    fn wait(&mut self) {
        let entry = &mut self.entries[0];
        entry.start_if_due(Instant::now());
        let spans = entry.job.take().unwrap().receive.recv().unwrap();
        entry.spans = Rc::new(spans);
        entry.due = None;
    }
}

impl Entry {
    /// Ask the worker to colour `lines` whole, if that is due and not asked.
    fn start_if_due(&mut self, now: Instant) {
        if self.job.is_some() || !self.due.is_some_and(|t| t <= now) {
            return;
        }
        let (reply, receive) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(WANTED));
        // A failed send drops `reply`, which `collect` sees as a failure.
        let _ = worker().send(Job {
            lines: Arc::clone(&self.lines),
            language: self.language.clone(),
            state: Arc::clone(&state),
            reply,
        });
        self.job = Some(Pending { receive, state });
    }

    /// Put in the worker's whole colouring, if it is done. True if it was.
    fn collect(&mut self) -> bool {
        let Some(job) = &self.job else { return false };
        match job.receive.try_recv() {
            Ok(spans) => {
                self.spans = Rc::new(spans);
                self.due = None;
                self.job = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            // Skipped while hidden: still due, so asked again when shown.
            Err(TryRecvError::Disconnected) if job.state.load(SeqCst) == SKIPPED => {
                self.job = None;
                false
            }
            // The colouring panicked, or the worker is gone: keep the
            // partial colouring rather than ask for another that would fail.
            Err(TryRecvError::Disconnected) => {
                self.due = None;
                self.job = None;
                false
            }
        }
    }
}

/// `new`'s colouring from `old`'s: the lines both share at the start and the
/// end keep their spans, and the lines between are coloured afresh with
/// `CONTEXT` lines either side.
fn patch(
    old: &[String],
    old_spans: &[Vec<Span>],
    new: &[String],
    language: &str,
) -> Vec<Vec<Span>> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) = (prefix, new.len() - suffix);
    let mut out: Vec<Vec<Span>> = old_spans[..prefix].to_vec();
    out.extend(highlight_window(new, language, a, b).drain(a..b));
    out.extend_from_slice(&old_spans[old.len() - suffix..]);
    out
}

/// The colouring of `lines` with only lines `a..b` parsed, and `CONTEXT`
/// lines either side. Leo's directives and the regions they cut are found in
/// the whole body, which is a scan and cheap.
fn highlight_window(lines: &[String], language: &str, a: usize, b: usize) -> Vec<Vec<Span>> {
    let mut out: Vec<Vec<Span>> = vec![Vec::new(); lines.len()];
    let (masked, regions) = plan(lines, language, &mut out);
    let (w0, w1) = (a.saturating_sub(CONTEXT), (b + CONTEXT).min(lines.len()));
    for region in regions {
        let (from, to) = (region.start.max(w0), region.end.min(w1));
        if from >= to {
            continue;
        }
        let slice = &masked[from..to];
        let spans = treesit::highlight(slice, &region.language).unwrap_or_else(|| {
            let rules = Rules::for_language(&region.language);
            let mut state = State::default();
            slice
                .iter()
                .map(|line| scan(line, &rules, &mut state))
                .collect()
        });
        for (k, line_spans) in spans.into_iter().enumerate() {
            if out[from + k].is_empty() {
                out[from + k] = line_spans;
            }
        }
    }
    out
}

/// Colour a body.
///
/// `language` is where the body starts, from `Outline::get_language`. The
/// result has one entry per line, holding only the runs that are not plain.
///
/// Two passes. The first claims Leo's own lines -- directives and section
/// references -- and cuts the body into regions, one per `@language` and one
/// either side of a `@nocolor` block. The second colours each region, with
/// tree-sitter where a grammar exists and the line scanner where it does not.
pub fn highlight(lines: &[String], language: &str) -> Vec<Vec<Span>> {
    let mut out: Vec<Vec<Span>> = vec![Vec::new(); lines.len()];
    let (masked, regions) = plan(lines, language, &mut out);
    for region in regions {
        let slice = &masked[region.start..region.end];
        let spans = treesit::highlight(slice, &region.language).unwrap_or_else(|| {
            let rules = Rules::for_language(&region.language);
            let mut state = State::default();
            slice
                .iter()
                .map(|line| scan(line, &rules, &mut state))
                .collect()
        });
        for (k, line_spans) in spans.into_iter().enumerate() {
            if out[region.start + k].is_empty() {
                out[region.start + k] = line_spans;
            }
        }
    }
    out
}

/// Claim Leo's own lines and cut the rest into single-language regions.
///
/// Returns the body with every claimed line blanked. A parser meeting
/// `@others` or `<< a section >>` reports an error there and mis-reads the
/// lines after it; spaces of the same byte width keep every offset intact.
fn plan(lines: &[String], language: &str, out: &mut [Vec<Span>]) -> (Vec<String>, Vec<Region>) {
    let mut masked: Vec<String> = Vec::with_capacity(lines.len());
    let mut regions: Vec<Region> = Vec::new();
    let mut start: Option<usize> = Some(0);
    let mut lang = language.to_string();
    let mut colouring = true;
    let mut killed = false;
    let mut close = |start: &mut Option<usize>, end: usize, lang: &str| {
        if let Some(from) = start.take() {
            if from < end {
                regions.push(Region {
                    start: from,
                    end,
                    language: lang.to_string(),
                });
            }
        }
    };

    for (i, line) in lines.iter().enumerate() {
        if killed {
            masked.push(blanked(line));
            continue;
        }
        if let Some((name, at)) = directive_at(line) {
            out[i] = vec![Span {
                start: at,
                end: line.trim_end().len(),
                class: Class::Directive,
            }];
            masked.push(blanked(line));
            match name {
                "@language" => {
                    let word: String = line[at + name.len()..]
                        .trim()
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    close(&mut start, i, &lang);
                    if !word.is_empty() {
                        lang = word;
                    }
                    // A language change inside a `@nocolor` block does not
                    // end it; only `@color` does.
                    if colouring {
                        start = Some(i + 1);
                    }
                }
                "@nocolor" | "@nocolor-node" => {
                    close(&mut start, i, &lang);
                    colouring = false;
                }
                "@color" => {
                    close(&mut start, i, &lang);
                    colouring = true;
                    start = Some(i + 1);
                }
                "@killcolor" => {
                    close(&mut start, i, &lang);
                    killed = true;
                }
                _ => {}
            }
            continue;
        }
        if start.is_none() {
            masked.push(blanked(line));
            continue;
        }
        if let Some(span) = section_reference(line) {
            out[i] = vec![span];
            masked.push(blanked(line));
            continue;
        }
        masked.push(line.clone());
    }
    close(&mut start, lines.len(), &lang);
    (masked, regions)
}

/// The line's width in spaces, so a parser skips it without losing offsets.
fn blanked(line: &str) -> String {
    " ".repeat(line.len())
}

/// The language declared at `p`, which the body may then change.
///
/// None when nothing above `p` declares one. Leo would answer its
/// `target_language` and colour the node as Python, which paints a prose node
/// wrong: `class` and `import` become keywords and an apostrophe opens a
/// string that swallows the rest of the body. A node nobody declared a
/// language for is left alone.
pub fn language_of(outline: &Outline, p: &leolib::Position) -> Option<String> {
    outline.language_at(p)
}

/// The directive starting `line`, and the column it starts at.
///
/// Leo's `directiveKind4` matches a directive at column 0, and `@others` and
/// `@all` after leading whitespace as well. Those two are the ones that sit
/// inside a class body, and left unclaimed a parser reads `@others` as a
/// decorator on whatever follows it.
pub(crate) fn directive_at(line: &str) -> Option<(&'static str, usize)> {
    if let Some(name) = DIRECTIVES.iter().find(|d| starts_word(line, d)).copied() {
        return Some((name, 0));
    }
    let indent = line.len() - line.trim_start().len();
    ["@others", "@all"]
        .into_iter()
        .find(|d| starts_word(&line[indent..], d))
        .map(|name| (name, indent))
}

/// True if `word` starts `line` and is not glued to more word characters.
fn starts_word(line: &str, word: &str) -> bool {
    let Some(rest) = line.strip_prefix(word) else {
        return false;
    };
    match rest.chars().next() {
        None => true,
        Some(c) => !(c.is_alphanumeric() || c == '_' || c == '-'),
    }
}

/// A line holding nothing but a section reference.
fn section_reference(line: &str) -> Option<Span> {
    let trimmed = line.trim();
    if !(trimmed.starts_with("<<") && trimmed.ends_with(">>") && trimmed.len() > 4) {
        return None;
    }
    let start = line.len() - line.trim_start().len();
    Some(Span {
        start,
        end: start + trimmed.len(),
        class: Class::Section,
    })
}

/// Comments, strings, numbers and keywords in one line of code.
fn scan(line: &str, rules: &Rules, state: &mut State) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut run_start: Option<usize> = None;

    // A string or comment left open by the previous line.
    if !state.target.is_empty() {
        let (class, close) = match state.target_is_comment {
            true => (
                Class::Comment,
                find_block_close(line, 0, rules.nesting_open(), &state.target, state.depth),
            ),
            false => (
                Class::Str,
                find_close(line, 0, &state.target, rules.lex.escapes).ok_or(0),
            ),
        };
        match close {
            Ok(end) => {
                spans.push(Span {
                    start: 0,
                    end,
                    class,
                });
                i = end;
                state.target.clear();
                state.depth = 0;
            }
            Err(depth) => {
                state.depth = depth;
                return vec![Span {
                    start: 0,
                    end: line.trim_end_matches('\n').len(),
                    class,
                }];
            }
        }
    }

    while i < bytes.len() {
        let rest = &line[i..];
        if rest.starts_with('\n') {
            break;
        }
        // A line comment runs to the end of the line.
        if !rules.line_comment.is_empty() && rest.starts_with(&rules.line_comment) {
            flush_word(line, &mut run_start, i, rules, &mut spans);
            spans.push(Span {
                start: i,
                end: line.trim_end_matches('\n').len(),
                class: Class::Comment,
            });
            return spans;
        }
        // A block comment may run past the end of the line.
        if !rules.block_start.is_empty() && rest.starts_with(&rules.block_start) {
            flush_word(line, &mut run_start, i, rules, &mut spans);
            let from = i + rules.block_start.len();
            match find_block_close(line, from, rules.nesting_open(), &rules.block_end, 1) {
                Ok(end) => {
                    spans.push(Span {
                        start: i,
                        end,
                        class: Class::Comment,
                    });
                    i = end;
                }
                Err(depth) => {
                    spans.push(Span {
                        start: i,
                        end: line.trim_end_matches('\n').len(),
                        class: Class::Comment,
                    });
                    state.target = rules.block_end.clone();
                    state.target_is_comment = true;
                    state.depth = depth;
                    return spans;
                }
            }
            continue;
        }
        // A string, taking the longest delimiter that matches.
        if let Some(delim) = rules.lex.strings.iter().find(|d| rest.starts_with(**d)) {
            flush_word(line, &mut run_start, i, rules, &mut spans);
            let from = i + delim.len();
            match find_close(line, from, delim, rules.lex.escapes) {
                Some(end) => {
                    spans.push(Span {
                        start: i,
                        end,
                        class: Class::Str,
                    });
                    i = end;
                }
                None => {
                    spans.push(Span {
                        start: i,
                        end: line.trim_end_matches('\n').len(),
                        class: Class::Str,
                    });
                    state.target = (*delim).to_string();
                    state.target_is_comment = false;
                    return spans;
                }
            }
            continue;
        }
        let ch = rest.chars().next().unwrap();
        if ch.is_alphanumeric() || ch == '_' {
            if run_start.is_none() {
                run_start = Some(i);
            }
            i += ch.len_utf8();
            continue;
        }
        flush_word(line, &mut run_start, i, rules, &mut spans);
        i += ch.len_utf8();
    }
    flush_word(
        line,
        &mut run_start,
        line.trim_end_matches('\n').len(),
        rules,
        &mut spans,
    );
    spans
}

/// Classify the word that just ended, if it is one worth colouring.
fn flush_word(
    line: &str,
    run_start: &mut Option<usize>,
    end: usize,
    rules: &Rules,
    spans: &mut Vec<Span>,
) {
    let Some(start) = run_start.take() else {
        return;
    };
    if end <= start {
        return;
    }
    let word = &line[start..end];
    let class = if word.starts_with(|c: char| c.is_ascii_digit()) {
        Class::Number
    } else if rules.keywords.contains(&word) {
        Class::Keyword
    } else if rules.builtins.contains(&word) {
        Class::BuiltinFunction
    } else {
        return;
    };
    spans.push(Span { start, end, class });
}

/// The offset just past the string's closing delimiter.
///
/// `escapes` is false for the languages that double the quote instead, where
/// a backslash is an ordinary character and skipping past it would run the
/// string on to the end of the body.
fn find_close(line: &str, from: usize, close: &str, escapes: bool) -> Option<usize> {
    if close.is_empty() {
        return None;
    }
    let mut i = from;
    while i < line.len() {
        let rest = &line[i..];
        if escapes {
            if let Some(after) = rest.strip_prefix('\\') {
                // The escape takes the next character with it.
                i += 1 + after.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
                continue;
            }
        }
        if rest.starts_with(close) {
            return Some(i + close.len());
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
    None
}

/// The offset just past the block comment's close, or the depth still open.
///
/// `open` is empty when the language's comments do not nest, which makes the
/// first close end the comment. Backslashes are ordinary here: `/* \*/` ends
/// the comment in C, and treating the escape as a string's would have run it
/// on.
fn find_block_close(
    line: &str,
    from: usize,
    open: &str,
    close: &str,
    mut depth: usize,
) -> Result<usize, usize> {
    if close.is_empty() {
        return Err(depth);
    }
    let mut i = from;
    while i < line.len() {
        let rest = &line[i..];
        if rest.starts_with(close) {
            depth -= 1;
            i += close.len();
            if depth == 0 {
                return Ok(i);
            }
            continue;
        }
        if !open.is_empty() && rest.starts_with(open) {
            depth += 1;
            i += open.len();
            continue;
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
    Err(depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(|s| s.to_string()).collect()
    }

    /// A Python body too long to colour whole on every key.
    fn long_body() -> Vec<String> {
        (0..800)
            .map(|i| format!("def f{i}(x):  return x + {i}  # note\n"))
            .collect()
    }

    /// `body` coloured whole as node `n`, as after its first visit settles.
    fn settled(c: &mut Colouring, body: &[String]) -> Rc<Vec<Vec<Span>>> {
        c.of("n", body, "python", 0..50);
        c.wait();
        let whole = c.of("n", body, "python", 0..50);
        assert!(c.due_in().is_none());
        whole
    }

    #[test]
    fn an_edit_to_a_long_body_recolours_near_it_as_the_whole_would() {
        let mut c = Colouring::default();
        let mut body = long_body();
        settled(&mut c, &body);
        body[400] = "def changed(y):  return 'text'\n".to_string();
        body.insert(401, "x = 1\n".to_string());
        let partial = c.of("n", &body, "python", 0..50);
        assert!(
            c.due_in().is_some(),
            "a long body is coloured near the edit first"
        );
        assert_eq!(*partial, highlight(&body, "python"));
    }

    #[test]
    fn a_partial_colouring_is_made_whole_after_the_pause() {
        let mut c = Colouring::default();
        let mut body = long_body();
        settled(&mut c, &body);
        // A string opened and never closed turns every later line into
        // string, far past the window: only the whole colouring can know.
        body[100] = "s = \"\"\"\n".to_string();
        let partial = c.of("n", &body, "python", 0..50);
        let whole = highlight(&body, "python");
        assert_ne!(*partial, whole);
        c.entries[0].due = Some(Instant::now());
        assert_eq!(c.due_in(), Some(Duration::ZERO));
        c.wait();
        assert_eq!(*c.of("n", &body, "python", 0..50), whole);
        assert!(c.due_in().is_none());
    }

    #[test]
    fn a_short_body_is_always_coloured_whole() {
        let mut c = Colouring::default();
        let mut body = lines("x = 1\ny = 2");
        c.of("n", &body, "python", 0..50);
        assert!(c.due_in().is_none());
        body[1] = "y = 'two'".to_string();
        assert_eq!(
            *c.of("n", &body, "python", 0..50),
            highlight(&body, "python")
        );
        assert!(c.due_in().is_none());
    }

    #[test]
    fn a_long_body_is_coloured_on_screen_first_and_whole_next() {
        let mut c = Colouring::default();
        let body = long_body();
        let whole = highlight(&body, "python");
        let first = c.of("n", &body, "python", 300..350);
        assert_eq!(first[300..350], whole[300..350]);
        assert!(first[..300 - CONTEXT].iter().all(|l| l.is_empty()));
        assert!(first[350 + CONTEXT..].iter().all(|l| l.is_empty()));
        assert!(c.entries[0].job.is_some(), "the whole is begun at once");
        c.wait();
        assert_eq!(*c.of("n", &body, "python", 300..350), whole);
        assert!(c.due_in().is_none());
    }

    #[test]
    fn switching_back_to_a_kept_body_does_not_recolour_it() {
        let mut c = Colouring::default();
        let (a, b) = (long_body(), lines("x = 1"));
        let first = settled(&mut c, &a);
        c.of("b", &b, "python", 0..50);
        assert!(Rc::ptr_eq(&first, &c.of("n", &a, "python", 0..50)));
        assert!(c.due_in().is_none());
    }

    #[test]
    fn a_body_pushed_out_of_the_cache_is_coloured_again() {
        let mut c = Colouring::default();
        let a = lines("x = 1");
        let first = c.of("a", &a, "python", 0..50);
        for k in 0..KEEP {
            c.of(&k.to_string(), &a, "python", 0..50);
        }
        assert!(!Rc::ptr_eq(&first, &c.of("a", &a, "python", 0..50)));
    }

    #[test]
    fn poll_puts_in_the_whole_colouring_when_the_worker_is_done() {
        let mut c = Colouring::default();
        let body = long_body();
        c.of("n", &body, "python", 0..50);
        let start = Instant::now();
        while !c.poll() {
            assert!(start.elapsed() < Duration::from_secs(10), "never finished");
            assert_eq!(c.due_in(), Some(JOB_POLL));
            thread::sleep(Duration::from_millis(1));
        }
        assert!(c.due_in().is_none());
        assert_eq!(
            *c.of("n", &body, "python", 0..50),
            highlight(&body, "python")
        );
    }

    #[test]
    fn an_edit_drops_the_whole_colouring_of_the_text_before_it() {
        let mut c = Colouring::default();
        let mut body = long_body();
        c.of("n", &body, "python", 0..50);
        body[10] = "s = 1\n".to_string();
        c.of("n", &body, "python", 0..50);
        assert!(c.entries[0].job.is_none(), "the old text's job is dropped");
        assert!(c.due_in().is_some_and(|d| d > Duration::ZERO));
    }

    #[test]
    fn the_worker_skips_a_job_nobody_wants() {
        let job = |state: u8| {
            let (reply, receive) = mpsc::channel();
            let state = Arc::new(AtomicU8::new(state));
            let job = Job {
                lines: Arc::new(lines("x = 1")),
                language: "python".into(),
                state: Arc::clone(&state),
                reply,
            };
            worker().send(job).unwrap();
            (receive, state)
        };
        let (unwanted, unwanted_state) = job(UNWANTED);
        let (wanted, wanted_state) = job(WANTED);
        assert!(wanted.recv().is_ok());
        assert_eq!(wanted_state.load(SeqCst), STARTED);
        // Jobs run in order, so the first is settled by now.
        assert!(unwanted.recv().is_err(), "skipped, so no reply");
        assert_eq!(unwanted_state.load(SeqCst), SKIPPED);
    }

    /// Show the long body `n`, then `b`, with `n`'s job replaced by one the
    /// test answers in place of the worker.
    fn hide_with_a_fake_job(c: &mut Colouring) -> (Sender<Vec<Vec<Span>>>, Arc<AtomicU8>) {
        c.of("n", &long_body(), "python", 0..50);
        let (reply, receive) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(WANTED));
        c.entries[0].job = Some(Pending {
            receive,
            state: Arc::clone(&state),
        });
        c.of("b", &lines("x = 1"), "python", 0..50);
        (reply, state)
    }

    #[test]
    fn hiding_a_body_unwants_its_job_and_showing_it_wants_it_again() {
        let mut c = Colouring::default();
        let (_reply, state) = hide_with_a_fake_job(&mut c);
        assert_eq!(state.load(SeqCst), UNWANTED);
        assert!(c.entries[1].job.is_some(), "kept, in case it has begun");
        c.of("n", &long_body(), "python", 0..50);
        assert_eq!(state.load(SeqCst), WANTED);
    }

    #[test]
    fn a_colouring_finished_while_hidden_is_kept() {
        let mut c = Colouring::default();
        let (reply, state) = hide_with_a_fake_job(&mut c);
        state.store(STARTED, SeqCst);
        let whole = highlight(&long_body(), "python");
        reply.send(whole.clone()).unwrap();
        assert_eq!(*c.of("n", &long_body(), "python", 0..50), whole);
        assert!(c.due_in().is_none());
    }

    #[test]
    fn a_job_skipped_while_hidden_is_asked_again_when_shown() {
        let mut c = Colouring::default();
        let (reply, state) = hide_with_a_fake_job(&mut c);
        state.store(SKIPPED, SeqCst);
        drop(reply);
        c.of("n", &long_body(), "python", 0..50);
        let job = c.entries[0].job.as_ref().expect("asked again");
        assert!(!Arc::ptr_eq(&job.state, &state));
        c.wait();
        assert_eq!(
            *c.of("n", &long_body(), "python", 0..50),
            highlight(&long_body(), "python")
        );
    }

    #[test]
    fn a_job_that_failed_is_not_asked_again() {
        let mut c = Colouring::default();
        let (reply, state) = hide_with_a_fake_job(&mut c);
        state.store(STARTED, SeqCst);
        drop(reply);
        c.of("n", &long_body(), "python", 0..50);
        assert!(c.entries[0].job.is_none());
        assert!(c.due_in().is_none());
    }

    #[test]
    fn only_the_shown_body_can_be_due() {
        let mut c = Colouring::default();
        c.of("n", &long_body(), "python", 0..50);
        assert!(c.due_in().is_some());
        c.of("b", &lines("x = 1"), "python", 0..50);
        assert!(
            c.due_in().is_none(),
            "a hidden partial body would wake for nothing"
        );
    }

    /// The classified runs of one line, as (text, class).
    fn classes<'a>(line: &'a str, spans: &[Span]) -> Vec<(&'a str, Class)> {
        spans
            .iter()
            .map(|s| (&line[s.start..s.end], s.class))
            .collect()
    }

    #[test]
    fn python_comments_strings_and_keywords() {
        let src = lines("def f(x):  # note\n    return \"hi\"");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("def", Class::Keyword),
                ("f", Class::Function),
                ("# note", Class::Comment)
            ]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("return", Class::Keyword), ("\"hi\"", Class::Str)]
        );
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment() {
        let src = lines("s = \"# not a comment\"");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("\"# not a comment\"", Class::Str)]
        );
    }

    #[test]
    fn a_quote_inside_a_comment_does_not_open_a_string() {
        let src = lines("# it's fine\nx = 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("# it's fine", Class::Comment)]
        );
        // The next line is scanned normally, not as the rest of a string.
        assert_eq!(classes(&src[1], &out[1]), vec![("1", Class::Number)]);
    }

    #[test]
    fn a_triple_quoted_string_runs_across_lines() {
        let src = lines("x = \"\"\"one\ntwo\"\"\"\ny = 2");
        let out = highlight(&src, "python");
        assert_eq!(out[0][0].class, Class::Str);
        assert_eq!(classes(&src[1], &out[1]), vec![("two\"\"\"", Class::Str)]);
        assert_eq!(classes(&src[2], &out[2]), vec![("2", Class::Number)]);
    }

    #[test]
    fn a_block_comment_runs_across_lines() {
        let src = lines("int x; /* one\ntwo */ int y;");
        let out = highlight(&src, "c");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("int", Class::BuiltinType), ("/* one", Class::Comment)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("two */", Class::Comment), ("int", Class::BuiltinType)]
        );
    }

    #[test]
    fn an_at_language_directive_switches_the_rules_from_that_line() {
        // The point of the module: one node, two languages.
        let src = lines("# a python comment\n@language c\n// a c comment\nint x;");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("# a python comment", Class::Comment)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("@language c", Class::Directive)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("// a c comment", Class::Comment)]
        );
        assert_eq!(classes(&src[3], &out[3])[0], ("int", Class::BuiltinType));
    }

    #[test]
    fn a_language_directive_must_start_the_line() {
        // Leo's match_at_language returns 0 unless i == 0.
        let src = lines("x = 1  # @language c\n// still python");
        let out = highlight(&src, "python");
        assert_eq!(out[0].last().unwrap().class, Class::Comment);
        // The rules did not change, so C's `//` is not a comment here.
        assert!(out[1].iter().all(|s| s.class != Class::Comment));
    }

    #[test]
    fn nocolor_and_color_bracket_a_plain_region() {
        let src = lines("def a():\n@nocolor\ndef b():\n@color\ndef c():");
        let out = highlight(&src, "python");
        assert_eq!(out[0][0].class, Class::Keyword);
        assert_eq!(out[1][0].class, Class::Directive);
        assert!(out[2].is_empty(), "@nocolor did not stop the colouring");
        assert_eq!(out[3][0].class, Class::Directive);
        assert_eq!(out[4][0].class, Class::Keyword);
    }

    #[test]
    fn killcolor_stops_for_good() {
        let src = lines("@killcolor\ndef a():\n@color\ndef b():");
        let out = highlight(&src, "python");
        assert_eq!(out[0][0].class, Class::Directive);
        assert!(out[1..].iter().all(|line| line.is_empty()));
    }

    #[test]
    fn leo_constructs_are_coloured_whatever_the_language() {
        let src = lines("@others\n    << a section >>\n@tabwidth -4");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("@others", Class::Directive)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("<< a section >>", Class::Section)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("@tabwidth -4", Class::Directive)]
        );
    }

    #[test]
    fn an_unknown_language_still_colours_what_it_can() {
        // No keyword table, but the delimiters come from the model's tables.
        let src = lines("-- a comment\nx = 1");
        let out = highlight(&src, "haskell");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("-- a comment", Class::Comment)]
        );
        assert_eq!(classes(&src[1], &out[1]), vec![("1", Class::Number)]);
    }

    #[test]
    fn a_language_with_no_data_at_all_is_left_plain() {
        let src = lines("some prose, and 1 number");
        let out = highlight(&src, "not_a_language");
        assert_eq!(classes(&src[0], &out[0]), vec![("1", Class::Number)]);
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        let src = lines("s = \"a \\\" b\" + 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("\"a \\\" b\"", Class::Str), ("1", Class::Number)]
        );
    }

    #[test]
    fn spans_never_overlap_and_stay_inside_the_line() {
        let src = lines("def f():  # x\n    return \"a\" + 1  # y\n@language c\nint x = 0; // z");
        for (line, spans) in src.iter().zip(highlight(&src, "python")) {
            let mut last = 0;
            for s in &spans {
                assert!(s.start >= last, "overlap in {line:?}: {spans:?}");
                assert!(s.end <= line.len(), "past the end of {line:?}: {s:?}");
                assert!(s.start < s.end, "empty span in {line:?}: {s:?}");
                last = s.end;
            }
        }
    }

    #[test]
    fn rust_strings_are_coloured_and_hide_a_line_comment() {
        // The importers give Rust an empty string_list, which left `//` inside
        // a literal opening a comment that ran to the end of the line.
        let src = lines("let s = \"a // b\"; // c");
        let out = highlight(&src, "rust");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("let", Class::Keyword),
                ("\"a // b\"", Class::Str),
                ("// c", Class::Comment)
            ]
        );
    }

    #[test]
    fn a_rust_lifetime_does_not_open_a_string() {
        let src = lines("fn f<'a>(s: &'a str) -> u8 { 1 }");
        let out = highlight(&src, "rust");
        assert!(out[0].iter().all(|s| s.class != Class::Str));
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    #[test]
    fn rust_block_comments_nest() {
        let src = lines("/* a /* b */ still */ let x = 1;");
        let out = highlight(&src, "rust");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("/* a /* b */ still */", Class::Comment),
                ("let", Class::Keyword),
                ("1", Class::Number)
            ]
        );
    }

    #[test]
    fn a_nested_comment_carries_its_depth_across_lines() {
        let src = lines("/* a /* b\nc */ d */ let x = 1;");
        let out = highlight(&src, "rust");
        assert_eq!(out[0][0].class, Class::Comment);
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![
                ("c */ d */", Class::Comment),
                ("let", Class::Keyword),
                ("1", Class::Number)
            ]
        );
    }

    #[test]
    fn c_block_comments_do_not_nest() {
        let src = lines("/* a /* b */ int x;");
        let out = highlight(&src, "c");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("/* a /* b */", Class::Comment),
                ("int", Class::BuiltinType)
            ]
        );
    }

    #[test]
    fn a_backslash_does_not_end_a_c_block_comment_early_or_late() {
        // A backslash is not an escape inside a comment, so `*/` still closes.
        let src = lines("/* a \\*/ int x;");
        let out = highlight(&src, "c");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("/* a \\*/", Class::Comment), ("int", Class::BuiltinType)]
        );
    }

    #[test]
    fn a_language_without_backslash_escapes_closes_its_string() {
        // SQL doubles the quote instead, so a trailing backslash is ordinary.
        let src = lines("select 'c:\\' , 1");
        let out = highlight(&src, "sql");
        assert_eq!(out[0].iter().filter(|s| s.class == Class::Str).count(), 1);
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    #[test]
    fn cplusplus_char_literals_do_not_open_a_string() {
        // Leo's language name is `cplusplus`; the importers only register `c`,
        // so this fell through to the default list that treats `'` as a quote.
        let src = lines("char c = 'x'; int n = 1;");
        let out = highlight(&src, "cplusplus");
        assert!(out[0].iter().all(|s| s.class != Class::Str));
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    // Leo knows delimiters for some 170 languages and only a dozen have a
    // grammar compiled in, so the line scanner still colours most of them.
    // These pin its own rules to languages tree-sitter does not claim.

    #[test]
    fn the_scanner_nests_the_comments_of_a_language_that_does() {
        let src = lines("{- a {- b -} still -} data X = 1");
        let out = highlight(&src, "haskell");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("{- a {- b -} still -}", Class::Comment),
                ("data", Class::Keyword),
                ("1", Class::Number)
            ]
        );
    }

    #[test]
    fn the_scanner_does_not_nest_the_comments_of_a_language_that_does_not() {
        let src = lines("/* a /* b */ int x;");
        let out = highlight(&src, "objective_c");
        assert_eq!(out[0][0].class, Class::Comment);
        assert_eq!(&src[0][out[0][0].start..out[0][0].end], "/* a /* b */");
    }

    #[test]
    fn the_scanner_treats_a_char_literal_as_no_string_at_all() {
        let src = lines("char c = 'x'; int n = 1;");
        let out = highlight(&src, "objective_c");
        assert!(out[0].iter().all(|s| s.class != Class::Str));
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    // A parse tree says what a table lookup cannot: which identifier is a
    // function, a type or a field. These cover the languages with a grammar
    // and the masking that gets a Leo body through a parser.

    #[test]
    fn a_body_that_is_only_a_method_body_is_still_coloured() {
        // The common Leo shape: no class above it, no `def`, just the body.
        let src = lines("self.count += 1\nreturn self.total(\"x\")  # done");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("count", Class::Property), ("1", Class::Number)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![
                ("return", Class::Keyword),
                ("total", Class::Property),
                ("\"x\"", Class::Str),
                ("# done", Class::Comment)
            ]
        );
    }

    #[test]
    fn a_parse_tree_tells_a_type_from_a_function_from_a_field() {
        let src = lines("struct P { n: u8 }\nfn f(p: P) -> u8 { g(p.n) }");
        let out = highlight(&src, "rust");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("struct", Class::Keyword),
                ("P", Class::Type),
                ("n", Class::Property),
                ("u8", Class::BuiltinType)
            ]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![
                ("fn", Class::Keyword),
                ("f", Class::Function),
                ("P", Class::Type),
                ("u8", Class::BuiltinType),
                ("g", Class::Function),
                ("n", Class::Property)
            ]
        );
    }

    #[test]
    fn a_builtin_type_is_told_from_a_declared_one() {
        let class_of = |src: &str, language: &str, word: &str| {
            let src = lines(src);
            let out = highlight(&src, language);
            classes(&src[0], &out[0])
                .into_iter()
                .find(|(w, _)| *w == word)
                .map(|(_, c)| c)
        };
        for language in ["c", "cplusplus"] {
            let src = "unsigned long n; int x; point_t p;";
            assert_eq!(class_of(src, language, "int"), Some(Class::BuiltinType));
            assert_eq!(
                class_of(src, language, "unsigned long"),
                Some(Class::BuiltinType)
            );
            assert_eq!(class_of(src, language, "point_t"), Some(Class::Type));
        }
        let src = "var n int; var p Point";
        assert_eq!(class_of(src, "go", "int"), Some(Class::BuiltinType));
        assert_eq!(class_of(src, "go", "Point"), Some(Class::Type));
        let src = "def f(n: int, p: Point) -> str: pass";
        assert_eq!(class_of(src, "python", "int"), Some(Class::BuiltinType));
        assert_eq!(class_of(src, "python", "str"), Some(Class::BuiltinType));
        assert_eq!(class_of(src, "python", "Point"), Some(Class::Type));
    }

    #[test]
    fn an_attribute_is_told_from_the_code_it_sits_on() {
        let src = lines("#[derive(Debug)]\nstruct P;");
        let out = highlight(&src, "rust");
        assert_eq!(out[0][0].class, Class::Attribute);
        assert_eq!(classes(&src[1], &out[1])[1], ("P", Class::Type));
    }

    #[test]
    fn an_indented_at_others_is_a_directive_and_spoils_nothing_below_it() {
        // Leo's `directiveKind4` allows leading whitespace before `@others`,
        // and unclaimed it reads as a decorator on the `def` beneath it.
        let src = lines("class Widget:\n    @others\n    def later(self):\n        return 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("@others", Class::Directive)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("def", Class::Keyword), ("later", Class::Function)]
        );
    }

    #[test]
    fn a_section_reference_spoils_nothing_below_it() {
        // Unmasked, the parser reads `>> return` as one expression and
        // `return` stops being a keyword.
        let src = lines("def f():\n    << do the work >>\n    return 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("<< do the work >>", Class::Section)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("return", Class::Keyword), ("1", Class::Number)]
        );
    }

    #[test]
    fn a_grammar_colours_one_region_and_the_scanner_the_next() {
        let src = lines("def f():\n    pass\n@language haskell\n{- a comment -}\ndata X = 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("def", Class::Keyword), ("f", Class::Function)]
        );
        assert_eq!(
            classes(&src[3], &out[3]),
            vec![("{- a comment -}", Class::Comment)]
        );
        assert_eq!(classes(&src[4], &out[4])[0], ("data", Class::Keyword));
    }

    #[test]
    fn the_kept_colouring_follows_an_edit_and_a_language_change() {
        let mut colouring = Colouring::default();
        let src = lines("def f():");
        assert_eq!(
            colouring.of("n", &src, "python", 0..50)[0][0].class,
            Class::Keyword
        );
        let src = lines("# f");
        assert_eq!(
            colouring.of("n", &src, "python", 0..50)[0][0].class,
            Class::Comment
        );
        // The same text again, in a language that has no comment delimiter.
        assert!(colouring.of("n", &src, "not_a_language", 0..50)[0].is_empty());
    }

    #[test]
    fn a_language_change_inside_a_nocolor_block_does_not_end_it() {
        // The change still lands: `@color` resumes in C, not in Python.
        let src = lines("@nocolor\ndef a():\n@language c\nint x;\n@color\nint y;");
        let out = highlight(&src, "python");
        assert!(out[1].is_empty(), "{:?}", out[1]);
        assert!(out[3].is_empty(), "{:?}", out[3]);
        assert_eq!(classes(&src[5], &out[5])[0], ("int", Class::BuiltinType));
    }

    #[test]
    fn a_builtin_function_a_builtin_type_and_a_builtin_constant_are_told_apart() {
        // Helix themes colour the three differently in 26 of the 31 that name
        // both `function.builtin` and `type.builtin`, so one class lost that.
        let src = lines("x = len([1]) if True else None");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("len", Class::BuiltinFunction),
                ("1", Class::Number),
                ("if", Class::Keyword),
                ("True", Class::BuiltinConstant),
                ("else", Class::Keyword),
                ("None", Class::BuiltinConstant)
            ]
        );
        let src = lines("let n: u8 = 1;");
        let out = highlight(&src, "rust");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("let", Class::Keyword),
                ("u8", Class::BuiltinType),
                ("1", Class::Number)
            ]
        );
    }

    #[test]
    fn a_rust_number_is_a_number_and_not_a_builtin_constant() {
        // tree-sitter-rust tags every literal `@constant.builtin`, numbers
        // included, so `treesit` appends a rule that tags them again. Without
        // it a Rust `1` and a Python `1` are different colours in the 29 of 38
        // themes that give `constant.builtin` and `constant.numeric` their own.
        let src = lines("let n = 1.5;\nlet ok = true;");
        let out = highlight(&src, "rust");
        assert_eq!(classes(&src[0], &out[0])[1], ("1.5", Class::Number));
        assert_eq!(
            classes(&src[1], &out[1])[1],
            ("true", Class::BuiltinConstant)
        );
    }

    #[test]
    fn the_scanners_one_builtin_list_lands_on_builtin_functions() {
        // jEdit's `keyword2` to `keyword4` is one list with no way to tell a
        // function from a type, and it holds mostly functions.
        let src = lines("print(math.pi)");
        let out = highlight(&src, "lua");
        assert_eq!(
            classes(&src[0], &out[0])[0],
            ("print", Class::BuiltinFunction)
        );
    }
}
