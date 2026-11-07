//! Ares TechnoTypeExt combat and movement keys, retained by the ordered Rules
//! processor and projected onto `ObjectType`. Sources: the extension
//! constructor `TechnoTypeExt::ExtData::ExtData @ 1003C710`, the first-read
//! `Initialize` in `ExtMap_LoadFromINI @ 1003DEC0` and the reader
//! `LoadFromINIFile @ 1003DFB0` (`src/Ext/TechnoType/Body.cpp`).
//!
//! Type pointers are Find-only Valueable reads (`detail::read<T*>` with
//! `allocate = false`): an absent or unknown name keeps the field, `none` or
//! `<none>` clears it. The caller resolves them against the registry as the
//! read reaches them ([`TypeRef`]).

use crate::rules::ini_parser::IniSection;
use crate::rules::missile_spawn::MissileSpawnParams;

/// The registry a Find-only pointer read looks in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeRef {
    Animation,
    Warhead,
    Weapon,
}

/// A pointer read's result: `None` keeps the field (absent, empty or
/// unknown), `Some(None)` is a blank `none`, `Some(Some(id))` a found type.
pub(crate) type Resolved = Option<Option<String>>;

/// Retained TechnoTypeExt keys of this chain.
#[derive(Debug, Clone, Default, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresTechnoTypeRules {
    /// `Initialize` ran (ExtMap state 1): it seeds once, at the type's first
    /// reached read.
    initialized: bool,
    /// `Gattling.Cycle` (+0x322, false).
    gattling_cyclic: bool,
    /// AircraftType-only custom missile block and its rocket puffs.
    missile: Option<Box<CustomMissileRules>>,
    ammo: AmmoTypeRules,
}

/// TechnoTypeExt+0x458..+0x468, the reload amounts and the out-of-ammo
/// weapon.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AmmoTypeRules {
    /// `ReloadAmount` (+0x458, 1).
    reload_amount: i32,
    /// `EmptyReloadAmount` (Nullable +0x45C/+0x460, unset).
    empty_reload_amount: Option<i32>,
    /// `NoAmmoAmount` (+0x464, 0).
    no_ammo_amount: i32,
    /// `NoAmmoWeapon` (+0x468, -1).
    no_ammo_weapon: i32,
}

impl Default for AmmoTypeRules {
    fn default() -> Self {
        Self {
            reload_amount: 1,
            empty_reload_amount: None,
            no_ammo_amount: 0,
            no_ammo_weapon: -1,
        }
    }
}

impl AmmoTypeRules {
    /// The rounds one `TechnoClass::Reload` adds (TechnoClass_Reload_
    /// ReloadAmount, Ares.dll 10047A70, hook 0x6FB05B): `ReloadAmount`, or
    /// `EmptyReloadAmount` when set and the count is zero.
    pub(crate) fn reload_amount(&self, ammo: i32) -> i32 {
        match self.empty_reload_amount {
            Some(empty) if ammo == 0 => empty,
            _ => self.reload_amount,
        }
    }

    /// The weapon index `What_Weapon_Should_I_Use` answers once the count is
    /// at or below `NoAmmoAmount` (TechnoClass_SelectWeapon_NoAmmoWeapon,
    /// Ares.dll 10047AE0, hook 0x6F3410); the caller tests `Ammo= >= 0`.
    pub(crate) fn no_ammo_weapon(&self, ammo: i32) -> Option<i32> {
        (self.no_ammo_weapon >= 0 && ammo <= self.no_ammo_amount).then_some(self.no_ammo_weapon)
    }
}

/// TechnoTypeExt+0x32D..+0x37C, read for an AircraftType only (`#245`).
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CustomMissileRules {
    /// `Missile.Custom` (+0x32D, false).
    custom: bool,
    /// `Missile.*` RocketStruct (+0x330, zeroed, Type = the owner type).
    /// Its warhead pair holds `Missile.Warhead` (+0x364) and
    /// `Missile.EliteWarhead` (+0x368), both null by default.
    block: MissileSpawnParams,
    /// `Missile.TakeOffAnim` (+0x36C); Initialize seeds `Find("V3TAKOFF")`.
    takeoff_anim: Option<String>,
    /// `Missile.TrailerAnim` (+0x370); Initialize seeds `Find("V3TRAIL")`.
    trailer_anim: Option<String>,
    /// `Missile.TrailerSeparation` (+0x374, 3).
    trailer_separation: i32,
    /// `Missile.Weapon` (+0x378) and `Missile.EliteWeapon` (+0x37C), null.
    weapons: [Option<String>; 2],
}

