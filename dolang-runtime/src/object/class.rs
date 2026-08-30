use std::{
    cell::OnceCell,
    collections::{HashMap, VecDeque, hash_map::Entry},
    mem,
    ops::ControlFlow,
};

use bitvec::{bitbox, boxed::BitBox};
use dolang_bytecode::Variadic;
use dolang_util::alias;

use crate::{
    arg::{Arg, Args},
    call,
    error::{Error, ErrorKind, Result},
    gc::{Annex, Collect, arena::Visit},
    method,
    object::{
        BoundMethod,
        field_iter::FieldIter,
        protocol::{
            Delegated, Dispatch, Inspect, MemberKind, Recv, Spread, SpreadContext, default_spread,
            members,
        },
        sym::SymObj,
    },
    sig::{self, Unpack},
    strand::Strand,
    sym::{self, Sym},
    unpack,
    value::{Output, Slot, Slots, Value},
    vm::Vm,
};

use super::protocol::{GcObj, GcObjBorrow, Protocol};

#[inline(never)]
pub(crate) fn create<'v, 's>(
    strand: &mut Strand<'v, 's>,
    mut args: Args<'v, '_>,
    out: Slot<'v, '_>,
) -> Result<'v, 's, ()> {
    let Some(Arg::Pos(name)) = args.next() else {
        return Err(Error::missing_positional(strand, 0));
    };
    let Some(Arg::Pos(module_name)) = args.next() else {
        return Err(Error::missing_positional(strand, 1));
    };

    // Extract class name
    let name: alias::Box<str> = name
        .as_str_raw(strand)
        .ok_or_else(|| Error::type_error(strand, "class_create: expected string name"))?
        .into();
    let module_name = module_name
        .as_str_raw(strand)
        .ok_or_else(|| Error::type_error(strand, "class_create: expected string module name"))?;
    let module_name = if module_name.is_empty() {
        None
    } else {
        Some(alias::Box::<str>::from(module_name))
    };

    let mut supers = Vec::new();
    let mut local_entries = HashMap::new();
    let mut field_defaults = Vec::new();
    let mut symbols = Vec::new();

    while let Some(arg) = args.next() {
        let (key, mut slot) = match arg {
            Arg::Pos(_slot) => {
                return Err(Error::type_error(
                    strand,
                    "class_create: unexpected positional argument",
                ));
            }
            Arg::Key(key, slot) => (key, slot),
        };

        match key.tag() {
            sym::SUPER => {
                if !slot.is_instance_of(strand, &strand.singletons().type_obj) {
                    return Err(Error::type_error(
                        strand,
                        "class_create: superclass must be a type object",
                    ));
                }
                supers.push(slot.take());
            }
            sym::FIELD | sym::FIELD_THUNK => {
                let sym = unsafe {
                    slot.as_sym(strand)
                        .ok_or_else(|| {
                            Error::type_error(strand, "class_create: field name must be a symbol")
                        })?
                        .into_static_scope_unchecked()
                };
                symbols.push(strand.sym_obj(sym));
                let Some(Arg::Pos(mut default)) = args.next() else {
                    return Err(Error::type_error(
                        strand,
                        "class_create: field entry must include a default value",
                    ));
                };
                let slot = field_defaults.len();
                let default = if key.tag() == sym::FIELD_THUNK {
                    if !default.is_instance_of(strand, &strand.singletons().func) {
                        return Err(Error::type_error(
                            strand,
                            "class_create: field thunk must be a function",
                        ));
                    }
                    FieldDefault::Thunk(default.take())
                } else {
                    FieldDefault::Value(default.take())
                };
                field_defaults.push(default);
                match local_entries.entry(sym) {
                    Entry::Vacant(entry) => {
                        entry.insert(ClassEntry::Field(slot));
                    }
                    Entry::Occupied(_) => {
                        return Err(Error::runtime(
                            strand,
                            format!(
                                "class_create: duplicate class member `{}`",
                                sym.as_str(strand)
                            ),
                        ));
                    }
                }
            }
            sym::METHOD => {
                let sym = unsafe {
                    slot.as_sym(strand)
                        .ok_or_else(|| {
                            Error::type_error(strand, "class_create: method name must be a symbol")
                        })?
                        .into_static_scope_unchecked()
                };
                symbols.push(strand.sym_obj(sym));
                let Some(Arg::Pos(mut value)) = args.next() else {
                    return Err(Error::type_error(
                        strand,
                        "class_create: method entry must include a value",
                    ));
                };
                let value = value.take();

                if value.is_instance_of(strand, &strand.singletons().getter) {
                    match local_entries.entry(sym) {
                        Entry::Vacant(entry) => {
                            entry.insert(ClassEntry::Property(Property {
                                getter: Some(value),
                                setter: None,
                            }));
                        }
                        Entry::Occupied(mut entry) => match entry.get_mut() {
                            ClassEntry::Property(Property { getter, .. }) if getter.is_none() => {
                                *getter = Some(value);
                            }
                            _ => {
                                return Err(Error::runtime(
                                    strand,
                                    format!(
                                        "class_create: duplicate class member `{}`",
                                        sym.as_str(strand)
                                    ),
                                ));
                            }
                        },
                    }
                    continue;
                }

                if value.is_instance_of(strand, &strand.singletons().setter) {
                    match local_entries.entry(sym) {
                        Entry::Vacant(entry) => {
                            entry.insert(ClassEntry::Property(Property {
                                getter: None,
                                setter: Some(value),
                            }));
                        }
                        Entry::Occupied(mut entry) => match entry.get_mut() {
                            ClassEntry::Property(Property { setter, .. }) if setter.is_none() => {
                                *setter = Some(value);
                            }
                            _ => {
                                return Err(Error::runtime(
                                    strand,
                                    format!(
                                        "class_create: duplicate class member `{}`",
                                        sym.as_str(strand)
                                    ),
                                ));
                            }
                        },
                    }
                    continue;
                }

                if !value.is_instance_of(strand, &strand.singletons().func) {
                    return Err(Error::type_error(
                        strand,
                        "class_create: method value must be a function, Getter, or Setter",
                    ));
                }

                match local_entries.entry(sym) {
                    Entry::Vacant(entry) => {
                        entry.insert(ClassEntry::Method(value));
                    }
                    Entry::Occupied(_) => {
                        return Err(Error::runtime(
                            strand,
                            format!(
                                "class_create: duplicate class member `{}`",
                                sym.as_str(strand)
                            ),
                        ));
                    }
                }
            }
            _ => return Err(Error::unexpected_key(strand, key)),
        }
    }

    // Build native_supers and entries in a single left-to-right MRO pass.
    // native_supers: non-abstract native type objects; index == ClassInstance native slot.
    // Abstract entries store the type-object value directly (no separate list needed).
    // entry_map: built left-to-right with first-insertion-wins (MRO order).
    let mut native_supers: Vec<Value<'v>> = Vec::new();
    let mut seen_abstract: Vec<Value<'v>> = Vec::new(); // for dedup only
    let mut entry_map = HashMap::new();

    for sup in supers.iter() {
        if let Some(cls) = sup.downcast_ref(strand.builtin_types().class_object) {
            let cls = cls.get();
            // Inherit parent's native supers (dedup by repr_eq).
            for type_obj in cls.native_supers.iter() {
                if native_supers.iter().any(|s| s.repr_eq(strand, type_obj)) {
                    continue;
                }
                native_supers.push(type_obj.dup());
            }
            // Merge parent's entries (left-wins). Abstract entries copy the value directly.
            for (sym, entry) in cls.entries.iter() {
                if entry_map.contains_key(sym) {
                    continue; // left wins
                }
                let new_entry = match entry {
                    ClassEntry::Field(old_slot) => {
                        let default = match &cls.field_defaults[*old_slot] {
                            FieldDefault::Value(v) => FieldDefault::Value(v.dup()),
                            FieldDefault::Thunk(v) => FieldDefault::Thunk(v.dup()),
                        };
                        let new_slot = field_defaults.len();
                        field_defaults.push(default);
                        ClassEntry::Field(new_slot)
                    }
                    ClassEntry::Method(v) => ClassEntry::Method(v.dup()),
                    ClassEntry::Property(property) => ClassEntry::Property(Property {
                        getter: property.getter.as_ref().map(Value::dup),
                        setter: property.setter.as_ref().map(Value::dup),
                    }),
                    ClassEntry::Delegate(parent_slot, kind) => {
                        // Remap via parent's native_supers → our native_supers.
                        let type_obj = &cls.native_supers[*parent_slot];
                        let our_slot = native_supers
                            .iter()
                            .position(|s| s.repr_eq(strand, type_obj))
                            .expect("bug: parent Delegate slot not found in our native_supers");
                        ClassEntry::Delegate(our_slot, *kind)
                    }
                    ClassEntry::Abstract(type_obj, kind) => {
                        ClassEntry::Abstract(type_obj.dup(), *kind)
                    }
                };
                entry_map.insert(*sym, new_entry);
            }
        } else {
            // Direct native super. Skip if already seen (inherited via a ClassObject parent).
            if native_supers.iter().any(|s| s.repr_eq(strand, sup))
                || seen_abstract.iter().any(|s| s.repr_eq(strand, sup))
            {
                continue;
            }
            let inspect = sup.op_inspect(strand).ok_or_else(|| {
                Error::type_error(strand, "inheritance not supported by superclass")
            })?;
            if inspect.is_abstract {
                // Abstract super: store the type-object directly in each entry.
                seen_abstract.push(sup.dup());
                for member in inspect.members {
                    entry_map
                        .entry(member.sym)
                        .or_insert_with(|| ClassEntry::Abstract(sup.dup(), member.kind));
                }
            } else {
                // Concrete native super: members dispatched via instance native slot.
                let our_slot = native_supers.len();
                native_supers.push(sup.dup());
                for member in inspect.members {
                    entry_map
                        .entry(member.sym)
                        .or_insert(ClassEntry::Delegate(our_slot, member.kind));
                }
            }
        }
    }

    // Apply this class's own entries, overriding
    for (sym, entry) in local_entries {
        entry_map.insert(sym, entry);
    }

    // Sort entries by sym
    let mut entries: Vec<_> = entry_map.into_iter().collect();
    entries.sort_by_key(|(s, _)| *s);

    let class_obj = ClassObject {
        name,
        module_name,
        symbols: symbols.into(),
        entries: unsafe {
            // SAFETY: every symbol in `entries` is explicitly rooted by the
            // corresponding object in `_symbols`, which this ClassObject owns.
            mem::transmute::<Vec<_>, Vec<(Sym<'v, 'static>, ClassEntry<'v>)>>(entries)
        }
        .into(),
        supers: supers.into(),
        field_defaults: field_defaults.into(),
        native_supers: native_supers.into(),
    };

    strand
        .vm()
        .builtin_types()
        .class_object
        .create(strand, class_obj, out);

    Ok(())
}

pub(crate) struct Getter;

unsafe impl Collect for Getter {
    const CYCLIC: bool = false;
    const IMMUTABLE: bool = true;
    type Annex = ();

    fn accept(&self, _visit: &mut dyn Visit) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn clear(&mut self) {}
}

impl<'v> Protocol<'v> for Getter {
    fn op_type<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) {
        Output::set(strand, out, &strand.singletons().type_obj)
    }

    fn op_debug<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        crate::fmt!(strand, w, "<type Getter>")
    }

    fn op_inspect<'a>(_this: Recv<'v, 'a, Self>, _vm: &Vm<'v>) -> Option<Inspect<'v, 'a>> {
        Some(Inspect {
            is_abstract: true,
            members: members![Method(sym::GET)],
        })
    }
}

