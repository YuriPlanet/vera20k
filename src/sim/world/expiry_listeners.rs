//! Which listeners a pointer expiry reaches.
//!
//! When a Techno expires, `DispatchPointerExpiredCleanup @ 0x007258D0`
//! walks registered listeners (`0x00B0F720`) and calls PointerExpired
//! (`0x00725954`). Most listeners hold nothing the expiry changes.
//! This index names the entities, anims, particle systems, bullets and
//! waves whose callbacks use the expiring id. A smudge's callback changes
//! nothing. [`Simulation::entity_expiry_holds`] and the other stores'
//! `expiry_names` describe the fields each callback compares.
//!
//! The index re-reads objects from each store's touch log. Of the entities it
//! names, only those noted since ([`EntityStore::take_unread_names`]): an
//! entity field takes an id only through a writer that notes its entity
//! (`GameEntity::note_names`), so an entity left out holds at most what the
//! index read, and an entity the index finds is asked before it is answered.
//! A fresh, cloned or restored store triggers a full rebuild
//! ([`ExpiryListeners::asked_of_every`]). The index is excluded from saves and
//! from the simulation hash.

use std::collections::BTreeSet;

use super::Simulation;
use super::substrate::ObjectKind;
use crate::sim::anim_class::{AnimStore, AnimTouchReader, is_local_anim_id};
use crate::sim::entity_store::EntityStore;
use crate::sim::particles::{ParticleSystemStore, ParticleTouchReader};
use crate::sim::projectile::ProjectileStore;
use crate::sim::terrain_object::TerrainObjectStore;
use crate::sim::touch_log::Touched;
use crate::sim::wave::WaveStore;
use crate::util::lookup_hash::LookupMap;

/// The fewest re-read entities a refresh hands to the worker pool.
const PARALLEL_FROM: usize = 256;

/// How many of them each of the pool's tasks re-reads.
const POOLED_TASK: usize = 128;

/// The stores whose objects listen to pointer expiry.
pub(crate) struct ListenerStores<'a> {
    pub(crate) entities: &'a mut EntityStore,
    pub(crate) anims: &'a mut AnimStore,
    pub(crate) particle_systems: &'a mut ParticleSystemStore,
    pub(crate) projectiles: &'a mut ProjectileStore,
    pub(crate) waves: &'a mut WaveStore,
    pub(crate) terrain: &'a mut TerrainObjectStore,
}

/// What one listener holds that an expiry can change.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Holds {
    /// The ids its compared fields name, ascending, without repeats.
    names: Vec<u64>,
    /// It holds an object a forward reaches whatever expires.
    every: bool,
}

impl Holds {
    /// The ids `holds` names: none for a listener an expiry passes over.
    fn names_of(holds: Option<&Self>) -> &[u64] {
        match holds {
            Some(holds) => &holds.names,
            None => &[],
        }
    }
}

