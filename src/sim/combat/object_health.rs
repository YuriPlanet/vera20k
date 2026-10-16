//! The object's health (`ObjectClass+0x6C`) and Object5F5390's signed HP
//! commit. Damage preparation never predicts this result.
//! Original-byte comparisons: tools/spatial_oracle/object_health (718 rows).
use super::{GameEntity, damage::DamageState};
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};

/// Signed actual ObjectClass health (`ObjectClass+0x6C`, Strength), owned by
/// this module: constructors start it with [`Health::new`], ReceiveDamage's
/// commit below writes it, and every other writer goes through
/// [`Health::set`] or [`Health::heal`]. The live ObjectType's `strength` is
/// the cap and ratio denominator; EstimatedHealth (`+0x70`) is independent
/// state these writes never update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Health {
    current: i32,
}

impl Health {
    /// A health of `current`: a new object's, or a value a caller only reads
    /// a ratio from.
    pub const fn new(current: i32) -> Self {
        Self { current }
    }

    /// The signed actual health.
    pub const fn current(self) -> i32 {
        self.current
    }

    /// Store `value`. The native writers outside ReceiveDamage: a crash's
    /// kill (`0x004DEC72`) and a parasite whose host dies under it
    /// (`ParasiteClass::ExitUnit`, `0x0062A854`, `0x0062A8A0`) store 0;
    /// PostMortem, ship sinking and the infantry health reset store 1; a
    /// completed repair (an engineer's, the repair step's or the depot's), a
    /// spawn reload and a slave heal store the type's Strength; map
    /// admission, conversion and crew survival store their computed value.
    ///
    /// RESIDUAL: VERA also stores 0 before UnInit where native leaves the
    /// health alone: a crushed object (`Crush_Or_Prescatter_Cell
    /// 0x007416A0`), a transport's dying passengers (`KillPassengers
    /// 0x00707CB0`, Unit ReceiveDamage's refused escape
    /// `0x00738188..0x0073819B`) and a garrison's occupants (`KillOccupants
    /// 0x004585C0`, `SellBuilding 0x00457DE0`). Trigger: each of those
    /// deaths. Effect: a reader of the dead object's health between that
    /// store and its removal sees 0 where native sees its last value.
    /// Frequency: every crush and every passenger or occupant death. Risk: a
    /// same-frame reader, such as `is_alive`, disagrees with native.
    pub(crate) fn set(&mut self, value: i32) {
        self.current = value;
    }

    /// Install a fixture's health.
    #[cfg(test)]
    pub(crate) fn set_for_test(&mut self, value: i32) {
        self.current = value;
    }

    /// `+0x6C += amount` with wrapping. The repair step and the depot's radio
    /// heal cap at the type's Strength themselves; the self-heal pulses add
    /// without a cap (an `INC`, `0x006FA751..0x006FA757`, behind the
    /// `!= Strength` test at `0x0070BF30`).
    pub(crate) fn heal(&mut self, amount: i32) {
        self.current = self.current.wrapping_add(amount);
    }

    /// ObjectClass::GetHealthRatio (gamemd.exe 0x005F5C60..0x005F5C7F).
    /// Signed loads and PC53/chop division; zero Strength keeps the native
    /// masked infinity/NaN result for the caller's actual comparison/conversion.
    pub fn ratio(self, strength: i32) -> crate::util::native_x87::MaskedX87Value {
        X87::div(X87::load_i32(self.current), X87::load_i32(strength))
    }

    /// [`Self::ratio`] compared `>=` `Rules+0x16F8` with an ordered FCOMP
    /// (the C0-clear test): 0/0 (NaN) is not full and x/0 for x > 0 (+inf)
    /// is. AudioVisual `0x0066B323..0x0066B32D` writes that threshold as 1.0
    /// unconditionally; there is no ConditionGreen key. Object radio
    /// (`0x005F5339`), Techno repair (`0x006F4DE5`), the healer fire errors and
    /// the hospital and depot tests ask it. The quotient of two dwords cannot
    /// round across 1.0, so the integer form is exact.
    pub(crate) fn is_full(self, strength: i32) -> bool {
        match strength.cmp(&0) {
            std::cmp::Ordering::Greater => self.current >= strength,
            std::cmp::Ordering::Less => self.current <= strength,
            std::cmp::Ordering::Equal => self.current > 0,
        }
    }

    /// Compare the native ratio without converting masked values to a host float.
    pub fn compare_ratio(self, strength: i32, threshold: f64) -> MaskedX87Ordering {
        X87::compare(
            self.ratio(strength),
            X87::load_f64(NativeF64Bits::from_bits(threshold.to_bits())),
        )
    }
}

