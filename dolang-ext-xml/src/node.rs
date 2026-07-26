use dolang::runtime::object::fmt;

use dolang::runtime::{
    Args, Error, Instance, Object, Output, Result, Slot, Strand, Type, Value,
    object::{ArrayLike, ArrayView, DictLike, DictView, DictViewSink, Mut, Ref, TypeBuilder},
    unpack,
    value::{Empty, Nil, TypeObject},
};

use crate::global::Global;

pub(crate) const CHILDREN: usize = 0;
pub(crate) const STACK: usize = 0;

pub(crate) struct Node {
    pub(crate) tag: String,
    pub(crate) attrs: Vec<(String, String)>,
}

pub(crate) struct NodeAnnex;

struct Children;

impl<'v> ArrayLike<'v> for Children {
    type Object = Node;
    const MODULE: &'v str = "xml";
    const NAME: &'v str = "Children";

    fn len(&self, this: Instance<'v, '_, Node>, strand: &mut Strand<'v, '_>) -> usize {
        Ref::slot::<CHILDREN>(&this.borrow_unwrap())
            .as_array(strand)
            .unwrap()
            .len(strand)
            .expect("conflicting child array borrow")
    }

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, Node>,
        strand: &'a mut Strand<'v, 's>,
        index: usize,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let found = Ref::slot::<CHILDREN>(&this.borrow(strand)?)
            .as_array(strand)
            .unwrap()
            .get(strand, index, out)?;
        debug_assert!(found);
        Ok(())
    }
}

struct Attrs;

impl<'v> DictLike<'v> for Attrs {
    type Object = Node;
    const MODULE: &'v str = "xml";
    const NAME: &'v str = "Attrs";

    fn len(&self, this: Instance<'v, '_, Node>, _strand: &mut Strand<'v, '_>) -> usize {
        this.borrow_unwrap().attrs.len()
    }

    fn get<'a, 's>(
        &self,
        this: Instance<'v, '_, Node>,
        strand: &'a mut Strand<'v, 's>,
        key: &Value<'v>,
        instance: i64,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, bool> {
        // Attribute names are unique per element, so there is never more
        // than one value to select among.
        if !matches!(instance, 0 | -1) {
            return Ok(false);
        }
        let Some(key) = key.as_str(strand) else {
            return Ok(false);
        };
        let borrow = this.borrow(strand)?;
        if let Some((_, value)) =
            strand.access(|x| borrow.attrs.iter().find(|(name, _)| name == key.as_str(x)))
        {
            Output::set(strand, out, value.as_str());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn set<'a, 's>(
        &self,
        this: Instance<'v, '_, Node>,
        strand: &'a mut Strand<'v, 's>,
        key: Slot<'v, 'a>,
        value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        <Node as Object>::assign(this, strand, key, value)
    }

    fn flatten<'s>(
        &self,
        this: Instance<'v, '_, Node>,
        strand: &mut Strand<'v, 's>,
        sink: &mut DictViewSink<'v, '_>,
    ) -> Result<'v, 's, ()> {
        let borrow = this.borrow(strand)?;
        for (key, value) in &borrow.attrs {
            sink.push(strand, key.as_str(), value.as_str());
        }
        Ok(())
    }
}

impl<'v> Object<'v> for Node {
    const MODULE: &'static str = "xml";
    const NAME: &'static str = "Node";
    const SLOTS: usize = 1;
    type Annex = NodeAnnex;
    type Type = ();
    type TypeAnnex = ();

    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ([tag], []) = unpack!(strand, args, 1, 0)?;
        let tag = tag
            .as_str(strand)
            .ok_or_else(|| Error::type_error(strand, "expected str"))?
            .to_string();
        this.create_with_annex(
            strand,
            Node {
                tag,
                attrs: Vec::new(),
            },
            NodeAnnex,
            &mut out,
        );
        this.cast(&out).unwrap().enter_sync(strand, |strand, inst| {
            let mut borrow = inst.borrow_mut_unwrap();
            Output::set(strand, Mut::slot_mut::<CHILDREN>(&mut borrow), Empty::Array);
        });
        Ok(())
    }

    fn debug<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn dolang::runtime::Format<'v>,
    ) -> Result<'v, 's, ()> {
        let borrow = this.borrow(strand)?;
        fmt!(strand, w, "<xml.Node {}>", borrow.tag)
    }