impl CustomMissileRules {
    fn new(type_name: &str) -> Self {
        Self {
            custom: false,
            block: MissileSpawnParams::ares_custom(type_name),
            takeoff_anim: None,
            trailer_anim: None,
            trailer_separation: 3,
            weapons: [None, None],
        }
    }

    /// The type's own RocketStruct when `Missile.Custom` is set: Process
    /// (hook 0x6622E0, Ares.dll 100610F0), Detonate (0x66305A, 10060FA0),
    /// Move_To (0x6632F2, 100610C0) and Draw_Matrix (0x6634F6, 10061090)
    /// use it in place of the Rules block.
    pub(crate) fn custom_block(&self) -> Option<&MissileSpawnParams> {
        self.custom.then_some(&self.block)
    }

    pub(crate) fn takeoff_anim(&self) -> Option<&str> {
        self.takeoff_anim.as_deref()
    }

    pub(crate) fn trailer_anim(&self) -> Option<&str> {
        self.trailer_anim.as_deref()
    }

    pub(crate) fn trailer_separation(&self) -> i32 {
        self.trailer_separation
    }

    /// `Missile.EliteWeapon` for an elite launcher, else `Missile.Weapon`,
    /// for a custom missile only (Explode_CustomMissile2 10060FF0).
    pub(crate) fn weapon(&self, elite: bool) -> Option<&str> {
        if !self.custom {
            return None;
        }
        self.weapons[usize::from(elite)].as_deref()
    }
}

impl AresTechnoTypeRules {
    /// One reached `LoadFromINIFile` pass. `type_name` is the type's ID,
    /// `aircraft` whether it is an AircraftType, and `resolve` the Find-only
    /// pointer read of a key (called in read order); `find` answers
    /// `Initialize`'s fixed-name `AnimTypeClass::Find`.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        type_name: &str,
        aircraft: bool,
        mut resolve: impl FnMut(&str, TypeRef) -> Resolved,
        find: impl Fn(&str) -> Option<String>,
    ) {
        if !self.initialized {
            self.initialized = true;
            if aircraft {
                let mut missile = CustomMissileRules::new(type_name);
                missile.trailer_anim = find("V3TRAIL");
                missile.takeoff_anim = find("V3TAKOFF");
                self.missile = Some(Box::new(missile));
            }
        }
        self.gattling_cyclic = section.read_ares_bool("Gattling.Cycle", self.gattling_cyclic);
        // 1003DFB0: "#245 custom missiles", after Gattling.Cycle.
        if let Some(missile) = self.missile.as_deref_mut() {
            missile.custom = section.read_ares_bool("Missile.Custom", missile.custom);
            missile.block.read_ares(section, "Missile");
            let mut pointer = |key: &str, kind: TypeRef, field: &mut Option<String>| {
                if let Some(value) = resolve(key, kind) {
                    *field = value;
                }
            };
            let mut warhead = non_empty(&missile.block.warhead);
            pointer("Missile.Warhead", TypeRef::Warhead, &mut warhead);
            let mut elite_warhead = non_empty(&missile.block.elite_warhead);
            pointer("Missile.EliteWarhead", TypeRef::Warhead, &mut elite_warhead);
            missile.block.warhead = warhead.unwrap_or_default();
            missile.block.elite_warhead = elite_warhead.unwrap_or_default();
            pointer(
                "Missile.TakeOffAnim",
                TypeRef::Animation,
                &mut missile.takeoff_anim,
            );
            pointer(
                "Missile.TrailerAnim",
                TypeRef::Animation,
                &mut missile.trailer_anim,
            );
            missile.trailer_separation =
                section.read_ares_int("Missile.TrailerSeparation", missile.trailer_separation);
            let [weapon, elite_weapon] = &mut missile.weapons;
            pointer("Missile.Weapon", TypeRef::Weapon, weapon);
            pointer("Missile.EliteWeapon", TypeRef::Weapon, elite_weapon);
        }
        // After the crush block and ReloadRate.
        let ammo = &mut self.ammo;
        ammo.reload_amount = section.read_ares_int("ReloadAmount", ammo.reload_amount);
        ammo.empty_reload_amount =
            section.read_ares_nullable_int("EmptyReloadAmount", ammo.empty_reload_amount);
        ammo.no_ammo_amount = section.read_ares_int("NoAmmoAmount", ammo.no_ammo_amount);
        ammo.no_ammo_weapon = section.read_ares_int("NoAmmoWeapon", ammo.no_ammo_weapon);
    }

    /// The AircraftType's missile fields; `None` for other types.
    pub(crate) fn missile(&self) -> Option<&CustomMissileRules> {
        self.missile.as_deref()
    }

    pub(crate) fn ammo(&self) -> &AmmoTypeRules {
        &self.ammo
    }

    /// `Gattling.Cycle`: a charge at the cap restarts the stages
    /// (TechnoClass_UpdateGattling_Cycle, Ares.dll 1004B6C0, hook 0x70DEBA).
    pub(crate) fn gattling_cyclic(&self) -> bool {
        self.gattling_cyclic
    }
}

