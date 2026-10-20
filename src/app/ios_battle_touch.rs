//! Battle touch ownership and gesture recognition; actions use existing app
//! command, pointer and camera owners rather than issuing simulation orders.
//!
//! The source WERHD controls use a 32 CSS-pixel joystick travel and 0.12 dead
//! zone (`VirtualJoystickLite.ts::updatePanFromClientPoint`), a 500 ms world
//! hold and 7 pixels on each axis (`WorldInteraction.ts::isClickRange`), and a
//! 28-pixel pair-centroid threshold (`TwoFingerGesture.ts`). Its
//! `PointerEvents.ts` bounds a two-finger tap to 280 ms. These physical-window
//! coordinates scale those screen-pixel tolerances by the display scale.

use std::time::{Duration, Instant};

use winit::event::{DeviceId, Touch, TouchPhase};

use super::ios_shortcuts::Shortcut;

const HOLD_TIME: Duration = Duration::from_millis(500);
const PAIR_TAP_TIME: Duration = Duration::from_millis(280);
const CLICK_SLOP: f32 = 7.0;
const PAIR_PAN_SLOP: f32 = 28.0;
const JOYSTICK_DEAD_ZONE: f32 = 0.12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TouchRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl TouchRect {
    pub fn contains(self, point: [f32; 2]) -> bool {
        point[0] >= self.x
            && point[1] >= self.y
            && point[0] < self.x + self.width
            && point[1] < self.y + self.height
    }

    pub fn center(self) -> [f32; 2] {
        [self.x + self.width / 2.0, self.y + self.height / 2.0]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct JoystickRegion {
    pub center: [f32; 2],
    pub radius: f32,
    /// Physical-window distance from the resting knob to full camera speed.
    pub travel: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ControlRegion {
    pub rect: TouchRect,
    pub command: Shortcut,
}

/// The HUD renderer supplies its own exact hit-test geometry. Controls win
/// over the joystick, then excluded overlay regions, then the tactical world.
/// Remaining points belong to the ordinary UI pointer (sidebar/minimap).
#[derive(Clone, Debug)]
pub(super) struct TouchLayout {
    pub scale_factor: f32,
    pub world: TouchRect,
    pub joystick: Option<JoystickRegion>,
    pub controls: Vec<ControlRegion>,
    pub excluded: Vec<TouchRect>,
}

impl TouchLayout {
    fn scale(&self) -> f32 {
        if self.scale_factor.is_finite() && self.scale_factor > 0.0 {
            self.scale_factor
        } else {
            1.0
        }
    }

    fn owner_at(&self, point: [f32; 2]) -> Owner {
        if let Some(control) = self
            .controls
            .iter()
            .find(|control| control.rect.contains(point))
        {
            return Owner::Control(*control);
        }
        if let Some(region) = self.joystick
            && distance(point, region.center) <= region.radius
        {
            return Owner::Joystick(region);
        }
        if self.excluded.iter().any(|rect| rect.contains(point)) {
            Owner::Ignored
        } else if self.world.contains(point) {
            Owner::World
        } else {
            Owner::UiPointer
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum TouchAction {
    Click {
        position: [f32; 2],
        force: bool,
        attack_move: bool,
    },
    BeginDrag {
        position: [f32; 2],
    },
    Drag {
        position: [f32; 2],
    },
    EndDrag {
        position: [f32; 2],
    },
    /// Drop a held synthetic pointer without committing its release action.
    CancelPointer,
    /// Semantic right-click/cancel, distinct from interrupted pointer cleanup.
    Cancel,
    /// Physical screen displacement in the direction the map was dragged.
    CameraPan {
        delta: [f32; 2],
    },
    Shortcut {
        command: Shortcut,
        held_long: bool,
    },
}

type TouchKey = (DeviceId, u64);

#[derive(Clone, Copy, Debug)]
enum Owner {
    World,
    Joystick(JoystickRegion),
    Control(ControlRegion),
    UiPointer,
    Ignored,
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    key: TouchKey,
    owner: Owner,
    start: [f32; 2],
    position: [f32; 2],
    started_at: Instant,
    /// Largest displacement on each axis; returning to the origin does not
    /// turn an interrupted drag back into a stationary long press.
    max_displacement: [f32; 2],
    mode: Option<Shortcut>,
    scale: f32,
}

impl Contact {
    fn move_to(&mut self, position: [f32; 2]) {
        self.position = position;
        for axis in 0..2 {
            self.max_displacement[axis] =
                self.max_displacement[axis].max((position[axis] - self.start[axis]).abs());
        }
    }

    fn within_click_slop(self) -> bool {
        self.max_displacement
            .iter()
            .all(|delta| *delta <= CLICK_SLOP * self.scale)
    }

    fn held_long(self, now: Instant) -> bool {
        self.within_click_slop() && now.saturating_duration_since(self.started_at) >= HOLD_TIME
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum WorldGesture {
    #[default]
    Idle,
    Single {
        key: TouchKey,
        dragging: bool,
    },
    Pair {
        keys: [TouchKey; 2],
        origin: [f32; 2],
        previous: [f32; 2],
        started_at: Instant,
        scale: f32,
        panning: bool,
    },
    /// A pair has ended/cancelled, or extra fingers joined. Wait until every
    /// world contact lifts; a leftover finger must never become a fresh tap.
    Suppressed,
}

#[derive(Default)]
pub(super) struct BattleTouch {
    contacts: Vec<Contact>,
    world: WorldGesture,
    velocity: [f32; 2],
    knob: [f32; 2],
}

impl BattleTouch {
    pub fn handle(&mut self, touch: Touch, now: Instant, layout: &TouchLayout) -> Vec<TouchAction> {
        let key = (touch.device_id, touch.id);
        let position = [touch.location.x as f32, touch.location.y as f32];
        if touch.phase == TouchPhase::Started {
            if self.contacts.iter().any(|contact| contact.key == key)
                || !position.iter().all(|value| value.is_finite())
            {
                return Vec::new();
            }
            return self.start_contact(key, position, now, layout);
        }
        let Some(index) = self.contacts.iter().position(|contact| contact.key == key) else {
            return Vec::new();
        };
        if position.iter().all(|value| value.is_finite()) {
            self.contacts[index].move_to(position);
        }
        let contact = self.contacts[index];
        let mut actions = Vec::new();
        if touch.phase == TouchPhase::Cancelled {
            match contact.owner {
                Owner::World => self.suppress_world(&mut actions),
                Owner::UiPointer => actions.push(TouchAction::CancelPointer),
                Owner::Control(region) if is_hold_mode(region.command) => {
                    self.suppress_world(&mut actions);
                }
                _ => {}
            }
        } else {
            match contact.owner {
                Owner::World => {
                    self.move_world(&mut actions);
                    if touch.phase == TouchPhase::Ended {
                        self.end_world(contact, now, &mut actions);
                    }
                }
                Owner::Joystick(region) => self.update_joystick(region, contact.position),
                Owner::Control(region) if touch.phase == TouchPhase::Ended => {
                    if !is_hold_mode(region.command) && region.rect.contains(contact.position) {
                        actions.push(TouchAction::Shortcut {
                            command: region.command,
                            held_long: contact.held_long(now),
                        });
                    }
                }
                Owner::UiPointer => actions.push(if touch.phase == TouchPhase::Ended {
                    TouchAction::EndDrag {
                        position: contact.position,
                    }
                } else {
                    TouchAction::Drag {
                        position: contact.position,
                    }
                }),
                _ => {}
            }
        }
        if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.contacts.remove(index);
            if matches!(contact.owner, Owner::Joystick(_)) {
                self.velocity = [0.0; 2];
                self.knob = [0.0; 2];
            }
            if !self
                .contacts
                .iter()
                .any(|contact| matches!(contact.owner, Owner::World))
            {
                self.world = WorldGesture::Idle;
            }
        }
        actions
    }

    /// Normalized screen axes, consumed by the existing camera owner each
    /// frame. Moving the joystick never produces a unit command.
    pub fn camera_velocity(&self) -> [f32; 2] {
        self.velocity
    }

    pub fn knob_offset(&self) -> [f32; 2] {
        self.knob
    }

    pub fn active_mode(&self) -> Option<Shortcut> {
        self.contacts
            .iter()
            .rev()
            .find_map(|contact| match contact.owner {
                Owner::Control(region) if is_hold_mode(region.command) => Some(region.command),
                _ => None,
            })
    }

    pub fn is_pressed(&self, command: Shortcut) -> bool {
        self.contacts.iter().any(|contact| match contact.owner {
            Owner::Control(region) if region.command == command => {
                is_hold_mode(command) || region.rect.contains(contact.position)
            }
            _ => false,
        })
    }

    /// Suppress desktop edge scrolling while a finger owns any screen region.
    pub fn is_interacting(&self) -> bool {
        !self.contacts.is_empty()
    }

    /// Call together with the app's ordinary pointer/modifier cleanup on
    /// focus loss, suspension, screen transitions and layout replacement.
    pub fn reset(&mut self) {
        self.contacts.clear();
        self.world = WorldGesture::Idle;
        self.velocity = [0.0; 2];
        self.knob = [0.0; 2];
    }

    fn start_contact(
        &mut self,
        key: TouchKey,
        position: [f32; 2],
        now: Instant,
        layout: &TouchLayout,
    ) -> Vec<TouchAction> {
        let mut owner = layout.owner_at(position);
        let ui_pointer_active = self
            .contacts
            .iter()
            .any(|contact| matches!(contact.owner, Owner::UiPointer));
        if (ui_pointer_active && matches!(owner, Owner::World | Owner::UiPointer))
            || (matches!(owner, Owner::Joystick(_))
                && self
                    .contacts
                    .iter()
                    .any(|contact| matches!(contact.owner, Owner::Joystick(_))))
        {
            owner = Owner::Ignored;
        }
        let contact = Contact {
            key,
            owner,
            start: position,
            position,
            started_at: now,
            max_displacement: [0.0; 2],
            mode: self.active_mode(),
            scale: layout.scale(),
        };
        let mut actions = Vec::new();
        if matches!(owner, Owner::UiPointer) {
            // Native UI and tactical drags share one mouse capture. Let the UI
            // own that capture until it releases; held custom controls and the
            // joystick remain independently usable.
            self.suppress_world(&mut actions);
            actions.push(TouchAction::BeginDrag { position });
        }
        self.contacts.push(contact);
        match owner {
            Owner::World => match self.world {
                WorldGesture::Idle => {
                    self.world = WorldGesture::Single {
                        key,
                        dragging: false,
                    }
                }
                WorldGesture::Single {
                    key: first,
                    dragging,
                } => {
                    if dragging {
                        actions.push(TouchAction::CancelPointer);
                    }
                    let origin = midpoint(self.contact(first).position, position);
                    self.world = WorldGesture::Pair {
                        keys: [first, key],
                        origin,
                        previous: origin,
                        started_at: now,
                        scale: layout.scale(),
                        panning: false,
                    };
                }
                WorldGesture::Pair { .. } | WorldGesture::Suppressed => {
                    self.suppress_world(&mut actions);
                }
            },
            Owner::Joystick(region) => self.update_joystick(region, position),
            _ => {}
        }
        actions
    }

    fn move_world(&mut self, actions: &mut Vec<TouchAction>) {
        match self.world {
            WorldGesture::Single { key, dragging } => {
                let contact = self.contact(key);
                // Held target modes are taps at the release point. They must
                // not turn into selection boxes while a second finger aims.
                if contact.mode.is_some() {
                    return;
                }
                if dragging {
                    actions.push(TouchAction::Drag {
                        position: contact.position,
                    });
                } else if !contact.within_click_slop() {
                    actions.push(TouchAction::BeginDrag {
                        position: contact.start,
                    });
                    actions.push(TouchAction::Drag {
                        position: contact.position,
                    });
                    self.world = WorldGesture::Single {
                        key,
                        dragging: true,
                    };
                }
            }
            WorldGesture::Pair {
                keys,
                origin,
                previous,
                started_at,
                scale,
                panning,
            } => {
                let current = midpoint(
                    self.contact(keys[0]).position,
                    self.contact(keys[1]).position,
                );
                let panning = panning || distance(current, origin) >= PAIR_PAN_SLOP * scale;
                if panning {
                    let delta = subtract(current, previous);
                    if delta != [0.0; 2] {
                        actions.push(TouchAction::CameraPan { delta });
                    }
                }
                // Keep pre-threshold motion buffered so crossing the slop
                // produces the entire accumulated drag, not one tiny event.
                self.world = WorldGesture::Pair {
                    keys,
                    origin,
                    previous: if panning { current } else { previous },
                    started_at,
                    scale,
                    panning,
                };
            }
            _ => {}
        }
    }

    fn end_world(&mut self, contact: Contact, now: Instant, actions: &mut Vec<TouchAction>) {
        match self.world {
            WorldGesture::Single { dragging: true, .. } => {
                actions.push(TouchAction::EndDrag {
                    position: contact.position,
                });
                self.world = WorldGesture::Idle;
            }
            WorldGesture::Single { .. } => {
                if contact.mode == Some(Shortcut::Cancel) {
                    actions.push(TouchAction::Cancel);
                } else {
                    actions.push(TouchAction::Click {
                        position: contact.position,
                        force: contact.mode == Some(Shortcut::ForceAttack)
                            || (contact.mode.is_none() && contact.held_long(now)),
                        attack_move: contact.mode == Some(Shortcut::AttackMove),
                    });
                }
                self.world = WorldGesture::Idle;
            }
            WorldGesture::Pair {
                keys,
                started_at,
                panning: false,
                ..
            } if now.saturating_duration_since(started_at) <= PAIR_TAP_TIME
                && keys
                    .into_iter()
                    .all(|key| self.contact(key).within_click_slop()) =>
            {
                actions.push(TouchAction::Cancel);
                self.world = WorldGesture::Suppressed;
            }
            WorldGesture::Pair { .. } => self.world = WorldGesture::Suppressed,
            _ => {}
        }
    }

    fn suppress_world(&mut self, actions: &mut Vec<TouchAction>) {
        if matches!(self.world, WorldGesture::Single { dragging: true, .. }) {
            actions.push(TouchAction::CancelPointer);
        }
        if !matches!(self.world, WorldGesture::Idle) {
            self.world = WorldGesture::Suppressed;
        }
    }

    fn contact(&self, key: TouchKey) -> Contact {
        *self
            .contacts
            .iter()
            .find(|contact| contact.key == key)
            .expect("active world gesture retains its contact until end dispatch")
    }

    fn update_joystick(&mut self, region: JoystickRegion, position: [f32; 2]) {
        let displacement = subtract(position, region.center);
        let length = displacement[0].hypot(displacement[1]);
        let travel = region.travel.max(1.0);
        let ratio = if length > travel {
            travel / length
        } else {
            1.0
        };
        self.knob = [displacement[0] * ratio, displacement[1] * ratio];
        self.velocity = [self.knob[0] / travel, self.knob[1] / travel];
        if self.velocity[0].hypot(self.velocity[1]) < JOYSTICK_DEAD_ZONE {
            self.velocity = [0.0; 2];
        }
    }
}

fn is_hold_mode(command: Shortcut) -> bool {
    matches!(
        command,
        Shortcut::AttackMove | Shortcut::ForceAttack | Shortcut::Cancel
    )
}

fn subtract(left: [f32; 2], right: [f32; 2]) -> [f32; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn midpoint(left: [f32; 2], right: [f32; 2]) -> [f32; 2] {
    [(left[0] + right[0]) / 2.0, (left[1] + right[1]) / 2.0]
}

fn distance(left: [f32; 2], right: [f32; 2]) -> f32 {
    (left[0] - right[0]).hypot(left[1] - right[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    fn layout(scale: f32) -> TouchLayout {
        TouchLayout {
            scale_factor: scale,
            world: TouchRect {
                x: 0.0,
                y: 0.0,
                width: 700.0 * scale,
                height: 400.0 * scale,
            },
            joystick: Some(JoystickRegion {
                center: [70.0 * scale, 300.0 * scale],
                radius: 53.0 * scale,
                travel: 32.0 * scale,
            }),
            controls: vec![
                ControlRegion {
                    rect: TouchRect {
                        x: 140.0 * scale,
                        y: 300.0 * scale,
                        width: 50.0 * scale,
                        height: 50.0 * scale,
                    },
                    command: Shortcut::ForceAttack,
                },
                ControlRegion {
                    rect: TouchRect {
                        x: 200.0 * scale,
                        y: 300.0 * scale,
                        width: 50.0 * scale,
                        height: 50.0 * scale,
                    },
                    command: Shortcut::AttackMove,
                },
                ControlRegion {
                    rect: TouchRect {
                        x: 260.0 * scale,
                        y: 300.0 * scale,
                        width: 50.0 * scale,
                        height: 50.0 * scale,
                    },
                    command: Shortcut::Team1,
                },
            ],
            excluded: vec![TouchRect {
                x: 0.0,
                y: 380.0 * scale,
                width: 700.0 * scale,
                height: 20.0 * scale,
            }],
        }
    }

    fn event(id: u64, phase: TouchPhase, point: [f32; 2]) -> Touch {
        Touch {
            device_id: DeviceId::dummy(),
            id,
            phase,
            location: PhysicalPosition::new(point[0] as f64, point[1] as f64),
            force: None,
        }
    }

    fn send(
        touch: &mut BattleTouch,
        id: u64,
        phase: TouchPhase,
        point: [f32; 2],
        now: Instant,
        layout: &TouchLayout,
    ) -> Vec<TouchAction> {
        touch.handle(event(id, phase, point), now, layout)
    }

    #[test]
    fn only_stationary_pairs_cancel_not_short_drags_pinches_or_out_and_back() {
        for scale in [1.0, 2.0] {
            let layout = layout(scale);
            for (left_move, right_move, return_to_origin, cancel) in [
                (3.0, 2.0, false, true),
                (15.0, 15.0, false, false),
                (-20.0, 20.0, false, false),
                (20.0, 20.0, true, false),
            ] {
                let mut touch = BattleTouch::default();
                let now = Instant::now();
                let mut points = [
                    [400.0 * scale, 100.0 * scale],
                    [500.0 * scale, 100.0 * scale],
                ];
                let origins = points;
                for (index, point) in points.iter().enumerate() {
                    assert!(
                        send(
                            &mut touch,
                            index as u64 + 1,
                            TouchPhase::Started,
                            *point,
                            now,
                            &layout
                        )
                        .is_empty()
                    );
                }
                for (index, movement) in [left_move, right_move].into_iter().enumerate() {
                    points[index][0] += movement * scale;
                    assert!(
                        send(
                            &mut touch,
                            index as u64 + 1,
                            TouchPhase::Moved,
                            points[index],
                            now + Duration::from_millis(50),
                            &layout
                        )
                        .is_empty()
                    );
                }
                if return_to_origin {
                    points = origins;
                    for (index, point) in points.iter().enumerate() {
                        assert!(
                            send(
                                &mut touch,
                                index as u64 + 1,
                                TouchPhase::Moved,
                                *point,
                                now + Duration::from_millis(75),
                                &layout
                            )
                            .is_empty()
                        );
                    }
                }
                let actions = send(
                    &mut touch,
                    1,
                    TouchPhase::Ended,
                    points[0],
                    now + Duration::from_millis(100),
                    &layout,
                );
                assert_eq!(
                    actions,
                    if cancel {
                        vec![TouchAction::Cancel]
                    } else {
                        vec![]
                    }
                );
                assert!(
                    send(
                        &mut touch,
                        2,
                        TouchPhase::Ended,
                        points[1],
                        now + Duration::from_millis(110),
                        &layout
                    )
                    .is_empty()
                );
            }
        }
    }

    #[test]
    fn taps_are_deferred_and_stationary_force_hold_starts_at_500_ms() {
        let layout = layout(2.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        let point = [800.0, 200.0];
        for (id, duration, force) in [(1, 499, false), (2, 500, true)] {
            assert!(send(&mut touch, id, TouchPhase::Started, point, now, &layout).is_empty());
            assert_eq!(
                send(
                    &mut touch,
                    id,
                    TouchPhase::Ended,
                    point,
                    now + Duration::from_millis(duration),
                    &layout
                ),
                vec![TouchAction::Click {
                    position: point,
                    force,
                    attack_move: false
                }]
            );
        }
    }

    #[test]
    fn drag_keeps_original_anchor_and_never_becomes_a_hold_after_returning() {
        let layout = layout(2.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        let start = [800.0, 200.0];
        send(&mut touch, 1, TouchPhase::Started, start, now, &layout);
        assert_eq!(
            send(
                &mut touch,
                1,
                TouchPhase::Moved,
                [815.0, 200.0],
                now,
                &layout
            ),
            vec![
                TouchAction::BeginDrag { position: start },
                TouchAction::Drag {
                    position: [815.0, 200.0]
                }
            ]
        );
        assert_eq!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                start,
                now + Duration::from_millis(600),
                &layout
            ),
            vec![
                TouchAction::Drag { position: start },
                TouchAction::EndDrag { position: start }
            ]
        );
    }

    #[test]
    fn pair_tap_cancels_once_and_leftover_finger_cannot_click() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        send(
            &mut touch,
            1,
            TouchPhase::Started,
            [400.0, 100.0],
            now,
            &layout,
        );
        assert!(
            send(
                &mut touch,
                2,
                TouchPhase::Started,
                [500.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Ended,
                [500.0, 100.0],
                now + Duration::from_millis(100),
                &layout
            ),
            vec![TouchAction::Cancel]
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Moved,
                [450.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                [450.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert!(!touch.is_interacting());
    }

    #[test]
    fn pair_timeout_and_extra_fingers_never_fall_back_to_world_clicks() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        for with_third in [false, true] {
            send(
                &mut touch,
                1,
                TouchPhase::Started,
                [400.0, 100.0],
                now,
                &layout,
            );
            send(
                &mut touch,
                2,
                TouchPhase::Started,
                [500.0, 100.0],
                now,
                &layout,
            );
            if with_third {
                assert!(
                    send(
                        &mut touch,
                        3,
                        TouchPhase::Started,
                        [600.0, 100.0],
                        now,
                        &layout
                    )
                    .is_empty()
                );
            }
            let ended_at = now + PAIR_TAP_TIME + Duration::from_millis(1);
            assert!(
                send(
                    &mut touch,
                    1,
                    TouchPhase::Ended,
                    [400.0, 100.0],
                    ended_at,
                    &layout
                )
                .is_empty()
            );
            assert!(
                send(
                    &mut touch,
                    2,
                    TouchPhase::Ended,
                    [500.0, 100.0],
                    ended_at,
                    &layout
                )
                .is_empty()
            );
            if with_third {
                assert!(
                    send(
                        &mut touch,
                        3,
                        TouchPhase::Moved,
                        [620.0, 100.0],
                        ended_at,
                        &layout
                    )
                    .is_empty()
                );
                assert!(
                    send(
                        &mut touch,
                        3,
                        TouchPhase::Ended,
                        [620.0, 100.0],
                        ended_at,
                        &layout
                    )
                    .is_empty()
                );
            }
            assert!(!touch.is_interacting());
        }
    }

    #[test]
    fn interrupted_world_drag_aborts_without_a_release_command() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        send(
            &mut touch,
            1,
            TouchPhase::Started,
            [400.0, 100.0],
            now,
            &layout,
        );
        send(
            &mut touch,
            1,
            TouchPhase::Moved,
            [420.0, 100.0],
            now,
            &layout,
        );
        assert_eq!(
            send(
                &mut touch,
                1,
                TouchPhase::Cancelled,
                [420.0, 100.0],
                now,
                &layout
            ),
            vec![TouchAction::CancelPointer]
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                [420.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        send(
            &mut touch,
            2,
            TouchPhase::Started,
            [400.0, 100.0],
            now,
            &layout,
        );
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Ended,
                [400.0, 100.0],
                now,
                &layout
            ),
            vec![TouchAction::Click {
                position: [400.0, 100.0],
                force: false,
                attack_move: false
            }]
        );
    }

    #[test]
    fn pair_small_moves_accumulate_to_pan_and_never_emit_right_click_or_drag() {
        let layout = layout(2.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        send(
            &mut touch,
            1,
            TouchPhase::Started,
            [800.0, 200.0],
            now,
            &layout,
        );
        send(
            &mut touch,
            2,
            TouchPhase::Started,
            [1000.0, 200.0],
            now,
            &layout,
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Moved,
                [850.0, 200.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert!(
            send(
                &mut touch,
                2,
                TouchPhase::Moved,
                [1050.0, 200.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(
            send(
                &mut touch,
                1,
                TouchPhase::Moved,
                [864.0, 200.0],
                now,
                &layout
            ),
            vec![TouchAction::CameraPan { delta: [57.0, 0.0] }]
        );
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Moved,
                [1064.0, 204.0],
                now,
                &layout
            ),
            vec![TouchAction::CameraPan { delta: [7.0, 2.0] }]
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                [864.0, 200.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert!(
            send(
                &mut touch,
                2,
                TouchPhase::Ended,
                [1064.0, 204.0],
                now,
                &layout
            )
            .is_empty()
        );
    }

    #[test]
    fn second_world_finger_aborts_a_held_selection_before_pairing() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        send(
            &mut touch,
            1,
            TouchPhase::Started,
            [400.0, 100.0],
            now,
            &layout,
        );
        send(
            &mut touch,
            1,
            TouchPhase::Moved,
            [420.0, 100.0],
            now,
            &layout,
        );
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Started,
                [500.0, 100.0],
                now,
                &layout
            ),
            vec![TouchAction::CancelPointer]
        );
        assert!(
            send(
                &mut touch,
                2,
                TouchPhase::Cancelled,
                [500.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                [420.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
    }

    #[test]
    fn held_ui_mode_never_pairs_and_survives_finger_release_order_for_one_tap() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        send(
            &mut touch,
            1,
            TouchPhase::Started,
            [160.0, 320.0],
            now,
            &layout,
        );
        assert_eq!(touch.active_mode(), Some(Shortcut::ForceAttack));
        assert!(touch.is_pressed(Shortcut::ForceAttack));
        send(
            &mut touch,
            2,
            TouchPhase::Started,
            [400.0, 100.0],
            now,
            &layout,
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                [160.0, 320.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(touch.active_mode(), None);
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Ended,
                [400.0, 100.0],
                now,
                &layout
            ),
            vec![TouchAction::Click {
                position: [400.0, 100.0],
                force: true,
                attack_move: false
            }]
        );
    }

    #[test]
    fn joystick_only_moves_camera_and_cancellation_and_reset_clear_holds() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Started,
                [102.0, 300.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(touch.camera_velocity(), [1.0, 0.0]);
        assert_eq!(touch.knob_offset(), [32.0, 0.0]);
        send(
            &mut touch,
            2,
            TouchPhase::Started,
            [160.0, 320.0],
            now,
            &layout,
        );
        send(
            &mut touch,
            3,
            TouchPhase::Started,
            [400.0, 100.0],
            now,
            &layout,
        );
        assert!(
            send(
                &mut touch,
                2,
                TouchPhase::Cancelled,
                [160.0, 320.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(touch.active_mode(), None);
        assert!(
            send(
                &mut touch,
                3,
                TouchPhase::Ended,
                [400.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Cancelled,
                [102.0, 300.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(touch.camera_velocity(), [0.0; 2]);
        send(
            &mut touch,
            4,
            TouchPhase::Started,
            [225.0, 320.0],
            now,
            &layout,
        );
        touch.reset();
        assert_eq!(touch.active_mode(), None);
        assert_eq!(touch.knob_offset(), [0.0; 2]);
        assert!(!touch.is_interacting());
        assert!(
            send(
                &mut touch,
                4,
                TouchPhase::Ended,
                [225.0, 320.0],
                now,
                &layout
            )
            .is_empty()
        );
    }

    #[test]
    fn team_long_press_is_stationary_and_quick_controls_do_not_join_world_pair() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        send(
            &mut touch,
            1,
            TouchPhase::Started,
            [280.0, 320.0],
            now,
            &layout,
        );
        send(
            &mut touch,
            2,
            TouchPhase::Started,
            [400.0, 100.0],
            now,
            &layout,
        );
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Ended,
                [400.0, 100.0],
                now,
                &layout
            ),
            vec![TouchAction::Click {
                position: [400.0, 100.0],
                force: false,
                attack_move: false
            }]
        );
        assert_eq!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                [280.0, 320.0],
                now + HOLD_TIME,
                &layout
            ),
            vec![TouchAction::Shortcut {
                command: Shortcut::Team1,
                held_long: true
            }]
        );
    }

    #[test]
    fn excluded_overlay_and_native_sidebar_keep_their_original_owner() {
        let layout = layout(1.0);
        let mut touch = BattleTouch::default();
        let now = Instant::now();
        send(
            &mut touch,
            1,
            TouchPhase::Started,
            [400.0, 390.0],
            now,
            &layout,
        );
        assert!(
            send(
                &mut touch,
                1,
                TouchPhase::Ended,
                [400.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Started,
                [750.0, 100.0],
                now,
                &layout
            ),
            vec![TouchAction::BeginDrag {
                position: [750.0, 100.0]
            }]
        );
        assert!(
            send(
                &mut touch,
                3,
                TouchPhase::Started,
                [400.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert!(
            send(
                &mut touch,
                3,
                TouchPhase::Ended,
                [400.0, 100.0],
                now,
                &layout
            )
            .is_empty()
        );
        assert_eq!(
            send(
                &mut touch,
                2,
                TouchPhase::Ended,
                [650.0, 100.0],
                now,
                &layout
            ),
            vec![TouchAction::EndDrag {
                position: [650.0, 100.0]
            }]
        );
    }
}