    fn index<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        index: &Value<'v>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let key = index
            .as_str(strand)
            .ok_or_else(|| Error::type_error(strand, "index: expected str"))?;
        let borrow = this.borrow(strand)?;
        if let Some((_, val)) = strand.access(|x| {
            let key = key.as_str(x);
            borrow.attrs.iter().find(|(k, _)| k == key)
        }) {
            Output::set(strand, out, val.as_str());
            Ok(())
        } else {
            Err(Error::index(strand))
        }
    }

    fn assign<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        index: Slot<'v, 'a>,
        value: Slot<'v, '_>,
    ) -> Result<'v, 's, ()> {
        let key = index
            .as_str(strand)
            .ok_or_else(|| Error::type_error(strand, "index: expected str"))?
            .to_string();
        let val = value
            .as_str(strand)
            .ok_or_else(|| Error::type_error(strand, "value: expected str"))?
            .to_string();
        let mut borrow = this.borrow_mut(strand)?;
        if let Some(pair) = borrow.attrs.iter_mut().find(|(k, _)| k == &key) {
            pair.1 = val;
        } else {
            borrow.attrs.push((key, val));
        }
        Ok(())
    }

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("tag", |this, strand, out| {
                let borrow = this.borrow(strand)?;
                Output::set(strand, out, borrow.tag.as_str());
                Ok(())
            })
            .set("tag", |this, strand, value| {
                this.borrow_mut(strand)?.tag = value
                    .as_str(strand)
                    .ok_or_else(|| Error::type_error(strand, "tag: expected str"))?
                    .to_string();
                Ok(())
            })
            .get("attrs", |this, strand, out| {
                Output::set(strand, out, DictView::new(this, Attrs));
                Ok(())
            })
            .get("children", |this, strand, out| {
                Output::set(strand, out, ArrayView::new(this, Children));
                Ok(())
            })
            .method("push", async move |this, strand, args, out| {
                let ([child], []) = unpack!(strand, args, 1, 0)?;
                let borrow = this.borrow(strand)?;
                let arr = Ref::slot::<CHILDREN>(&borrow).as_array(strand).unwrap();
                arr.push(strand, child)?;
                Output::set(strand, out, Nil);
                Ok(())
            })
            .method("traverse", async move |this, strand, args, mut out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let global = strand.state::<Global<'v>>();
                global
                    .traverse_iter_type
                    .create(strand, TraverseIter, &mut out);
                global
                    .traverse_iter_type
                    .cast(&out)
                    .unwrap()
                    .enter_sync(strand, |strand, inst| {
                        {
                            let mut borrow = inst.borrow_mut_unwrap();
                            Output::set(strand, Mut::slot_mut::<STACK>(&mut borrow), Empty::Array);
                        }
                        let borrow = inst.borrow(strand)?;
                        let stack = Ref::slot::<STACK>(&borrow).as_array(strand).unwrap();
                        stack.push(strand, this)?;
                        Ok(())
                    })
            })
    }
}

/// Depth-first, parent-first traversal iterator over a Node tree.
pub(crate) struct TraverseIter;

impl<'v> Object<'v> for TraverseIter {
    const MODULE: &'static str = "xml";
    const NAME: &'static str = "TraverseIter";
    const SLOTS: usize = 1; // STACK: GC array of pending nodes/values
    type Annex = ();
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder.supertype(TypeObject::Iter)
    }

    async fn iter<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        Output::set(strand, out, this);
        Ok(())
    }

    async fn next<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        mut out: Slot<'v, 'a>,
    ) -> Result<'v, 's, bool> {
        let global = strand.state::<Global<'v>>();
        let borrow = this.borrow(strand)?;
        let stack = Ref::slot::<STACK>(&borrow).as_array(strand).unwrap();
        strand.with_slots_sync(|strand, [mut tmp]| {
            // Pop the top of the stack into `out`.
            if !stack.pop(strand, &mut out)? {
                return Ok(false);
            }
            // If it's a Node, push its children in reverse so first child is next.
            if let Some(node_cast) = global.node_type.cast(&out) {
                node_cast.enter_sync(strand, |strand, node_inst| {
                    let node_borrow = node_inst.borrow(strand)?;
                    let children = Ref::slot::<CHILDREN>(&node_borrow)
                        .as_array(strand)
                        .unwrap();
                    let children_len = children.len(strand)?;
                    for i in (0..children_len).rev() {
                        children.get(strand, i, &mut tmp)?;
                        stack.push(strand, &mut tmp)?;
                    }
                    Ok(())
                })?;
            }
            Ok(true)
        })
    }
}