pub(crate) struct Setter;

unsafe impl Collect for Setter {
    const CYCLIC: bool = false;
    const IMMUTABLE: bool = true;
    type Annex = ();

    fn accept(&self, _visit: &mut dyn Visit) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn clear(&mut self) {}
}

impl<'v> Protocol<'v> for Setter {
    fn op_type<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) {
        Output::set(strand, out, &strand.singletons().type_obj)
    }

    fn op_debug<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        crate::fmt!(strand, w, "<type Setter>")
    }

    fn op_inspect<'a>(_this: Recv<'v, 'a, Self>, _vm: &Vm<'v>) -> Option<Inspect<'v, 'a>> {
        Some(Inspect {
            is_abstract: true,
            members: members![Method(sym::SET)],
        })
    }
}

pub(crate) struct Property<'v> {
    pub(crate) getter: Option<Value<'v>>,
    pub(crate) setter: Option<Value<'v>>,
}

pub(crate) enum FieldDefault<'v> {
    Value(Value<'v>),
    Thunk(Value<'v>),
}

/// A single entry in a class's unified symbol table.
pub(crate) enum ClassEntry<'v> {
    /// Index into `field_defaults` / instance `fields`.
    Field(usize),
    /// A Do function value (method).
    Method(Value<'v>),
    /// A getter/setter pair whose `get`/`set` methods mediate instance access.
    Property(Property<'v>),
    /// Native delegation: index into instance `natives`.
    Delegate(usize, MemberKind),
    /// Abstract delegation: the type-object singleton to dispatch to.
    /// Dispatched on the type object with the instance as delegator.
    Abstract(Value<'v>, MemberKind),
}

pub(crate) struct ClassObject<'v> {
    // Class name (for debug formatting)
    pub(crate) name: alias::Box<str>,
    // Optional module name for debug formatting.
    pub(crate) module_name: Option<alias::Box<str>>,
    // Direct superclasses (for subtype checking); may be ClassObject or built-in type objects
    pub(crate) supers: alias::Box<[Value<'v>]>,
    // Unified MRO-ordered lookup table sorted by Sym. Symbols are rooted by `_symbols`.
    pub(crate) entries: alias::Box<[(Sym<'v, 'static>, ClassEntry<'v>)]>,
    // Roots for the symbols used by `entries`.
    pub(crate) symbols: alias::Box<[GcObj<'v, SymObj>]>,
    // Default values for field slots (indexed by ClassEntry::Field(n))
    pub(crate) field_defaults: alias::Box<[FieldDefault<'v>]>,
    // Non-abstract native supers, in transitive collection order (left-to-right).
    // Index in this slice == slot index in ClassInstance::native_slots.
    pub(crate) native_supers: alias::Box<[Value<'v>]>,
}

impl<'v> ClassObject<'v> {
    /// Look up an entry by symbol.
    pub(crate) fn entry(&self, sym: Sym<'v, '_>) -> Option<&ClassEntry<'v>> {
        self.entries
            .binary_search_by_key(&sym, |(s, _)| *s)
            .ok()
            .map(|idx| &self.entries[idx].1)
    }

    /// Look up a method by symbol.
    pub(crate) fn method(&self, sym: Sym<'v, '_>) -> Option<&Value<'v>> {
        self.entry(sym).and_then(|entry| match entry {
            ClassEntry::Method(v) => Some(v),
            _ => None,
        })
    }

    /// Look up the (init) method.
    pub(crate) fn init(&self) -> Option<&Value<'v>> {
        self.method(Sym::well_known(sym::INIT_METHOD))
    }

    /// Look up an entry by well-known symbol tag.
    pub(crate) fn entry_by_tag(&self, tag: sym::Tag) -> Option<&ClassEntry<'v>> {
        self.entry(Sym::well_known(tag))
    }
}

unsafe impl<'v> Collect for ClassObject<'v> {
    const CYCLIC: bool = true;
    const IMMUTABLE: bool = true;
    type Annex = ();

    fn accept(&self, visit: &mut dyn Visit) -> ControlFlow<()> {
        for sup in self.supers.iter() {
            sup.accept(visit)?;
        }
        for sym in self.symbols.iter() {
            sym.accept(visit)?;
        }
        for (_, entry) in self.entries.iter() {
            match entry {
                ClassEntry::Method(v) | ClassEntry::Abstract(v, _) => v.accept(visit)?,
                ClassEntry::Property(property) => {
                    if let Some(getter) = &property.getter {
                        getter.accept(visit)?;
                    }
                    if let Some(setter) = &property.setter {
                        setter.accept(visit)?;
                    }
                }
                _ => {}
            }
        }
        for v in self.field_defaults.iter() {
            match v {
                FieldDefault::Value(v) | FieldDefault::Thunk(v) => v.accept(visit)?,
            }
        }
        for v in self.native_supers.iter() {
            v.accept(visit)?;
        }
        ControlFlow::Continue(())
    }

    fn clear(&mut self) {
        for v in self.supers.iter_mut() {
            *v = Value::NIL;
        }
        for (_, entry) in self.entries.iter_mut() {
            match entry {
                ClassEntry::Method(v) | ClassEntry::Abstract(v, _) => *v = Value::NIL,
                ClassEntry::Property(property) => {
                    property.getter = None;
                    property.setter = None;
                }
                _ => {}
            }
        }
        for v in self.field_defaults.iter_mut() {
            match v {
                FieldDefault::Value(v) | FieldDefault::Thunk(v) => *v = Value::NIL,
            }
        }
        for v in self.native_supers.iter_mut() {
            *v = Value::NIL;
        }
    }
}

impl<'v> Protocol<'v> for ClassObject<'v> {
    fn op_type<'a, 's>(
        _this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) {
        Output::set(strand, out, &strand.singletons().type_obj)
    }

    fn op_debug<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        let this = this.get();
        if let Some(module_name) = &this.module_name {
            crate::fmt!(strand, w, "<type {module_name}.{}>", this.name)
        } else {
            crate::fmt!(strand, w, "<type {}>", this.name)
        }
    }

    fn op_get<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        field: Sym<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        match field.tag() {
            sym::GET_METHOD | sym::SET_METHOD => {
                BoundMethod::create(strand, &this, field, out);
                return Ok(());
            }
            _ => (),
        }

        let me = this.get();

        // Only methods are accessible on the class type object itself
        if let Some(v) = me.method(field) {
            out.store(v.dup());
            return Ok(());
        }
        Err(Error::field(strand, field))
    }

    fn op_subtype<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        supertype: &Value<'v>,
    ) -> bool {
        supertype.eq(strand, &this)
            || this
                .get()
                .supers
                .iter()
                .any(|sup| sup.op_subtype(strand, supertype))
            || supertype.eq(strand, &strand.singletons().value)
    }

    async fn op_mcall<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        method: Sym<'v, 'a>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let me = this.get();
        match method.tag() {
            sym::GET_METHOD if args.len() == 1 => {
                let ([field], []) = unpack!(strand, args, 1, 0)?;
                let field = field
                    .as_sym(strand)
                    .ok_or_else(|| Error::type_error(strand, "field: expected `Sym`"))?;
                Self::op_get(this, strand, field, out)
            }
            sym::GET_METHOD => {
                let ([obj, field], []) = unpack!(strand, args, 2, 0)?;
                let field = field
                    .as_sym(strand)
                    .ok_or_else(|| Error::type_error(strand, "field: expected `Sym`"))?;
                if !obj.is_instance_of(strand, this.clone()) {
                    return Err(Error::type_error(strand, "invalid class object type"));
                }
                match me.entry(field) {
                    Some(ClassEntry::Field(_)) => {
                        let recv = obj
                            .downcast_ref(strand.builtin_types().class_instance)
                            .expect("object is a class instance");
                        let slot_idx = match recv.annex().class.entry(field) {
                            Some(ClassEntry::Field(slot_idx)) => *slot_idx,
                            _ => {
                                return Err(Error::runtime(
                                    strand,
                                    "can't access plain superclass field that was overridden as a different member type in a dervied class",
                                ));
                            }
                        };
                        let borrow = recv.borrow().ok_or_else(|| Error::concurrency(strand))?;
                        Output::set(strand, out, &borrow.fields[slot_idx]);
                        Ok(())
                    }
                    Some(ClassEntry::Property(Property {
                        getter: Some(getter),
                        ..
                    })) => strand.sync(async |strand| {
                        method!(strand, getter, Sym::well_known(sym::GET), out, obj).await
                    }),
                    Some(ClassEntry::Method(_) | ClassEntry::Abstract(_, _)) => {
                        BoundMethod::create(strand, obj, field, out);
                        Ok(())
                    }
                    Some(ClassEntry::Property { .. }) => Err(Error::field(strand, field)),
                    Some(ClassEntry::Delegate(_slot, _)) => {
                        let native = obj
                            .downcast_ref(strand.builtin_types().class_instance)
                            .ok_or_else(|| Error::type_error(strand, "invalid class object type"))?
                            .annex()
                            .class
                            .entry(field)
                            .and_then(|entry| match entry {
                                ClassEntry::Delegate(slot, _) => obj
                                    .downcast_ref(strand.builtin_types().class_instance)
                                    .and_then(|recv| recv.annex().natives[*slot].get()),
                                _ => None,
                            })
                            .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                        Delegated::new(native, &obj).op_get(strand, field, out)
                    }
                    _ => match me.entry_by_tag(sym::GET_METHOD) {
                        Some(ClassEntry::Method(v)) => {
                            strand.sync(async |strand| call!(strand, v, out, obj, field).await)
                        }
                        Some(ClassEntry::Delegate(slot, _)) => {
                            let recv = obj
                                .downcast_ref(strand.builtin_types().class_instance)
                                .ok_or_else(|| {
                                    Error::type_error(strand, "invalid class object type")
                                })?;
                            let native = recv.annex().natives[*slot].get().ok_or_else(|| {
                                Error::runtime(strand, "native slot uninitialized")
                            })?;
                            Delegated::new(native, &obj).op_get(strand, field, out)
                        }
                        _ => Err(Error::field(strand, field)),
                    },
                }
            }
            sym::SET_METHOD if args.len() == 2 => {
                let ([field, value], []) = unpack!(strand, args, 2, 0)?;
                let field = field
                    .as_sym(strand)
                    .ok_or_else(|| Error::type_error(strand, "field: expected `Sym`"))?;
                Self::op_set(this, strand, field, value)
            }
            sym::SET_METHOD => {
                let ([obj, field, mut value], []) = unpack!(strand, args, 3, 0)?;
                let field = field
                    .as_sym(strand)
                    .ok_or_else(|| Error::type_error(strand, "field: expected `Sym`"))?;
                if !obj.is_instance_of(strand, this.clone()) {
                    return Err(Error::type_error(strand, "invalid class object type"));
                }
                match me.entry(field) {
                    Some(ClassEntry::Field(_)) => {
                        let recv = obj
                            .downcast_ref(strand.builtin_types().class_instance)
                            .ok_or_else(|| {
                                Error::type_error(strand, "invalid class object type")
                            })?;
                        let slot_idx = match recv.annex().class.entry(field) {
                            Some(ClassEntry::Field(slot_idx)) => *slot_idx,
                            _ => return Err(Error::field(strand, field)),
                        };
                        let mut borrow = recv
                            .borrow_mut()
                            .ok_or_else(|| Error::concurrency(strand))?;
                        borrow.fields[slot_idx] = value.take();
                        Ok(())
                    }
                    Some(ClassEntry::Property(Property {
                        setter: Some(setter),
                        ..
                    })) => strand.with_slots_sync(move |strand, [mut tmp]| {
                        strand.sync(async |strand| {
                            method!(
                                strand,
                                setter,
                                Sym::well_known(sym::SET),
                                &mut tmp,
                                obj,
                                &value
                            )
                            .await
                        })
                    }),
                    Some(
                        ClassEntry::Method(_)
                        | ClassEntry::Abstract(_, _)
                        | ClassEntry::Property { .. },
                    ) => Err(Error::field(strand, field)),
                    Some(ClassEntry::Delegate(slot, _)) => {
                        let recv = obj
                            .downcast_ref(strand.builtin_types().class_instance)
                            .ok_or_else(|| {
                                Error::type_error(strand, "invalid class object type")
                            })?;
                        let native = recv.annex().natives[*slot]
                            .get()
                            .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                        Delegated::new(native, &obj).op_set(strand, field, value)
                    }
                    _ => match me.entry_by_tag(sym::SET_METHOD) {
                        Some(ClassEntry::Method(v)) => {
                            strand.with_slots_sync(move |strand, [mut tmp]| {
                                strand.sync(async |strand| {
                                    call!(strand, v, &mut tmp, obj, field, &value).await
                                })
                            })
                        }
                        Some(ClassEntry::Delegate(slot, _)) => {
                            let recv = obj
                                .downcast_ref(strand.builtin_types().class_instance)
                                .ok_or_else(|| {
                                    Error::type_error(strand, "invalid class object type")
                                })?;
                            let native = recv.annex().natives[*slot].get().ok_or_else(|| {
                                Error::runtime(strand, "native slot uninitialized")
                            })?;
                            Delegated::new(native, &obj).op_set(strand, field, value)
                        }
                        _ => Err(Error::field(strand, field)),
                    },
                }
            }
            _ => {
                // Only methods are callable on the class type object itself
                if let Some(v) = me.method(method) {
                    return v.op_call(strand, args, out).await;
                }
                Err(Error::field(strand, method))
            }
        }
    }

    async fn op_call<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        mut args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let me = this.get();
        let native_slot_count = me.native_supers.len();
        let class_obj = this.to_strong();

        strand
            .with_slots(async move |strand, [mut inst, mut tmp]| {
                let mut defaults = Vec::with_capacity(me.field_defaults.len());
                for default in me.field_defaults.iter() {
                    match default {
                        FieldDefault::Value(value) => defaults.push(value.dup()),
                        FieldDefault::Thunk(thunk) => {
                            call!(strand, thunk, &mut tmp).await?;
                            defaults.push(tmp.take());
                        }
                    }
                }
                inst.store(Value::from_object(GcObj::new_annex(
                    strand.arena(),
                    strand.builtin_types().class_instance,
                    ClassInstance {
                        fields: defaults.into(),
                    },
                    ClassInstanceAnnex {
                        class: class_obj,
                        natives: (0..native_slot_count)
                            .map(|_| OnceCell::new())
                            .collect::<Vec<_>>()
                            .into(),
                    },
                )));

                if let Some(func) = me.init() {
                    args.prepend_self(inst.dup());
                    func.op_call(strand, args, tmp).await?;
                } else {
                    let ([], []) = unpack!(strand, args, 0, 0)?;
                }

                // Verify all native slots are initialized
                if inst
                    .downcast_ref(strand.builtin_types().class_instance)
                    .unwrap()
                    .annex()
                    .natives
                    .iter()
                    .any(|slot| slot.get().is_none())
                {
                    return Err(Error::runtime(strand, "native supertypes not initialized"));
                }

                Output::set(strand, out, inst);
                Ok(())
            })
            .await
    }
}

