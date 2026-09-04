use crate::{
    arg::Args,
    error::{Error, Result},
    object::native::{Instance, Mut, Object, Ref, Type, TypeBuilder},
    strand::Strand,
    sym::Sym,
    unpack,
    value::{
        Input, Output, Slot, StrEmbryo, Value,
        fmt::{Align, Fill, Format, Kind, Pad, Sign, Spec},
    },
    vm::{Builder, State, Stateful, Vm},
};

/// Every symbol this module needs: the keyword parameter names, and the symbol
/// naming each enumerated option value.
struct Symbols<'v> {
    fill: Sym<'v, 'v>,
    align: Sym<'v, 'v>,
    sign: Sym<'v, 'v>,
    width: Sym<'v, 'v>,
    precision: Sym<'v, 'v>,
    alt: Sym<'v, 'v>,
    kind: Sym<'v, 'v>,
    source: Sym<'v, 'v>,
    zero: Sym<'v, 'v>,
    left: Sym<'v, 'v>,
    right: Sym<'v, 'v>,
    center: Sym<'v, 'v>,
    plus: Sym<'v, 'v>,
    space: Sym<'v, 'v>,
    str: Sym<'v, 'v>,
    dbg: Sym<'v, 'v>,
    verbatim: Sym<'v, 'v>,
    hex: Sym<'v, 'v>,
    oct: Sym<'v, 'v>,
    bin: Sym<'v, 'v>,
    dec: Sym<'v, 'v>,
    exp: Sym<'v, 'v>,
    fixed: Sym<'v, 'v>,
}

impl<'v> Symbols<'v> {
    fn new(builder: &mut Builder<'v>) -> Self {
        Self {
            fill: builder.sym("fill"),
            align: builder.sym("align"),
            sign: builder.sym("sign"),
            width: builder.sym("width"),
            precision: builder.sym("precision"),
            alt: builder.sym("alt"),
            kind: builder.sym("kind"),
            source: builder.sym("source"),
            zero: builder.sym("ZERO"),
            left: builder.sym("LEFT"),
            right: builder.sym("RIGHT"),
            center: builder.sym("CENTER"),
            plus: builder.sym("PLUS"),
            space: builder.sym("SPACE"),
            str: builder.sym(Kind::Str.symbol()),
            dbg: builder.sym(Kind::Dbg.symbol()),
            verbatim: builder.sym(Kind::Verbatim.symbol()),
            hex: builder.sym(Kind::Hex.symbol()),
            oct: builder.sym(Kind::Oct.symbol()),
            bin: builder.sym(Kind::Bin.symbol()),
            dec: builder.sym(Kind::Dec.symbol()),
            exp: builder.sym(Kind::Exp.symbol()),
            fixed: builder.sym(Kind::Fixed.symbol()),
        }
    }

    fn align_symbol(&self, align: Align) -> Sym<'v, 'v> {
        match align {
            Align::Left => self.left,
            Align::Right => self.right,
            Align::Center => self.center,
        }
    }

    fn sign_symbol(&self, sign: Sign) -> Sym<'v, 'v> {
        match sign {
            Sign::Plus => self.plus,
            Sign::Space => self.space,
        }
    }

    fn kind_symbol(&self, kind: Kind) -> Sym<'v, 'v> {
        match kind {
            Kind::Str => self.str,
            Kind::Dbg => self.dbg,
            Kind::Verbatim => self.verbatim,
            Kind::Hex => self.hex,
            Kind::Oct => self.oct,
            Kind::Bin => self.bin,
            Kind::Dec => self.dec,
            Kind::Exp => self.exp,
            Kind::Fixed => self.fixed,
        }
    }
}

/// A symbol-keyed enumeration of option values, sorted for binary search.
struct Table<'v, T, const N: usize>([(Sym<'v, 'v>, T); N]);

