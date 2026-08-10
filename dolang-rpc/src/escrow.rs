use std::{collections::HashMap, os::fd::OwnedFd};

enum Entry {
    Sending { fds: Vec<OwnedFd>, done: bool },
    Released,
}

#[derive(Default)]
pub(crate) struct FdEscrow {
    entries: HashMap<u64, Entry>,
}

impl FdEscrow {
    pub(crate) fn register(&mut self, id: u64) {
        assert!(
            self.entries
                .insert(
                    id,
                    Entry::Sending {
                        fds: Vec::new(),
                        done: false,
                    },
                )
                .is_none(),
            "file descriptor escrow id is already registered"
        );
    }

    pub(crate) fn sent(&mut self, id: u64, mut fds: Vec<OwnedFd>, done: bool) {
        match self
            .entries
            .get_mut(&id)
            .expect("file descriptor escrow id was not registered")
        {
            Entry::Sending {
                fds: escrow,
                done: sending_done,
            } => {
                escrow.append(&mut fds);
                *sending_done = done;
            }
            Entry::Released if done => {
                self.entries.remove(&id);
            }
            Entry::Released => {}
        }
    }

    pub(crate) fn discard_unsent(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    /// Releases `id`, returning false when it is unknown or already released.
    pub(crate) fn release(&mut self, id: u64) -> bool {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        match entry {
            Entry::Sending { done: true, .. } => {
                self.entries.remove(&id);
            }
            Entry::Sending { .. } => *entry = Entry::Released,
            Entry::Released => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_before_send_completion_is_remembered_only_until_completion() {
        let mut escrow = FdEscrow::default();
        escrow.register(1);
        assert!(escrow.release(1));
        assert!(!escrow.release(1));
        escrow.sent(1, Vec::new(), true);
        assert!(!escrow.release(1));
    }

    #[test]
    fn completed_entry_is_removed_by_exactly_one_release() {
        let mut escrow = FdEscrow::default();
        escrow.register(1);
        escrow.sent(1, Vec::new(), true);
        assert!(escrow.release(1));
        assert!(!escrow.release(1));
    }
}
