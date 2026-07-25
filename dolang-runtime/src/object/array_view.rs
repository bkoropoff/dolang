//! Lazy array-like projections over native objects.

use std::{cell::Cell, ops::ControlFlow};

use dolang_bytecode::Variadic;

use crate::{
    arg::Args,
    error::{Error, Result},
    gc::{Collect, arena::Visit},
    object::{
        array, index, iter,
        native::{Instance, Object},
        protocol::{GcObj, Protocol, Recv, Spread, SpreadContext},
        range,
    },
    sig::{Unpack, UnpackKeyKind},
    strand::Strand,
    sym,
    sym::Sym,
    value::{Input, InputBy, Output, Slot, Slots, TypeObject, Value, private::Sealed},
    vm::Vm,
};

/// Implements a lazy array-like projection over a native object.
///
/// Implement this trait on a marker type. Different marker types may expose
/// different views of the same [`Object`]. Methods take `&self` (rather than
/// being purely associated functions on a zero-sized marker) so a view can
/// be parametrized by a runtime value if needed — e.g. a view scoped to one
/// sub-range or filtered by some predicate captured at construction time.
pub trait ArrayLike<'v>: 'v {
    type Object: Object<'v>;

    const MODULE: &'v str;
    const NAME: &'v str;

    fn len(&self, this: Instance<'v, '_, Self::Object>, strand: &mut Strand<'v, '_>) -> usize;

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, Self::Object>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()>;

    fn set<'a, 's>(
        &self,
        _this: Instance<'v, '_, Self::Object>,
        strand: &'a mut Strand<'v, 's>,
        _index: usize,
        _value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        Err(Error::immutable(strand))
    }
}

/// Input wrapper that creates a lazy array view of a native object.
pub struct ArrayView<'v, 'a, I: ArrayLike<'v>> {
    owner: Instance<'v, 'a, I::Object>,
    // `Option` so `input_take` can steal `view` out rather than requiring
    // `I: Clone`. Using an `ArrayView` more than once (it's meant to be
    // constructed and immediately handed to `Output::set`/`Value::from_input`)
    // is a programming error, not something to recover from.
    view: Option<I>,
}

impl<'v, 'a, I: ArrayLike<'v>> ArrayView<'v, 'a, I> {
    pub fn new(owner: Instance<'v, 'a, I::Object>, view: I) -> Self {
        Self {
            owner,
            view: Some(view),
        }
    }
}

impl<'v, I: ArrayLike<'v>> Input<'v> for ArrayView<'v, '_, I> {
    #[allow(private_interfaces)]
    fn input_take<'a>(&'a mut self, vm: &'a Vm<'v>, _: Sealed) -> InputBy<'v, 'a> {
        let owner = Value::from_input(vm, self.owner);
        let view = self.view.take().expect("ArrayView used more than once");
        let value = GcObj::new(
            vm.arena(),
            vm.builtin_types().array_view,
            View {
                owner,
                glue: Box::new(Glue(view)),
            },
        );
        InputBy::Value(Value::from_object(value), None)
    }
}

trait ArrayViewGlue<'v>: 'v {
    fn module(&self) -> &'v str;
    fn name(&self) -> &'v str;
    fn len(&self, owner: &Value<'v>, strand: &mut Strand<'v, '_>) -> usize;
    fn get<'a, 's>(
        &self,
        owner: &Value<'v>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()>;
    fn set<'a, 's>(
        &self,
        owner: &Value<'v>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()>;
}

struct Glue<I>(I);

impl<'v, I: ArrayLike<'v>> ArrayViewGlue<'v> for Glue<I> {
    fn module(&self) -> &'v str {
        I::MODULE
    }
    fn name(&self) -> &'v str {
        I::NAME
    }
    fn len(&self, owner: &Value<'v>, strand: &mut Strand<'v, '_>) -> usize {
        // SAFETY: ArrayView::input_take pairs this glue with an I::Object value.
        self.0
            .len(unsafe { Instance::from_value_unchecked(owner) }, strand)
    }
    fn get<'a, 's>(
        &self,
        owner: &Value<'v>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        // SAFETY: ArrayView::input_take pairs this glue with an I::Object value.
        self.0.get(
            unsafe { Instance::from_value_unchecked(owner) },
            strand,
            index,
            out,
        )
    }
    fn set<'a, 's>(
        &self,
        owner: &Value<'v>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        // SAFETY: ArrayView::input_take pairs this glue with an I::Object value.
        self.0.set(
            unsafe { Instance::from_value_unchecked(owner) },
            strand,
            index,
            value,
        )
    }
}

