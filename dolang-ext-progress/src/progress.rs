use std::{
    cell::{Cell, RefCell},
    io,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use dolang::runtime::object::fmt;

use dolang::runtime::{
    Error, Instance, Object, Output, Result, State, Strand, Value, call,
    object::TypeBuilder,
    strand::{self, Local},
    unpack,
    vm::Builder,
};
use dolang_ext_shell::with_terminal;
use indicatif as ix;
use ix::MultiProgress;
use tokio::io::AsyncWrite;

use crate::{
    global::Global,
    plain,
    style::{self, Color, ColorKeys, DEFAULT_ICON, Mode, Style, StyleKeys, Units},
};

// --- Strand-local state ---

pub(crate) struct ProgressLocal {
    depth: Cell<u16>,
    parent_id: Cell<u64>,
    state: RefCell<Option<SharedState>>,
}

/// Shared state for the enclosing `progress.with` scope: either interactive
/// (indicatif `MultiProgress`) or plain (non-terminal, plain-text lines).
#[derive(Clone)]
enum SharedState {
    Interactive(Rc<RefCell<ProgressState>>),
    Plain(Rc<plain::PlainConfig>),
}

struct Widget {
    id: u64,
    depth: u16,
    bar: ix::ProgressBar,
    mode: Mode,
    units: Option<Units>,
}

struct ProgressState {
    multi: Option<MultiProgress>,
    style: Style,
    widgets: Vec<Widget>,
    next_id: u64,
}

impl ProgressState {
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn find_widget_idx(&self, id: u64) -> Option<usize> {
        self.widgets.iter().position(|w| w.id == id)
    }
}

impl<'v> Local<'v> for ProgressLocal {
    fn init() -> Self {
        Self {
            depth: Cell::new(0),
            parent_id: Cell::new(0),
            state: RefCell::new(None),
        }
    }

    fn inherit(&self, _strand: &strand::Strand<'v, '_>, _kind: strand::InheritKind) -> Self {
        Self {
            depth: Cell::new(self.depth.get()),
            parent_id: Cell::new(self.parent_id.get()),
            state: RefCell::new(self.state.borrow().clone()),
        }
    }
}

// --- Leaf detection ---

fn is_leaf(widgets: &[Widget], idx: usize) -> bool {
    widgets
        .get(idx + 1)
        .map(|w| w.depth <= widgets[idx].depth)
        .unwrap_or(true)
}

// --- Depth map operations ---

/// Find the insertion index in the widget list for a new child of `parent_id`.
fn find_insert_index(widgets: &[Widget], parent_id: u64) -> usize {
    let parent_idx = widgets
        .iter()
        .position(|w| w.id == parent_id)
        .expect("parent not in widget list");
    let parent_depth = widgets[parent_idx].depth;
    let mut idx = parent_idx + 1;
    while idx < widgets.len() && widgets[idx].depth > parent_depth {
        idx += 1;
    }
    idx
}

/// Insert a new progress bar into the MultiProgress at the correct position.
/// If the parent was a leaf spinner, hides its spinner animation.
fn do_insert_bar(
    state: &mut ProgressState,
    multi: &MultiProgress,
    parent_id: u64,
    depth: u16,
    pb: ix::ProgressBar,
    mode: Mode,
    units: Option<Units>,
) -> (ix::ProgressBar, u64) {
    let id = state.alloc_id();
    let insert_idx = find_insert_index(&state.widgets, parent_id);

    // Check if parent was a leaf before insertion
    let parent_idx = state
        .widgets
        .iter()
        .position(|w| w.id == parent_id)
        .unwrap();
    let parent_was_leaf = is_leaf(&state.widgets, parent_idx);

    let multi_pos = insert_idx - 1;
    let pb = multi.insert(multi_pos, pb);
    state.widgets.insert(
        insert_idx,
        Widget {
            id,
            depth: depth + 1,
            bar: pb.clone(),
            mode,
            units,
        },
    );

    // If parent was a leaf spinner, it's now non-leaf — hide its spinner
    if parent_was_leaf && state.widgets[parent_idx].mode == Mode::Spinner {
        let pw = &state.widgets[parent_idx];
        style::apply_spinner_style(&pw.bar, &state.style, pw.depth - 1, pw.units, false);
    }

    (pb, id)
}

/// Remove a widget and all its transitive descendants from the widget list
/// and MultiProgress display. If the parent becomes a leaf spinner, restores
/// its spinner animation.
fn do_remove(state: &mut ProgressState, multi: &MultiProgress, widget_id: u64) {
    if let Some(idx) = state.widgets.iter().position(|w| w.id == widget_id) {
        let widget_depth = state.widgets[idx].depth;

        // Find parent (widget with depth < widget_depth, scanning backward)
        let parent_idx = (0..idx)
            .rev()
            .find(|&i| state.widgets[i].depth < widget_depth);

        multi.remove(&state.widgets[idx].bar);
        state.widgets.remove(idx);
        // Remove transitive descendants (entries immediately following with depth > widget_depth)
        while idx < state.widgets.len() && state.widgets[idx].depth > widget_depth {
            multi.remove(&state.widgets[idx].bar);
            state.widgets.remove(idx);
        }

        // If parent became a leaf spinner, restore its spinner
        if let Some(pi) = parent_idx
            && is_leaf(&state.widgets, pi)
            && state.widgets[pi].mode == Mode::Spinner
        {
            let pw = &state.widgets[pi];
            style::apply_spinner_style(&pw.bar, &state.style, pw.depth - 1, pw.units, true);
        }
    }
}

// --- MultiProgressWriter ---

struct MultiProgressWriter {
    multi: MultiProgress,
    buf: Vec<u8>,
}

impl MultiProgressWriter {
    fn new(multi: MultiProgress) -> Self {
        Self {
            multi,
            buf: Vec::new(),
        }
    }

    fn flush_lines(&mut self) -> io::Result<()> {
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&self.buf[..pos]).into_owned();
            self.buf.drain(..=pos);
            self.multi.println(&line).map_err(io::Error::other)?;
        }
        Ok(())
    }
}

