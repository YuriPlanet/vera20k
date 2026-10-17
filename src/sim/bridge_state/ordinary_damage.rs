//! Ordinary wooden57BAA0/57BCF0/57C2B0 and concrete57CCF0/57CF60/57D530
//! bridge damage, with sibling57DD50/57E2A0 and57E7A0/57ED00 propagation.
//! Cell writes, Recalc and live occupants are synchronous. Neither first
//! damage nor final collapse calls structural BlowUpBridge47DD70.
//! Native executable corpora: tools/spatial_oracle/bridge_ordinary_damage
//! and tools/spatial_oracle/shrapnel_damage/scalar.
use super::Axis;
use super::ordinary::{OrdinaryBridgeHost, centered, member, offset};
use super::publication::CellCoord;
use super::ramp_repair::Family;

pub(crate) trait OrdinaryDamageHost: OrdinaryBridgeHost {
    fn notify_span(&mut self, first: CellCoord, end: CellCoord) -> Result<(), Self::Error>;
}

pub(crate) fn damage<H: OrdinaryDamageHost>(
    host: &mut H,
    input: CellCoord,
    family: Family,
) -> Result<bool, H::Error> {
    let Some((point, ns)) = centered(host, input, family) else {
        return Ok(false);
    };
    root(host, point, ns, family)
}

fn group<H: OrdinaryBridgeHost>(host: &mut H, point: CellCoord, ns: bool) -> [H::Cell; 3] {
    let across = if ns { (0, 1) } else { (1, 0) };
    [
        host.lookup(point),
        host.lookup(offset(point, -across.0, -across.1)),
        host.lookup(offset(point, across.0, across.1)),
    ]
}

fn recalc_and_occupants<H: OrdinaryBridgeHost>(
    host: &mut H,
    cells: [H::Cell; 3],
    mode: u8,
) -> Result<(), H::Error> {
    for cell in cells {
        host.recalc(cell)?;
    }
    for cell in cells {
        host.occupants(cell, mode)?;
    }
    Ok(())
}

fn root<H: OrdinaryDamageHost>(
    host: &mut H,
    point: CellCoord,
    ns: bool,
    family: Family,
) -> Result<bool, H::Error> {
    let [center, negative, positive] = group(host, point, ns);
    let prior = host.overlay(center);
    let base = match family {
        Family::Low => 74,
        Family::High => 205,
    };
    let threshold = base + if ns { 6 } else { 15 };
    let end = base + if ns { 18 } else { 22 };
    let destroyed = base + if ns { 26 } else { 27 };
    let (next, before, after, collapsed) = if prior == end {
        (end + 1, ns, !ns, false)
    } else if prior == end + 2 {
        (end + 3, !ns, ns, false)
    } else if prior < threshold {
        (threshold, true, true, false)
    } else if prior < threshold + 3 {
        (destroyed, true, true, true)
    } else {
        return Ok(false);
    };
    // Both families' roots store both sides before center. The retained
    // identities survive propagation callbacks and their recursive damage.
    for cell in [negative, positive, center] {
        host.write_overlay(cell, next as u8);
    }
    if collapsed {
        for cell in [negative, positive, center] {
            host.radar(host.coord(cell));
        }
    }
    let along = if ns { (1, 0) } else { (0, 1) };
    if before {
        propagate(host, offset(point, -along.0, -along.1), ns, family)?;
    }
    if after {
        propagate(host, offset(point, along.0, along.1), ns, family)?;
    }
    if collapsed {
        //57C990/57C870 and57DC20/57DAF0 scan after sibling callbacks. Membership is
        // live; 575EE0 performs every cell lookup and raises event31 on Tags.
        let first = endpoint(host, point, -along.0, -along.1, family);
        let last = endpoint(host, point, along.0, along.1, family);
        host.notify_span(first, last)?;
    }
    host.redraw(center);
    // Wooden57C24D/57C80D and concrete NS use the final-collapse byte.
    // Concrete EW always passes1, including first damage. Both native corpora
    // execute these distinct caller modes; family names do not imply height.
    recalc_and_occupants(
        host,
        [center, negative, positive],
        u8::from((family == Family::High && !ns) || collapsed),
    )?;
    if collapsed {
        host.connectivity()?;
        host.rebuild_rectangle([i32::from(point.0) - 1, i32::from(point.1) - 1, 3, 3])?;
    }
    Ok(collapsed)
}

fn endpoint<H: OrdinaryBridgeHost>(
    host: &mut H,
    start: CellCoord,
    dx: i16,
    dy: i16,
    family: Family,
) -> CellCoord {
    let mut point = start;
    loop {
        let next = offset(point, dx, dy);
        let cell = host.lookup(next);
        if !member(host.overlay(cell), family) {
            return point;
        }
        point = next;
    }
}

fn propagate<H: OrdinaryBridgeHost>(
    host: &mut H,
    point: CellCoord,
    ns: bool,
    family: Family,
) -> Result<(), H::Error> {
    let selected = host.lookup(point);
    if !member(host.overlay(selected), family) {
        return Ok(());
    }
    let [center, negative, positive] = group(host, point, ns);
    //57B870/57CAB0 read east then west;57B990/57CBE0 north then south. Keep lookup
    // order even for a shared dummy. The center is reread after both probes.
    let step = if ns { (1, 0) } else { (0, -1) };
    let first = host.lookup(offset(point, step.0, step.1));
    let first = host.overlay(first);
    let second = host.lookup(offset(point, -step.0, -step.1));
    let second = host.overlay(second);
    let family_base = match family {
        Family::Low => 74,
        Family::High => 205,
    };
    let base = family_base + if ns { 0 } else { 9 };
    let end = family_base + if ns { 18 } else { 22 };
    let destroyed = family_base + if ns { 26 } else { 27 };
    let mut index = 0;
    if [base + 4, base + 6, base + 8, end + 1].contains(&first) {
        index |= 1;
    } else if [base + 7, destroyed].contains(&first) {
        index |= 2;
    }
    if [base + 5, base + 6, base + 7, end + 3].contains(&second) {
        index |= 4;
    } else if [base + 8, destroyed].contains(&second) {
        index |= 8;
    }
    if index == 0 {
        return Ok(());
    }
    let prior = host.overlay(center);
    let next = if prior < end {
        let Some(next) = crate::sim::bridge_specs::pick_destruction_overlay(
            index,
            if ns { Axis::NS } else { Axis::EW },
            family == Family::High,
        ) else {
            return Ok(());
        };
        if i32::from(next) == prior {
            return Ok(());
        }
        next
    } else if prior == end || prior == end + 2 {
        (prior + 1) as u8
    } else {
        return Ok(());
    };
    for cell in [center, negative, positive] {
        host.write_overlay(cell, next);
    }
    host.redraw(center);
    let collapsed = i32::from(next) == destroyed;
    if collapsed {
        let across = if ns { (0, 1) } else { (1, 0) };
        for coord in [
            point,
            offset(point, across.0, across.1),
            offset(point, -across.0, -across.1),
        ] {
            host.radar(coord);
        }
    }
    recalc_and_occupants(host, [center, negative, positive], u8::from(collapsed))
}

#[cfg(test)]
#[path = "ordinary_damage_tests.rs"]
mod tests;
