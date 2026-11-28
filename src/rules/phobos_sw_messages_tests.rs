//! Reader controls for Phobos `Message.Activated.*`/`EVA.Activated.*` and the
//! linked pair (`Ext/SWType/Body.cpp:241-250`, defaults `Ext/SWType/Body.h:119-126`,
//! `:208-215`), plus the `TechnoTypeExt::EVA.Sold` (`:844`) shape.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{
    NativeRulesRegistryState, RulesLayerKind, RulesLayerStack, RulesProcessContext,
};
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};

/// The `[DialogList]` catalog the references resolve against. Two of the three
/// names exist so the unknown-name retention can be told apart from a
/// resolved one.
const CATALOG: &str = "[DialogList]\n0=EVA_UnitSold\n1=EVA_StructureSold\n2=EVA_LinkedSW\n";

fn read(section_ini: &str) -> (PhobosSuperWeaponMessages, EvaRegistry) {
    let ini = IniFile::from_str(&format!("{CATALOG}{section_ini}"));
    let eva = EvaRegistry::from_ini(&ini);
    let section = ini.section_or_empty("TEST");
    (PhobosSuperWeaponMessages::from_section(section, &eva), eva)
}

#[test]
fn absent_keys_keep_the_constructor_defaults() {
    let (messages, _) = read("[TEST]\nName=Other\n");
    for audience in [
        PhobosActivatedAudience::Owner,
        PhobosActivatedAudience::Allies,
        PhobosActivatedAudience::Enemies,
    ] {
        assert_eq!(messages.activated_message(audience), "");
        assert_eq!(messages.activated_eva(audience), None);
    }
    assert_eq!(messages.linked_acquired_message(), "");
    assert_eq!(messages.linked_acquired_eva(), None);
}

#[test]
fn the_ordered_pass_reads_all_eight_keys() {
    let (messages, _) = read(
        "[TEST]\n\
         Message.LinkedSWAcquired=Msg:Linked\n\
         EVA.LinkedSWAcquired=EVA_UnitSold\n\
         Message.Activated.Owner=Msg:Own\n\
         Message.Activated.Allies=Msg:Ally\n\
         Message.Activated.Enemies=Msg:Enemy\n\
         EVA.Activated.Owner=EVA_LinkedSW\n\
         EVA.Activated.Allies=EVA_StructureSold\n\
         EVA.Activated.Enemies=EVA_UnitSold\n",
    );
    assert_eq!(messages.linked_acquired_message(), "Msg:Linked");
    assert_eq!(messages.linked_acquired_eva(), Some("EVA_UnitSold"));
    assert_eq!(
        messages.activated_message(PhobosActivatedAudience::Owner),
        "Msg:Own"
    );
    assert_eq!(
        messages.activated_message(PhobosActivatedAudience::Allies),
        "Msg:Ally"
    );
    assert_eq!(
        messages.activated_message(PhobosActivatedAudience::Enemies),
        "Msg:Enemy"
    );
    assert_eq!(
        messages.activated_eva(PhobosActivatedAudience::Owner),
        Some("EVA_LinkedSW")
    );
    assert_eq!(
        messages.activated_eva(PhobosActivatedAudience::Allies),
        Some("EVA_StructureSold")
    );
    assert_eq!(
        messages.activated_eva(PhobosActivatedAudience::Enemies),
        Some("EVA_UnitSold")
    );
}

