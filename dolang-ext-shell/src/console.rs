use dolang::runtime::{
    Error, Instance, Object, Output, Result, Slot, Strand, Value, object::TypeBuilder, unpack,
    value::TypeObject,
};
use dolang_shell_vfs::OperatingSystem;
use tokio::io::AsyncWriteExt;

use crate::{
    error::ErrorExt as _,
    global::Global,
    io_mode::{ValueEncoding, encode_value, write_raw},
};

/// The console: where human-readable output goes.
///
/// It may be a full terminal, or it may be a plain byte sink; it may support
/// styling even when it is not a terminal. `term.echo`/`term.print` render to
/// it, and it follows extension terminal takeover (`with_terminal`), so writing
/// here during a progress display goes through the display's writer rather than
/// fighting it.
///
/// Distinct from `shell.stderr`, which is the process's error stream and
/// ignores takeover.
pub(crate) struct Console;

impl Default for Console {
    fn default() -> Self {
        Self
    }
}

impl<'v> Object<'v> for Console {
    const NAME: &'v str = "Console";
    const MODULE: &'v str = "term";
    type Annex = ();
    type Type = ();
    type TypeAnnex = ();

    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .supertype(TypeObject::Sink)
            .method("write", async move |_this, strand, args, out| {
                let ([data], []) = unpack!(strand, args, 1, 0)?;
                let global = strand.state::<Global<'v>>();
                let mut writer = global.terminal.writer.lock().await;
                write_raw(&mut *writer, data, strand, out).await
            })
            .method("flush", async move |_this, strand, args, _out| {
                let ([], []) = unpack!(strand, args, 0, 0)?;
                let global = strand.state::<Global<'v>>();
                global
                    .terminal
                    .writer
                    .lock()
                    .await
                    .flush()
                    .await
                    .map_err(|error| error.into_sys(strand))
            })
    }

    /// There is exactly one `Console` per VM, so having the type is having the
    /// object.
    fn eq<'a, 's>(
        _this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        other: &Value<'v>,
    ) -> Result<'v, 's, bool> {
        let global = strand.state::<Global<'v>>();
        Ok(global.types.console.cast(other).is_some())
    }

    async fn sink<'a, 's>(
        this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        out: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        Output::set(strand, out, this);
        Ok(())
    }

    async fn put<'a, 's>(
        _this: Instance<'v, 'a, Self>,
        strand: &'a mut Strand<'v, 's>,
        value: Slot<'v, 'a>,
    ) -> Result<'v, 's, ()> {
        let global = strand.state::<Global<'v>>();
        let mode = global.local.get(strand).io_mode();
        let bytes = encode_value(
            strand,
            &value,
            mode,
            ValueEncoding::Display,
            OperatingSystem::current(),
        )?;
        let mut writer = global.terminal.writer.lock().await;
        writer
            .write_all(&bytes)
            .await
            .map_err(|error| error.into_sys(strand))
    }
}

/// Writes bytes to the console, serialized with `echo`/`print` and diagnostics.
///
/// Used by the child-stdio byte pump, which copies a child's output straight
/// through without any value framing.
pub(crate) async fn write_bytes<'v, 's>(
    strand: &mut Strand<'v, 's>,
    bytes: &[u8],
) -> Result<'v, 's, ()> {
    let global = strand.state::<Global<'v>>();
    let mut writer = global.terminal.writer.lock().await;
    writer
        .write_all(bytes)
        .await
        .map_err(|error| Error::runtime(strand, error))
}
