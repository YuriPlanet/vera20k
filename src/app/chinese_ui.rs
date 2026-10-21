//! Simplified Chinese presentation for the branded native clients.
//!
//! Retail archives, CSF identifiers, simulation data and sealed captures stay
//! untouched. Startup and each incoming map project their display string table
//! once; every existing menu, briefing and tactical label then uses that same
//! process-owned table. VERA's own English controls are translated before text
//! measurement, rather than substituting glyphs during drawing.

use std::borrow::Cow;

use crate::assets::csf_file::CsfFile;

pub(crate) const fn enabled(capture: bool) -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )) && !capture
}

pub(crate) fn project(csf: CsfFile, ordinary_ui: bool) -> CsfFile {
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    if enabled(!ordinary_ui) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let transform = apple::Simplifier::new();
        #[cfg(target_os = "android")]
        let transform = portable::Simplifier::new();
        let Some(transform) = transform else {
            log::warn!("Simplified Chinese transform unavailable; retaining retail display text");
            return csf;
        };
        let mut failures = 0;
        let mut values: Vec<_> = csf
            .entries()
            .map(|(key, original)| {
                let text = csf_override(key).map_or_else(
                    || {
                        transform.convert(original).unwrap_or_else(|| {
                            failures += 1;
                            original.to_owned()
                        })
                    },
                    str::to_owned,
                );
                (key.to_owned(), text)
            })
            .collect();
        // This native-dialog key is absent from the installed Traditional
        // table. Supply it only in the application translation layer.
        values.push(("GUI:OptionsInternet".into(), "网络选项".into()));
        let csf = csf.with_display_overrides(values);
        log::info!(
            "Simplified Chinese display table: {} labels, {} retained after transform failure",
            csf.len(),
            failures,
        );
        return csf;
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    let _ = ordinary_ui;
    csf
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
fn csf_override(key: &str) -> Option<&'static str> {
    Some(match key {
        "GUI:NULLMODEM" => "串口直连",
        "TXT_NONAME" => "[新玩家]",
        "TXT_UNKNOWN_PING" => "延迟：?",
        "MSG:PINGINFO" => "延迟 = %d毫秒",
        "WOL:GAMEPING" => "延迟：%d毫秒",
        "GUI:WWBRAND" => "WESTWOOD STUDIOS 是 ELECTRONIC ARTS 的品牌",
        "GUI:TRADEMARKTOP" => "Command & Conquer 和 Yuri's Revenge 是 Electronic Arts Inc.",
        "GUI:TRADEMARKBOTTOM" => "在美国和/或其他国家及地区的商标或注册商标。",
        "TXT_COPYRIGHT" => "© 2000, 2001 ELECTRONIC ARTS INC. 版权所有",
        "DESC:E31" => "演示地图",
        "DESC:E32" => "演示地图 2",
        _ => return None,
    })
}

