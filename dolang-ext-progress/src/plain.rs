use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use indicatif as ix;
use unicode_width::UnicodeWidthStr;

use crate::style::{
    self, ANSI_RESET, BAR_CHARS, ElementStyle, Style, Units, cap_status, fit, write_styled,
};

/// Default minimum interval between non-forced plain-mode status lines for a
/// single indicator, overridable via `progress.with`'s `interval:` kwarg.
/// Deliberately independent of `tick:` (which governs TTY redraw
/// smoothness, a different concern) — plain-mode output goes to a log, not
/// a redrawn terminal cell, so it warrants a much coarser default.
pub(crate) const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// Shared, scope-wide configuration for the plain (non-interactive)
/// rendering path: the `style:` dict parsed once for the enclosing
/// `progress.with`, the rate limit for non-forced status lines, and the
/// allocator for the compact `<parent:id>` tags that stand in for the
/// tree structure a live redraw would otherwise show visually.
pub(crate) struct PlainConfig {
    pub(crate) style: Style,
    pub(crate) interval: Duration,
    pub(crate) line_ending: Vec<u8>,
    pub(crate) ansi: bool,
    next_id: Cell<u64>,
}

impl PlainConfig {
    pub(crate) fn new(style: Style, interval: Duration, line_ending: Vec<u8>, ansi: bool) -> Self {
        Self {
            style,
            interval,
            line_ending,
            ansi,
            next_id: Cell::new(1),
        }
    }

    fn alloc_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }
}

/// Per-indicator state needed to render plain (non-interactive) status
/// lines.
pub(crate) struct PlainInfo {
    pub(crate) depth: u16,
    units: Cell<Option<Units>>,
    pub(crate) ansi: bool,
    id: u64,
    parent_id: Option<u64>,
    config: Rc<PlainConfig>,
    last_emit: Cell<Option<Instant>>,
    /// `(sample time, position)` this indicator's throughput was last
    /// measured from — the reference point the next `sample_rate` call
    /// computes `delta_pos / delta_t` against. Left in place (not advanced)
    /// across a call whose `delta_t` is too small to trust, so a burst of
    /// closely-spaced calls doesn't reset the clock on every one of them.
    rate_sample: Cell<Option<(Instant, u64)>>,
    /// Exponentially-weighted average of the throughput samples so far, or
    /// `None` before the first trustworthy one. Indicatif's own
    /// `ProgressState::per_sec` estimator isn't used here on purpose: it's
    /// only updated by `enable_steady_tick`'s background thread or by
    /// position changes, and plain-mode output is sampled at its own, much
    /// coarser `interval` cadence (default 5s vs. the interactive redraw's
    /// ~80ms), so this keeps the rate's smoothing window matched to what's
    /// actually being reported rather than to indicatif's redraw-oriented
    /// one.
    rate_smoothed: Cell<Option<f64>>,
}

impl PlainInfo {
    /// `parent_id` is whatever the enclosing `progress.show`'s own id was
    /// (0 meaning "no enclosing plain indicator in this scope" — the same
    /// sentinel convention `ProgressLocal.parent_id` already uses for the
    /// interactive widget tree), translated to `None` for top-level here.
    pub(crate) fn new(
        depth: u16,
        units: Option<Units>,
        ansi: bool,
        config: Rc<PlainConfig>,
        parent_id: u64,
    ) -> Self {
        let id = config.alloc_id();
        Self {
            depth,
            units: Cell::new(units),
            ansi,
            id,
            parent_id: (parent_id != 0).then_some(parent_id),
            config,
            last_emit: Cell::new(None),
            rate_sample: Cell::new(None),
            rate_smoothed: Cell::new(None),
        }
    }

    /// This indicator's own id, to be recorded as `local.parent_id` for the
    /// duration of its callback so any nested `progress.show` calls can
    /// pick it up as their parent.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Change the display units and discard rate samples from the previous
    /// unit system.
    pub(crate) fn set_units(&self, units: Option<Units>) {
        if self.units.replace(units) != units {
            self.rate_sample.set(None);
            self.rate_smoothed.set(None);
        }
    }

    pub(crate) fn line_ending(&self) -> &[u8] {
        &self.config.line_ending
    }

    /// Half-life (in seconds) of the throughput EWA — a fresh sample and the
    /// existing smoothed estimate are weighted equally after this much time,
    /// so a stall or a burst is mostly forgotten after a couple of these.
    /// Independent of `interval`: whatever cadence lines actually get
    /// emitted at, samples further apart in time still count for more.
    const RATE_HALF_LIFE_SECS: f64 = 15.0;

    /// Shortest interval a rate is computed from. Below this, `delta_pos /
    /// delta_t` is dividing by a duration too close to the clock's own
    /// jitter/quantization to trust — e.g. a `BYTES` transfer that finishes
    /// within a millisecond of starting isn't actually running at whatever
    /// huge extrapolated rate that implies, it just completed too fast to
    /// time. Below this threshold the sample is skipped rather than fed in.
    const MIN_SAMPLE_DT_SECS: f64 = 0.05;