/// 5F5DD0 uses inclusive red/yellow thresholds and live type Strength.
pub fn health_category(current: i32, strength: i32, red: f64, yellow: f64) -> u8 {
    use crate::util::native_x87::MaskedX87Ordering::Greater;
    let health = crate::sim::combat::Health::new(current);
    // Object5F5DD0: all three TEST AH,41 comparisons include unordered;
    // only the first red branch also requires signed actual > 0.
    // Original whole-function outputs: health_ratio_predicates.json.
    let above_red = health.compare_ratio(strength, red) == Greater;
    if current > 0 && !above_red {
        0
    } else if above_red && health.compare_ratio(strength, yellow) != Greater {
        1
    } else {
        2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HealthCallback {
    Changed,
    Tag { kind: i32, with_source: bool },
    Kill,
    Destroy,
}

/// Short object borrows let the same Object5F5390 body invoke synchronous world
/// callbacks. The host owns access, not a second health or event algorithm.
pub(super) trait ObjectHealthHost {
    fn object(&self) -> Option<&GameEntity>;
    fn object_mut(&mut self) -> Option<&mut GameEntity>;
    fn current_strength(&self) -> Option<i32>;
    fn invoke(&mut self, callback: HealthCallback);
}

/// Object5F5390, Steam3e81a617: the threshold/first-damage Tags precede Kill
/// and Destroy; every synchronous return can change HP, Type, Alive or Tag.
/// Executed bodies and callback mutations: tag_lifecycle --damage-events.
/// No own RNG, timers or detach; the invoked owners retain those effects.
pub(super) fn commit(
    host: &mut impl ObjectHealthHost,
    packet: &mut i32,
    strength: i32,
    admitted: bool,
    building_no_c4: bool,
    condition_red: f64,
    source_present: bool,
) -> DamageState {
    let Some(previous) = host.object().map(|target| target.health.current) else {
        return DamageState::Unaffected;
    };
    if previous <= 0 || !admitted {
        return DamageState::Unaffected;
    }
    if building_no_c4 {
        *packet = (*packet).max(1);
    }
    if *packet == 0 {
        return DamageState::Unaffected;
    }
    if *packet < 0 {
        // 5F546A..5495: the intermediate write precedes the signed cap;
        // changed callback receives the capped value, and cannot change return0.
        let changed = {
            let target = host.object_mut().expect("represented receive target");
            target.health.current = previous.wrapping_sub(*packet);
            if target.health.current > strength {
                target.health.current = strength;
            }
            target.health.current != previous
        };
        if changed {
            host.invoke(HealthCallback::Changed);
        }
        return DamageState::Unaffected;
    }
    *packet = (*packet).min(previous);
    let post = previous.wrapping_sub(*packet);
    let mut result = DamageState::Damaged;
    let yellow = strength >> 1;
    if yellow <= previous && post < yellow {
        result = DamageState::Yellow;
    }
    let red = X87::mul(
        X87::load_i32(strength),
        X87::load_f64(NativeF64Bits::from_bits(condition_red.to_bits())),
    );
    if X87::compare(red, X87::load_i32(previous)) == MaskedX87Ordering::Less
        && X87::compare(X87::load_i32(post), red) == MaskedX87Ordering::Less
    {
        result = DamageState::Red;
    }
    host.object_mut()
        .expect("represented receive target")
        .health
        .current = post;
    //5F55F5..5643: Yellow's combat visit comes before its first Alive gate.
    if result == DamageState::Yellow {
        if source_present {
            host.invoke(HealthCallback::Tag {
                kind: 39,
                with_source: false,
            });
        }
        if !host
            .object()
            .is_some_and(|target| target.lifecycle.object_alive())
        {
            return DamageState::AlreadyDead;
        }
        host.invoke(HealthCallback::Tag {
            kind: 42,
            with_source: false,
        });
    }
    if !host
        .object()
        .is_some_and(|target| target.lifecycle.object_alive())
    {
        return DamageState::AlreadyDead;
    }
    if result == DamageState::Red {
        if source_present {
            host.invoke(HealthCallback::Tag {
                kind: 40,
                with_source: false,
            });
        }
        if !host
            .object()
            .is_some_and(|target| target.lifecycle.object_alive())
        {
            return DamageState::AlreadyDead;
        }
        host.invoke(HealthCallback::Tag {
            kind: 43,
            with_source: false,
        });
    }
    if !host
        .object()
        .is_some_and(|target| target.lifecycle.object_alive())
    {
        return DamageState::AlreadyDead;
    }
    //5F56AD..573F reads live HP/Type once before the first-damage group.
    //41(NULL) and41(attacker) are distinct original calls, not duplicates
    //to collapse. A source pointer, rather than hostility, gates combat kinds.
    if host
        .object()
        .is_some_and(|target| target.health.current != previous)
        && host.current_strength() == Some(previous)
    {
        if source_present {
            host.invoke(HealthCallback::Tag {
                kind: 38,
                with_source: false,
            });
        }
        if !host
            .object()
            .is_some_and(|target| target.lifecycle.object_alive())
        {
            return DamageState::AlreadyDead;
        }
        host.invoke(HealthCallback::Tag {
            kind: 41,
            with_source: false,
        });
        if !host
            .object()
            .is_some_and(|target| target.lifecycle.object_alive())
        {
            return DamageState::AlreadyDead;
        }
        if source_present {
            host.invoke(HealthCallback::Tag {
                kind: 41,
                with_source: true,
            });
        }
    }
    if !host
        .object()
        .is_some_and(|target| target.lifecycle.object_alive())
    {
        return DamageState::AlreadyDead;
    }
    //5F5752 retains the pre-Tag HP snapshot, then575A re-reads live HP.
    //A positive snapshot killed by a Tag returns5 without another Kill.
    if post > 0
        && host
            .object()
            .is_none_or(|target| target.health.current <= 0)
    {
        return DamageState::AlreadyDead;
    }
    if host
        .object()
        .is_some_and(|target| target.health.current == 0)
    {
        host.invoke(HealthCallback::Kill);
        result = DamageState::Dead;
        //5F579F..57AF always invokes Destroy after Kill, even if that
        //callback changed HP or Alive. Later Alive gates return local result.
        host.invoke(HealthCallback::Destroy);
    }
    if !host
        .object()
        .is_some_and(|target| target.lifecycle.object_alive())
    {
        return result;
    }
    if result != DamageState::Dead && source_present {
        host.invoke(HealthCallback::Tag {
            kind: 6,
            with_source: true,
        });
        if !host
            .object()
            .is_some_and(|target| target.lifecycle.object_alive())
        {
            return result;
        }
        host.invoke(HealthCallback::Tag {
            kind: 44,
            with_source: true,
        });
    }
    result
}

#[cfg(test)]
#[path = "object_health_tests.rs"]
mod tests;