pub(crate) struct ClassInstanceAnnex<'v> {
    pub(crate) class: GcObj<'v, ClassObject<'v>>,
    pub(crate) natives: alias::Box<[OnceCell<Value<'v>>]>,
}

impl<'v> Annex for ClassInstanceAnnex<'v> {
    fn accept(&self, visit: &mut dyn Visit) -> ControlFlow<()> {
        self.class.accept(visit)?;
        for slot in self.natives.iter() {
            if let Some(v) = slot.get() {
                v.accept(visit)?;
            }
        }
        ControlFlow::Continue(())
    }

    fn clear(&self) {
        // GC cannot safely clear annexes with outstanding immutable references
    }
}

pub(crate) struct ClassInstance<'v> {
    fields: alias::Box<[Value<'v>]>,
}

unsafe impl<'v> Collect for ClassInstance<'v> {
    const CYCLIC: bool = true;
    const IMMUTABLE: bool = false;
    type Annex = ClassInstanceAnnex<'v>;

    fn accept(&self, visit: &mut dyn Visit) -> ControlFlow<()> {
        self.fields.accept(visit)
    }

    fn clear(&mut self) {
        self.fields.fill_with(|| Value::NIL);
    }
}

/// Given a [`ClassInstance`] GC borrow (as returned by [`Value::downcast_ref`]) and a
/// native super's type object, returns a reference to the initialized native slot value,
/// if any.  The returned reference's lifetime is linked to that of the borrow.
pub(crate) fn get_native_slot<'v, 'a>(
    vm: &Vm<'v>,
    borrow: GcObjBorrow<'v, 'a, ClassInstance<'v>>,
    type_obj: &Value<'v>,
) -> Option<&'a Value<'v>> {
    let annex = borrow.annex();
    let slot = annex
        .class
        .native_supers
        .iter()
        .position(|s| s.repr_eq(vm, type_obj))?;
    annex.natives[slot].get()
}