/// `Valueable<CSFText>` clears its label on a present-but-blank read;
/// `ValueableIdx`/`NullableIdx` store native `-1` there, which is silence for
/// both. An unresolvable name is rejected, so the constructor `-1` stands.
#[test]
fn blank_and_unknown_references_keep_native_silence() {
    let (messages, _) = read(
        "[TEST]\n\
         Message.Activated.Owner=\n\
         Message.LinkedSWAcquired=\n\
         EVA.Activated.Owner=\n\
         EVA.Activated.Allies=EVA_NoSuchEvent\n\
         EVA.LinkedSWAcquired=EVA_NoSuchEvent\n",
    );
    assert_eq!(
        messages.activated_message(PhobosActivatedAudience::Owner),
        ""
    );
    assert_eq!(messages.linked_acquired_message(), "");
    assert_eq!(messages.activated_eva(PhobosActivatedAudience::Owner), None);
    // The unknown name leaves the field at the constructor's -1.
    assert_eq!(
        messages.activated_eva(PhobosActivatedAudience::Allies),
        None
    );
    assert_eq!(messages.linked_acquired_eva(), None);
}

/// `CSFText` stores `FixedString<0x20>`: the label keeps at most 31 bytes.
#[test]
fn labels_are_cut_to_the_native_fixed_string() {
    let long = "L".repeat(40);
    let (messages, _) = read(&format!("[TEST]\nMessage.Activated.Enemies={long}\n"));
    assert_eq!(
        messages.activated_message(PhobosActivatedAudience::Enemies),
        "L".repeat(31)
    );
}

/// `TechnoTypeExt::LoadFromINIFile`'s `EVA.Sold` (`:844`,
/// `NullableIdx<VoxClass>`): an absent key keeps the ctor's unset state and a
/// name is retained for `RuleSet::bind_type_eva_references`, which resolves
/// it against the catalog. The text parser drops `Key=` (gamemd's parser
/// shape), so a file-authored blank coincides with absent; the reader's
/// explicit-none arm serves retained projections that keep empty values.
#[test]
fn techno_eva_sold_reads_named_and_treats_blank_as_absent() {
    let ini = IniFile::from_str(&format!(
        "{CATALOG}\
         [ABSENT]\nStrength=1\n\
         [BLANK]\nEVA.Sold=\n\
         [NAMED]\nEVA.Sold=EVA_StructureSold\n"
    ));
    let object = |id: &str| {
        crate::rules::object_type::ObjectType::from_ini_section(
            id,
            ini.section_or_empty(id),
            crate::rules::object_type::ObjectCategory::Vehicle,
        )
    };
    assert_eq!(object("ABSENT").phobos_eva_sold(), None);
    assert_eq!(object("BLANK").phobos_eva_sold(), None);
    assert_eq!(
        object("NAMED").phobos_eva_sold(),
        Some(Some("EVA_StructureSold"))
    );
}

/// A later rules pass that re-declares the superweapon section without the
/// announcement keys keeps the root pass's labels: `Valueable`/
/// `ValueableIdx` write only on a present key
/// (`Ext/SWType/Body.cpp:241-250`).
#[test]
fn a_later_pass_keeps_announcement_keys_absent_from_its_section() {
    let eva = EvaRegistry::from_ini(&IniFile::from_str(CATALOG));
    let mut stack = RulesLayerStack::new(IniFile::from_str(
        "[SuperWeaponTypes]\n0=RootSpecial\n\
         [RootSpecial]\nType=PsychicReveal\n\
         Message.Activated.Enemies=Msg:Enemy\n\
         EVA.Activated.Enemies=EVA_UnitSold\n",
    ));
    stack.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[RootSpecial]\nRechargeTime=5\n"),
    );
    let processed = stack
        .process_with_fixed_art_and_registry_state_recovering(
            &IniFile::empty(),
            NativeRulesRegistryState::default(),
            RulesProcessContext::Ordinary,
            &eva,
            &SoundRegistry::default(),
        )
        .expect("rules parse");
    let rules = RuleSet::from_processed_rules(&processed).expect("rules project");
    let messages = rules
        .phobos_super_weapon_messages("RootSpecial")
        .expect("the type keeps its message entry");
    assert_eq!(
        messages.activated_message(PhobosActivatedAudience::Enemies),
        "Msg:Enemy"
    );
    assert_eq!(
        messages.activated_eva(PhobosActivatedAudience::Enemies),
        Some("EVA_UnitSold")
    );
}
