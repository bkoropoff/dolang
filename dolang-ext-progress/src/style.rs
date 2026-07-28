use dolang::runtime::{Error, Result, Strand, Sym, Value};
use indicatif as ix;

// --- Color and attribute enums ---

#[derive(Clone, Copy)]
pub(crate) enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    /// Just `.bright` / `.on_bright` — brightens the default color without changing it.
    Bright,
}

impl Color {
    fn fg_fmt(self, s: &mut String) {
        use Color::*;
        s.push('.');
        match self {
            Black => s.push_str("black"),
            Red => s.push_str("red"),
            Green => s.push_str("green"),
            Yellow => s.push_str("yellow"),
            Blue => s.push_str("blue"),
            Magenta => s.push_str("magenta"),
            Cyan => s.push_str("cyan"),
            White => s.push_str("white"),
            BrightBlack => s.push_str("bright.black"),
            BrightRed => s.push_str("bright.red"),
            BrightGreen => s.push_str("bright.green"),
            BrightYellow => s.push_str("bright.yellow"),
            BrightBlue => s.push_str("bright.blue"),
            BrightMagenta => s.push_str("bright.magenta"),
            BrightCyan => s.push_str("bright.cyan"),
            BrightWhite => s.push_str("bright.white"),
            Bright => s.push_str("bright"),
        }
    }

    fn bg_fmt(self, s: &mut String) {
        use Color::*;
        match self {
            Black => s.push_str(".on_black"),
            Red => s.push_str(".on_red"),
            Green => s.push_str(".on_green"),
            Yellow => s.push_str(".on_yellow"),
            Blue => s.push_str(".on_blue"),
            Magenta => s.push_str(".on_magenta"),
            Cyan => s.push_str(".on_cyan"),
            White => s.push_str(".on_white"),
            BrightBlack => s.push_str(".on_bright.on_black"),
            BrightRed => s.push_str(".on_bright.on_red"),
            BrightGreen => s.push_str(".on_bright.on_green"),
            BrightYellow => s.push_str(".on_bright.on_yellow"),
            BrightBlue => s.push_str(".on_bright.on_blue"),
            BrightMagenta => s.push_str(".on_bright.on_magenta"),
            BrightCyan => s.push_str(".on_bright.on_cyan"),
            BrightWhite => s.push_str(".on_bright.on_white"),
            Bright => s.push_str(".on_bright"),
        }
    }

    /// Raw ANSI SGR code for this color as a foreground.
    fn fg_ansi(self) -> &'static str {
        use Color::*;
        match self {
            Black => "30",
            Red => "31",
            Green => "32",
            Yellow => "33",
            Blue => "34",
            Magenta => "35",
            Cyan => "36",
            White => "37",
            BrightBlack => "90",
            BrightRed => "91",
            BrightGreen => "92",
            BrightYellow => "93",
            BrightBlue => "94",
            BrightMagenta => "95",
            BrightCyan => "96",
            BrightWhite => "97",
            // No specific color: brighten whatever the terminal default is.
            Bright => "1",
        }
    }

    /// Raw ANSI SGR code for this color as a background, or `None` if there
    /// is no direct SGR equivalent (bare `Bright` used as a background).
    fn bg_ansi(self) -> Option<&'static str> {
        use Color::*;
        Some(match self {
            Black => "40",
            Red => "41",
            Green => "42",
            Yellow => "43",
            Blue => "44",
            Magenta => "45",
            Cyan => "46",
            White => "47",
            BrightBlack => "100",
            BrightRed => "101",
            BrightGreen => "102",
            BrightYellow => "103",
            BrightBlue => "104",
            BrightMagenta => "105",
            BrightCyan => "106",
            BrightWhite => "107",
            Bright => return None,
        })
    }
}

fn parse_color_value<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &str,
    colors: ColorKeys<'v>,
) -> Result<'v, 's, Color> {
    let value = value
        .as_sym(strand)
        .ok_or_else(|| Error::type_error(strand, format!("style: {name}: expected `sym`")))?;
    colors
        .get(value)
        .ok_or_else(|| Error::value(strand, format!("style: {name}: unknown color")))
}

#[derive(Clone, Copy)]
pub(crate) enum Attr {
    Bold,
    Dim,
    Italic,
    Underlined,
    Blink,
    Reverse,
    Hidden,
    Strikethrough,
}