/// Returns an iterator over all initialized native values in a [`ClassInstance`].
/// The lifetime of each yielded `&Value<'v>` is linked to the borrow lifetime `'a`.
pub(crate) fn iter_natives<'v, 'a>(
    borrow: GcObjBorrow<'v, 'a, ClassInstance<'v>>,
) -> impl Iterator<Item = &'a Value<'v>> {
    borrow.annex().natives.iter().filter_map(|s| s.get())
}

fn readable_class_entry(entry: &ClassEntry<'_>) -> bool {
    matches!(
        entry,
        ClassEntry::Field(_)
            | ClassEntry::Property(Property {
                getter: Some(_),
                ..
            })
            | ClassEntry::Delegate(_, MemberKind::Getter | MemberKind::Property)
    )
}

fn read_class_entry<'v, 'a, 's>(
    this: Recv<'v, 'a, ClassInstance<'v>>,
    strand: &mut Strand<'v, 's>,
    sym: Sym<'v, '_>,
    entry: &ClassEntry<'v>,
    mut out: Slot<'v, '_>,
) -> Result<'v, 's, bool> {
    let result = match entry {
        ClassEntry::Field(slot_index) => {
            let borrow = this.borrow(strand)?;
            out.store(borrow.fields[*slot_index].dup());
            Ok(())
        }
        ClassEntry::Property(Property {
            getter: Some(getter),
            ..
        }) => strand.sync(async |strand| {
            method!(strand, getter, Sym::well_known(sym::GET), out, &this).await
        }),
        ClassEntry::Delegate(slot, MemberKind::Getter | MemberKind::Property) => {
            let native = this.annex().natives[*slot]
                .get()
                .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
            strand.with_slots_sync(|strand, [mut delegator]| {
                Output::set(strand, Slot::reborrow(&mut delegator), &this);
                Delegated::new(native, &delegator).op_get(strand, sym, out)
            })
        }
        _ => unreachable!(),
    };
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::Field => Ok(false),
        Err(error) => Err(error),
    }
}