pub(crate) struct View<'v> {
    owner: Value<'v>,
    glue: Box<dyn ArrayViewGlue<'v> + 'v>,
}

/// Holds a strong reference to the parent [`View`] rather than duplicating
/// its `owner`/glue. `View` is immutable (`Collect::IMMUTABLE`), so reading
/// through `parent` needs no runtime borrow check — see its `Deref` impl.
/// `index` is a `Cell` so `Iter` itself can be declared immutable too (it's
/// only ever mutated in place, never replaced), giving the same check-free
/// `.get()` access as `View`.
pub(crate) struct Iter<'v> {
    parent: GcObj<'v, View<'v>>,
    index: Cell<usize>,
}

unsafe impl<'v> Collect for View<'v> {
    const CYCLIC: bool = true;
    const IMMUTABLE: bool = true;
    type Annex = ();
    fn accept(&self, visit: &mut dyn Visit) -> ControlFlow<()> {
        self.owner.accept(visit)
    }
    fn clear(&mut self) {
        self.owner.clear()
    }
}

unsafe impl<'v> Collect for Iter<'v> {
    const CYCLIC: bool = true;
    const IMMUTABLE: bool = true;
    type Annex = ();
    fn accept(&self, visit: &mut dyn Visit) -> ControlFlow<()> {
        self.parent.accept(visit)
    }
    fn clear(&mut self) {
        // `Iter` is immutable, so it can't be the thing responsible for
        // breaking a reference cycle running through `parent` — some other,
        // mutable link in the cycle has to do that instead.
    }
}

fn debug<'v, 's>(
    module: &str,
    name: &str,
    strand: &mut Strand<'v, 's>,
    w: &mut dyn crate::value::Format<'v>,
) -> Result<'v, 's, ()> {
    crate::fmt!(strand, w, "<{module}.{name}>")
}

fn normalize<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    len: usize,
) -> Result<'v, 's, usize> {
    let index = value.to_i64(strand).map_err(|_| Error::index(strand))?;
    index::element(len, index).ok_or_else(|| Error::index(strand))
}

impl<'v> Protocol<'v> for View<'v> {
    fn op_subtype<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        supertype: &Value<'v>,
    ) -> bool {
        supertype.eq(strand, &strand.singletons().iterable)
            || supertype.eq(strand, TypeObject::Value)
    }
    fn op_debug<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        let view = this.get();
        debug(view.glue.module(), view.glue.name(), strand, w)
    }
    fn op_bool<'a, 's>(this: Recv<'v, 'a, Self>, strand: &mut Strand<'v, 's>) -> bool {
        let view = this.get();
        view.glue.len(&view.owner, strand) != 0
    }
    fn op_eq<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        let equal = other
            .downcast_ref(strand.builtin_types().array_view)
            .is_some_and(|other| this.as_header() == other.into_raw().cast());
        Ok(Value::from_bool(equal))
    }
    fn op_index<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        index: &Value<'v>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let view = this.get();
        let len = view.glue.len(&view.owner, strand);
        if let Some(slice) = range::slice(index, strand, len)? {
            let indices: Box<dyn Iterator<Item = usize>> = match slice {
                range::Slice::Contiguous { start, end } => {
                    if start > end {
                        return Err(Error::index(strand));
                    }
                    Box::new(start..end)
                }
                range::Slice::Stepped(indices) => Box::new(indices.into_iter()),
            };
            let mut array = array::Array::new();
            for index in indices {
                let mut value = Value::NIL;
                view.glue
                    .get(&view.owner, strand, index, Slot::new(&mut value))?;
                array.inner.push(value);
            }
            strand.builtin_types().array.create(strand, array, out);
            return Ok(());
        }
        let index = normalize(strand, index, len)?;
        view.glue.get(&view.owner, strand, index, out)
    }
    fn op_assign<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        index: Slot<'v, 'a>,
        value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let view = this.get();
        let len = view.glue.len(&view.owner, strand);
        let index = normalize(strand, &index, len)?;
        view.glue.set(&view.owner, strand, index, value)
    }
    fn op_get<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        field: Sym<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        if field.tag() == sym::LEN {
            let view = this.get();
            let len = view.glue.len(&view.owner, strand);
            Output::set(strand, out, len);
            Ok(())
        } else {
            iter::iterable_get(strand, &this, field, out)
        }
    }
    async fn op_mcall<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        method: Sym<'v, 'a>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        if method.tag() == sym::LEN {
            return Err(Error::type_error(
                strand,
                "array view len is a field, not a method",
            ));
        }
        iter::iterable_mcall(strand, &this, method, args, out).await
    }
    async fn op_iter<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        strand.builtin_types().array_view_iter.create(
            strand,
            Iter {
                parent: this.to_strong(),
                index: Cell::new(0),
            },
            out,
        );
        Ok(())
    }
    async fn op_spread<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        _context: SpreadContext,
        sink: &'a mut dyn Spread<'v, 's>,
    ) -> Result<'v, 's, ()> {
        let view = this.get();
        let len = view.glue.len(&view.owner, strand);
        let mut value = Value::NIL;
        for index in 0..len {
            view.glue
                .get(&view.owner, strand, index, Slot::new(&mut value))?;
            sink.positional(strand, Slot::new(&mut value))?;
        }
        Ok(())
    }
    async fn op_unpack<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        sig: &'a Unpack<'v, 'a>,
        mut out: Slots<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let view = this.get();
        let consumed = unpack_from(strand, sig, &mut out, &view.owner, &*view.glue, 0)?;
        if sig.variadic == Variadic::Capture {
            strand.builtin_types().array_view_iter.create(
                strand,
                Iter {
                    parent: this.to_strong(),
                    index: Cell::new(consumed),
                },
                out.at(sig.len() - 1),
            );
        }
        Ok(())
    }
    fn op_type<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) {
        Output::set(strand, out, TypeObject::Value)
    }
}