impl<'v, T: Copy, const N: usize> Table<'v, T, N> {
    fn new(mut entries: [(Sym<'v, 'v>, T); N]) -> Self {
        entries.sort_unstable_by_key(|(symbol, _)| *symbol);
        Self(entries)
    }

    /// Resolves a symbol to its value, or reports `name` as invalid.
    fn value<'s>(
        &self,
        strand: &mut Strand<'v, 's>,
        name: &str,
        symbol: Sym<'v, 'v>,
    ) -> Result<'v, 's, T> {
        match self.0.binary_search_by_key(&symbol, |(symbol, _)| *symbol) {
            Ok(index) => Ok(self.0[index].1),
            Err(_) => Err(Error::value(strand, format!("{name}: invalid symbol"))),
        }
    }
}

pub(crate) struct Types<'v> {
    pub(crate) spec: Type<'v, FmtSpec>,
    pub(crate) value: Type<'v, FmtValue>,
}

pub(crate) struct Global<'v> {
    pub(crate) types: Types<'v>,
    symbols: Symbols<'v>,
    aligns: Table<'v, Align, 3>,
    signs: Table<'v, Sign, 2>,
    kinds: Table<'v, Kind, 9>,
}

impl<'v> Global<'v> {
    fn new(builder: &mut Builder<'v>, types: Types<'v>) -> Self {
        let symbols = Symbols::new(builder);
        let aligns = Table::new(
            [Align::Left, Align::Right, Align::Center]
                .map(|align| (symbols.align_symbol(align), align)),
        );
        let signs =
            Table::new([Sign::Plus, Sign::Space].map(|sign| (symbols.sign_symbol(sign), sign)));
        let kinds = Table::new(
            [
                Kind::Str,
                Kind::Dbg,
                Kind::Verbatim,
                Kind::Hex,
                Kind::Oct,
                Kind::Bin,
                Kind::Dec,
                Kind::Exp,
                Kind::Fixed,
            ]
            .map(|kind| (symbols.kind_symbol(kind), kind)),
        );
        Self {
            types,
            symbols,
            aligns,
            signs,
            kinds,
        }
    }
}

pub(crate) struct Tag;

impl<'v> Stateful<'v> for Global<'v> {
    type Tag = Tag;
}

/// Returns the [`FmtValue`] type object singleton, for
/// [`TypeObject::FmtValue`](crate::value::TypeObject).
pub(crate) fn fmt_value_singleton<'v, 'a>(vm: &'a Vm<'v>) -> &'a Value<'v> {
    vm.state::<Global<'v>>().types.value.singleton(vm)
}

/// Immutable per-instance data shared by [`FmtSpec`] and [`FmtValue`].
///
/// Carrying the specification alongside the global keeps both types unit
/// structs with nothing to borrow-check at runtime.
pub(crate) struct SpecAnnex<'v> {
    global: State<'v, Global<'v>>,
    spec: Spec,
}

pub(crate) struct FmtSpec;

pub(crate) struct FmtValue;

impl<'v> Object<'v> for FmtSpec {
    const MODULE: &'v str = "std";
    const NAME: &'v str = "FmtSpec";

    type Annex = SpecAnnex<'v>;
    type Type = ();
    type TypeAnnex = ();

    /// Constructs a specification from keyword options alone. Binding a value
    /// is [`FmtValue`]'s job, so no positional is accepted here.
    async fn new<'a, 's>(
        _this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let global = strand.vm().state::<Global<'v>>();
        let spec = merge_spec(strand, global, Spec::default(), args)?;
        create_spec(strand, global, spec, out);
        Ok(())
    }

    async fn call<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        create(strand, annex.global, annex.spec, args, out).await
    }
}

impl<'v> Object<'v> for FmtValue {
    const MODULE: &'v str = "std";
    const NAME: &'v str = "FmtValue";
    /// Slot 0 is the bound value; slot 1 the source text, or nil.
    const SLOTS: usize = 2;