impl Attr {
    fn fmt(self, s: &mut String) {
        use Attr::*;
        s.push('.');
        match self {
            Bold => s.push_str("bold"),
            Dim => s.push_str("dim"),
            Italic => s.push_str("italic"),
            Underlined => s.push_str("underlined"),
            Blink => s.push_str("blink"),
            Reverse => s.push_str("reverse"),
            Hidden => s.push_str("hidden"),
            Strikethrough => s.push_str("strikethrough"),
        }
    }
}

impl TryFrom<&str> for Attr {
    type Error = String;

    fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
        use Attr::*;
        match s {
            "bold" => Ok(Bold),
            "dim" => Ok(Dim),
            "italic" => Ok(Italic),
            "underlined" => Ok(Underlined),
            "blink" => Ok(Blink),
            "reverse" => Ok(Reverse),
            "hidden" => Ok(Hidden),
            "strikethrough" => Ok(Strikethrough),
            _ => Err(format!("unknown attribute: '{s}'")),
        }
    }
}

impl Attr {
    fn ansi(self) -> &'static str {
        use Attr::*;
        match self {
            Bold => "1",
            Dim => "2",
            Italic => "3",
            Underlined => "4",
            Blink => "5",
            Reverse => "7",
            Hidden => "8",
            Strikethrough => "9",
        }
    }
}

// --- Element style ---

#[derive(Clone, Default)]
pub(crate) struct ElementStyle {
    fg: Option<Color>,
    bg: Option<Color>,
    attrs: Vec<Attr>,
}

impl ElementStyle {
    fn to_template_suffix(&self) -> String {
        let mut s = String::new();
        for attr in &self.attrs {
            attr.fmt(&mut s);
        }
        if let Some(fg) = self.fg {
            fg.fg_fmt(&mut s);
        }
        if let Some(bg) = self.bg {
            bg.bg_fmt(&mut s);
        }
        s
    }

    fn to_template_suffix_with_alt(&self, alt: &ElementStyle) -> String {
        let mut s = self.to_template_suffix();
        let alt_s = alt.to_template_suffix();
        if !alt_s.is_empty() {
            s.push('/');
            // Strip leading '.' from alt since '/' already separates
            s.push_str(&alt_s[1..]);
        }
        s
    }

    /// Appends a raw ANSI SGR escape sequence for this style to `s`, or
    /// nothing if the style has no attrs/colors set. Used for the
    /// non-interactive (plain) rendering path, parallel to
    /// [`to_template_suffix`](Self::to_template_suffix) which drives
    /// indicatif's own template mini-language for the interactive path.
    pub(crate) fn write_ansi_prefix(&self, s: &mut String) {
        let mut codes: Vec<&str> = Vec::new();
        for attr in &self.attrs {
            codes.push(attr.ansi());
        }
        if let Some(fg) = self.fg {
            codes.push(fg.fg_ansi());
        }
        if let Some(bg) = self.bg
            && let Some(code) = bg.bg_ansi()
        {
            codes.push(code);
        }
        if !codes.is_empty() {
            s.push_str("\x1b[");
            s.push_str(&codes.join(";"));
            s.push('m');
        }
    }
}

/// ANSI SGR reset sequence, paired with [`ElementStyle::write_ansi_prefix`].
pub(crate) const ANSI_RESET: &str = "\x1b[0m";

// --- Style ---

#[derive(Clone)]
pub(crate) struct Style {
    pub(crate) bar_width: u16,
    pub(crate) message_width: u16,
    pub(crate) icon_width: u16,
    bar: ElementStyle,
    bar_alt: ElementStyle,
    spinner: ElementStyle,
    message: ElementStyle,
    icon: ElementStyle,
    elapsed: ElementStyle,
    position: ElementStyle,
    total: ElementStyle,
}

impl Style {
    // Accessors for the plain (non-interactive) rendering path, which
    // builds raw ANSI lines directly rather than going through indicatif's
    // template mini-language.
    pub(crate) fn bar(&self) -> &ElementStyle {
        &self.bar
    }

    pub(crate) fn bar_alt(&self) -> &ElementStyle {
        &self.bar_alt
    }

    pub(crate) fn message(&self) -> &ElementStyle {
        &self.message
    }

