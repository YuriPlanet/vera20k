//! VERA-only: the multiplayer rules agreement in parts, so a launcher or a
//! lobby can say which file or setting two players' games disagree on
//! (docs/spawn.md, "Rules parts").
//!
//! `ProcessAssets::multiplayer_rules_parts` builds it from the sources
//! `ProcessAssets::multiplayer_rules_digest` digests, one file part per
//! component in that order. The parts never replace the digest: two games
//! play together when their `rules_digest`s agree, and the parts only
//! explain a difference. This module owns the object's shape: its canonical
//! form and digest, and the size cap.

use serde::Serialize;

/// The object's form; a later form gets another number.
pub(crate) const VERSION: u8 = 1;
/// The most bytes the serialized object takes: lists are cut to fit.
pub(crate) const MAX_BYTES: usize = 8192;
/// The most names one list holds before the cap needs fewer.
const LISTED: usize = 20;
/// The most characters a free-text string keeps when the lists alone do not
/// bring the object under the cap.
const TEXT: usize = 64;

/// This game's rules agreement in parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RulesParts {
    v: u8,
    /// The first 16 hex digits of the SHA-256 of the canonical form: this
    /// object serialized without `digest`, fields in declaration order.
    /// Empty only while that form is serialized.
    #[serde(skip_serializing_if = "String::is_empty")]
    digest: String,
    files: Vec<RulesFile>,
    settings: RulesSettings,
    mod_check: Option<ModCheck>,
}

/// One component of the rules digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RulesFile {
    /// `rules`, `langrule`, `art`, `ai`, `mode`, `theater:<ini name>`,
    /// `tiles`, `mod_files` or `map_additions`.
    pub(crate) id: String,
    /// The file name players recognise, never a path.
    pub(crate) name: String,
    /// The first 16 hex digits of the component's digest, or `absent`.
    pub(crate) digest: String,
    /// `retail`, `loose`, `mix:<archive>`, `mod` or `unknown`.
    pub(crate) source: String,
    /// Differs from the retail copy.
    pub(crate) changed: bool,
    /// A changed INI's sections that differ from the retail copy, sorted;
    /// with [`Self::sections_more`], which counts the ones left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sections: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sections_more: Option<usize>,
}

/// What the game plays besides its files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RulesSettings {
    pub(crate) terrain_deformation: bool,
    pub(crate) superweapon_limit: u8,
    /// The Building Production Queue switch. Left out while off, so a game
    /// without it has the parts it had before the switch was named here.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) building_production_queue: bool,
    /// The Ares setting where it decides which game this is, as the digest
    /// counts it (`ProcessAssets::agreed_ares_setting`): `on`, `off`, or
    /// `auto` for Auto and for a setting that opens the game Auto opens.
    pub(crate) ares: &'static str,
    /// `ares.mix` is mounted.
    pub(crate) ares_mix: bool,
    /// The INI roots listed under `resources` and the theater suffix are all
    /// YR's (`ProcessAssets::stock_ini_roots`).
    pub(crate) ini_roots_stock: bool,
    /// RA2 Mode is on (docs/ra2-mode.md). Left out while off, so a game
    /// without it has the parts it had before the setting was named here.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) ra2: bool,
    /// The INI roots and archive names players' games compare, when a mod
    /// requires Ares or any of them is not YR's; absent otherwise. The rules
    /// digest compares them then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resources: Option<ResourceSelection>,
    #[serde(rename = "mod")]
    pub(crate) mod_in_use: Option<ModIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ModIdentity {
    pub(crate) title: String,
    /// `mod.ini`'s `Version=`; empty without one.
    pub(crate) version: String,
}

/// The resources a game selects (`ProcessAssets::resource_selection`), each
/// name lowercased.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ResourceSelection {
    /// The selected mod's `mod.ini` requires Ares.
    pub(crate) requires_ares: bool,
    /// The Rules, Art, AI, mode and map catalog files, in that order
    /// (`IniRoots::agreed_files`).
    pub(crate) roots: Vec<String>,
    /// The expansion and map archive prefixes, the multiplayer and theme
    /// archives and the movie archive prefix, in that order.
    pub(crate) archives: Vec<String>,
}

/// Where the mod's files part from its maker's `mod-files.txt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ModCheck {
    pub(crate) changed: Vec<String>,
    pub(crate) missing: Vec<String>,
    pub(crate) extra: Vec<String>,
}

/// The first 16 hex digits of a component's digest, or `absent`.
pub(crate) fn short_digest(digest: Option<&str>) -> String {
    digest.map_or_else(
        || "absent".to_owned(),
        |digest| digest.get(..16).unwrap_or(digest).to_owned(),
    )
}

