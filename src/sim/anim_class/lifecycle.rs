//! An anim's life after construction: destruction and scalar deletion, owner
//! attachment and detach, display layer and coordinates, pointer expiry, and
//! the substrate-owned mutations of an anim's retained fields.

use super::object::{AnimViewerHidden, anim_owner_world_coords};
use super::{AnimId, AnimObject, AnimWorldCoord, anim_world_coords, is_local_anim_id};
use crate::rules::art_data::AnimTypeRuntimeConfig;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{SimSoundEvent, Simulation};

impl Simulation {
    pub(crate) fn destroy_anim(&mut self, id: AnimId, rules: &RuleSet) {
        self.destroy_anim_with_context(id, rules);
    }

    pub(crate) fn destroy_anim_with_context(&mut self, id: AnimId, rules: &RuleSet) {
        if self.substrate.pending_delete.contains_object(&id)
            || self.substrate.anims.local_delete_queued(id)
        {
            return;
        }
        // `AnimClass::GetCoords @ 0x00422BE0` — the sound plays at the anim's
        // resolved world position, not its owner-relative stored delta.
        let Some(world) = self.anim_absolute_coord(id) else {
            return;
        };
        let Some(stop_sound) = self
            .anim(id)
            .map(|anim| (!anim.no_effects()).then_some(anim.stop_sound_id).flatten())
        else {
            return;
        };
        // UnInit4255C3 calls owner+60 before SetOwner(NULL)4255CA. Keep
        // the shared attachment flag and every matching owner link in that
        // operation, including the ordinary non-Building path.
        self.detach_anim_from_owner(id, rules);
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.start_sound_active = false;
        }
        // Original4255B0..425628 never writes19B; ObjectUnInit5F6625 clears
        // ObjectAlive after expiry/Limbo, then queues this physical object.
        // Executed MGUN/PIFF retirement receipts retain inactive0 while
        // removing Logic/Display and adding deferred deletion:
        // tools/spatial_oracle/anytown_damage/foot_missions.json,
        // ground_emission_receipt.
        // UnInit4255D5 calls Release406060, leaving a one-shot Report
        // playing; StopAndClear405D40 would cut it off. Original execution:
        // tools/rules_oracle/bridge_child_sound.{py,json,md}.
        self.sound_events
            .emit(SimSoundEvent::ObjectSoundReleased { owner: id });
        if let Some(stop_sound_id) = stop_sound {
            // Native425618 plays StopSound after releasing the Report. The
            // preceding release removes this owner's handle, so the existing
            // stop/play consumer cannot interrupt the released Report.
            self.sound_events.emit(SimSoundEvent::AnimationStopped {
                anim_id: id,
                stop_sound_id: Some(stop_sound_id),
                world,
            });
        }
        // Object::UnInit5F6616 broadcasts the Anim's expiry before Limbo;
        // Building44EA1A..44EA4F clears its matching damage-fire slot or
        // runs the building-slot transition before this Anim leaves Logic.
        self.notify_anim_pointer_expired(id);
        self.substrate.clear_damage_fire_anim_reference(id);
        self.building_anim_pointer_expired(id, rules);
        // Limbo5F4D30→DetachAll5F4D61 announces again. The first callback
        // normally removed the reverse index; preserve the second visit for
        // native dead-building gates that retained the matched slot. Both
        // notifications precede Display5F4D79 and Logic5F4DD3 removal.
        self.notify_anim_pointer_expired(id);
        self.building_anim_pointer_expired(id, rules);
        self.conceal_anim(id);
        // VERA-only: a local anim waits for the drain outside the shared,
        // hashed queue gamemd puts it in.
        if is_local_anim_id(id) {
            self.substrate.anims.queue_local_delete(id);
        } else {
            self.substrate.pending_delete.queue_object(id);
        }
    }

    /// Building451A2C, ClearAnimSlot451E40 and Infantry5185C3 use scalar
    /// deletion: the old object is gone before the caller continues. Keep this
    /// separate from an animation's ordinary deferred Destroy operation.
    pub(crate) fn scalar_delete_anim(&mut self, id: AnimId) {
        // Anim VT7E3354+20 ->426590 ->4228E0 releases sound handles but
        // never reaches UnInit4255B0 or its StopSound playback. The slot was
        // cleared by the caller before these synchronous destructor effects.
        let sound_active = self.anim(id).is_some_and(|anim| anim.start_sound_active);
        self.notify_anim_pointer_expired(id);
        self.substrate.clear_damage_fire_anim_reference(id);
        self.substrate.release_last_anim_owner_flag(id);
        self.substrate.release_anim_owner_reference(id);
        self.substrate.clear_building_anim_reference(id);
        if sound_active {
            self.sound_events
                .emit(SimSoundEvent::ObjectSoundReleased { owner: id });
        }
        self.conceal_anim(id);
        self.substrate.pending_delete.remove_object(id);
        // Scalar slot deletion451A2C ends its inherited FoggedOut199 flag;
        // the immutable FoggedObject record does not retain this Anim pointer.
        self.fog.forget_fogged_live_object(id);
        self.delete_anim_alpha_shape(id);
        self.substrate.anims.remove(id);
    }

    /// AnimClass::SetOwnerObject424B50. Normal detach preserves absolute
    /// coordinates and gates Remove/Submit on the entry Object+74 mark. Attach
    /// always removes, stores owner-relative coordinates, then submits Ground.
    /// The owner+17C callback is a bare RET5F43C0 for supported Techno owners.
    /// Last detach clears Object+84; every attach sets it before publishing CC.
    pub(crate) fn set_anim_owner_object(
        &mut self,
        id: AnimId,
        new_owner: Option<u64>,
        rules: &RuleSet,
    ) -> bool {
        let Some(anim) = self.anim(id) else {
            return false;
        };
        let marked = anim.display.marked_on_map;
        // A terrain owner (Ares terrain fire): the same detach, whose
        // last-anim callback is TerrainClass vt+0x17C, Finish_Fire_Death
        // (`0x00424BB0`). Its stored coordinate is already absolute.
        let owner_terrain = anim.owner_terrain;
        if let Some(terrain) = owner_terrain {
            if marked {
                self.substrate.display.remove(id);
            }
            let last = !self.substrate.anims.another_owned_by_terrain(id, terrain);
            if last {
                self.terrain_finish_fire_death(terrain);
            }
            self.substrate.anims.set_owner_terrain(id, None);
            if marked {
                self.submit_anim_display(id, rules);
            }
        }
        let Some(anim) = self.anim(id) else {
            return false;
        };
        if anim.owner_entity.is_some() {
            if marked {
                self.substrate.display.remove(id);
            }
            self.substrate.release_last_anim_owner_flag(id);
            let absolute = self.anim_absolute_coord(id).expect("live Anim");
            self.substrate.anims.set_owner_entity(id, None);
            let anim = self.substrate.anims.get_mut(id).expect("live Anim");
            anim.world_coord = absolute;
            if marked {
                self.submit_anim_display(id, rules);
            }
        }
        if let Some(owner_id) = new_owner {
            let Some(owner_coord) = self.anim_owner_coords(owner_id) else {
                return false;
            };
            if !self.substrate.entities.contains(owner_id) {
                // A terrain owner: attach without the coordinate conversion
                // (see `AnimObject::owner_terrain`) or the Object+84 flag,
                // which no terrain reader asks.
                self.substrate.display.remove(id);
                self.substrate.anims.set_owner_terrain(id, Some(owner_id));
                self.submit_anim_display(id, rules);
                return true;
            }
            self.substrate.display.remove(id);
            // A presentation anim leaves the owner's flag alone
            // (`AnimConstructorInput::Presentation`).
            if !self.anim(id).is_some_and(AnimObject::is_presentation) {
                self.substrate
                    .entities
                    .get_mut(owner_id)
                    .expect("the resolved Anim owner remains stored")
                    .set_attached_anim_flag(true);
            }
            let anim = self.substrate.anims.get_mut(id).expect("live Anim");
            anim.world_coord = AnimWorldCoord {
                x: anim.world_coord.x.wrapping_sub(owner_coord.x),
                y: anim.world_coord.y.wrapping_sub(owner_coord.y),
                z: anim.world_coord.z.wrapping_sub(owner_coord.z),
            };
            self.substrate.anims.set_owner_entity(id, Some(owner_id));
            self.submit_anim_display(id, rules);
        }
        true
    }

    /// Anim GetLayer424CB0: attached -> Ground; missing type -> Air; otherwise
    /// the current type's layer.
    pub(crate) fn submit_anim_display(&mut self, id: AnimId, rules: &RuleSet) {
        // A presentation anim keeps the layer it draws in instead of a slot.
        if self.anim(id).is_some_and(AnimObject::is_presentation) {
            let layer = self.anim_display_layer(id, rules);
            let order = self.substrate.display.unshared_order();
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.presentation = Some(layer);
                anim.presentation_order = order;
            }
            return;
        }
        if let Some(layer) = self.anim_display_layer(id, rules) {
            Self::submit_object_display(&mut self.substrate, id, layer);
        } else {
            self.substrate.display.remove(id);
        }
    }

    pub(crate) fn anim_display_layer(
        &self,
        id: AnimId,
        rules: &RuleSet,
    ) -> Option<crate::sim::world::display_layers::DisplayLayer> {
        use crate::rules::art_data::AnimLayer;
        use crate::sim::world::display_layers::DisplayLayer;
        let anim = self.substrate.anims.get(id)?;
        let attached = anim.owner_entity.is_some() || anim.owner_terrain.is_some();
        if attached {
            return Some(DisplayLayer::GROUND);
        }
        let config = self.interner.anim_type_config(anim.type_id, rules);
        match config.map(|config| config.layer) {
            None => Some(DisplayLayer::AIR),
            Some(AnimLayer::Ground) => Some(DisplayLayer::GROUND),
            Some(AnimLayer::Top) => Some(DisplayLayer::TOP),
            Some(AnimLayer::Other(index)) => {
                u8::try_from(index).ok().and_then(DisplayLayer::from_index)
            }
        }
    }

    /// gamemd-derived: `AnimClass::GetCoords @ 0x00422BE0` — with an owner at
    /// `Anim+0xCC` it returns `stored + owner->GetCoords()`, otherwise the
    /// stored coordinate unchanged. This is the only correct way to read an
    /// anim's world position: [`AnimObject::world_coord`] is owner-relative
    /// whenever `owner_entity` is set, exactly as native stores it.
    ///
    /// Returns `None` only when the anim does not exist.
    ///
    /// VERA-internal, gamemd equivalent UNREACHABLE: an anim naming an owner
    /// the store no longer holds is treated as unattached, so the stored
    /// coordinate is returned as-is. Native cannot produce that state —
    /// `AnimClass::Detach @ 0x00425150` runs from the owner's own uninit, via
    /// `ObjectClass::Detach_From_All_Lists`, before the pointer can dangle, and
    /// this engine mirrors it in `anim_pointer_expired`. The fallback
    /// exists so every caller agrees on one behaviour instead of one panicking
    /// and another silently dropping the anim; treating a dangling owner as no
    /// owner is also the only reading under which the stored coordinate means
    /// anything.
    pub fn anim_absolute_coord(&self, id: AnimId) -> Option<AnimWorldCoord> {
        Some(anim_world_coords(
            self.anim(id)?,
            &self.substrate.entities,
            self.resolved_terrain.as_ref(),
        ))
    }

    /// Intern now every name a presentation anim of type `config` may intern
    /// on a later visit: the types it can become or make (`Next=`,
    /// `TrailerAnim=`, `ExpireAnim=`, `BounceAnim=`) and their StopSounds.
    /// A game loaded or rejoined while the anim lives lacks the anim, so the
    /// interner must not wait for its visits to learn them.
    pub(super) fn intern_presentation_chain<'r>(
        &mut self,
        rules: &'r RuleSet,
        config: &'r AnimTypeRuntimeConfig,
    ) {
        // The names are borrowed from the rules, which the interning leaves
        // alone.
        let children = |config: &'r AnimTypeRuntimeConfig| {
            [
                config.next.as_deref(),
                config.trailer_anim.as_deref(),
                config.expire_anim.as_deref(),
                config.bounce_anim.as_deref(),
            ]
            .into_iter()
            .flatten()
        };
        // Most effect anims name none of these, and for them neither list
        // allocates; only a type with a chain pays for walking it.
        let mut pending: Vec<&str> = children(config).collect();
        let mut seen: Vec<&str> = Vec::new();
        while let Some(name) = pending.pop() {
            if seen.contains(&name) {
                continue;
            }
            self.interner.intern(name);
            if let Some(child) = rules.art().anim_runtime_config(name) {
                if let Some(sound) = child.stop_sound.as_deref() {
                    self.interner.intern(sound);
                }
                pending.extend(children(child));
            }
            seen.push(name);
        }
    }

    /// Whether presentation anim `id` rides an owner its UnInit has ended.
    pub(crate) fn presentation_anim_orphaned(&self, id: AnimId) -> bool {
        self.anim(id)
            .filter(|anim| anim.is_presentation())
            .and_then(|anim| anim.owner_entity)
            .is_some_and(|owner| {
                self.substrate
                    .entities
                    .get(owner)
                    .is_none_or(|entity| !entity.lifecycle.object_alive())
            })
    }

    /// The presentation anims ([`AnimConstructorInput::Presentation`]) that
    /// draw this frame, each with its layer, in the order they were made. The
    /// picture draws each where Display would hold it as an ordinary anim
    /// (`Simulation::picture_draw_order`).
    pub fn presentation_anims(
        &self,
    ) -> impl Iterator<Item = (AnimId, crate::sim::world::display_layers::DisplayLayer)> + '_ {
        self.substrate
            .anims
            .presentation_iter()
            .filter(|(id, _)| !self.substrate.anims.local_delete_queued(**id))
            .filter_map(|(&id, anim)| Some((id, anim.presentation_layer()?)))
    }

    /// Every Anim's [`Self::anim_absolute_coord`], in one walk of the store,
    /// the local anims last: the presentation places what it draws by these.
    pub fn anim_absolute_coords(&self) -> impl Iterator<Item = (AnimId, AnimWorldCoord)> + '_ {
        let terrain = self.resolved_terrain.as_ref();
        let anims = &self.substrate.anims;
        anims
            .iter()
            .chain(anims.local_iter())
            .map(move |(&id, anim)| {
                (
                    id,
                    anim_world_coords(anim, &self.substrate.entities, terrain),
                )
            })
    }

    pub(crate) fn anim_owner_coords(&self, owner_id: u64) -> Option<AnimWorldCoord> {
        // A terrain owner's centre is its GetCoords (Terrain VT+48 ->
        // Object5F65A0), for the Ares terrain fire.
        if !self.substrate.entities.contains(owner_id) {
            let centre = self.terrain_objects.get(owner_id)?.world_coord();
            return Some(AnimWorldCoord {
                x: centre.x,
                y: centre.y,
                z: centre.z,
            });
        }
        Some(anim_owner_world_coords(
            self.substrate.entities.get(owner_id)?,
            self.resolved_terrain.as_ref(),
        ))
    }

    /// Anim Destroy's owner callback (Techno710410 -> Object5F6DA0) clears
    /// other owner references, not Building damage-fire slots. Those clear
    /// only when this Anim broadcasts its own pointer expiry (44EA45).
    pub(crate) fn detach_anim_from_owner(&mut self, id: AnimId, rules: &RuleSet) -> Option<u64> {
        // A terrain owner's +60 is the Object base, which keeps no anim.
        if let Some(terrain) = self.substrate.anims.owner_terrain(id) {
            self.set_anim_owner_object(id, None, rules);
            return Some(terrain);
        }
        let owner = self.anim(id)?.owner_entity?;
        if let Some(entity) = self.substrate.entities.get_mut(owner) {
            entity.attached_anim_expired(id);
        }
        self.set_anim_owner_object(id, None, rules);
        Some(owner)
    }

    /// Anim PointerExpired425150 removes Display, calls owner+60, clears +CC,
    /// sets +19B and Mark(REMOVE). Stored relative coordinates remain unchanged.
    /// Normal SetOwner(NULL) would instead convert and potentially resubmit.
    /// Its independent attached-Bullet arm clears +17C before UnInit (4251AD).
    /// Native comparisons: tools/spatial_oracle/display_anim_owner.json.
    ///
    /// AI checks +19B at42435F after its bounce/trailer/DropZone prefix. The
    /// same gate serves owner expiry, occupied-cell424358 and the later
    /// IsAnimatedTiberium424427 writer; the latter finishes its current visit.
    pub(crate) fn anim_pointer_expired(
        &mut self,
        id: AnimId,
        expired_id: u64,
        rules: &RuleSet,
    ) -> bool {
        let mut changed = false;
        let owner = self
            .anim(id)
            .and_then(|anim| anim.owner_entity.or(anim.owner_terrain));
        if owner == Some(expired_id) {
            self.substrate.display.remove(id);
            if let Some(entity) = self.substrate.entities.get_mut(expired_id) {
                entity.attached_anim_expired(id);
            }
            self.substrate.release_anim_owner_reference(id);
            self.substrate.mark_anim_display(id, false);
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.inactive = true;
            }
            changed = true;
        }
        // Independent later +17C arm4251AD..4251C8. Clear before UnInit;
        // the owner arm above does not suppress this callback.
        if self
            .anim(id)
            .is_some_and(|anim| anim.attached_bullet == Some(expired_id))
        {
            self.substrate.anims.set_attached_bullet(id, None);
            self.destroy_anim_with_context(id, rules);
            changed = true;
        }
        changed
    }

    /// Manual frame writer62979A..6297D7: restart the owned stage timer,
    /// write its reload rate (+C0 at6297B7), then set the requested frame.
    /// The animation remains in Logic and must retain this rate on expiry.
    pub(crate) fn set_anim_manual_frame(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        id: AnimId,
        frame: i32,
        duration: u16,
    ) {
        let current = session.binary_frame() as i32;
        if let Some(anim) = substrate.anims.get_mut(id) {
            anim.runtime.frame_timer = CdTimer::started(current, i32::from(duration));
            anim.runtime.rate_reload = duration;
            anim.runtime.current_frame = frame;
        }
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// Mutation of the retained Anim+198 byte belongs to its instance owner.
    pub(crate) fn set_anim_no_effects(&mut self, id: AnimId, no_effects: bool) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.no_effects = no_effects;
        }
    }
    pub(crate) fn clear_damage_fire_anim_reference(&mut self, id: AnimId) {
        let link = self
            .anims
            .get_mut(id)
            .and_then(|anim| anim.damage_fire_slot.take());
        if let Some((owner, slot)) = link
            && let Some(entity) = self.entities.get_mut(owner)
            && entity.damage_fires.anim_ids[usize::from(slot)] == Some(id)
        {
            entity.damage_fires.anim_ids[usize::from(slot)] = None;
        }
    }
    pub(crate) fn mark_anim_display(&mut self, id: AnimId, marked: bool) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.display.marked_on_map = marked;
        }
    }
    /// Retail gamemd.exe SetOwnerObject424B79..424BB6 and active-scenario
    /// destructor42291A..42295B clear Object+84 only for the last attached
    /// Anim. Scan the whole retained array, excluding only this Anim: inactive,
    /// concealed and pending-delete entries still count while CC names owner.
    /// Owner+17C is bare RET5F43C0 for supported Techno owners. Rust world
    /// disposal drops the stores; it does not simulate native gate0 exit teardown.
    pub(crate) fn release_last_anim_owner_flag(&mut self, id: AnimId) {
        // A presentation anim never raised the flag, and the scan below does
        // not count it.
        let Some(owner) = self
            .anims
            .get(id)
            .filter(|anim| !anim.is_presentation())
            .and_then(|anim| anim.owner_entity)
        else {
            return;
        };
        if !self.anims.another_owned_by(id, owner)
            && let Some(entity) = self.entities.get_mut(owner)
        {
            entity.set_attached_anim_flag(false);
        }
    }

    /// Clear only CC, without SetOwner's coordinate conversion or display
    /// submission (422961 / 425190). Callers own the distinct +60 and +84
    /// effects: PointerExpired clears CC without releasing the retained flag.
    pub(crate) fn release_anim_owner_reference(&mut self, id: AnimId) -> Option<u64> {
        // Runs in every game: only the Ares terrain fire gives an anim a
        // terrain owner, and without one this falls through to the stock clear.
        if let Some(Some(terrain)) = self.anims.set_owner_terrain(id, None) {
            return Some(terrain);
        }
        self.anims.set_owner_entity(id, None)?
    }
    pub(crate) fn set_anim_frame_and_z_adjust(&mut self, id: AnimId, frame: i32, z_adjust: i32) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.runtime.current_frame = frame;
            anim.z_adjust = z_adjust;
        }
    }
    /// Apply CellClass's producer-owned `AnimClass +0x100` write after the
    /// delay-zero constructor has already run `Middle`.
    pub(crate) fn set_terrain_anim_z_adjust_after_construction(
        &mut self,
        id: AnimId,
        z_adjust: i32,
    ) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        if !anim.terrain_attached || anim.z_adjust != 0 {
            return false;
        }
        anim.z_adjust = z_adjust;
        true
    }
    /// A producer's plain post-construction `AnimClass +0x100` (ZAdjust)
    /// write, such as CaptureUnit's building ring (`0x00471F66`).
    pub(crate) fn set_anim_z_adjust(&mut self, id: AnimId, z_adjust: i32) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.z_adjust = z_adjust;
        true
    }
    /// A producer's plain Anim+19D store. Mirage's attached-ring tail
    /// `746A9C..746B19` changes drawing without suspending the Anim AI.
    pub(crate) fn set_anim_hidden(&mut self, id: AnimId, hidden: bool) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.draw_runtime.hidden = hidden;
        anim.viewer_hidden = None;
        true
    }

    /// Anim+19D per viewer: `viewers` holds named houses' bytes and
    /// `other_hidden` every other viewer's. Ares common Launch100330DC writes
    /// it once, from the alliance relation at construction rather than a live
    /// visibility query; the disguised ring tail writes it every frame.
    pub(crate) fn set_anim_viewer_hidden(
        &mut self,
        id: AnimId,
        viewers: Vec<(InternedId, bool)>,
        other_hidden: bool,
    ) {
        if let Some(anim) = self.anims.get_mut(id) {
            anim.viewer_hidden = Some(AnimViewerHidden {
                viewers,
                other_hidden,
            });
        }
    }
    /// `AnimClass::SetOwnerHouse @ 0x00424CA0`: store `+0x180`.
    pub(crate) fn set_anim_owner_house(&mut self, id: AnimId, house: InternedId) -> bool {
        let Some(anim) = self.anims.get_mut(id) else {
            return false;
        };
        anim.owner_house = Some(house);
        true
    }
    /// `0x00424C90`: store `+0x17C`, the bullet whose expiry ends the anim.
    pub(crate) fn set_anim_attached_bullet(&mut self, id: AnimId, bullet: Option<u64>) -> bool {
        self.anims.set_attached_bullet(id, bullet)
    }
}