    pub(crate) fn icon(&self) -> &ElementStyle {
        &self.icon
    }

    pub(crate) fn elapsed(&self) -> &ElementStyle {
        &self.elapsed
    }
}

impl Default for Style {
    fn default() -> Self {
        Self {
            bar_width: 20,
            message_width: 40,
            icon_width: 2,
            bar: ElementStyle {
                fg: Some(Color::Cyan),
                ..Default::default()
            },
            bar_alt: ElementStyle {
                fg: Some(Color::Blue),
                ..Default::default()
            },
            spinner: ElementStyle {
                fg: Some(Color::Cyan),
                ..Default::default()
            },
            message: ElementStyle {
                ..Default::default()
            },
            icon: ElementStyle {
                fg: Some(Color::Bright),
                attrs: vec![Attr::Bold],
                ..Default::default()
            },
            elapsed: ElementStyle {
                attrs: vec![Attr::Dim],
                ..Default::default()
            },
            position: ElementStyle::default(),
            total: ElementStyle::default(),
        }
    }
}

// --- Units ---

#[derive(Clone, Copy)]
pub(crate) enum Units {
    Count,
    Bytes,
}

// --- Mode ---

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Bar,
    Spinner,
}

// --- Template generation ---

const MIN_MSG_WIDTH: u16 = 10;

pub(crate) fn effective_indent(style: &Style, depth: u16) -> u16 {
    let max_indent = style.message_width.saturating_sub(MIN_MSG_WIDTH);
    (depth * 2).min(max_indent)
}

pub(crate) fn bar_template(style: &Style, depth: u16, units: Option<Units>) -> String {
    let indent = effective_indent(style, depth);
    let iw = style.icon_width + indent;
    let mw = style.message_width - indent;
    let bw = style.bar_width;
    let ic = style.icon.to_template_suffix();
    let mc = style.message.to_template_suffix();
    let bc = style.bar.to_template_suffix_with_alt(&style.bar_alt);
    let ec = style.elapsed.to_template_suffix();
    let pc = style.position.to_template_suffix();
    let tc = style.total.to_template_suffix();
    match units {
        None | Some(Units::Count) => format!(
            "{{prefix:>{iw}{ic}}} {{msg:{mw}!{mc}}} {{bar:{bw}{bc}}} {{pos:{pc}}}/{{len:{tc}}} {{elapsed:{ec}}}"
        ),
        Some(Units::Bytes) => format!(
            "{{prefix:>{iw}{ic}}} {{msg:{mw}!{mc}}} {{bar:{bw}{bc}}} {{bytes:{pc}}}/{{total_bytes:{tc}}} {{elapsed:{ec}}}"
        ),
    }
}

pub(crate) const BAR_CHARS: &str = "━╸━";
pub(crate) const DEFAULT_ICON: &str = "●";

pub(crate) fn spinner_template(
    style: &Style,
    depth: u16,
    units: Option<Units>,
    leaf: bool,
) -> String {
    let indent = effective_indent(style, depth);
    let iw = style.icon_width + indent;
    let ic = style.icon.to_template_suffix();
    let mc = style.message.to_template_suffix();
    let ec = style.elapsed.to_template_suffix();
    let pc = style.position.to_template_suffix();
    const SW: usize = 1;

    // Leaf nodes show a spinner; non-leaf nodes hide it and give the space to the message.
    let (spinner_part, mw_extra) = if leaf {
        let sc = style.spinner.to_template_suffix();
        (format!(" {{spinner:>{sc}}}"), style.bar_width - SW as u16)
    } else {
        (format!("{:<SW$}", ""), style.bar_width)
    };
    let mw = style.message_width - indent + mw_extra;

    match units {
        None => {
            format!("{{prefix:>{iw}{ic}}} {{msg:{mw}!{mc}}}{spinner_part} {{elapsed:{ec}}}")
        }
        Some(Units::Count) => {
            format!(
                "{{prefix:>{iw}{ic}}} {{msg:{mw}!{mc}}}{spinner_part} {{pos:{pc}}} {{elapsed:{ec}}}"
            )
        }
        Some(Units::Bytes) => {
            format!(
                "{{prefix:>{iw}{ic}}} {{msg:{mw}!{mc}}}{spinner_part} {{bytes:{pc}}} {{elapsed:{ec}}}"
            )
        }
    }
}

