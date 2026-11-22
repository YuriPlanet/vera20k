//! The ordered paint list the host draws.

use crate::Rect;
use crate::node::{NodeId, Rgba};

use super::Hud;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    Normal,
    Hovered,
    Pressed,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintOp<'a> {
    Fill { rect: Rect, color: Rgba },
    Image { rect: Rect, art: &'a str, state: ButtonState },
    /// `key` is a string-table key or literal text; the host resolves it.
    Text { rect: Rect, key: &'a str, color: Option<Rgba>, align: i32, enabled: bool },
    External { rect: Rect, key: &'a str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintCmd<'a> {
    pub node: NodeId,
    /// None is the whole viewport.
    pub clip: Option<Rect>,
    pub op: PaintOp<'a>,
}

impl Hud {
    /// Appends the last layout's commands to `out`, bottom to top. Drawing
    /// them in this order is correct; see [`batches`].
    pub fn paint<'a>(&'a self, out: &mut Vec<PaintCmd<'a>>) {
        todo!("paint into {}", out.len())
    }
}

/// Runs of neighbouring commands one draw call can take: the same clip and
/// material (`material` names the host's texture or pipeline). Order is never
/// changed.
pub fn batches<'c, 'a, M: PartialEq>(
    cmds: &'c [PaintCmd<'a>],
    material: impl Fn(&PaintOp<'a>) -> M + 'c,
) -> impl Iterator<Item = std::ops::Range<usize>> + 'c
where
    'a: 'c,
    M: 'c,
{
    let mut start = 0;
    std::iter::from_fn(move || {
        let first = cmds.get(start)?;
        let kind = material(&first.op);
        let len = cmds[start..].iter().take_while(|c| c.clip == first.clip && material(&c.op) == kind).count();
        let run = start..start + len.max(1);
        start = run.end;
        Some(run)
    })
}