    type Annex = SpecAnnex<'v>;
    type Type = ();
    type TypeAnnex = ();

    /// Binds a value to keyword options. `source:` is accepted so a caller
    /// that has the originating text — the compiler, above all — can record it.
    async fn new<'a, 's>(
        _this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let global = strand.vm().state::<Global<'v>>();
        let source_sym = global.symbols.source;
        let ([value], [source], rest) = unpack!(strand, args, 1, 0, source_sym = None, ...)?;
        let spec = merge_spec(strand, global, Spec::default(), rest)?;
        create_value(strand, global, spec, value, source.as_deref(), out);
        Ok(())
    }

    /// Re-merges options over the bound value. The result is synthetic rather
    /// than source-derived, so it carries no `source`.
    async fn call<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        let global = annex.global;
        let spec = merge_spec(strand, global, annex.spec, args)?;
        let borrow = this.borrow(strand)?;
        create_value(strand, global, spec, Ref::slot::<0>(&borrow), None, out);
        Ok(())
    }

    fn verbatim<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: &mut dyn Format<'v>,
    ) -> Result<'v, 's, ()> {
        format_bound(this, strand, Kind::Verbatim, out)
    }

    fn display<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: &mut dyn Format<'v>,
    ) -> Result<'v, 's, ()> {
        format_bound(this, strand, Kind::Str, out)
    }

    fn debug<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: &mut dyn Format<'v>,
    ) -> Result<'v, 's, ()> {
        format_bound(this, strand, Kind::Dbg, out)
    }
}

/// Formats the bound value, letting the surrounding conversion supply an unset kind.
fn format_bound<'v, 's>(
    this: Instance<'v, '_, FmtValue>,
    strand: &mut Strand<'v, 's>,
    kind: Kind,
    out: &mut dyn Format<'v>,
) -> Result<'v, 's, ()> {
    // A bound value may itself be bound, so rendering recurses. Bound by the
    // ordinary call depth rather than a mechanism of its own.
    let _depth = strand.inner.push_call_depth()?;
    let mut spec = this.annex().spec;
    spec.kind.get_or_insert(kind);
    let borrow = this.borrow(strand)?;
    Ref::slot::<0>(&borrow).fmt(strand, &spec, out)
}

fn build_spec_members<'v, 'a, T>(builder: TypeBuilder<'v, 'a, T>) -> TypeBuilder<'v, 'a, T>
where
    T: Object<'v, Annex = SpecAnnex<'v>>,
{
    builder
        .get("fill", |this, strand, mut out| {
            let annex = this.annex();
            match annex.spec.fill {
                Fill::Default => out.store(Value::NIL),
                Fill::Zero => Output::set(strand, out, annex.global.symbols.zero),
                Fill::Char(ch) => {
                    let mut buf = [0; 4];
                    Output::set(strand, out, &*ch.encode_utf8(&mut buf));
                }
            }
            Ok(())
        })
        .get("align", |this, strand, mut out| {
            let annex = this.annex();
            match annex.spec.align {
                None => out.store(Value::NIL),
                Some(align) => Output::set(strand, out, annex.global.symbols.align_symbol(align)),
            }
            Ok(())
        })
        .get("sign", |this, strand, mut out| {
            let annex = this.annex();
            match annex.spec.sign {
                None => out.store(Value::NIL),
                Some(sign) => Output::set(strand, out, annex.global.symbols.sign_symbol(sign)),
            }
            Ok(())
        })
        .get("width", |this, strand, mut out| {
            match this.annex().spec.width {
                Some(width) => Output::set(strand, out, width),
                None => out.store(Value::NIL),
            }
            Ok(())
        })
        .get("precision", |this, strand, mut out| {
            match this.annex().spec.precision {
                Some(precision) => Output::set(strand, out, precision),
                None => out.store(Value::NIL),
            }
            Ok(())
        })
        .get("alt", |this, strand, out| {
            let alt = this.annex().spec.alt;
            Output::set(strand, out, alt);
            Ok(())
        })
        .get("kind", |this, strand, mut out| {
            let annex = this.annex();
            match annex.spec.kind {
                None => out.store(Value::NIL),
                Some(kind) => Output::set(strand, out, annex.global.symbols.kind_symbol(kind)),
            }
            Ok(())
        })
        .method("pad", async |this, strand, args, out| {
            let ([value], []) = unpack!(strand, args, 1, 0)?;
            let value = value
                .as_str_raw(strand)
                .ok_or_else(|| Error::type_error(strand, "FmtSpec.pad: expected Str"))?;
            let spec = this.annex().spec;
            let mut embryo = StrEmbryo::new();
            let mut pad = Pad::new(spec, &mut embryo);
            pad.write_str(strand, value)?;
            pad.finish(strand)?;
            embryo.finish(strand, out);
            Ok(())
        })
}