// --- Style application helpers ---

pub(crate) fn apply_bar_style(
    bar: &ix::ProgressBar,
    style: &Style,
    depth: u16,
    units: Option<Units>,
) {
    let tmpl = bar_template(style, depth, units);
    let s = ix::ProgressStyle::with_template(&tmpl)
        .expect("valid bar template")
        .progress_chars(BAR_CHARS);
    bar.set_style(s);
}

pub(crate) fn apply_spinner_style(
    bar: &ix::ProgressBar,
    style: &Style,
    depth: u16,
    units: Option<Units>,
    leaf: bool,
) {
    let tmpl = spinner_template(style, depth, units, leaf);
    let s = ix::ProgressStyle::with_template(&tmpl).expect("valid spinner template");
    bar.set_style(s);
}

// --- Style dict parsing ---

/// Keys needed for style dict parsing. All are `Copy` `Sym` values.
#[derive(Clone, Copy)]
pub(crate) struct StyleKeys<'v> {
    pub(crate) bar: Sym<'v, 'v>,
    pub(crate) spinner: Sym<'v, 'v>,
    pub(crate) message: Sym<'v, 'v>,
    pub(crate) icon: Sym<'v, 'v>,
    pub(crate) elapsed: Sym<'v, 'v>,
    pub(crate) position: Sym<'v, 'v>,
    pub(crate) total: Sym<'v, 'v>,
    pub(crate) width: Sym<'v, 'v>,
    pub(crate) fg: Sym<'v, 'v>,
    pub(crate) bg: Sym<'v, 'v>,
    pub(crate) attrs: Sym<'v, 'v>,
    pub(crate) alt: Sym<'v, 'v>,
    pub(crate) colors: ColorKeys<'v>,
}

#[derive(Clone, Copy)]
pub(crate) struct ColorKeys<'v> {
    pub(crate) values: [(Sym<'v, 'v>, Color); 17],
}

impl<'v> ColorKeys<'v> {
    fn get<'a>(self, value: Sym<'v, 'a>) -> Option<Color>
    where
        'v: 'a,
    {
        self.values
            .binary_search_by_key(&value, |(symbol, _)| -> Sym<'v, 'a> { *symbol })
            .ok()
            .map(|index| self.values[index].1)
    }
}

fn unknown_key_error<'v, 's>(strand: &mut Strand<'v, 's>, sym: Sym<'v, '_>) -> Error<'v, 's> {
    Error::value(
        strand,
        format!("style: unknown key: {}", sym.as_str(strand)),
    )
}

fn as_style_dict<'v, 's, 'a>(
    strand: &mut Strand<'v, 's>,
    val: &'a Value<'v>,
) -> Result<'v, 's, dolang::runtime::value::Dict<'v, 'a>> {
    val.as_dict(strand)
        .ok_or_else(|| Error::type_error(strand, "style: expected `dict`"))
}

fn parse_attrs<'v, 's>(strand: &mut Strand<'v, 's>, val: &Value<'v>) -> Result<'v, 's, Vec<Attr>> {
    let arr = val
        .as_array(strand.vm())
        .ok_or_else(|| Error::type_error(strand, "style: attrs: expected array"))?;
    let len = arr.len(strand)?;
    let mut attrs = Vec::with_capacity(len);
    for i in 0..len {
        strand.with_slots_sync(|strand, [mut elem]| {
            arr.get(strand, i, &mut elem)?;
            let s = elem
                .as_str(strand)
                .ok_or_else(|| Error::type_error(strand, "style: attrs: expected `str` element"))?
                .to_string();
            let attr = Attr::try_from(s.as_str())
                .map_err(|e| Error::runtime(strand, format!("style: attrs: {e}")))?;
            attrs.push(attr);
            Ok(())
        })?;
    }
    Ok(attrs)
}