/// An object's re-read whose callback compares `held`: it names those ids,
/// and no object but an entity holds something any expiry reaches.
fn named(names: &mut Vec<u64>, held: impl IntoIterator<Item = Option<u64>>) -> bool {
    names.extend(held.into_iter().flatten());
    false
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ExpiryListeners {
    entities: StoreIndex,
    anims: StoreIndex,
    particle_systems: StoreIndex,
    projectiles: StoreIndex,
    waves: StoreIndex,
    terrain: StoreIndex,
    /// Test builds: the fixture notes taken when the entities were last read
    /// ([`Self::after_fixture_notes`]).
    #[cfg(any(test, feature = "test-support"))]
    fixture_notes: u64,
    /// The empty stand-in of a presentation copy of the world
    /// ([`Self::detached`]). Debug builds refuse reads of it.
    detached: bool,
}

/// The listeners of one store.
#[derive(Debug, Clone, Default)]
struct StoreIndex {
    /// What each listener holding anything held when last read. Only looked
    /// up; the one walk over it sorts what it finds.
    holds: LookupMap<u64, Holds>,
    /// (named id, listener naming it): an id's namers are one ascending range,
    /// and a listener that stops naming it leaves in one step, however many
    /// others name it too.
    named_by: BTreeSet<(u64, u64)>,
    /// The listeners an expiry of anything reaches.
    every: BTreeSet<u64>,
    /// Whether every listener has been read.
    built: bool,
    /// Scratch for the ids a re-read listener names, kept from one refresh
    /// to the next so only a listener whose holdings changed allocates.
    /// Empty between refreshes.
    names: Vec<u64>,
}

impl ExpiryListeners {
    /// The empty index a presentation copy of the world holds in place of
    /// this one (`Simulation::presentation_copy`): only gameplay expires
    /// pointers. Debug builds panic on a read of it.
    pub(crate) fn detached() -> Self {
        Self {
            detached: true,
            ..Self::default()
        }
    }

    /// Debug builds: refuse a read of a [`Self::detached`] index.
    #[inline]
    fn check_attached(&self) {
        debug_assert!(
            !self.detached,
            "read of the pointer-expiry index of a presentation copy of the world: only the live world holds it"
        );
    }

    /// The listeners `expired`'s expiry reaches, ascending, each with its
    /// store: the entity itself while stored, those naming it, and those any
    /// expiry reaches. The listeners that [`Self::asked_of_every`] selects.
    pub(crate) fn reached_by(
        &mut self,
        stores: ListenerStores<'_>,
        expired: u64,
    ) -> Vec<(u64, ObjectKind)> {
        self.check_attached();
        let ListenerStores {
            entities,
            anims,
            particle_systems,
            projectiles,
            waves,
            terrain,
        } = stores;
        self.refresh_entities(entities);
        let entities = &*entities;
        // A full scan visits shared anims. The touch log also names local
        // anims, including VERA's presentation anims, which keep their owner
        // link until UnInit. Use the same membership for incremental updates.
        let touched = match anims.take_touched(AnimTouchReader::ExpiryListeners) {
            Touched::Ids(ids) => Touched::Ids(
                ids.into_iter()
                    .filter(|&id| !is_local_anim_id(id))
                    .collect(),
            ),
            touched => touched,
        };
        let ids = self.anims.rereads(touched, anims.len(), usize::MAX, || {
            anims.iter().map(|(&id, _)| id).collect()
        });
        self.anims.reread(&ids, |id, names| {
            anims.get(id).map(|anim| named(names, anim.expiry_names()))
        });
        // The other listeners hold only a few ids: reading them costs
        // less than handing them to the worker pool.
        let ids = self.particle_systems.rereads(
            particle_systems.take_touched(ParticleTouchReader::ExpiryListeners),
            particle_systems.len(),
            usize::MAX,
            || particle_systems.iter().map(|(&id, _)| id).collect(),
        );
        self.particle_systems.reread(&ids, |id, names| {
            particle_systems
                .get(id)
                .map(|system| named(names, system.expiry_names()))
        });
        self.refresh_projectiles(projectiles);
        let ids = self
            .waves
            .rereads(waves.take_touched(), waves.len(), usize::MAX, || {
                waves.iter().map(|(&id, _)| id).collect()
            });
        self.waves.reread(&ids, |id, names| {
            waves.get(id).map(|wave| named(names, wave.expiry_names()))
        });
        let ids = self.terrain.rereads(
            terrain.take_touched(),
            terrain.objects().len(),
            usize::MAX,
            || terrain.objects().keys().copied().collect(),
        );
        self.terrain.reread(&ids, |id, names| {
            terrain
                .get(id)
                .map(|_| named(names, [terrain.next_in_cell(id)]))
        });

        // A clear takes no note, so the index may still hold a name an entity
        // dropped: each entity it finds is asked, and one that no longer acts
        // is re-read, so the index forgets what it no longer holds.
        let mut found: Vec<u64> = self.entities.reaching(expired).collect();
        found.sort_unstable();
        found.dedup();
        let mut reached: Vec<(u64, ObjectKind)> = Vec::with_capacity(found.len() + 1);
        for id in found {
            if entities
                .get(id)
                .is_some_and(|entity| Simulation::entity_expiry_listener_acts(entity, id, expired))
            {
                reached.push((id, ObjectKind::Entity));
            } else {
                self.entities.reread(&[id], |id, names| {
                    entities.get(id).map(|entity| {
                        Simulation::entity_expiry_holds(entity, |name| names.push(name))
                    })
                });
            }
        }
        if entities.contains(expired) {
            reached.push((expired, ObjectKind::Entity));
        }
        for (index, kind) in [
            (&self.anims, ObjectKind::Anim),
            (&self.particle_systems, ObjectKind::ParticleSystem),
            (&self.projectiles, ObjectKind::Projectile),
            (&self.waves, ObjectKind::Wave),
            (&self.terrain, ObjectKind::Terrain),
        ] {
            reached.extend(index.reaching(expired).map(|id| (id, kind)));
        }
        // Object ids are unique across stores; an entity can be reached more
        // than one way.
        reached.sort_unstable_by_key(|&(id, _)| id);
        reached.dedup_by_key(|&mut (id, _)| id);
        reached
    }

    /// The listeners `expired`'s expiry reaches, found by asking every stored
    /// object: what [`Self::reached_by`] answers, for checks.
    pub(crate) fn asked_of_every(
        stores: &ListenerStores<'_>,
        expired: u64,
    ) -> Vec<(u64, ObjectKind)> {
        let names = |names: &[Option<u64>]| names.contains(&Some(expired));
        let mut reached: Vec<(u64, ObjectKind)> = stores
            .entities
            .par_select(|id, entity| Simulation::entity_expiry_listener_acts(entity, id, expired))
            .into_iter()
            .map(|id| (id, ObjectKind::Entity))
            .collect();
        reached.extend(
            stores
                .anims
                .iter()
                .filter(|(_, anim)| names(&anim.expiry_names()))
                .map(|(&id, _)| (id, ObjectKind::Anim)),
        );
        reached.extend(
            stores
                .particle_systems
                .iter()
                .filter(|(_, system)| system.expiry_names().any(|name| name == Some(expired)))
                .map(|(&id, _)| (id, ObjectKind::ParticleSystem)),
        );
        reached.extend(
            stores
                .projectiles
                .iter()
                .filter(|(_, bullet)| names(&bullet.expiry_names()))
                .map(|(&id, _)| (id, ObjectKind::Projectile)),
        );
        reached.extend(
            stores
                .waves
                .iter()
                .filter(|(_, wave)| names(&wave.expiry_names()))
                .map(|(&id, _)| (id, ObjectKind::Wave)),
        );
        reached.extend(
            stores
                .terrain
                .objects()
                .keys()
                .filter(|&&id| stores.terrain.next_in_cell(id) == Some(expired))
                .map(|&id| (id, ObjectKind::Terrain)),
        );
        reached.sort_unstable_by_key(|&(id, _)| id);
        reached
    }

    fn refresh_entities(&mut self, entities: &mut EntityStore) {
        let unread = entities.take_unread_names();
        #[cfg(any(test, feature = "test-support"))]
        let unread = self.after_fixture_notes(unread);
        // The ids the worker pool takes stay as logged: sorting thousands
        // of them costs more than re-reading the few that repeat.
        let ids = self
            .entities
            .rereads(unread, entities.len(), PARALLEL_FROM, || {
                entities.keys_sorted()
            });
        self.entities.reread_pooled(&ids, |id, names| {
            entities
                .get(id)
                .map(|entity| Simulation::entity_expiry_holds(entity, |name| names.push(name)))
        });
    }

    /// Test builds: every entity once a fixture took a note since the last
    /// read, since a fixture's note names no entity ([`NameNote::fixture`]).
    ///
    /// [`NameNote::fixture`]: crate::sim::game_entity::NameNote::fixture
    #[cfg(any(test, feature = "test-support"))]
    fn after_fixture_notes(&mut self, unread: Touched) -> Touched {
        let notes = crate::sim::game_entity::NameNote::fixture_notes();
        if std::mem::replace(&mut self.fixture_notes, notes) == notes {
            unread
        } else {
            Touched::All
        }
    }

    /// The Bullet subset of the same index, for Anim4's dedicated holder
    /// dispatch725A16..725A4C. No generic Object callbacks run on this route.
    pub(crate) fn projectiles_reached_by(
        &mut self,
        projectiles: &mut ProjectileStore,
        expired: u64,
    ) -> Vec<u64> {
        self.check_attached();
        self.refresh_projectiles(projectiles);
        self.projectiles.reaching(expired).collect()
    }

    fn refresh_projectiles(&mut self, projectiles: &mut ProjectileStore) {
        let ids = self.projectiles.rereads(
            projectiles.take_touched(),
            projectiles.len(),
            usize::MAX,
            || projectiles.iter().map(|(&id, _)| id).collect(),
        );
        self.projectiles.reread(&ids, |id, names| {
            projectiles
                .get(id)
                .map(|bullet| named(names, bullet.expiry_names()))
        });
    }
}

impl StoreIndex {
    /// The listeners to re-read: those `touched` names, or every stored one
    /// (`stored_ids`, ascending) when the log cannot say which or names as
    /// many ids as the store holds (its ids repeat, so sorting them would
    /// cost more), after dropping what the removed ones held. Fewer than
    /// `sort_below` logged ids come back ascending without repeats; more
    /// come back as logged, where a repeat is re-read and reports what its
    /// first reading did.
    fn rereads(
        &mut self,
        touched: Touched,
        stored: usize,
        sort_below: usize,
        stored_ids: impl FnOnce() -> Vec<u64>,
    ) -> Vec<u64> {
        match touched {
            Touched::Ids(mut ids) if self.built && ids.len() < stored => {
                if ids.len() < sort_below {
                    ids.sort_unstable();
                    ids.dedup();
                }
                ids
            }
            _ => {
                self.built = true;
                let ids = stored_ids();
                let mut gone: Vec<u64> = self
                    .holds
                    .keys()
                    .copied()
                    .filter(|id| ids.binary_search(id).is_err())
                    .collect();
                gone.sort_unstable();
                for id in gone {
                    self.replace(id, None);
                }
                ids
            }
        }
    }

    /// Re-read `ids` with `read`, which pushes the ids a stored listener
    /// names (repeats possible) and says whether any expiry reaches it, or
    /// answers `None` for a removed one, and record what changed.
    fn reread(&mut self, ids: &[u64], read: impl Fn(u64, &mut Vec<u64>) -> Option<bool>) {
        let mut names = std::mem::take(&mut self.names);
        for &id in ids {
            if let Some(now) = self.change(id, &mut names, |names| read(id, names)) {
                self.replace(id, now);
            }
        }
        names.clear();
        self.names = names;
    }

    /// [`Self::reread`], on the worker pool when there are many.
    fn reread_pooled(
        &mut self,
        ids: &[u64],
        read: impl Fn(u64, &mut Vec<u64>) -> Option<bool> + Sync,
    ) {
        // Handing a few ids to the worker pool costs more than reading them.
        if ids.len() < PARALLEL_FROM {
            return self.reread(ids, read);
        }
        let index = &*self;
        let tasks: Vec<&[u64]> = ids.chunks(POOLED_TASK).collect();
        // Each worker names into its own scratch, and each task answers only
        // the listeners whose holdings changed, in the order of its ids.
        let changes = crate::sim::parallel::map_with(&tasks, Vec::new(), |names, task| {
            task.iter()
                .filter_map(|&id| {
                    index
                        .change(id, names, |names| read(id, names))
                        .map(|now| (id, now))
                })
                .collect::<Vec<_>>()
        });
        for (id, now) in changes.into_iter().flatten() {
            self.replace(id, now);
        }
    }

    /// What re-reading listener `id` with `read` changes: `None` when it
    /// holds what the index holds for it, else what it holds now (`None`
    /// when an expiry passes it over). A listener that holds what it held
    /// allocates nothing; `names` is scratch.
    fn change(
        &self,
        id: u64,
        names: &mut Vec<u64>,
        read: impl FnOnce(&mut Vec<u64>) -> Option<bool>,
    ) -> Option<Option<Holds>> {
        names.clear();
        let every = read(names);
        names.sort_unstable();
        names.dedup();
        let held = self.holds.get(&id);
        let unchanged = match every {
            Some(every) => held.map_or(!every && names.is_empty(), |held| {
                held.every == every && held.names == *names
            }),
            None => held.is_none(),
        };
        (!unchanged).then(|| {
            every
                .filter(|&every| every || !names.is_empty())
                .map(|every| Holds {
                    names: names.clone(),
                    every,
                })
        })
    }

    /// The listeners an expiry of `expired` reaches: those any expiry
    /// reaches, then those naming it.
    fn reaching(&self, expired: u64) -> impl Iterator<Item = u64> + '_ {
        self.every.iter().copied().chain(
            self.named_by
                .range((expired, 0)..=(expired, u64::MAX))
                .map(|&(_, listener)| listener),
        )
    }

    /// Record that listener `id` holds `now`, changing only the names and
    /// the flag that differ from what it held.
    fn replace(&mut self, id: u64, now: Option<Holds>) {
        let old = self.holds.get(&id);
        if old == now.as_ref() {
            return;
        }
        let every = |holds: Option<&Holds>| holds.is_some_and(|holds| holds.every);
        // Both lists ascend without repeats, so one pass finds the names
        // that left and the names that came.
        let (mut left, mut came) = (Holds::names_of(old), Holds::names_of(now.as_ref()));
        loop {
            match (left.first(), came.first()) {
                (None, None) => break,
                (Some(&was), Some(&is)) if was == is => {
                    left = &left[1..];
                    came = &came[1..];
                }
                (Some(&was), Some(&is)) if was < is => {
                    self.named_by.remove(&(was, id));
                    left = &left[1..];
                }
                (Some(&was), None) => {
                    self.named_by.remove(&(was, id));
                    left = &left[1..];
                }
                (_, Some(&is)) => {
                    self.named_by.insert((is, id));
                    came = &came[1..];
                }
            }
        }
        match (every(old), every(now.as_ref())) {
            (true, false) => {
                self.every.remove(&id);
            }
            (false, true) => {
                self.every.insert(id);
            }
            _ => {}
        }
        match now {
            Some(now) => {
                self.holds.insert(id, now);
            }
            None => {
                self.holds.remove(&id);
            }
        }
    }
}