impl AsyncWrite for MultiProgressWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.buf.extend_from_slice(buf);
        if let Err(e) = self.flush_lines() {
            return Poll::Ready(Err(e));
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        if !self.buf.is_empty() {
            let line = String::from_utf8_lossy(&self.buf).into_owned();
            self.buf.clear();
            if let Err(e) = self.multi.println(&line) {
                return Poll::Ready(Err(io::Error::other(e)));
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

// --- Helpers ---

fn check_closed<'v, 's>(strand: &mut Strand<'v, 's>, closed: &Cell<bool>) -> Result<'v, 's, ()> {
    if closed.get() {
        Err(Error::state_error(strand, "closed"))
    } else {
        Ok(())
    }
}

fn parse_units<'v, 's>(
    strand: &mut Strand<'v, 's>,
    units_val: Option<&Value<'v>>,
) -> Result<'v, 's, Option<Units>> {
    match units_val {
        Some(v) => {
            if let Some(sym) = v.as_sym(strand) {
                match sym.as_str(strand) {
                    "COUNT" => Ok(Some(Units::Count)),
                    "BYTES" => Ok(Some(Units::Bytes)),
                    _ => Err(Error::value(strand, "units: expected :COUNT: or :BYTES:")),
                }
            } else if let Some(s) = v.as_str(strand).map(|m| m.to_string()) {
                match s.as_str() {
                    "COUNT" => Ok(Some(Units::Count)),
                    "BYTES" => Ok(Some(Units::Bytes)),
                    _ => Err(Error::value(
                        strand,
                        "units: expected \"COUNT\" or \"BYTES\"",
                    )),
                }
            } else {
                Err(Error::type_error(strand, "units: expected `sym` or `str`"))
            }
        }
        None => Ok(None),
    }
}

fn parse_icon<'v, 's>(
    strand: &mut Strand<'v, 's>,
    icon_val: Option<&Value<'v>>,
) -> Result<'v, 's, String> {
    match icon_val {
        Some(v) => Ok(v
            .as_str(strand)
            .ok_or_else(|| Error::type_error(strand, "icon: expected `str`"))?
            .into()),
        None => Ok(DEFAULT_ICON.to_owned()),
    }
}

fn apply_message<'v, 's>(
    strand: &mut Strand<'v, 's>,
    pb: &ix::ProgressBar,
    message: Option<&Value<'v>>,
) -> Result<'v, 's, ()> {
    if let Some(msg) = message {
        let msg = msg
            .as_str(strand)
            .ok_or_else(|| Error::type_error(strand, "message: expected `str`"))?
            .to_string();
        pb.set_message(msg);
    }
    Ok(())
}

fn parse_duration_secs<'v, 's>(
    strand: &mut Strand<'v, 's>,
    name: &str,
    val: Option<&Value<'v>>,
    default: Duration,
) -> Result<'v, 's, Duration> {
    match val {
        Some(v) => {
            let secs = v
                .as_f64(strand)
                .ok_or_else(|| Error::type_error(strand, format!("{name}: expected `float`")))?;
            Ok(Duration::from_secs_f64(secs))
        }
        None => Ok(default),
    }
}

/// Get the multi from shared state, returning an error if the progress context
/// has been closed (e.g. background strand outlived progress.with).
fn get_multi<'v, 's>(
    strand: &mut Strand<'v, 's>,
    state: &RefCell<ProgressState>,
) -> Result<'v, 's, MultiProgress> {
    state
        .borrow()
        .multi
        .clone()
        .ok_or_else(|| Error::state_error(strand, "progress context closed"))
}

// --- VM configuration ---

pub(crate) fn configure_vm<'v>(builder: &mut Builder<'v>, global: State<'v, Global<'v>>) {
    let style_kw = builder.sym("style");
    let interval_kw = builder.sym("interval");
    let total_kw = builder.sym("total");
    let message_kw = builder.sym("message");
    let icon_kw = builder.sym("icon");
    let units_kw = builder.sym("units");
    let tick_kw = builder.sym("tick");
    let mut colors = [
        ("BLACK", Color::Black),
        ("RED", Color::Red),
        ("GREEN", Color::Green),
        ("YELLOW", Color::Yellow),
        ("BLUE", Color::Blue),
        ("MAGENTA", Color::Magenta),
        ("CYAN", Color::Cyan),
        ("WHITE", Color::White),
        ("BRIGHT_BLACK", Color::BrightBlack),
        ("BRIGHT_RED", Color::BrightRed),
        ("BRIGHT_GREEN", Color::BrightGreen),
        ("BRIGHT_YELLOW", Color::BrightYellow),
        ("BRIGHT_BLUE", Color::BrightBlue),
        ("BRIGHT_MAGENTA", Color::BrightMagenta),
        ("BRIGHT_CYAN", Color::BrightCyan),
        ("BRIGHT_WHITE", Color::BrightWhite),
        ("BRIGHT", Color::Bright),
    ]
    .map(|(name, color)| (builder.sym(name), color));
    colors.sort_unstable_by_key(|(symbol, _)| *symbol);
    let style_keys = StyleKeys {
        bar: builder.sym("bar"),
        spinner: builder.sym("spinner"),
        message: message_kw,
        icon: icon_kw,
        elapsed: builder.sym("elapsed"),
        position: builder.sym("position"),
        total: total_kw,
        width: builder.sym("width"),
        fg: builder.sym("fg"),
        bg: builder.sym("bg"),
        attrs: builder.sym("attrs"),
        alt: builder.sym("alt"),
        colors: ColorKeys { values: colors },
    };

    builder
        .module("progress")
        .function("with", async move |strand, args, mut out| {
            let ([func], [style_val, interval_val]) =
                unpack!(strand, args, 1, 0, style_kw = None, interval_kw = None)?;

            let style = match style_val {
                Some(sv) => style::parse_style(strand, &sv, &style_keys)?,
                None => Style::default(),
            };

            // If stderr is not a terminal, use the plain (non-interactive)
            // rendering path instead of indicatif's MultiProgress.
            if !dolang_ext_shell::is_terminal() {
                let interval = parse_duration_secs(
                    strand,
                    "interval",
                    interval_val.as_deref(),
                    plain::DEFAULT_INTERVAL,
                )?;
                let config = plain::PlainConfig::new(style, interval);

                let local = global.local.get(strand);
                let prev_depth = local.depth.replace(0);
                let prev_parent_id = local.parent_id.replace(0);
                let prev_state = local
                    .state
                    .replace(Some(SharedState::Plain(Rc::new(config))));

                let result = call!(strand, &func, &mut out).await;

                let local = global.local.get(strand);
                local.depth.replace(prev_depth);
                local.parent_id.replace(prev_parent_id);
                local.state.replace(prev_state);

                return result;
            }

            let multi = MultiProgress::new();
            let state_rc = Rc::new(RefCell::new(ProgressState {
                multi: Some(multi.clone()),
                style,
                widgets: vec![Widget {
                    id: 0,
                    depth: 0,
                    bar: ix::ProgressBar::hidden(),
                    mode: Mode::Bar,
                    units: None,
                }],
                next_id: 1,
            }));

            let local = global.local.get(strand);
            let prev_depth = local.depth.replace(0);
            let prev_parent_id = local.parent_id.replace(0);
            let prev_state = local
                .state
                .replace(Some(SharedState::Interactive(state_rc.clone())));

            let writer: Pin<Box<dyn AsyncWrite>> =
                Box::pin(MultiProgressWriter::new(multi.clone()));
            let result = with_terminal(strand, writer, async |strand| {
                let res = call!(strand, &func, &mut out).await;
                let _ = multi.clear();
                res
            })
            .await;

            // Invalidate shared state so background strands see the closure
            state_rc.borrow_mut().multi = None;

            // Restore previous local state
            let local = global.local.get(strand);
            local.depth.replace(prev_depth);
            local.parent_id.replace(prev_parent_id);
            local.state.replace(prev_state);

            result
        })
        .function_with_slots("show", async move |strand, args, mut out, [mut slot]| {
            let ([func], [total_val, msg_val, icon_val, units_val, tick_ms]) = unpack!(
                strand,
                args,
                1,
                0,
                total_kw = None,
                message_kw = None,
                icon_kw = None,
                units_kw = None,
                tick_kw = None
            )?;

            let units = parse_units(strand, units_val.as_deref())?;
            let icon_str = parse_icon(strand, icon_val.as_deref())?;
            let tick_interval = parse_duration_secs(
                strand,
                "tick",
                tick_ms.as_deref(),
                Duration::from_secs_f64(0.08),
            )?;

            // Determine mode and total from the total kwarg
            let (mode, total_n) = match &total_val {
                Some(v) => (Mode::Bar, Some(v.to_u64(strand)?)),
                None => (Mode::Spinner, None),
            };

            let local = global.local.get(strand);
            let shared_state = local.state.borrow().clone();

            // Set up the progress bar and track what kind of scope it lives in
            struct MultiState {
                multi: MultiProgress,
                state_rc: Rc<RefCell<ProgressState>>,
                widget_id: u64,
                prev_depth: u16,
                prev_parent_id: u64,
            }

            enum ShowKind {
                None,
                Interactive(MultiState),
                Plain {
                    prev_depth: u16,
                    prev_parent_id: u64,
                    info: Rc<plain::PlainInfo>,
                },
            }

            let (pb, kind) = match shared_state {
                None => {
                    // Outside progress.with: dummy hidden indicator
                    let pb = ix::ProgressBar::hidden();
                    if let Some(n) = total_n {
                        pb.set_length(n);
                    }
                    (pb, ShowKind::None)
                }
                Some(SharedState::Interactive(state_rc)) => {
                    let multi = get_multi(strand, &state_rc)?;
                    let local = global.local.get(strand);
                    let depth = local.depth.get();
                    let parent_id = local.parent_id.get();

                    let pb_init = match total_n {
                        Some(n) => ix::ProgressBar::new(n),
                        None => ix::ProgressBar::new_spinner(),
                    };

                    let (pb, widget_id) = {
                        let mut state = state_rc.borrow_mut();
                        let (pb, widget_id) = do_insert_bar(
                            &mut state, &multi, parent_id, depth, pb_init, mode, units,
                        );

                        // Apply initial style (new indicator is always a leaf)
                        match mode {
                            Mode::Bar => style::apply_bar_style(&pb, &state.style, depth, units),
                            Mode::Spinner => {
                                style::apply_spinner_style(&pb, &state.style, depth, units, true);
                            }
                        }
                        drop(state);
                        (pb, widget_id)
                    };

                    // Update strand-local nesting for the callback scope
                    local.depth.set(depth + 1);
                    local.parent_id.set(widget_id);

                    let ms = MultiState {
                        multi,
                        state_rc,
                        widget_id,
                        prev_depth: depth,
                        prev_parent_id: parent_id,
                    };
                    (pb, ShowKind::Interactive(ms))
                }
                Some(SharedState::Plain(config)) => {
                    let pb = ix::ProgressBar::hidden();
                    if let Some(n) = total_n {
                        pb.set_length(n);
                    }

                    let local = global.local.get(strand);
                    let depth = local.depth.get();
                    let parent_id = local.parent_id.get();

                    let ansi = dolang_ext_shell::ansi_enabled(strand);
                    let info =
                        Rc::new(plain::PlainInfo::new(depth, units, ansi, config, parent_id));

                    local.depth.set(depth + 1);
                    local.parent_id.set(info.id());

                    (
                        pb,
                        ShowKind::Plain {
                            prev_depth: depth,
                            prev_parent_id: parent_id,
                            info,
                        },
                    )
                }
            };

            pb.set_prefix(icon_str);
            pb.enable_steady_tick(tick_interval);
            apply_message(strand, &pb, msg_val.as_deref())?;

            let plain_info = match &kind {
                ShowKind::Plain { info, .. } => Some(info.clone()),
                _ => None,
            };

            let annex = IndicatorAnnex {
                bar: pb.clone(),
                state_rc: match &kind {
                    ShowKind::Interactive(ms) => Some(ms.state_rc.clone()),
                    _ => None,
                },
                widget_id: match &kind {
                    ShowKind::Interactive(ms) => ms.widget_id,
                    _ => 0,
                },
                plain: plain_info,
                closed: Cell::new(false),
            };

            global
                .types
                .indicator
                .create_with_annex(strand, Indicator, annex, &mut slot);

            // Show an initial line immediately so every plain-mode indicator
            // is visible even if it's never updated again.
            if let ShowKind::Plain { info, .. } = &kind
                && let Some(line) = plain::maybe_format(&pb, info, true, plain::LineEvent::Start)
            {
                dolang_ext_shell::write_terminal_line(strand, &line).await?;
            }

            let res = call!(strand, &func, &mut out, &slot).await;

            // Mark closed
            global
                .types
                .indicator
                .cast(&slot)
                .unwrap()
                .enter_sync(strand, |_strand, inst| {
                    inst.annex().closed.set(true);
                });

            // Clean up scope state and show final status.
            let final_result: Result<'_, '_, ()> = match kind {
                ShowKind::None => Ok(()),
                ShowKind::Interactive(ms) => {
                    let local = global.local.get(strand);
                    local.depth.set(ms.prev_depth);
                    local.parent_id.set(ms.prev_parent_id);

                    if !pb.is_finished() {
                        pb.finish_and_clear();
                    }
                    let mut state = ms.state_rc.borrow_mut();
                    do_remove(&mut state, &ms.multi, ms.widget_id);
                    Ok(())
                }
                ShowKind::Plain {
                    prev_depth,
                    prev_parent_id,
                    info,
                } => {
                    let local = global.local.get(strand);
                    local.depth.set(prev_depth);
                    local.parent_id.set(prev_parent_id);

                    // Always show final status, bypassing the rate limit.
                    match plain::maybe_format(&pb, &info, true, plain::LineEvent::End) {
                        Some(line) => dolang_ext_shell::write_terminal_line(strand, &line).await,
                        None => Ok(()),
                    }
                }
            };

            res.and(final_result)
        })
        .commit();
}

// --- Indicator ---

pub(crate) struct Indicator;

pub(crate) struct IndicatorAnnex {
    bar: ix::ProgressBar,
    state_rc: Option<Rc<RefCell<ProgressState>>>,
    widget_id: u64,
    plain: Option<Rc<plain::PlainInfo>>,
    closed: Cell<bool>,
}

/// Plain-mode emit checkpoint shared by `update` and `delta`: writes a
/// rate-limited (or, if `force`, unconditional) status line if this
/// indicator belongs to a non-interactive `progress.with` scope.
async fn maybe_emit_plain<'v, 's>(
    strand: &mut Strand<'v, 's>,
    bar: &ix::ProgressBar,
    plain_info: &Option<Rc<plain::PlainInfo>>,
    force: bool,
) -> Result<'v, 's, ()> {
    let Some(info) = plain_info else {
        return Ok(());
    };
    // Skip a routine (non-forced) update that just completed the bar — the
    // indicator's scope is almost certainly about to exit, which prints a
    // `finished` line anyway, so a "100%" line right before that would just
    // be redundant. A forced update (icon/message change) still goes
    // through even at completion, since that's a real content change.
    let completed = matches!(bar.length(), Some(len) if bar.position() >= len);
    if !force && completed {
        return Ok(());
    }
    if let Some(line) = plain::maybe_format(bar, info, force, plain::LineEvent::Update) {
        dolang_ext_shell::write_terminal_line(strand, &line).await?;
    }
    Ok(())
}

impl<'v> Object<'v> for Indicator {
    const NAME: &'v str = "Indicator";
    const MODULE: &'v str = "progress";
    type Annex = IndicatorAnnex;
    type Type = ();
    type TypeAnnex = ();

    fn debug<'a, 's>(
        _this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn dolang::runtime::Format<'v>,
    ) -> Result<'v, 's, ()> {
        fmt!(strand, w, "<progress.Indicator>")
    }

    fn build<'a>(mut builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        let icon_kw = builder.sym("icon");
        let message_kw = builder.sym("message");
        let total_kw = builder.sym("total");
        let position_kw = builder.sym("position");
        let delta_kw = builder.sym("delta");
        builder
            // --- Getters (read-only; see `update` for writes) ---
            .get("message", |this, strand, out| {
                check_closed(strand, &this.annex().closed)?;
                Output::set(strand, out, this.annex().bar.message().as_str());
                Ok(())
            })
            .get("icon", |this, strand, out| {
                check_closed(strand, &this.annex().closed)?;
                Output::set(strand, out, this.annex().bar.prefix().as_str());
                Ok(())
            })
            .get("total", |this, strand, out| {
                check_closed(strand, &this.annex().closed)?;
                if let Some(n) = this.annex().bar.length() {
                    Output::set(strand, out, n);
                }
                Ok(())
            })
            .get("position", |this, strand, out| {
                check_closed(strand, &this.annex().closed)?;
                Output::set(strand, out, this.annex().bar.position());
                Ok(())
            })
            // --- Methods ---
            .method("delta", async move |this, strand, args, _out| {
                check_closed(strand, &this.annex().closed)?;
                let ([], [amount]) = unpack!(strand, args, 0, 1)?;
                let n = match amount {
                    Some(v) => v
                        .to_i64(strand)
                        .map_err(|_| Error::type_error(strand, "expected `int`"))?,
                    None => 1,
                };
                if n >= 0 {
                    this.annex().bar.inc(n as u64); // safe: n >= 0
                } else {
                    this.annex().bar.dec(n.unsigned_abs());
                }
                let annex = this.annex();
                maybe_emit_plain(strand, &annex.bar, &annex.plain, false).await
            })
            .method("update", async move |this, strand, args, _out| {
                check_closed(strand, &this.annex().closed)?;
                let ([], [icon, message, total, position, delta]) = unpack!(
                    strand,
                    args,
                    0,
                    0,
                    icon_kw = None,
                    message_kw = None,
                    total_kw = None,
                    position_kw = None,
                    delta_kw = None
                )?;

                if position.is_some() && delta.is_some() {
                    return Err(Error::value(
                        strand,
                        "update: position: and delta: are exclusive",
                    ));
                }

                // Message/icon changes always go through immediately in
                // plain mode, even mid-debounce — they're identity changes
                // ("what is this indicator doing now"), not progress noise.
                // Position/delta/total stay rate-limited.
                let force = icon.is_some() || message.is_some();

                if let Some(v) = icon {
                    let icon = v
                        .as_str(strand)
                        .ok_or_else(|| Error::type_error(strand, "icon: expected `str`"))?
                        .to_string();
                    this.annex().bar.set_prefix(icon);
                }
                if let Some(v) = message {
                    let msg = v
                        .as_str(strand)
                        .ok_or_else(|| Error::type_error(strand, "message: expected `str`"))?
                        .to_string();
                    this.annex().bar.set_message(msg);
                }
                if let Some(v) = total {
                    let annex = this.annex();
                    if v.is_nil() {
                        // Switch to spinner mode
                        annex.bar.unset_length();
                        if let Some(state_rc) = &annex.state_rc {
                            let mut state = state_rc.borrow_mut();
                            if let Some(idx) = state.find_widget_idx(annex.widget_id)
                                && state.widgets[idx].mode != Mode::Spinner
                            {
                                state.widgets[idx].mode = Mode::Spinner;
                                let leaf = is_leaf(&state.widgets, idx);
                                let w = &state.widgets[idx];
                                style::apply_spinner_style(
                                    &w.bar,
                                    &state.style,
                                    w.depth - 1,
                                    w.units,
                                    leaf,
                                );
                            }
                        }
                    } else {
                        annex.bar.set_length(v.to_u64(strand)?);
                        if let Some(state_rc) = &annex.state_rc {
                            let mut state = state_rc.borrow_mut();
                            if let Some(idx) = state.find_widget_idx(annex.widget_id)
                                && state.widgets[idx].mode != Mode::Bar
                            {
                                state.widgets[idx].mode = Mode::Bar;
                                let w = &state.widgets[idx];
                                style::apply_bar_style(&w.bar, &state.style, w.depth - 1, w.units);
                            }
                        }
                    }
                }
                if let Some(v) = position {
                    this.annex().bar.set_position(v.to_u64(strand)?);
                }
                if let Some(v) = delta {
                    let n = v
                        .to_i64(strand)
                        .map_err(|_| Error::type_error(strand, "delta: expected `int`"))?;
                    if n >= 0 {
                        this.annex().bar.inc(n as u64); // safe: n >= 0
                    } else {
                        this.annex().bar.dec(n.unsigned_abs());
                    }
                }

                let annex = this.annex();
                maybe_emit_plain(strand, &annex.bar, &annex.plain, force).await
            })
    }
}