async fn default_class_unpack<'v, 'a, 's>(
    this: Recv<'v, 'a, ClassInstance<'v>>,
    strand: &'a mut Strand<'v, 's>,
    sig: &'a Unpack<'v, 'a>,
    mut out: Slots<'v, 'a>,
) -> Result<'v, 's, ()> {
    let pos_count = sig.required + sig.optional.len();
    if sig.required != 0 {
        return Err(Error::missing_positional(strand, 0));
    }

    strand
        .with_slots_dynamic(sig.len(), async |strand, mut staged| {
            for (index, default) in sig.optional.iter().enumerate() {
                staged.at(index).store(default.dup());
            }

            let class = &this.annex().class;
            let entries = &class.entries;
            let track = sig.variadic != Variadic::Discard;
            let mut matched: Option<BitBox> = track.then(|| bitbox![0; entries.len()]);

            for (key_index, key) in sig.keys.iter().enumerate() {
                let dest = pos_count + key_index;
                let found = match &key.kind {
                    sig::UnpackKeyKind::Sym(sym) => entries
                        .binary_search_by_key(sym, |(candidate, _)| *candidate)
                        .ok()
                        .filter(|index| {
                            let (sym, entry) = &entries[*index];
                            !strand.sym_obj(*sym).private && readable_class_entry(entry)
                        }),
                    sig::UnpackKeyKind::Const(_) => None,
                };
                let present = if let Some(index) = found {
                    let (sym, entry) = &entries[index];
                    read_class_entry(this.clone(), strand, *sym, entry, staged.at(dest))?
                } else {
                    false
                };
                if !present {
                    if let Some(default) = &key.default {
                        staged.at(dest).store(default.dup());
                    } else {
                        return Err(match &key.kind {
                            sig::UnpackKeyKind::Sym(sym) => Error::missing_key(strand, *sym),
                            sig::UnpackKeyKind::Const(value) => Error::missing_key(strand, value),
                        });
                    }
                }
                if let (Some(index), Some(matched)) = (found, &mut matched) {
                    matched.set(index, true);
                }
            }

            if sig.variadic == Variadic::None {
                let matched = matched.as_mut().unwrap();
                for (index, (sym, entry)) in entries.iter().enumerate() {
                    if matched[index]
                        || strand.sym_obj(*sym).private
                        || !readable_class_entry(entry)
                    {
                        continue;
                    }
                    let present = strand.with_slots_sync(|strand, [tmp]| {
                        read_class_entry(this.clone(), strand, *sym, entry, tmp)
                    })?;
                    if present {
                        return Err(Error::unexpected_key(strand, *sym));
                    }
                    matched.set(index, true);
                }
            }

            if sig.variadic == Variadic::Capture {
                let matched = matched.as_ref().unwrap();
                let symbols = entries
                    .iter()
                    .enumerate()
                    .filter(|(index, (sym, entry))| {
                        !matched[*index]
                            && !strand.sym_obj(*sym).private
                            && readable_class_entry(entry)
                    })
                    .map(|(_, (sym, _))| strand.sym_obj(*sym))
                    .collect::<VecDeque<_>>();
                strand.builtin_types().field_iter.create(
                    strand,
                    FieldIter::new(Value::from_object(this.to_strong()), symbols),
                    staged.at(sig.len() - 1),
                );
            }

            for index in 0..sig.len() {
                out.at(index).store(staged.at(index).take());
            }
            Ok(())
        })
        .await
}