/// Parses a plain element-style dict — `fg`, `bg`, `attrs` only. Used for
/// `spinner`/`elapsed`/`position`/`total`, and for `bar.alt`. Iterates the
/// dict's actual entries (rather than probing for expected keys) so an
/// unrecognized key is caught as an error instead of silently ignored.
fn parse_element_style<'v, 's>(
    strand: &mut Strand<'v, 's>,
    cat: &Value<'v>,
    keys: &StyleKeys<'v>,
    es: &mut ElementStyle,
) -> Result<'v, 's, ()> {
    let dict = as_style_dict(strand, cat)?;
    let mut pairs = dict.pairs();
    strand.with_slots_sync(|strand, [mut key, mut val]| {
        while pairs.next(strand, &mut key, &mut val)? {
            let sym = key
                .as_sym(strand)
                .ok_or_else(|| Error::type_error(strand, "style: expected `sym` key"))?;
            if sym == keys.fg {
                es.fg = Some(parse_color_value(strand, &val, "fg", keys.colors)?);
            } else if sym == keys.bg {
                es.bg = Some(parse_color_value(strand, &val, "bg", keys.colors)?);
            } else if sym == keys.attrs {
                es.attrs = parse_attrs(strand, &val)?;
            } else {
                return Err(unknown_key_error(strand, sym));
            }
        }
        Ok(())
    })
}

/// Parses a width+color category dict — `width`, `fg`, `bg`, `attrs`, and
/// (bar only, when `alt` is `Some`) `alt`. Used for `bar`/`message`/`icon`.
fn parse_width_category<'v, 's>(
    strand: &mut Strand<'v, 's>,
    cat: &Value<'v>,
    keys: &StyleKeys<'v>,
    width: &mut u16,
    es: &mut ElementStyle,
    mut alt: Option<&mut ElementStyle>,
) -> Result<'v, 's, ()> {
    let dict = as_style_dict(strand, cat)?;
    let mut pairs = dict.pairs();
    strand.with_slots_sync(|strand, [mut key, mut val]| {
        while pairs.next(strand, &mut key, &mut val)? {
            let sym = key
                .as_sym(strand)
                .ok_or_else(|| Error::type_error(strand, "style: expected `sym` key"))?;
            if sym == keys.width {
                let n = val
                    .to_i64(strand)
                    .map_err(|_| Error::type_error(strand, "style: width: expected `int`"))?;
                *width = n as u16;
            } else if sym == keys.fg {
                es.fg = Some(parse_color_value(strand, &val, "fg", keys.colors)?);
            } else if sym == keys.bg {
                es.bg = Some(parse_color_value(strand, &val, "bg", keys.colors)?);
            } else if sym == keys.attrs {
                es.attrs = parse_attrs(strand, &val)?;
            } else if sym == keys.alt
                && let Some(a) = alt.as_deref_mut()
            {
                parse_element_style(strand, &val, keys, a)?;
            } else {
                return Err(unknown_key_error(strand, sym));
            }
        }
        Ok(())
    })
}

pub(crate) fn parse_style<'v, 's>(
    strand: &mut Strand<'v, 's>,
    style_val: &Value<'v>,
    keys: &StyleKeys<'v>,
) -> Result<'v, 's, Style> {
    let mut style = Style::default();

    let dict = as_style_dict(strand, style_val)?;
    let mut pairs = dict.pairs();
    strand.with_slots_sync(|strand, [mut key, mut val]| {
        while pairs.next(strand, &mut key, &mut val)? {
            let sym = key
                .as_sym(strand)
                .ok_or_else(|| Error::type_error(strand, "style: expected `sym` key"))?;
            if sym == keys.bar {
                parse_width_category(
                    strand,
                    &val,
                    keys,
                    &mut style.bar_width,
                    &mut style.bar,
                    Some(&mut style.bar_alt),
                )?;
            } else if sym == keys.message {
                parse_width_category(
                    strand,
                    &val,
                    keys,
                    &mut style.message_width,
                    &mut style.message,
                    None,
                )?;
            } else if sym == keys.icon {
                parse_width_category(
                    strand,
                    &val,
                    keys,
                    &mut style.icon_width,
                    &mut style.icon,
                    None,
                )?;
            } else if sym == keys.spinner {
                parse_element_style(strand, &val, keys, &mut style.spinner)?;
            } else if sym == keys.elapsed {
                parse_element_style(strand, &val, keys, &mut style.elapsed)?;
            } else if sym == keys.position {
                parse_element_style(strand, &val, keys, &mut style.position)?;
            } else if sym == keys.total {
                parse_element_style(strand, &val, keys, &mut style.total)?;
            } else {
                return Err(unknown_key_error(strand, sym));
            }
        }
        Ok(())
    })?;

    Ok(style)
}