impl RulesFile {
    /// A changed INI's differing sections: the first [`LISTED`] and how many
    /// more.
    pub(crate) fn set_sections(&mut self, mut sections: Vec<String>) {
        let more = sections.len().saturating_sub(LISTED);
        sections.truncate(LISTED);
        self.sections = Some(sections);
        self.sections_more = Some(more);
    }
}

impl RulesParts {
    pub(crate) fn new(
        files: Vec<RulesFile>,
        settings: RulesSettings,
        mod_check: Option<ModCheck>,
    ) -> Self {
        let mut parts = Self {
            v: VERSION,
            digest: String::new(),
            files,
            settings,
            mod_check,
        };
        // The cap cuts lists first, shorter each time, till the object fits;
        // then the free text (names, the mod's title and version, the selected
        // resources), and last the files from the end. The digest is always the emitted object's.
        for limit in [LISTED, 10, 5, 2, 0] {
            parts.limit_lists(limit);
            if parts.fits() {
                return parts;
            }
        }
        parts.shorten_text();
        while !parts.fits() && parts.files.pop().is_some() {}
        parts
    }

    /// Digest the object as it stands and say whether it is under the cap.
    fn fits(&mut self) -> bool {
        self.digest = self.canonical_digest();
        self.json().len() <= MAX_BYTES
    }

    /// Cut each free-text string to [`TEXT`] characters.
    fn shorten_text(&mut self) {
        let shorten = |text: &mut String| {
            if let Some((cut, _)) = text.char_indices().nth(TEXT) {
                text.truncate(cut);
                text.push('…');
            }
        };
        for file in &mut self.files {
            shorten(&mut file.name);
            shorten(&mut file.source);
        }
        if let Some(identity) = &mut self.settings.mod_in_use {
            shorten(&mut identity.title);
            shorten(&mut identity.version);
        }
        if let Some(selection) = &mut self.settings.resources {
            selection
                .roots
                .iter_mut()
                .chain(&mut selection.archives)
                .for_each(shorten);
        }
    }

    /// The object as one line of JSON.
    pub(crate) fn json(&self) -> String {
        serde_json::to_string(self).expect("rules parts serialize")
    }