    /// Updates and returns the smoothed byte-rate estimate for `BYTES`
    /// indicators, sampled once per emitted line (not every `delta`/`update`
    /// call — plain-mode output is already rate-limited to `interval`, so
    /// sampling here reuses that same cadence rather than layering a second,
    /// finer one under it). Returns `None` for non-`BYTES` indicators, or
    /// when no trustworthy sample exists yet (nothing to compute a rate
    /// from, or every sample so far has been too close together to time
    /// accurately) — showing `0 B/s` in either case would be misleading,
    /// not just empty.
    fn sample_rate(&self, pos: u64, now: Instant) -> Option<f64> {
        if !matches!(self.units.get(), Some(Units::Bytes)) {
            return None;
        }
        let Some((last_time, last_pos)) = self.rate_sample.get() else {
            self.rate_sample.set(Some((now, pos)));
            return None;
        };
        let dt = now.duration_since(last_time).as_secs_f64();
        if dt < Self::MIN_SAMPLE_DT_SECS || pos < last_pos {
            // Leave the reference sample in place — the next call's `dt`
            // should accumulate from the last *trustworthy* point, not
            // reset every time a burst of calls lands within a few
            // milliseconds of each other.
            return self.rate_smoothed.get();
        }
        self.rate_sample.set(Some((now, pos)));
        let instantaneous = (pos - last_pos) as f64 / dt;
        let smoothed = match self.rate_smoothed.get() {
            // First trustworthy sample: take it as-is rather than blending
            // against a fabricated zero-rate baseline, which would just
            // dilute an accurate reading back toward "0 B/s" whenever this
            // first sample's `dt` happens to be small.
            None => instantaneous,
            Some(prev) => {
                let weight = 0.5_f64.powf(dt / Self::RATE_HALF_LIFE_SECS);
                prev * weight + instantaneous * (1.0 - weight)
            }
        };
        self.rate_smoothed.set(Some(smoothed));
        Some(smoothed)
    }
}

// ASCII by design — grep-able in CI logs and guaranteed to render correctly
// everywhere. Swap these for fancier glyphs if desired; nothing else needs
// to change since width is always measured, never assumed.
const ID_OPEN: char = '<';
const ID_CLOSE: char = '>';
const ID_SEP: char = ':';

fn format_id_tag(info: &PlainInfo) -> String {
    match info.parent_id {
        Some(parent) => format!("{ID_OPEN}{parent}{ID_SEP}{}{ID_CLOSE}", info.id),
        None => format!("{ID_OPEN}{}{ID_CLOSE}", info.id),
    }
}

/// Which point in an indicator's life a status line is being emitted for —
/// this only affects how the elapsed-time field is worded, not whether the
/// line is rate-limited (that's `force`, chosen independently by the
/// caller).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum LineEvent {
    /// The very first line, printed right after creation. Shows `started`
    /// in place of an elapsed time, since "0s" doesn't mean anything yet.
    Start,
    /// A periodic or forced-by-message/icon-change update mid-lifetime.
    /// Shows the real elapsed time, bare (e.g. `4s`).
    Update,
    /// The final line, printed when the indicator's scope exits. Always
    /// forced (bypasses the rate limit) so the last status is never lost.
    /// Explicitly labeled `finished (Xs)` rather than a bare elapsed time —
    /// otherwise a fast-finishing indicator's closing line reads as just
    /// another "0s" update immediately after "started", which looks like
    /// the rate limit failed rather than the indicator actually finishing.
    End,
}

/// Returns a formatted status line if one is due: `force` always emits
/// (used for the start/end lines, so both are always visible regardless of
/// the rate limit); otherwise emits only if the rate limit has elapsed
/// since the last emission.
pub(crate) fn maybe_format(
    bar: &ix::ProgressBar,
    info: &PlainInfo,
    force: bool,
    event: LineEvent,
) -> Option<String> {
    let due = force
        || info
            .last_emit
            .get()
            .is_none_or(|last| last.elapsed() >= info.config.interval);
    if !due {
        return None;
    }
    info.last_emit.set(Some(Instant::now()));
    Some(format_line(bar, info, event))
}