fn class_sync_binary_op<'v, 'a, 's>(
    this: Recv<'v, 'a, ClassInstance<'v>>,
    strand: &mut Strand<'v, 's>,
    method_sym: sym::Tag,
    other: &Value<'v>,
) -> Result<'v, 's, Value<'v>> {
    let annex = this.annex();
    match annex.class.entry_by_tag(method_sym) {
        Some(ClassEntry::Method(v)) => strand.with_slots_sync(move |strand, [mut result]| {
            strand.sync(async |strand| call!(strand, v, &mut result, &this, other).await)?;
            Ok(result.take())
        }),
        Some(ClassEntry::Delegate(slot, _)) => {
            let native = annex.natives[*slot]
                .get()
                .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
            strand.with_slots_sync(|strand, [mut delegator]| {
                Output::set(strand, Slot::reborrow(&mut delegator), &this);
                let native = Delegated::new(native, &delegator);
                match method_sym {
                    sym::ADD_METHOD => native.op_add(strand, other),
                    sym::SUB_METHOD => native.op_sub(strand, other),
                    sym::RSUB_METHOD => native.op_rsub(strand, other),
                    sym::MUL_METHOD => native.op_mul(strand, other),
                    sym::DIV_METHOD => native.op_div(strand, other),
                    sym::RDIV_METHOD => native.op_rdiv(strand, other),
                    sym::EDIV_METHOD => native.op_ediv(strand, other),
                    sym::REDIV_METHOD => native.op_rediv(strand, other),
                    sym::MOD_METHOD => native.op_mod(strand, other),
                    sym::RMOD_METHOD => native.op_rmod(strand, other),
                    sym::BAND_METHOD => native.op_band(strand, other),
                    sym::BOR_METHOD => native.op_bor(strand, other),
                    sym::BXOR_METHOD => native.op_bxor(strand, other),
                    sym::SHL_METHOD => native.op_shl(strand, other),
                    sym::SHR_METHOD => native.op_shr(strand, other),
                    sym::EQ_METHOD => native.op_eq(strand, other),
                    sym::LT_METHOD => native.op_lt(strand, other),
                    _ => unreachable!(),
                }
            })
        }
        _ => Err(Error::not_supported(strand)),
    }
}

fn class_sync_unary_op<'v, 'a, 's>(
    this: Recv<'v, 'a, ClassInstance<'v>>,
    strand: &mut Strand<'v, 's>,
    method_sym: sym::Tag,
) -> Result<'v, 's, Value<'v>> {
    let annex = this.annex();
    match annex.class.entry_by_tag(method_sym) {
        Some(ClassEntry::Method(v)) => strand.with_slots_sync(move |strand, [mut result]| {
            strand.sync(async |strand| call!(strand, v, &mut result, &this).await)?;
            Ok(result.take())
        }),
        Some(ClassEntry::Delegate(slot, _)) => {
            let native = annex.natives[*slot]
                .get()
                .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
            strand.with_slots_sync(|strand, [mut delegator]| {
                Output::set(strand, Slot::reborrow(&mut delegator), &this);
                let native = Delegated::new(native, &delegator);
                match method_sym {
                    sym::NEG_METHOD => native.op_neg(strand),
                    sym::BNOT_METHOD => native.op_bnot(strand),
                    _ => unreachable!(),
                }
            })
        }
        _ => Err(Error::not_supported(strand)),
    }
}