impl<'v> Protocol<'v> for Iter<'v> {
    fn op_debug<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        let view = &*this.get().parent;
        debug(view.glue.module(), view.glue.name(), strand, w)
    }
    async fn op_iter<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        Output::set(strand, out, &this);
        Ok(())
    }
    async fn op_next<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, bool> {
        let iter = this.get();
        let view = &*iter.parent;
        let index = iter.index.get();
        if index >= view.glue.len(&view.owner, strand) {
            return Ok(false);
        }
        view.glue.get(&view.owner, strand, index, out)?;
        iter.index.set(index + 1);
        Ok(true)
    }
    async fn op_unpack<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        sig: &'a Unpack<'v, 'a>,
        mut out: Slots<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let iter = this.get();
        let view = &*iter.parent;
        let consumed = unpack_from(
            strand,
            sig,
            &mut out,
            &view.owner,
            &*view.glue,
            iter.index.get(),
        )?;
        iter.index.set(iter.index.get() + consumed);
        if sig.variadic == Variadic::Capture {
            Output::set(strand, out.at(sig.len() - 1), &this);
        }
        Ok(())
    }
    fn op_get<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        field: Sym<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        iter::iter_get(strand, &this, field, out)
    }
    async fn op_mcall<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        method: Sym<'v, 'a>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        iter::iter_mcall(strand, &this, method, args, out).await
    }
    fn op_type<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) {
        Output::set(strand, out, &strand.singletons().input_iter)
    }
}

fn unpack_from<'v, 's>(
    strand: &mut Strand<'v, 's>,
    sig: &Unpack<'v, '_>,
    out: &mut Slots<'v, '_>,
    owner: &Value<'v>,
    glue: &(dyn ArrayViewGlue<'v> + '_),
    start: usize,
) -> Result<'v, 's, usize> {
    let len = glue.len(owner, strand).saturating_sub(start);
    let pos_count = sig.required + sig.optional.len();
    if sig.required > len {
        return Err(Error::missing_positional(strand, sig.required));
    }
    if pos_count < len && sig.variadic == Variadic::None {
        return Err(Error::unexpected_positional(strand, sig.required));
    }
    let min = pos_count.min(len);
    for i in 0..min {
        glue.get(owner, strand, start + i, out.at(i))?;
    }
    if len < pos_count {
        for (i, default) in sig.optional[(len - sig.required)..].iter().enumerate() {
            out.at(min + i).store(default.dup());
        }
    }
    for (i, key) in sig.keys.iter().enumerate() {
        if let Some(default) = &key.default {
            out.at(min + i).store(default.dup());
        } else {
            return Err(match &key.kind {
                UnpackKeyKind::Sym(sym) => Error::missing_key(strand, *sym),
                UnpackKeyKind::Const(value) => Error::missing_key(strand, value),
            });
        }
    }
    Ok(min + sig.keys.len())
}