pub(crate) fn register<'v>(builder: &mut Builder<'v>) -> State<'v, Global<'v>> {
    let spec = build_spec_members(builder.build_type::<FmtSpec>((), ())).build();
    let value = build_spec_members(builder.build_type::<FmtValue>((), ()))
        .get("value", |this, strand, out| {
            let borrow = this.borrow(strand)?;
            Output::set(strand, out, Ref::slot::<0>(&borrow));
            Ok(())
        })
        .get("source", |this, strand, out| {
            let borrow = this.borrow(strand)?;
            Output::set(strand, out, Ref::slot::<1>(&borrow));
            Ok(())
        })
        .build();
    let global = Global::new(builder, Types { spec, value });
    builder.register_state(global)
}

/// Merges keyword options over `base`, producing a new `FmtSpec` or, when a
/// positional value is supplied, a `FmtValue` bound to it.
pub(crate) async fn create<'v, 'a, 's>(
    strand: &'a mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    base: Spec,
    args: Args<'v, 'a>,
    out: Slot<'v, 'a>,
) -> Result<'v, 's, ()> {
    let source_sym = global.symbols.source;
    let ([], [value, source], rest) = unpack!(strand, args, 0, 1, source_sym = None, ...)?;
    let spec = merge_spec(strand, global, base, rest)?;
    let Some(value) = value else {
        if source.is_some() {
            return Err(Error::type_error(
                strand,
                "source: requires a value to bind it to",
            ));
        }
        create_spec(strand, global, spec, out);
        return Ok(());
    };
    create_value(strand, global, spec, value, source.as_deref(), out);
    Ok(())
}

/// Extracts the specification carried by a Do [`FmtSpec`] or [`FmtValue`].
///
/// This is the inverse of [`reify_spec`]: it accepts the value a Do `(fmt)`
/// method was handed and recovers the specification for a native formatter.
pub(crate) fn spec_of<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
) -> Result<'v, 's, Spec> {
    let global = strand.vm().state::<Global<'v>>();
    if let Some(cast) = global.types.spec.cast(value) {
        return Ok(cast.enter_sync(strand, |_, this| this.annex().spec));
    }
    if let Some(cast) = global.types.value.cast(value) {
        return Ok(cast.enter_sync(strand, |_, this| this.annex().spec));
    }
    Err(Error::type_error(strand, "expected FmtSpec"))
}

/// Reifies a specification as a Do [`FmtSpec`], looking up the module state.
///
/// This is the entry point for code outside this module — notably the class
/// protocol, which hands a `FmtSpec` to a Do-defined `(fmt)` method.
pub(crate) fn reify_spec<'v>(strand: &mut Strand<'v, '_>, spec: Spec, out: Slot<'v, '_>) {
    let global = strand.vm().state::<Global<'v>>();
    create_spec(strand, global, spec, out);
}

fn create_spec<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    spec: Spec,
    out: Slot<'v, '_>,
) {
    global
        .types
        .spec
        .create_with_annex(strand, FmtSpec, SpecAnnex { global, spec }, out);
}

