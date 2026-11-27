//! Who made a node, template or draw list, and what of a limit is theirs.

/// Who made a node, template or draw list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    Host,
    Layout,
    Player,
    External(u16),
}

/// What others than the host share of a limit: a quarter, rounded up, is
/// the host's alone.
pub(crate) fn others_share(limit: usize) -> usize {
    limit - limit.div_ceil(4)
}

impl Origin {
    /// What of `limit` this origin may hold: all of it for the host,
    /// [`others_share`] (three quarters) for any other.
    pub fn share(self, limit: usize) -> usize {
        if self == Origin::Host {
            limit
        } else {
            others_share(limit)
        }
    }
}
