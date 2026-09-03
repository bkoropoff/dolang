//! Shared plumbing for the per-domain public modules.

use std::collections::VecDeque;

use dolang_vfs::{
    Vfs,
    error::{Error, ErrorKind},
    extension::VfsExtension,
};

use crate::wire::{WinNetExt, WinNetRequest, WinNetResponse};

fn unsupported() -> Error {
    Error::new(
        ErrorKind::Unsupported,
        "Windows NetAPI management is not supported by this VFS backend",
    )
}

pub(crate) fn unexpected(request: &str) -> Error {
    Error::new(
        ErrorKind::InvalidData,
        format!("unexpected response for {request}"),
    )
}

pub(crate) async fn call(vfs: &Vfs, request: WinNetRequest) -> Result<WinNetResponse, Error> {
    if vfs
        .extensions()
        .maximum_common_version(WinNetExt::NAME, &[WinNetExt::VERSION])
        .is_none()
    {
        return Err(unsupported());
    }
    vfs.call_extension::<WinNetExt>(request).await?
}

/// The buffered state behind a paged forward enumeration.
pub(crate) struct Paged<T> {
    vfs: Vfs,
    resume: u64,
    entries: VecDeque<T>,
    done: bool,
}

impl<T> Paged<T> {
    pub(crate) fn new(vfs: &Vfs) -> Self {
        Self {
            vfs: vfs.clone(),
            resume: 0,
            entries: VecDeque::new(),
            done: false,
        }
    }

    /// Yields the next entry, fetching further pages as needed.
    ///
    /// `request` builds the page request for a resume handle and `page`
    /// destructures the matching response. `label` names the enumeration in
    /// error messages.
    pub(crate) async fn next_entry(
        &mut self,
        request: impl Fn(u64) -> WinNetRequest,
        page: impl Fn(WinNetResponse) -> Option<(Vec<T>, u64, bool)>,
        label: &str,
    ) -> Result<Option<T>, Error> {
        loop {
            if let Some(entry) = self.entries.pop_front() {
                return Ok(Some(entry));
            }
            if self.done {
                return Ok(None);
            }
            let previous = self.resume;
            let response = call(&self.vfs, request(self.resume)).await?;
            let (entries, resume, done) = page(response).ok_or_else(|| unexpected(label))?;
            if entries.is_empty() && !done && resume == previous {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("empty {label} page did not advance"),
                ));
            }
            self.resume = resume;
            self.done = done;
            self.entries.extend(entries);
        }
    }
}