fn create_value<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    spec: Spec,
    value: impl Input<'v>,
    source: Option<&Value<'v>>,
    mut out: Slot<'v, '_>,
) {
    let ty = global.types.value;
    ty.create_with_annex(strand, FmtValue, SpecAnnex { global, spec }, &mut out);
    ty.cast(&out)
        .unwrap()
        .enter_sync(strand, |strand, instance| {
            let mut borrow = instance.borrow_mut_unwrap();
            Output::set(strand, Mut::slot_mut::<0>(&mut borrow), value);
            if let Some(source) = source {
                Output::set(strand, Mut::slot_mut::<1>(&mut borrow), source);
            }
        });
}

/// Merges the keyword options in `args` over `base`.
fn merge_spec<'v, 'a, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    base: Spec,
    args: Args<'v, 'a>,
) -> Result<'v, 's, Spec> {
    let Symbols {
        fill,
        align,
        sign,
        width,
        precision,
        alt,
        kind,
        ..
    } = global.symbols;
    let ([], [fill, align, sign, width, precision, alt, kind]) = unpack!(
        strand,
        args,
        0,
        0,
        fill = None,
        align = None,
        sign = None,
        width = None,
        precision = None,
        alt = None,
        kind = None
    )?;
    let mut spec = base;
    if let Some(value) = fill {
        spec.fill = parse_fill(strand, global, &value)?;
    }
    if let Some(value) = align {
        spec.align = parse_symbol(strand, &global.aligns, "align", &value)?;
    }
    if let Some(value) = sign {
        spec.sign = parse_symbol(strand, &global.signs, "sign", &value)?;
    }
    if let Some(value) = width {
        spec.width = parse_size(strand, &value)?;
    }
    if let Some(value) = precision {
        spec.precision = parse_size(strand, &value)?;
    }
    if let Some(value) = alt {
        spec.alt = if value.is_nil() {
            false
        } else {
            value
                .as_bool(strand)
                .ok_or_else(|| Error::type_error(strand, "alt: expected Bool or nil"))?
        };
    }
    if let Some(value) = kind {
        spec.kind = parse_symbol(strand, &global.kinds, "kind", &value)?;
    }
    Ok(spec)
}

fn parse_fill<'v, 's>(
    strand: &mut Strand<'v, 's>,
    global: State<'v, Global<'v>>,
    value: &Value<'v>,
) -> Result<'v, 's, Fill> {
    if value.is_nil() {
        return Ok(Fill::Default);
    }
    if value.as_sym(strand) == Some(global.symbols.zero) {
        return Ok(Fill::Zero);
    }
    let value = value
        .as_str_raw(strand)
        .ok_or_else(|| Error::type_error(strand, "fill: expected Str, :ZERO:, or nil"))?;
    let mut chars = value.chars();
    let Some(ch) = chars.next() else {
        return Err(Error::value(strand, "fill: expected one Unicode scalar"));
    };
    if chars.next().is_some() {
        return Err(Error::value(strand, "fill: expected one Unicode scalar"));
    }
    Ok(Fill::Char(ch))
}

fn parse_symbol<'v, 's, T: Copy, const N: usize>(
    strand: &mut Strand<'v, 's>,
    table: &Table<'v, T, N>,
    name: &str,
    value: &Value<'v>,
) -> Result<'v, 's, Option<T>> {
    if value.is_nil() {
        return Ok(None);
    }
    let symbol = value
        .as_sym(strand)
        .ok_or_else(|| Error::type_error(strand, format!("{name}: expected Sym or nil")))?;
    table.value(strand, name, symbol).map(Some)
}

fn parse_size<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
) -> Result<'v, 's, Option<u32>> {
    if value.is_nil() {
        Ok(None)
    } else {
        value.to_u32(strand).map(Some)
    }
}