/// Renders a status line matching indicatif's own column layout (see
/// `style::bar_template`/`spinner_template`): the icon column is widened by
/// the same depth-based indent indicatif uses (so indentation comes from
/// alignment, not literal leading spaces), the message is padded/truncated
/// (with a unicode ellipsis) to a fixed width, there are no brackets around
/// the bar (indicatif doesn't draw any), and a spinner's bar-width area is
/// blank space rather than an animated glyph. Like indicatif's own live
/// redraw, a spinner reclaims the bar's width for its message (since
/// there's no bar to draw), so `message_width` acts as a minimum there
/// rather than a hard cap — but it's still a truncation limit, just a
/// larger one, so elapsed stays column-aligned across spinner siblings.
fn format_line(bar: &ix::ProgressBar, info: &PlainInfo, event: LineEvent) -> String {
    let indent = style::effective_indent(&info.config.style, info.depth);
    let icon_width = (info.config.style.icon_width + indent) as usize;
    let bar_width = (info.config.style.bar_width as usize).max(1);

    let mut line = String::new();

    // Icon column: right-aligned, like indicatif's `{prefix:>...}` — the
    // padding this produces at small depths *is* the tree indent. Uses
    // display width, not char count, so wide glyphs (most emoji) don't
    // throw off alignment against narrow ones.
    let icon_text = bar.prefix();
    for _ in 0..icon_width.saturating_sub(UnicodeWidthStr::width(icon_text.as_str())) {
        line.push(' ');
    }
    write_styled(&mut line, info.ansi, info.config.style.icon(), &icon_text);
    line.push(' ');

    // Compact `<parent:id>` tag standing in for the tree structure a live
    // redraw would otherwise show visually. Its width varies (id digit
    // counts grow over a long run), so it eats into the message's own
    // budget rather than getting a reserved column — the same
    // self-compensating trick the icon column uses for indent.
    let id_tag = format_id_tag(info);
    let id_tag_width = UnicodeWidthStr::width(id_tag.as_str());
    write_styled(&mut line, info.ansi, info.config.style.elapsed(), &id_tag);
    line.push(' ');

    let mw = (info.config.style.message_width.saturating_sub(indent) as usize)
        .saturating_sub(id_tag_width + 1);
    // `message_width` is still a truncation limit either way — needed to
    // keep the position/elapsed columns aligned across sibling rows — but
    // for a spinner there's no bar to draw, so (matching indicatif, which
    // reclaims the bar's width for the message here) it's really only a
    // *minimum* allocation: the truncation limit grows to swallow the
    // space a bar row would have spent on "[SP][bar][SP]" (`bar_width + 1`
    // columns), leaving just the one separator already pushed below —
    // *not* an additional blank bar-shaped placeholder, which would just
    // add that space back on top instead of reclaiming it.
    let msg = if bar.length().is_some() {
        fit(&bar.message(), mw)
    } else {
        fit(&bar.message(), mw + bar_width + 1)
    };
    write_styled(&mut line, info.ansi, info.config.style.message(), &msg);
    line.push(' ');

    let status_width = info.config.style.status_width as usize;
    if let Some(total) = bar.length() {
        write_bar(&mut line, info, bar_width, bar.position(), total);
        line.push(' ');
        let rate = info.sample_rate(bar.position(), Instant::now());
        line.push_str(&cap_status(
            info.units.get(),
            bar.position(),
            Some(total),
            rate,
            status_width,
            info.ansi,
            &info.config.style,
        ));
        line.push(' ');
    } else if info.units.get().is_some() {
        let rate = info.sample_rate(bar.position(), Instant::now());
        line.push_str(&cap_status(
            info.units.get(),
            bar.position(),
            None,
            rate,
            status_width,
            info.ansi,
            &info.config.style,
        ));
        line.push(' ');
    }

    let elapsed_text = match event {
        LineEvent::Start => "started".to_string(),
        LineEvent::Update => format!("{:#}", ix::HumanDuration(bar.elapsed())),
        LineEvent::End => format!("finished ({:#})", ix::HumanDuration(bar.elapsed())),
    };
    write_styled(
        &mut line,
        info.ansi,
        info.config.style.elapsed(),
        &elapsed_text,
    );
    line
}

fn write_bar(line: &mut String, info: &PlainInfo, width: usize, pos: u64, total: u64) {
    let mut chars = BAR_CHARS.chars();
    let filled_char = chars.next().unwrap_or('#');
    let current_char = chars.next().unwrap_or(filled_char);
    let empty_char = chars.next().unwrap_or(filled_char);

    let frac = if total == 0 {
        1.0
    } else {
        (pos as f64 / total as f64).clamp(0.0, 1.0)
    };
    let filled = ((frac * width as f64).round() as usize).min(width);
    let has_current = filled > 0 && filled < width;
    let filled_run = filled - if has_current { 1 } else { 0 };
    let empty_run = width - filled_run - if has_current { 1 } else { 0 };

    write_styled_run(
        line,
        info.ansi,
        info.config.style.bar(),
        filled_char,
        filled_run,
    );
    if has_current {
        write_styled_run(line, info.ansi, info.config.style.bar(), current_char, 1);
    }
    write_styled_run(
        line,
        info.ansi,
        info.config.style.bar_alt(),
        empty_char,
        empty_run,
    );
}

fn write_styled_run(line: &mut String, ansi: bool, style: &ElementStyle, ch: char, count: usize) {
    if count == 0 {
        return;
    }
    if !ansi {
        for _ in 0..count {
            line.push(ch);
        }
        return;
    }
    let before = line.len();
    style.write_ansi_prefix(line);
    let styled = line.len() != before;
    for _ in 0..count {
        line.push(ch);
    }
    if styled {
        line.push_str(ANSI_RESET);
    }
}