fn non_empty(name: &str) -> Option<String> {
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::RulesLayerStack;
    use crate::rules::ruleset::RuleSet;

    fn rules(extra: &str) -> RuleSet {
        let text = format!(
            "[AircraftTypes]\n0=MYMISL\n1=PLAIN\n[Animations]\n0=V3TRAIL\n1=MYPUFF\n\
             [Warheads]\n0=MYWH\n[MYMISL]\nPrimary=MYGUN\n{extra}\n[PLAIN]\nStrength=1\n\
             [MYGUN]\nDamage=50\nWarhead=MYWH\n"
        );
        let processed = RulesLayerStack::new(IniFile::from_str(&text))
            .process()
            .unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    }

    /// Initialize's Find-only seeds and the 1003C710 defaults: without
    /// `Missile.Custom` the Rules block flies, V3TRAIL is found and an
    /// unregistered V3TAKOFF leaves the takeoff puff null.
    #[test]
    fn aircraft_types_seed_their_puffs_and_keep_the_rules_block() {
        let rules = rules("");
        let ext = rules.rocket_type_ext("PLAIN");
        assert_eq!(ext.trailer_anim, Some("V3TRAIL"));
        assert_eq!(ext.takeoff_anim, None);
        assert_eq!((ext.trailer_separation, ext.weapons), (3, [None, None]));
        assert!(std::ptr::eq(
            rules.rocket_block("PLAIN").0,
            rules.missile_spawn.rocket_block("PLAIN").0,
        ));
        assert_eq!(rules.spawn_missile_wait_frames("PLAIN"), None);
    }

    /// `Missile.Custom` makes the type fly, wait and detonate by its own
    /// zero-seeded block (TechnoTypeExt+0x330) with `Missile.*` pointers
    /// read Find-only: an unknown name keeps the null default.
    #[test]
    fn custom_missile_reads_its_block_warheads_and_weapons() {
        let rules = rules(
            "Missile.Custom=yes\nMissile.PauseFrames=5\nMissile.TiltFrames=40\n\
             Missile.PitchInitial=0.25\nMissile.RaiseRate=2.9\nMissile.Damage=300\n\
             Missile.LazyCurve=yes\nMissile.Warhead=MYWH\nMissile.EliteWarhead=UNKNOWN\n\
             Missile.TrailerAnim=none\nMissile.TakeOffAnim=MYPUFF\n\
             Missile.TrailerSeparation=7\nMissile.EliteWeapon=MYGUN\n",
        );
        let (block, cmisl) = rules.rocket_block("MYMISL");
        assert!(!cmisl);
        assert_eq!(block.type_name, "MYMISL");
        assert_eq!((block.pause_frames, block.tilt_frames), (5, 40));
        assert_eq!(block.pitch_initial.bits(), 0x3E80_0000);
        assert_eq!(
            (block.raise_rate, block.damage, block.elite_damage),
            (2, 300, 0)
        );
        assert_eq!((block.altitude, block.body_length), (0, 0));
        assert!(block.lazy_curve);
        assert_eq!(block.warhead_for(false), "MYWH");
        assert_eq!(block.warhead_for(true), "");
        let ext = rules.rocket_type_ext("MYMISL");
        assert_eq!(ext.trailer_anim, None);
        assert_eq!(ext.takeoff_anim, Some("MYPUFF"));
        assert_eq!(ext.trailer_separation, 7);
        assert_eq!(ext.weapons, [None, Some("MYGUN")]);
        assert_eq!(rules.spawn_missile_wait_frames("MYMISL"), Some(45));
    }
}
