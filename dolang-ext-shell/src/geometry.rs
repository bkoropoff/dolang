use dolang::runtime::{
    Args, Error, Format, Instance, Object, Output, Result, Slot, Strand, Type,
    object::{TypeBuilder, fmt},
    unpack,
};

/// The dimensions of a terminal-backed console.
///
/// Returned by `term.Console.geometry()`, which answers `nil` for a console
/// that is just a stream. So the presence of a `Geometry` — not the identity of
/// the console — is the "does this have a layout" test.
///
/// Native extension types cannot be abstract, so the fields here throw rather
/// than being absent. A Do class subclassing this declares `pub field rows` and
/// `pub field cols`, which shadow them.
pub(crate) struct Geometry;

impl<'v> Object<'v> for Geometry {
    const NAME: &'v str = "Geometry";
    const MODULE: &'v str = "term";
    type Annex = ();
    type Type = ();
    type TypeAnnex = ();

    /// Constructible so that Do classes can subclass it: a native supertype has
    /// to be initializable for `Geometry.(init) $self` to fill its slot.
    async fn new<'a, 's>(
        this: Type<'v, Self>,
        strand: &'a mut Strand<'v, 's>,
        args: Args<'v, 'a>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let ([], []) = unpack!(strand, args, 0, 0)?;
        this.create(strand, Geometry, out);
        Ok(())
    }

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("rows", |_this, strand, _out| {
                Err(Error::not_supported(strand))
            })
            .get("cols", |_this, strand, _out| {
                Err(Error::not_supported(strand))
            })
    }
}

/// The dimensions of the real terminal, as `shell.Console.geometry()` reports
/// them.
pub(crate) struct HostGeometry;

/// `rows`/`cols` are independently optional: `DOLANG_CONSOLE` may pin one
/// without the other (e.g. `cols=120` alone), in which case the unpinned
/// dimension falls back to a live ioctl query that may itself come up empty.
pub(crate) struct HostGeometryAnnex {
    pub(crate) rows: Option<u32>,
    pub(crate) cols: Option<u32>,
}

impl<'v> Object<'v> for HostGeometry {
    const NAME: &'v str = "Geometry";
    const MODULE: &'v str = "shell";
    type Annex = HostGeometryAnnex;
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("rows", |this, strand, out| {
                if let Some(rows) = this.annex().rows {
                    Output::set(strand, out, rows);
                }
                Ok(())
            })
            .get("cols", |this, strand, out| {
                if let Some(cols) = this.annex().cols {
                    Output::set(strand, out, cols);
                }
                Ok(())
            })
    }

    fn debug<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        w: &mut dyn Format<'v>,
    ) -> Result<'v, 's, ()> {
        let HostGeometryAnnex { rows, cols } = *this.annex();
        match (rows, cols) {
            (Some(rows), Some(cols)) => fmt!(strand, w, "<geometry {cols}x{rows}>"),
            (rows, cols) => fmt!(strand, w, "<geometry {cols:?}x{rows:?}>"),
        }
    }
}
