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
    /// The Ares setting: `auto`, `on` or `off`.
    pub(crate) ares: &'static str,
    /// `ares.mix` is mounted.
    pub(crate) ares_mix: bool,
    /// config.toml's `[ini]` roots and theater suffix are YR's.
    pub(crate) ini_roots_stock: bool,
    #[serde(rename = "mod")]
    pub(crate) mod_in_use: Option<ModIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ModIdentity {
    pub(crate) title: String,
    /// `mod.ini`'s `Version=`; empty without one.
    pub(crate) version: String,
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
        // The cap cuts lists first, shorter each time, till the object fits.
        for limit in [LISTED, 10, 5, 2, 0] {
            parts.limit_lists(limit);
            parts.digest = parts.canonical_digest();
            if parts.json().len() <= MAX_BYTES {
                break;
            }
        }
        parts
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
                ares: "auto",
                ares_mix: false,
                ini_roots_stock: true,
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
}