/// Captions absent from retail CSF, plus the asset-independent recovery cards.
/// Unknown text (including map filenames, player names and diagnostics) is
/// retained verbatim. Callers pass their capture policy before layout.
pub(crate) fn custom_text(text: &str, simplified: bool) -> Cow<'_, str> {
    if !simplified {
        return Cow::Borrowed(text);
    }
    use crate::ui::main_menu_dialogs::options::*;
    let translated = match text {
        ZOOM_LABEL => "画面缩放",
        ZOOM_HELP => "调整战场画面的大小。自动模式会选择适合当前屏幕的比例。",
        SIDEBAR_LABEL => "侧边栏大小",
        SIDEBAR_HELP => "调整战斗中侧边栏和底栏的大小。",
        FULL_SCREEN_LABEL => "全屏显示",
        FULL_SCREEN_HELP => "填满整个屏幕。取消勾选后使用窗口模式。",
        ORIGINAL_SHAPE_LABEL => "原始 4:3 比例",
        ORIGINAL_SHAPE_HELP => "战场保持原始 4:3 比例，宽屏两侧显示黑边。",
        YR_ORIGINAL_STYLE_LABEL => "尤里原版加载与菜单",
        YR_ORIGINAL_STYLE_HELP => "使用《尤里的复仇》原版加载画面和菜单动画。",
        RENDER_INTERPOLATION_LABEL => "画面插值",
        RENDER_INTERPOLATION_HELP => {
            "平滑游戏帧之间的画面运动。增加一帧显示延迟，不改变游戏运行速度。"
        }
        SHIFT_QUEUE_LABEL => "Shift 点击批量生产",
        SHIFT_QUEUE_HELP => "按住 Shift 点击步兵、战车或飞机图标，可一次加入多个生产任务。",
        DEVELOPER_OPTIONS_LABEL => "开发者选项",
        VERA_OPTIONS_LABEL => "王二火大选项",
        VERA_OPTIONS_HELP => "调整全屏显示、侧边栏大小、画面插值等扩展选项。",
        FPS_COUNTER_LABEL => "帧率显示",
        FPS_COUNTER_HELP => "在左上角显示每秒帧数。每次启动时关闭，也可按 Ctrl+Alt+F 切换。",
        WOL_TEST_SERVICE_LABEL => "离线互联网测试服务",
        WOL_TEST_SERVICE_HELP => "使用模拟服务器、大厅和玩家，测试互联网界面。",
        "Normal" => "标准",
        "Large" => "大型",
        "XL" => "超大型",
        "Quit" => "退出",
        "Back To Menu" => "返回主菜单",
        "Resume Mission" => "继续任务",
        "Abort Mission" => "放弃任务",
        "Unable to load the game menu" => "无法加载游戏菜单",
        "Leave this mission?" => "离开当前任务？",
        "Menu resources unavailable" => "菜单资源不可用",
        "Loading..." => "正在加载……",
        "Loading Failed" => "加载失败",
        "Required retail dialog artwork could not be loaded. Check your game installation. Full details are in logs/ra2.log." => {
            "无法加载菜单所需的游戏美术资源，请检查游戏文件。详细信息请查看 logs/ra2.log。"
        }
        _ => {
            if let Some(size) = text
                .strip_prefix("Auto (")
                .and_then(|s| s.strip_suffix(')'))
            {
                return Cow::Owned(format!("自动 ({size})"));
            }
            if let Some(map) = text.strip_prefix("Map: ") {
                return Cow::Owned(format!(
                    "地图：{}",
                    if map == "auto" { "自动" } else { map }
                ));
            }
            return Cow::Borrowed(text);
        }
    };
    Cow::Borrowed(translated)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple {
    use std::{ffi::c_void, ptr};

    type Ref = *const c_void;

    #[repr(C)]
    struct Range {
        location: isize,
        length: isize,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: Ref);
        fn CFStringCreateWithCharacters(allocator: Ref, chars: *const u16, count: isize) -> Ref;
        fn CFStringCreateMutableCopy(allocator: Ref, capacity: isize, value: Ref) -> Ref;
        fn CFStringTransform(value: Ref, range: *mut Range, transform: Ref, reverse: u8) -> u8;
        fn CFStringGetLength(value: Ref) -> isize;
        fn CFStringGetCharacters(value: Ref, range: Range, buffer: *mut u16);
    }

    struct Owned(Ref);

    impl Owned {
        fn new(value: Ref) -> Option<Self> {
            (!value.is_null()).then(|| Self(value))
        }

        fn text(text: &str) -> Option<Self> {
            let units: Vec<_> = text.encode_utf16().collect();
            // SAFETY: CoreFoundation copies the live UTF-16 slice.
            Self::new(unsafe {
                CFStringCreateWithCharacters(ptr::null(), units.as_ptr(), units.len() as isize)
            })
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns one successful Create result.
            unsafe { CFRelease(self.0) };
        }
    }

    pub(super) struct Simplifier(Owned);

    impl Simplifier {
        pub(super) fn new() -> Option<Self> {
            Some(Self(Owned::text("Traditional-Simplified")?))
        }

        pub(super) fn convert(&self, text: &str) -> Option<String> {
            if text.is_ascii() {
                return Some(text.to_owned());
            }
            let original = Owned::text(text)?;
            // Public CFStringTransform accepts ICU transform identifiers:
            // https://developer.apple.com/documentation/corefoundation/cfstringtransform(_:_:_:_:)
            // SAFETY: CreateMutableCopy owns its copy. A null range transforms
            // the complete string, forward; every CF object lives through it.
            let result = unsafe {
                let result = Owned::new(CFStringCreateMutableCopy(ptr::null(), 0, original.0))?;
                if CFStringTransform(result.0, ptr::null_mut(), self.0.0, 0) == 0 {
                    return None;
                }
                let length = CFStringGetLength(result.0);
                let mut units = vec![0; usize::try_from(length).ok()?];
                CFStringGetCharacters(
                    result.0,
                    Range {
                        location: 0,
                        length,
                    },
                    units.as_mut_ptr(),
                );
                String::from_utf16(&units).ok()?
            };
            // The installed table has two uncommon spellings whose ICU result
            // exceeds GAME.FNT's cells (including one supplementary character).
            // Use normal Simplified spellings and an available space glyph.
            Some(super::normalize_simplified(result))
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android", test))]
fn normalize_simplified(text: String) -> String {
    text.replace("猪㑩", "猪猡")
        .replace("𫫇心", "恶心")
        .replace('\u{3000}', " ")
}

/// Generated offline from public CoreFoundation's transform. The small
/// character table also handles new display strings; contextual exceptions
/// reproduce the existing retail corpus exactly, without Android ICU/JNI.
#[cfg(any(target_os = "android", test))]
mod portable {
    use std::collections::HashMap;

    #[derive(serde::Deserialize)]
    pub(super) struct Simplifier {
        characters: HashMap<char, String>,
        contexts: HashMap<String, String>,
    }

    impl Simplifier {
        pub(super) fn new() -> Option<Self> {
            serde_json::from_str(include_str!(
                "../../art/localization/zh-hans-transform.json"
            ))
            .ok()
        }

        pub(super) fn convert(&self, text: &str) -> Option<String> {
            if let Some(result) = self.contexts.get(text) {
                return Some(result.clone());
            }
            let mut result = String::with_capacity(text.len());
            for ch in text.chars() {
                if let Some(replacement) = self.characters.get(&ch) {
                    result.push_str(replacement);
                } else {
                    result.push(ch);
                }
            }
            Some(super::normalize_simplified(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_android_translation_preserves_formats_and_common_chinese() {
        let transform = portable::Simplifier::new().unwrap();
        assert_eq!(
            transform.convert("單人遊戲 遭遇戰 載入儲存遊戲"),
            Some("单人游戏 遭遇战 载入储存游戏".into())
        );
        assert_eq!(
            transform.convert("玩家 %s 擁有 %02d 部戰車。"),
            Some("玩家 %s 拥有 %02d 部战车。".into())
        );
        assert_eq!(
            transform.convert("/reply PlayerAlice"),
            Some("/reply PlayerAlice".into())
        );
        assert_eq!(
            transform.convert("豬儸令人噁心。\u{3000}"),
            Some("猪猡令人恶心。 ".into())
        );
    }

    #[test]
    fn custom_controls_translate_before_layout_and_keep_captures_verbatim() {
        use crate::ui::main_menu_dialogs::options::*;
        for label in [
            ZOOM_LABEL,
            SIDEBAR_LABEL,
            FULL_SCREEN_LABEL,
            ORIGINAL_SHAPE_LABEL,
            YR_ORIGINAL_STYLE_LABEL,
            RENDER_INTERPOLATION_LABEL,
            SHIFT_QUEUE_LABEL,
            DEVELOPER_OPTIONS_LABEL,
            VERA_OPTIONS_LABEL,
            VERA_OPTIONS_HELP,
            FPS_COUNTER_LABEL,
            FPS_COUNTER_HELP,
            WOL_TEST_SERVICE_LABEL,
            ZOOM_HELP,
            SIDEBAR_HELP,
            FULL_SCREEN_HELP,
            ORIGINAL_SHAPE_HELP,
            YR_ORIGINAL_STYLE_HELP,
            RENDER_INTERPOLATION_HELP,
            SHIFT_QUEUE_HELP,
            WOL_TEST_SERVICE_HELP,
            "Normal",
            "Large",
            "XL",
            "Quit",
            "Back To Menu",
        ] {
            assert_eq!(custom_text(label, false), label);
            assert!(
                custom_text(label, true)
                    .chars()
                    .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)),
                "{label}"
            );
        }
        assert_eq!(custom_text("Auto (1920x1080)", true), "自动 (1920x1080)");
        assert_eq!(custom_text("150% (1280x720)", true), "150% (1280x720)");
        assert_eq!(custom_text("Player Alice", true), "Player Alice");
        assert!(!enabled(true));
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn localized_copy_keeps_identifiers_formats_and_source_table() {
        let retail = crate::assets::csf_file::test_csf(&[
            ("GUI:SinglePlayer", "單人遊戲"),
            ("NAME:MTNK", "灰熊坦克"),
            ("BRIEF:TEST", "消滅敵軍，建立基地。\n佔領發電廠。"),
            ("TXT_FORMAT", "玩家 %s 擁有 %02d 部戰車。"),
            ("TXT_REPLY_COMMAND", "/reply"),
            ("TXT_NONAME", "[New Player]"),
            ("RARE", "資本主義的豬儸真令人噁心。\u{3000}"),
        ]);
        let localized = project(retail.clone(), true);
        let portable = portable::Simplifier::new().unwrap();
        let apple = apple::Simplifier::new().unwrap();
        for (key, text) in retail.entries() {
            assert_eq!(
                portable.convert(text),
                apple.convert(text),
                "Android locale differs at {key}"
            );
        }
        assert_eq!(retail.get("GUI:SinglePlayer"), Some("單人遊戲"));
        assert_eq!(
            project(retail.clone(), false).get("GUI:SinglePlayer"),
            Some("單人遊戲")
        );
        assert_eq!(localized.get("gui:singleplayer"), Some("单人游戏"));
        assert_eq!(
            localized.get("brief:test"),
            Some("消灭敌军，建立基地。\n占领发电厂。")
        );
        assert_eq!(
            localized.get("TXT_FORMAT"),
            Some("玩家 %s 拥有 %02d 部战车。")
        );
        assert_eq!(localized.get("TXT_REPLY_COMMAND"), Some("/reply"));
        assert_eq!(
            crate::assets::csf_file::format_csf(
                localized.get("TXT_FORMAT").unwrap(),
                &[
                    crate::assets::csf_file::CsfArg::Str("Alice"),
                    crate::assets::csf_file::CsfArg::Int(3),
                ],
            ),
            "玩家 Alice 拥有 03 部战车。",
        );
        assert_eq!(localized.get("TXT_NONAME"), Some("[新玩家]"));
        assert_eq!(localized.get("RARE"), Some("资本主义的猪猡真令人恶心。 "));
        assert_eq!(localized.version, retail.version);
        assert_eq!(localized.language, retail.language);
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    #[ignore = "requires installed RA2_DIR language archives and GAME.FNT"]
    fn real_simplified_table_has_font_cells() {
        use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
        let root = std::env::var("RA2_DIR").expect("RA2_DIR");
        let assets =
            AssetManager::new(std::path::Path::new(&root), MediaArchiveMode::STOCK_DIGITAL)
                .unwrap();
        let retail = crate::app::loading::init::load_csf(&assets).unwrap();
        let localized = project(retail.clone(), true);
        let portable = portable::Simplifier::new().unwrap();
        let apple = apple::Simplifier::new().unwrap();
        for (key, text) in retail.entries() {
            assert_eq!(
                portable.convert(text),
                apple.convert(text),
                "Android locale differs at {key}"
            );
        }
        let font =
            crate::assets::fnt_file::FntFile::from_bytes(assets.get_ref("GAME.FNT").unwrap())
                .unwrap();
        assert_eq!(localized.get("GUI:SinglePlayer"), Some("单人游戏"));
        assert_eq!(localized.get("GUI:OptionsInternet"), Some("网络选项"));
        assert_eq!(
            localized.len(),
            retail.len() + usize::from(retail.get("GUI:OptionsInternet").is_none())
        );
        for (key, text) in localized.entries() {
            for ch in text.chars().filter(|ch| !ch.is_control()) {
                assert!(
                    u16::try_from(ch as u32)
                        .ok()
                        .and_then(|cp| font.glyph(cp))
                        .is_some(),
                    "missing {ch} ({:X}) in {key}",
                    ch as u32
                );
            }
        }
    }
}