impl<'v> Protocol<'v> for ClassInstance<'v> {
    fn op_fill<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        type_obj: &Value<'v>,
        native: Value<'v>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        let idx = annex
            .class
            .native_supers
            .iter()
            .position(|sup| sup.repr_eq(strand, type_obj))
            .ok_or_else(|| {
                Error::type_error(strand, "not a concrete native super of this class")
            })?;

        // Check if slot is already set with the same value (idempotent for diamond inheritance)
        if let Some(existing) = annex.natives[idx].get() {
            if existing.repr_eq(strand, &native) {
                return Ok(());
            } else {
                return Err(Error::runtime(
                    strand,
                    "native slot already initialized with a different value",
                ));
            }
        }

        annex.natives[idx]
            .set(native)
            .map_err(|_| Error::state_error(strand, "native slot already initialized"))
    }

    fn op_type<'a, 's>(
        this: Recv<'v, 'a, Self>,
        _strand: &'a mut Strand<'v, 's>,
        mut out: Slot<'v, 'a>,
    ) {
        out.store(Value::from_object(this.annex().class.clone()))
    }

    fn op_display<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        let res = {
            let w = &mut *w;
            let annex = this.annex();
            match annex.class.entry_by_tag(sym::STR_METHOD) {
                Some(ClassEntry::Method(v)) => strand.with_slots_sync(|strand, [mut result]| {
                    strand.sync(async |strand| call!(strand, v, &mut result, &this).await)?;
                    let result = result
                        .as_str_raw(strand)
                        .ok_or_else(|| Error::type_error(strand, "expected Str result"))?;
                    crate::fmt!(strand, w, "{result}")?;
                    Ok(false)
                })?,
                Some(ClassEntry::Delegate(slot, _)) => {
                    let native = annex.natives[*slot]
                        .get()
                        .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                    strand.with_slots_sync(|strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator).op_display(strand, w)
                    })?;
                    false
                }
                _ => true,
            }
        };
        if res {
            Self::op_debug(this, strand, w)
        } else {
            Ok(())
        }
    }

    fn op_debug<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::DBG_METHOD) {
            Some(ClassEntry::Method(v)) => strand.with_slots_sync(move |strand, [mut result]| {
                strand.sync(async |strand| call!(strand, v, &mut result, &this).await)?;
                let result = result
                    .as_str_raw(strand)
                    .ok_or_else(|| Error::type_error(strand, "expected Str result"))?;
                crate::fmt!(strand, w, "{result}")
            }),
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand.with_slots_sync(|strand, [mut delegator]| {
                    Output::set(strand, Slot::reborrow(&mut delegator), &this);
                    Delegated::new(native, &delegator).op_debug(strand, w)
                })
            }
            _ => {
                if let Some(module) = &annex.class.module_name {
                    crate::fmt!(strand, w, "<{module}.{}>", annex.class.name)
                } else {
                    crate::fmt!(strand, w, "<{}>", annex.class.name)
                }
            }
        }
    }

    fn op_verbatim<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn crate::value::Format<'v>,
    ) -> Result<'v, 's, ()> {
        let res = {
            let w = &mut *w;
            let this = this.clone();
            let annex = this.annex();
            match annex.class.entry_by_tag(sym::VERBATIM_METHOD) {
                Some(ClassEntry::Method(v)) => {
                    strand.with_slots_sync(move |strand, [mut result]| {
                        strand.sync(async |strand| call!(strand, v, &mut result, &this).await)?;
                        let result = result
                            .as_str_raw(strand)
                            .ok_or_else(|| Error::type_error(strand, "expected Str result"))?;
                        crate::fmt!(strand, w, "{result}")?;
                        Ok(false)
                    })?
                }
                Some(ClassEntry::Delegate(slot, _)) => {
                    let native = annex.natives[*slot]
                        .get()
                        .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                    strand.with_slots_sync(|strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator).op_verbatim(strand, w)
                    })?;
                    false
                }
                _ => true,
            }
        };
        if res {
            Self::op_display(this, strand, w)
        } else {
            Ok(())
        }
    }

    fn op_get<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        field: Sym<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        match field.tag() {
            sym::GET_METHOD | sym::SET_METHOD => {
                BoundMethod::create(strand, &this, field, out);
                return Ok(());
            }
            _ => (),
        }
        let annex = this.annex();
        match annex.class.entry(field) {
            Some(ClassEntry::Field(slot_idx)) => {
                let borrow = this.borrow(strand)?;
                out.store(borrow.fields[*slot_idx].dup());
                Ok(())
            }
            Some(ClassEntry::Property(Property {
                getter: Some(getter),
                ..
            })) => strand.sync(async |strand| {
                method!(strand, getter, Sym::well_known(sym::GET), out, &this).await
            }),
            Some(ClassEntry::Method(_) | ClassEntry::Abstract(_, _)) => {
                BoundMethod::create(strand, &this, field, out);
                Ok(())
            }
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand.with_slots_sync(|strand, [mut delegator]| {
                    Output::set(strand, Slot::reborrow(&mut delegator), &this);
                    Delegated::new(native, &delegator).op_get(strand, field, out)
                })
            }
            _ => match annex.class.entry_by_tag(sym::GET_METHOD) {
                Some(ClassEntry::Method(v)) => {
                    strand.sync(async |strand| call!(strand, v, out, &this, field).await)
                }
                Some(ClassEntry::Delegate(slot, _)) => {
                    let native = annex.natives[*slot]
                        .get()
                        .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                    strand.with_slots_sync(|strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator).op_get(strand, field, out)
                    })
                }
                _ => Err(Error::field(strand, field)),
            },
        }
    }

    fn op_set<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        field: Sym<'v, 'a>,
        mut value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry(field) {
            Some(ClassEntry::Field(slot_idx)) => {
                let mut borrow = this.borrow_mut(strand)?;
                borrow.fields[*slot_idx] = value.take();
                Ok(())
            }
            Some(ClassEntry::Property(Property {
                setter: Some(setter),
                ..
            })) => strand.with_slots_sync(move |strand, [mut tmp]| {
                strand.sync(async |strand| {
                    method!(
                        strand,
                        setter,
                        Sym::well_known(sym::SET),
                        &mut tmp,
                        &this,
                        &value
                    )
                    .await
                })
            }),
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand.with_slots_sync(|strand, [mut delegator]| {
                    Output::set(strand, Slot::reborrow(&mut delegator), &this);
                    Delegated::new(native, &delegator).op_set(strand, field, value)
                })
            }
            _ => match annex.class.entry_by_tag(sym::SET_METHOD) {
                Some(ClassEntry::Method(v)) => strand.with_slots_sync(move |strand, [mut tmp]| {
                    strand
                        .sync(async |strand| call!(strand, v, &mut tmp, &this, field, &value).await)
                }),
                Some(ClassEntry::Delegate(slot, _)) => {
                    let native = annex.natives[*slot]
                        .get()
                        .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                    strand.with_slots_sync(|strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator).op_set(strand, field, value)
                    })
                }
                _ => Err(Error::field(strand, field)),
            },
        }
    }

    async fn op_mcall<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        method: Sym<'v, 'a>,
        mut args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        match method.tag() {
            sym::GET_METHOD => {
                let ([field], []) = unpack!(strand, args, 1, 0)?;
                let field = field
                    .as_sym(strand)
                    .ok_or_else(|| Error::type_error(strand, "field: expected `Sym`"))?;
                return Self::op_get(this, strand, field, out);
            }
            sym::SET_METHOD => {
                let ([field, value], []) = unpack!(strand, args, 2, 0)?;
                let field = field
                    .as_sym(strand)
                    .ok_or_else(|| Error::type_error(strand, "field: expected `Sym`"))?;
                return Self::op_set(this, strand, field, value);
            }
            _ => (),
        }

        let class = &this.annex().class;
        match class.entry(method) {
            Some(ClassEntry::Method(v)) => {
                args.prepend_self(Value::from_object(this.to_strong()));
                return v.op_call(strand, args, out).await;
            }
            Some(ClassEntry::Field(slot_idx)) => {
                let func = {
                    let borrow = this.borrow(strand)?;
                    borrow.fields[*slot_idx].dup()
                };
                return func.op_call(strand, args, out).await;
            }
            Some(ClassEntry::Property(Property {
                getter: Some(getter),
                ..
            })) => {
                return strand
                    .with_slots(async move |strand, [mut callable]| {
                        method!(
                            strand,
                            getter,
                            Sym::well_known(sym::GET),
                            &mut callable,
                            &this
                        )
                        .await?;
                        callable.op_call(strand, args, out).await
                    })
                    .await;
            }
            Some(ClassEntry::Property(Property { getter: None, .. })) => {}
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = this.annex().natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                return strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_mcall(strand, method, args, out)
                            .await
                    })
                    .await;
            }
            Some(ClassEntry::Abstract(type_obj, _)) => {
                return strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(type_obj, &delegator)
                            .op_mcall(strand, method, args, out)
                            .await
                    })
                    .await;
            }
            None => {}
        }

        if let Some(v) = class.method(Sym::well_known(sym::GET_METHOD)) {
            return strand
                .with_slots(async move |strand, [mut callable]| {
                    call!(strand, v, &mut callable, &this, method).await?;
                    callable.op_call(strand, args, out).await
                })
                .await;
        }

        Err(Error::field(strand, method))
    }

    async fn op_call<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        mut args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::CALL_METHOD) {
            Some(ClassEntry::Method(v)) => {
                args.prepend_self(Value::from_object(this.to_strong()));
                v.op_call(strand, args, out).await
            }
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_call(strand, args, out)
                            .await
                    })
                    .await
            }
            _ => Err(Error::not_supported(strand)),
        }
    }

    async fn op_unpack<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        sig: &'a Unpack<'v, 'a>,
        out: Slots<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::UNPACK_METHOD) {
            Some(ClassEntry::Method(v)) => {
                strand
                    .with_slots(async move |strand, [mut proxy]| {
                        call!(strand, v, &mut proxy, &this).await?;
                        proxy.op_unpack(strand, sig, out).await
                    })
                    .await
            }
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_unpack(strand, sig, out)
                            .await
                    })
                    .await
            }
            _ => default_class_unpack(this, strand, sig, out).await,
        }
    }

    async fn op_iter<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::ITER_METHOD) {
            Some(ClassEntry::Method(v)) => call!(strand, v, out, &this).await,
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_iter(strand, out)
                            .await
                    })
                    .await
            }
            _ => Err(Error::not_supported(strand)),
        }
    }

    async fn op_sink<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::SINK_METHOD) {
            Some(ClassEntry::Method(v)) => call!(strand, v, out, &this).await,
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_sink(strand, out)
                            .await
                    })
                    .await
            }
            _ => Err(Error::not_supported(strand)),
        }
    }

    async fn op_spread<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        context: SpreadContext,
        sink: &'a mut dyn Spread<'v, 's>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::SPREAD_METHOD) {
            Some(ClassEntry::Method(v)) => {
                let proxy = strand
                    .with_slots(async move |strand, [mut proxy]| {
                        call!(strand, v, &mut proxy, &this).await?;
                        Ok(proxy.take())
                    })
                    .await?;
                proxy.op_spread(strand, context, sink).await
            }
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_spread(strand, context, sink)
                            .await
                    })
                    .await
            }
            _ => default_spread(strand, this.clone(), context, sink).await,
        }
    }

    async fn op_next<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, bool> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::NEXT_METHOD) {
            Some(ClassEntry::Method(v)) => match call!(strand, v, out, &this).await {
                Ok(()) => Ok(true),
                Err(err) if err.kind() == ErrorKind::IterStop => Ok(false),
                Err(err) => Err(err),
            },
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_next(strand, out)
                            .await
                    })
                    .await
            }
            _ => Err(Error::not_supported(strand)),
        }
    }

    async fn op_put<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        item: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::PUT_METHOD) {
            Some(ClassEntry::Method(v)) => {
                strand
                    .with_slots(async move |strand, [tmp]| {
                        call!(strand, v, tmp, &this, &item).await
                    })
                    .await
            }
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand
                    .with_slots(async move |strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator)
                            .op_put(strand, item)
                            .await
                    })
                    .await
            }
            _ => Err(Error::not_supported(strand)),
        }
    }

    fn op_add<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::ADD_METHOD, other)
    }

    fn op_sub<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::SUB_METHOD, other)
    }

    fn op_rsub<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::RSUB_METHOD, other)
    }

    fn op_mul<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::MUL_METHOD, other)
    }

    fn op_div<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::DIV_METHOD, other)
    }

    fn op_rdiv<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::RDIV_METHOD, other)
    }

    fn op_ediv<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::EDIV_METHOD, other)
    }

    fn op_rediv<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::REDIV_METHOD, other)
    }

    fn op_mod<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::MOD_METHOD, other)
    }

    fn op_rmod<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::RMOD_METHOD, other)
    }

    fn op_band<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::BAND_METHOD, other)
    }

    fn op_bor<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::BOR_METHOD, other)
    }

    fn op_bxor<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::BXOR_METHOD, other)
    }

    fn op_shl<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::SHL_METHOD, other)
    }

    fn op_shr<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::SHR_METHOD, other)
    }

    fn op_neg<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_unary_op(this, strand, sym::NEG_METHOD)
    }

    fn op_bnot<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_unary_op(this, strand, sym::BNOT_METHOD)
    }

    fn op_eq<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::EQ_METHOD, other)
    }

    fn op_lt<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, Value<'v>> {
        class_sync_binary_op(this, strand, sym::LT_METHOD, other)
    }

    fn op_bool<'a, 's>(this: Recv<'v, 'a, Self>, strand: &mut Strand<'v, 's>) -> bool {
        let this = this.clone();
        let annex = this.annex();
        match annex.class.entry_by_tag(sym::BOOL_METHOD) {
            Some(ClassEntry::Method(v)) => strand
                .with_slots_sync(move |strand, [mut result]| {
                    strand.sync(async |strand| call!(strand, v, &mut result, &this).await)?;
                    Ok::<_, crate::error::Error<'v, 's>>(result.take().op_bool(strand))
                })
                .unwrap_or(true),
            Some(ClassEntry::Delegate(slot, _)) => annex.natives[*slot]
                .get()
                .map(|native| {
                    strand.with_slots_sync(|strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator).op_bool(strand)
                    })
                })
                .unwrap_or(true),
            _ => true,
        }
    }

    fn op_index<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        index: &Value<'v>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry(Sym::well_known(sym::INDEX_METHOD)) {
            Some(ClassEntry::Method(v)) => {
                strand.sync(async |strand| call!(strand, v, out, &this, index).await)
            }
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand.with_slots_sync(|strand, [mut delegator]| {
                    Output::set(strand, Slot::reborrow(&mut delegator), &this);
                    Delegated::new(native, &delegator).op_index(strand, index, out)
                })
            }
            _ => Err(Error::type_error(strand, "indexing not supported")),
        }
    }

    fn op_assign<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &mut Strand<'v, 's>,
        index: Slot<'v, 'a>,
        value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let annex = this.annex();
        match annex.class.entry(Sym::well_known(sym::ASSIGN_METHOD)) {
            Some(ClassEntry::Method(v)) => strand.with_slots_sync(move |strand, [mut tmp]| {
                strand.sync(async |strand| call!(strand, v, &mut tmp, &this, &index, &value).await)
            }),
            Some(ClassEntry::Delegate(slot, _)) => {
                let native = annex.natives[*slot]
                    .get()
                    .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                strand.with_slots_sync(|strand, [mut delegator]| {
                    Output::set(strand, Slot::reborrow(&mut delegator), &this);
                    Delegated::new(native, &delegator).op_assign(strand, index, value)
                })
            }
            _ => Err(Error::type_error(strand, "index assignment not supported")),
        }
    }

    fn op_hash<'a, 's>(
        this: Recv<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        hasher: &mut std::collections::hash_map::DefaultHasher,
    ) -> Result<'v, 's, ()> {
        use std::hash::Hash;
        let did_hash = {
            let hasher = &mut *hasher;
            let this = this.clone();
            let annex = this.annex();
            match annex.class.entry(Sym::well_known(sym::HASH_METHOD)) {
                Some(ClassEntry::Method(v)) => {
                    strand.with_slots_sync(move |strand, [mut result]| {
                        strand.sync(async |strand| call!(strand, v, &mut result, &this).await)?;
                        let v = result.to_int(strand).map_err(|_| {
                            Error::type_error(strand, "expected Int result from (hash)")
                        })?;
                        v.hash(hasher);
                        Ok(true)
                    })?
                }
                Some(ClassEntry::Delegate(slot, _)) => {
                    let native = annex.natives[*slot]
                        .get()
                        .ok_or_else(|| Error::runtime(strand, "native slot uninitialized"))?;
                    strand.with_slots_sync(|strand, [mut delegator]| {
                        Output::set(strand, Slot::reborrow(&mut delegator), &this);
                        Delegated::new(native, &delegator).op_hash(strand, hasher)
                    })?;
                    true
                }
                _ => false,
            }
        };
        if !did_hash {
            this.receiver.into_raw().hash(hasher);
        }
        Ok(())
    }
}