#[cfg(all(test, tests_sim_world_c_o))]
mod tests {
    use super::*;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::anim_class::{AnimObject, AnimRuntime, AnimWorldCoord};
    use crate::sim::capture_manager::CaptureManagerState;
    use crate::sim::combat::{RAD_NO_ATTACKER, TargetKind};
    use crate::sim::components::NavTargetRef;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::InternedId;
    use crate::sim::particles::ParticleSystem;
    use crate::sim::projectile::{ProjectileCoord, ProjectilePayload, ProjectileTarget};
    use crate::sim::spawn_manager::{SpawnManagerState, SpawnSlot, SpawnSlotState};
    use crate::sim::timer::CdTimer;
    use crate::sim::wave::Wave;

    /// Every listener store.
    #[derive(Default)]
    struct Stores {
        entities: EntityStore,
        anims: AnimStore,
        particle_systems: ParticleSystemStore,
        projectiles: ProjectileStore,
        waves: WaveStore,
        terrain: TerrainObjectStore,
    }

    impl Stores {
        fn listeners(&mut self) -> ListenerStores<'_> {
            ListenerStores {
                entities: &mut self.entities,
                anims: &mut self.anims,
                particle_systems: &mut self.particle_systems,
                projectiles: &mut self.projectiles,
                waves: &mut self.waves,
                terrain: &mut self.terrain,
            }
        }
    }

    /// Reach `expired` through the index and check it against asking every
    /// stored object.
    fn reach(index: &mut ExpiryListeners, stores: &mut Stores, expired: u64) -> Vec<u64> {
        let reached = index.reached_by(stores.listeners(), expired);
        assert_eq!(
            reached,
            ExpiryListeners::asked_of_every(&stores.listeners(), expired),
            "index and stores disagree on {expired}"
        );
        reached.into_iter().map(|(id, _)| id).collect()
    }

    fn anim(id: u64) -> AnimObject {
        AnimObject::new_unattached_for_test(
            id,
            id as i32,
            InternedId::from_index(0),
            AnimWorldCoord { x: 0, y: 0, z: 0 },
            AnimRuntime {
                current_frame: 0,
                frame_step: 1,
                delay_remaining: 0,
                rate_reload: 1,
                frame_timer: crate::sim::timer::CdTimer::started(0, 1),
                loop_remaining: 0,
                first_ai_guard: false,
                constructor_reverse: false,
                inactive: false,
                paused: false,
                temporal_paused: false,
            },
        )
    }

    /// A local anim: a VERA presentation anim, which rides its owner but is
    /// not in the Anim array.
    fn presentation_anim(id: u64) -> AnimObject {
        let mut anim = anim(id);
        anim.mark_presentation_for_test();
        anim
    }

    fn bullet(stores: &mut Stores, id: u64, source: u64, target: ProjectileTarget) {
        let payload = ProjectilePayload::new(40, InternedId::from_index(3), None, false);
        stores
            .projectiles
            .construct(id, 0, 0, source, target, payload);
    }

    /// Retarget a stored entity through the class setter's store.
    fn retarget(entities: &mut EntityStore, id: u64, target: u64) {
        let entity = entities.get_mut(id).unwrap();
        let note = entity.note_names();
        entity
            .attack_target
            .set(Some(TargetKind::Entity(target)), note);
    }

    fn attacker(id: u64, target: u64) -> GameEntity {
        let mut entity = GameEntity::test_default(id, "MTNK", "Americans", 10, 10);
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target)));
        entity
    }

    #[test]
    fn the_index_reaches_what_asking_every_entity_reaches() {
        let mut entities = EntityStore::new();
        entities.insert(attacker(1, 3));
        let mut controller = GameEntity::test_default(2, "YURI", "Russians", 12, 12);
        controller.capture_manager = Some(Box::new(CaptureManagerState::with_victims_for_test(
            3,
            false,
            &[3],
        )));
        entities.insert(controller);
        entities.insert(GameEntity::test_default(3, "HTNK", "Russians", 14, 14));
        let mut docked = attacker(4, 3);
        docked.mark_live_contact_with(3);
        entities.insert(docked);
        let mut infested = GameEntity::test_default(6, "HTNK", "Russians", 16, 16);
        infested.parasite.set_eater_for_test(Some(8));
        entities.insert(infested);
        let mut stores = Stores {
            entities,
            ..Stores::default()
        };
        let mut index = ExpiryListeners::default();

        // Itself, the entities naming it once or twice (one through its
        // CaptureManager), and the one that any expiry reaches.
        assert_eq!(reach(&mut index, &mut stores, 3), [1, 2, 3, 4, 6]);
        assert_eq!(reach(&mut index, &mut stores, 9), [6]);
        // A retarget is read back on the next expiry.
        retarget(&mut stores.entities, 1, 4);
        assert_eq!(reach(&mut index, &mut stores, 3), [2, 3, 4, 6]);
        assert_eq!(reach(&mut index, &mut stores, 4), [1, 4, 6]);
        // An entity inserted after the index was built.
        stores.entities.insert(attacker(5, 3));
        assert_eq!(reach(&mut index, &mut stores, 3), [2, 3, 4, 5, 6]);
        // A removed entity reaches nothing and is no listener itself.
        stores.entities.remove(4);
        assert_eq!(reach(&mut index, &mut stores, 4), [1, 6]);
        assert_eq!(reach(&mut index, &mut stores, 3), [2, 3, 5, 6]);
        // A walk over every entity makes it re-read them all.
        for entity in stores.entities.values_mut() {
            entity.capture_manager = None;
            entity.parasite.set_eater_for_test(None);
        }
        assert_eq!(reach(&mut index, &mut stores, 3), [3, 5]);
        assert_eq!(reach(&mut index, &mut stores, 4), [1]);
    }

    /// Entities 1..=5, read once by `index`.
    fn read_entities(index: &mut ExpiryListeners) -> Stores {
        let mut stores = Stores::default();
        for id in 1..=5 {
            stores
                .entities
                .insert(GameEntity::test_default(id, "MTNK", "Americans", 10, 10));
        }
        assert!(reach(index, &mut stores, 20).is_empty());
        stores
    }

    #[test]
    fn a_name_each_kind_of_writer_stores_is_found_at_the_next_expiry() {
        let mut index = ExpiryListeners::default();
        let mut stores = read_entities(&mut index);
        // A field's writer takes its entity's note, an entity's own writer
        // and the store's held-Anim writer note it themselves, and a forked
        // turn's note comes with its commit.
        retarget(&mut stores.entities, 1, 20);
        let entity = stores.entities.get_mut(2).unwrap();
        let note = entity.note_names();
        entity
            .navigation
            .publish_nav_com(Some(NavTargetRef::object(21)), note);
        stores
            .entities
            .get_mut(3)
            .unwrap()
            .set_archive_target(Some(TargetKind::Entity(22)));
        stores.entities.hold_parachute_animation(4, 23);
        stores.entities.share();
        let mut fork = stores.entities.fork();
        retarget(&mut fork, 5, 24);
        let writes = fork.take_fork_writes();
        drop(fork);
        stores.entities.unshare();
        stores.entities.commit_writes(writes);
        for (expired, namer) in [(20, 1), (21, 2), (22, 3), (23, 4), (24, 5)] {
            assert_eq!(reach(&mut index, &mut stores, expired), [namer]);
        }
    }

    #[test]
    fn a_cleared_name_is_not_found() {
        let mut index = ExpiryListeners::default();
        let mut stores = read_entities(&mut index);
        let entity = stores.entities.get_mut(1).unwrap();
        let note = entity.note_names();
        entity
            .navigation
            .publish_nav_com(Some(NavTargetRef::object(20)), note);
        assert!(reach(&mut index, &mut stores, 30).is_empty());
        // A clear takes no note: the index still names 1 for 20, asks it,
        // and forgets it.
        stores.entities.get_mut(1).unwrap().navigation.stop_moving();
        assert!(reach(&mut index, &mut stores, 20).is_empty());
        assert_eq!(
            index
                .entities
                .named_by
                .range((20, 0)..=(20, u64::MAX))
                .count(),
            0
        );
    }

    #[test]
    fn an_entity_stored_again_is_read_whole() {
        let mut index = ExpiryListeners::default();
        let mut stores = read_entities(&mut index);
        retarget(&mut stores.entities, 1, 20);
        assert_eq!(reach(&mut index, &mut stores, 20), [1]);
        // A copied field takes no note; storing the entity again reads it.
        let copied = stores.entities.get(1).unwrap().attack_target;
        let mut moved = stores.entities.remove(3).unwrap();
        moved.attack_target = copied;
        stores.entities.insert(moved);
        assert_eq!(reach(&mut index, &mut stores, 20), [1, 3]);
    }

    #[test]
    fn a_spawn_manager_reaches_its_owner_by_its_targets_and_children() {
        let mut launcher = GameEntity::test_default(1, "V3", "Russians", 10, 10);
        let mut manager = SpawnManagerState::for_test(
            InternedId::from_index(0),
            vec![SpawnSlot::for_test(
                Some(5),
                SpawnSlotState::InFlight,
                CdTimer::default(),
                true,
            )],
        );
        manager.set_current_target_for_test(Some(TargetKind::Entity(6)));
        manager.set_queued_target_for_test(Some(TargetKind::Cell(3, 3)));
        launcher.spawn_manager = Some(Box::new(manager));
        let mut entities = EntityStore::new();
        entities.insert(launcher);
        let mut stores = Stores {
            entities,
            ..Stores::default()
        };
        let mut index = ExpiryListeners::default();

        assert_eq!(reach(&mut index, &mut stores, 1), [1]);
        assert_eq!(reach(&mut index, &mut stores, 5), [1]);
        assert_eq!(reach(&mut index, &mut stores, 6), [1]);
        assert!(reach(&mut index, &mut stores, 7).is_empty());
        // A queued entity target is read back on the next expiry.
        let launcher = stores.entities.get_mut(1).unwrap();
        let note = launcher.note_names();
        launcher
            .spawn_manager
            .as_mut()
            .unwrap()
            .queue_target(Some(TargetKind::Entity(7)), note);
        assert_eq!(reach(&mut index, &mut stores, 7), [1]);
    }

    #[test]
    fn many_entities_naming_one_id_are_reread_on_the_worker_pool() {
        let mut entities = EntityStore::new();
        for id in 1..=600 {
            entities.insert(attacker(id, 1000 + id % 3));
        }
        let mut stores = Stores {
            entities,
            ..Stores::default()
        };
        let mut index = ExpiryListeners::default();
        assert_eq!(reach(&mut index, &mut stores, 1000).len(), 200);
        // Every entity handed out and half of them retargeted: those and the
        // removed ones are re-read, on the worker pool, and the removed ones
        // drop out.
        for id in 1..=600 {
            if id % 2 == 0 {
                retarget(&mut stores.entities, id, 1000);
            }
            stores.entities.get_mut(id).unwrap();
        }
        for id in (5..=600).step_by(5) {
            stores.entities.remove(id);
        }
        for expired in [1000, 1001, 1002, 5] {
            reach(&mut index, &mut stores, expired);
        }
        // Fewer than the store holds, but enough for the worker pool.
        for id in 1..=400 {
            if stores.entities.contains(id) {
                retarget(&mut stores.entities, id, 1001);
            }
        }
        for expired in [1000, 1001, 1002] {
            reach(&mut index, &mut stores, expired);
        }
        // The pool takes a long log as it stands, here descending and then
        // repeating: a listener read once reports what it holds now.
        for (id, target) in (1..=250)
            .rev()
            .map(|id| (id, 1002))
            .chain((1..=120).map(|id| (id, 1000 + id % 3)))
        {
            if stores.entities.contains(id) {
                retarget(&mut stores.entities, id, target);
            }
        }
        for expired in [1000, 1001, 1002] {
            reach(&mut index, &mut stores, expired);
        }
    }

    #[test]
    fn the_index_reaches_the_anims_systems_bullets_and_waves_naming_an_id() {
        let mut stores = Stores::default();
        stores
            .entities
            .insert(GameEntity::test_default(3, "HTNK", "Russians", 14, 14));
        // An anim owned by 3 and one owning nothing; a system on 3 owned by
        // 4; a bullet from 4 at 3 and one from no attacker at a cell; a wave
        // from 3 at 4.
        stores.anims.insert(anim(10));
        stores.anims.set_owner_entity_for_test(10, Some(3));
        stores.anims.insert(anim(11));
        let mut system =
            ParticleSystem::test_fixture(20, ParticleSystemTypeId(0), glam::IVec3::ZERO);
        system.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(3)));
        system.set_owner_entity_for_test(Some(4));
        stores.particle_systems.insert(system);
        bullet(&mut stores, 30, 4, ProjectileTarget::Entity(3));
        bullet(
            &mut stores,
            31,
            RAD_NO_ATTACKER,
            ProjectileTarget::Cell { rx: 1, ry: 1 },
        );
        let origin = ProjectileCoord::new(0, 0, 0);
        let mut wave = Wave::new(0, origin, origin);
        wave.set_owner_id_for_test(Some(3));
        wave.set_target_ref_for_test(Some(TargetKind::Entity(4)));
        stores.waves.spawn(40, wave);
        let mut index = ExpiryListeners::default();

        assert_eq!(reach(&mut index, &mut stores, 3), [3, 10, 20, 30, 40]);
        assert_eq!(reach(&mut index, &mut stores, 4), [20, 30, 40]);
        // A bullet without an attacker names nothing.
        assert_eq!(reach(&mut index, &mut stores, RAD_NO_ATTACKER), [0u64; 0]);
        // A new owner, a retarget, a cleared target and a removal are read
        // back on the next expiry.
        stores.anims.set_owner_entity_for_test(11, Some(4));
        stores
            .projectiles
            .get_mut(30)
            .unwrap()
            .set_target_for_test(ProjectileTarget::Entity(5));
        assert!(stores.particle_systems.pointer_expired(20, 3));
        stores.waves.remove(40);
        assert_eq!(reach(&mut index, &mut stores, 3), [3, 10]);
        assert_eq!(reach(&mut index, &mut stores, 4), [11, 20, 30]);
        assert_eq!(reach(&mut index, &mut stores, 5), [30]);
        // A cloned store and a walk over every bullet make it re-read them.
        stores.anims = stores.anims.clone();
        stores.anims.remove(10);
        for (_, bullet) in stores.projectiles.iter_mut() {
            bullet.set_source_id_for_test(6);
        }
        assert_eq!(reach(&mut index, &mut stores, 3), [3]);
        assert_eq!(reach(&mut index, &mut stores, 6), [30, 31]);
    }

    #[test]
    fn a_reread_changes_only_the_names_that_came_and_left() {
        let mut index = StoreIndex::default();
        let holds = |names: &[u64], every| {
            Some(Holds {
                names: names.to_vec(),
                every,
            })
        };
        let pairs = |index: &StoreIndex| index.named_by.iter().copied().collect::<Vec<_>>();
        index.replace(5, holds(&[10, 20, 30], false));
        index.replace(6, holds(&[20], true));
        assert_eq!(pairs(&index), [(10, 5), (20, 5), (20, 6), (30, 5)]);
        // 10 and 30 leave, 15 and 40 come, 20 stays; the other listener's
        // pair on 20 is not touched.
        index.replace(5, holds(&[15, 20, 40], true));
        assert_eq!(pairs(&index), [(15, 5), (20, 5), (20, 6), (40, 5)]);
        assert_eq!(index.reaching(20).collect::<Vec<_>>(), [5, 6, 5, 6]);
        // The flag alone, then everything.
        index.replace(6, holds(&[20], false));
        assert_eq!(index.every.iter().copied().collect::<Vec<_>>(), [5]);
        index.replace(5, None);
        assert_eq!(pairs(&index), [(20, 6)]);
        assert!(index.every.is_empty() && !index.holds.contains_key(&5));
    }

    #[test]
    fn a_local_anim_is_not_a_pointer_expiry_listener() {
        let mut stores = Stores::default();
        stores
            .entities
            .insert(GameEntity::test_default(3, "HTNK", "Russians", 14, 14));
        // Two anims in the Anim array, so the index re-reads after the first
        // expiry only what the touch log names.
        stores.anims.insert(anim(10));
        stores.anims.insert(anim(11));
        let mut index = ExpiryListeners::default();
        assert_eq!(reach(&mut index, &mut stores, 3), [3]);

        // A presentation anim rides its owner. It is a local anim, outside the
        // Anim array, so the expiry of its owner does not reach it: it ends
        // with the owner's UnInit instead.
        let local = stores.anims.allocate_local_id();
        stores.anims.insert(presentation_anim(local));
        stores.anims.set_owner_entity_for_test(local, Some(3));
        let _ = stores.anims.get_mut(local).expect("the local anim is stored");
        assert_eq!(reach(&mut index, &mut stores, 3), [3]);
    }

    #[test]
    fn awaited_anim_names_share_the_existing_bullet_index_and_rebuilds() {
        let mut stores = Stores::default();
        // An awaited-only holder, and one with all three compared fields
        // aliasing. Membership follows +154 independently of the wait flag.
        bullet(&mut stores, 30, RAD_NO_ATTACKER, ProjectileTarget::None);
        bullet(&mut stores, 31, 7, ProjectileTarget::Entity(7));
        stores.projectiles.await_anim(30, Some(7));
        stores.projectiles.await_anim(31, Some(7));
        let mut index = ExpiryListeners::default();
        assert_eq!(reach(&mut index, &mut stores, 7), [30, 31]);
        assert_eq!(
            index.projectiles_reached_by(&mut stores.projectiles, 7),
            [30, 31]
        );
        assert!(
            index
                .projectiles_reached_by(&mut stores.projectiles, 8)
                .is_empty()
        );
        // Replacement removes the old relation, and an awaited-only clear
        // reports changed. The aliased holder occurs just once.
        stores.projectiles.await_anim(30, Some(8));
        assert_eq!(reach(&mut index, &mut stores, 7), [31]);
        assert_eq!(reach(&mut index, &mut stores, 8), [30]);
        assert_eq!(
            stores
                .projectiles
                .pointer_expired(30, 8, ProjectileTarget::None, false),
            Some((true, false))
        );
        assert!(reach(&mut index, &mut stores, 8).is_empty());
        assert_eq!(
            stores
                .projectiles
                .pointer_expired(31, 7, ProjectileTarget::None, false),
            Some((true, false))
        );
        assert!(reach(&mut index, &mut stores, 7).is_empty());
        stores.projectiles.await_anim(30, Some(9));
        assert_eq!(reach(&mut index, &mut stores, 9), [30]);
        stores.projectiles.remove(30);
        assert!(reach(&mut index, &mut stores, 9).is_empty());
        stores.projectiles.await_anim(31, Some(10));
        let restored: ProjectileStore = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&stores.projectiles).unwrap(),
        )
        .unwrap();
        stores.projectiles.await_anim(31, None);
        assert!(reach(&mut index, &mut stores, 10).is_empty());
        stores.projectiles = restored;
        assert_eq!(reach(&mut index, &mut stores, 10), [31]);
        let cloned = stores.projectiles.clone();
        stores.projectiles.await_anim(31, None);
        assert!(reach(&mut index, &mut stores, 10).is_empty());
        stores.projectiles = cloned;
        assert_eq!(reach(&mut index, &mut stores, 10), [31]);
    }
}