    /// Parts of a game with no file and nothing switched on.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::new(
            Vec::new(),
            RulesSettings {
                terrain_deformation: false,
                superweapon_limit: 0,
                building_production_queue: false,
                ares: "auto",
                ares_mix: false,
                ini_roots_stock: true,
                ra2: false,
                resources: None,
                mod_in_use: None,
            },
            None,
        )
    }

    fn canonical_digest(&self) -> String {
        let canonical = Self {
            digest: String::new(),
            ..self.clone()
        };
        short_digest(Some(&crate::util::sha256::sha256_hex(
            canonical.json().as_bytes(),
        )))
    }

    fn limit_lists(&mut self, limit: usize) {
        for file in &mut self.files {
            if let (Some(sections), Some(more)) = (&mut file.sections, &mut file.sections_more) {
                *more += sections.len().saturating_sub(limit);
                sections.truncate(limit);
            }
        }
        if let Some(check) = &mut self.mod_check {
            for list in [&mut check.changed, &mut check.missing, &mut check.extra] {
                list.truncate(limit);
            }
        }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    fn settings() -> RulesSettings {
        RulesParts::for_test().settings
    }

    fn file(id: &str, changed: bool) -> RulesFile {
        RulesFile {
            id: id.into(),
            name: format!("{id}md.ini"),
            digest: "0123456789abcdef".into(),
            source: if changed { "loose" } else { "retail" }.into(),
            changed,
            sections: None,
            sections_more: None,
        }
    }

    /// The digest is the canonical form's, which the contract fixes byte for
    /// byte: other platforms and the browser give the same.
    #[test]
    fn the_digest_hashes_the_canonical_form_without_itself() {
        let parts = RulesParts::new(vec![file("rules", false)], settings(), None);
        let canonical = r#"{"v":1,"files":[{"id":"rules","name":"rulesmd.ini","digest":"0123456789abcdef","source":"retail","changed":false}],"settings":{"terrain_deformation":false,"superweapon_limit":0,"ares":"auto","ares_mix":false,"ini_roots_stock":true,"mod":null},"mod_check":null}"#;
        assert_eq!(
            parts.digest,
            &crate::util::sha256::sha256_hex(canonical.as_bytes())[..16]
        );
        assert_eq!(
            parts.json(),
            canonical.replacen(
                r#""v":1,"#,
                &format!(r#""v":1,"digest":"{}","#, parts.digest),
                1
            )
        );
        let mut deformation = settings();
        deformation.terrain_deformation = true;
        assert_ne!(
            RulesParts::new(vec![file("rules", false)], deformation, None).digest,
            parts.digest
        );
        let mut queue = settings();
        queue.building_production_queue = true;
        let queue = RulesParts::new(vec![file("rules", false)], queue, None);
        assert_ne!(queue.digest, parts.digest);
        assert!(queue.json().contains(r#""superweapon_limit":0,"building_production_queue":true,"#));
        let mut ra2 = settings();
        ra2.ra2 = true;
        let ra2 = RulesParts::new(vec![file("rules", false)], ra2, None);
        assert_ne!(ra2.digest, parts.digest);
        assert!(ra2.json().contains(r#""ini_roots_stock":true,"ra2":true,"#));
    }

    /// Each of the player's own rule switches (`PersonalRules`, which the
    /// rules digest covers through `compatibility_crc`) has its setting
    /// here by the same name, so a launcher can name the one two games
    /// differ on (AGENTS.md, "Multiplayer rules").
    #[test]
    fn every_personal_rules_switch_is_a_setting() {
        let mut all = settings();
        all.building_production_queue = true;
        let settings = serde_json::to_value(&all).unwrap();
        let personal = serde_json::to_value(
            crate::rules::personal_rules::PersonalRules::default(),
        )
        .unwrap();
        for key in personal.as_object().unwrap().keys() {
            assert!(
                settings.get(key).is_some(),
                "PersonalRules::{key} changes the rules digest but is not in RulesSettings"
            );
        }
    }

    /// A changed INI lists twenty sections and counts the rest; the cap cuts
    /// the lists further, sections and the mod's alike, till the object fits.
    #[test]
    fn lists_are_cut_to_twenty_names_and_then_to_the_size_cap() {
        let mut changed = file("art", true);
        changed.set_sections((0..25).map(|index| format!("Section{index:02}")).collect());
        let parts = RulesParts::new(vec![changed.clone()], settings(), None);
        let value: serde_json::Value = serde_json::from_str(&parts.json()).unwrap();
        assert_eq!(value["files"][0]["sections"].as_array().unwrap().len(), 20);
        assert_eq!(value["files"][0]["sections"][19], "Section19");
        assert_eq!(value["files"][0]["sections_more"], 5);

        let long = |prefix: &str| -> Vec<String> {
            (0..20)
                .map(|index| format!("{prefix}/{index}/{}", "x".repeat(150)))
                .collect()
        };
        changed.set_sections(long("Section"));
        let check = ModCheck {
            changed: long("Art"),
            missing: long("Maps"),
            extra: long("Notes"),
        };
        let parts = RulesParts::new(vec![changed], settings(), Some(check));
        let json = parts.json();
        assert!(json.len() <= MAX_BYTES, "{}", json.len());
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let sections = value["files"][0]["sections"].as_array().unwrap().len();
        assert!(sections < 20);
        assert_eq!(value["files"][0]["sections_more"], 20 - sections);
        assert_eq!(
            value["mod_check"]["changed"].as_array().unwrap().len(),
            sections
        );
    }

    /// With every list empty and still too big, the free text is cut and then
    /// the last files dropped: the cap holds, and the digest is the emitted
    /// object's.
    #[test]
    fn the_cap_holds_past_the_lists_with_the_digest_of_what_is_emitted() {
        let long = |name: &str| format!("{name}{}", "x".repeat(3000));
        let mut settings = settings();
        settings.mod_in_use = Some(ModIdentity {
            title: long("Title"),
            version: long("1."),
        });
        let mut additions = file("map_additions", true);
        additions.name = long("Additions");
        let parts = RulesParts::new(vec![additions], settings.clone(), None);
        let value: serde_json::Value = serde_json::from_str(&parts.json()).unwrap();
        assert_eq!(value["files"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["files"][0]["name"].as_str().unwrap().chars().count(),
            TEXT + 1
        );
        assert_eq!(
            value["settings"]["mod"]["title"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            TEXT + 1
        );

        let files: Vec<RulesFile> = (0..200)
            .map(|index| file(&format!("theater:{index}"), false))
            .collect();
        let parts = RulesParts::new(files, settings, None);
        let json = parts.json();
        assert!(json.len() <= MAX_BYTES, "{}", json.len());
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value["files"].as_array().unwrap().len() < 200);
        let canonical = json.replacen(&format!(r#""digest":"{}","#, parts.digest), "", 1);
        assert_eq!(
            parts.digest,
            &crate::util::sha256::sha256_hex(canonical.as_bytes())[..16]
        );
    }
}
