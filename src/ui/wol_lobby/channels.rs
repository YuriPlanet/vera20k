//! The custom lobby `0x113`'s channel and user records and the native rules
//! that read them: the game list `0x00781480` (with its filter `0x00781130`
//! and ping comparators `0x00781380`/`0x007813B0`), the chat-lobby list
//! `0x007806C0`, the player list `0x00783C80` with the membership inserts
//! `0x00781FB0`/`0x00782320`/`0x00782BB0`, the sort buttons, admission
//! `0x0079BA60`, the resolution limit of `0x00789960`, the `" ,;"` password
//! gate of `0xC3`/`0xE1` and the chat-lobby picker `0x00788180`.
//!
//! Records are the service's decoded channel/user data, never a transport
//! packet. None of these paths draws RNG.

use crate::rules::ini_value::{crt_atoi, strtok};
use crate::util::native_string::crt_strcmpi;
use std::cmp::Ordering;

/// Channel type of a Yuri's Revenge game channel (`+0x00`); chat lobbies
/// are type 0.
pub(crate) const GAME_CHANNEL: u32 = 0x29;
/// `0x0084A214`: every chat lobby's name starts with this.
pub(crate) const LOBBY_PREFIX: &str = "Lob_41_";
/// `0x00788A30` joins a chat lobby with this key.
pub(crate) const LOBBY_KEY: &str = "zotclot9";
/// `0x00777B50`: the exInfo version (`[0x822D1C]`).
pub(crate) const EX_INFO_VERSION: i32 = 25;
/// `0x00788180` skips lobbies with at least this many users (`cmp 0x19`).
const LOBBY_CROWDED: i32 = 0x1A;

/// `+0x1C` bit: the channel has a key.
pub(crate) const CHANNEL_KEYED: u32 = 0x100;
/// `+0x20` bits written by the create path `0x00789B60`.
pub(crate) const GAME_CLAN: u32 = 0x100;
pub(crate) const GAME_LIMIT_640: u32 = 0x200;
pub(crate) const GAME_LIMIT_800: u32 = 0x400;
/// Quick Match / World Domination games (`>> 0xB & 1`, `>> 0xC & 1`).
const GAME_RANKED: u32 = 0x1800;

/// One channel record (`0x10C` bytes): a chat lobby in `0xB73E18` or a game
/// in `0xB76AE8`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WolChannel {
    /// `+0x00`: 0 chat lobby, [`GAME_CHANNEL`].
    pub(crate) kind: u32,
    /// `+0x08`.
    pub(crate) max_users: i32,
    /// `+0x0C`.
    pub(crate) users: i32,
    /// `+0x14`.
    pub(crate) tournament: i32,
    /// `+0x1C`: [`CHANNEL_KEYED`].
    pub(crate) flags: u32,
    /// `+0x20`: [`GAME_CLAN`], the resolution limits, the ranked bits and
    /// the host's chat-lobby number added in.
    pub(crate) game_flags: u32,
    /// `+0x28`: -1 unknown.
    pub(crate) ping: i32,
    /// `+0x34`.
    pub(crate) name: String,
    /// `+0xE0`: `"%d,%d,%d,%d,%d,%s"` (`0x0077E7A0`).
    pub(crate) ex_info: String,
}

/// `0x0077E820`: the exInfo fields, each a CRT `strtok(",")` token through
/// `atol` (0 when missing), the last the scenario token.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExInfo {
    pub(crate) version: i32,
    /// Rules ^ AI ^ Art INI hashes of the host.
    pub(crate) hash: i32,
    /// `0x005E94D0`: AI players.
    pub(crate) ai_players: i32,
    /// `0x005E9580`: a human occupies the observer slot.
    pub(crate) observer_taken: i32,
    /// `!0x005E9560`: an open observer slot exists.
    pub(crate) observer_open: i32,
    pub(crate) scenario: String,
}

impl ExInfo {
    pub(crate) fn parse(text: &str) -> Self {
        let mut tokens = strtok(text, &[',']);
        let mut number = || tokens.next().map_or(0, crt_atoi);
        let version = number();
        let hash = number();
        let ai_players = number();
        let observer_taken = number();
        let observer_open = number();
        Self {
            version,
            hash,
            ai_players,
            observer_taken,
            observer_open,
            scenario: tokens.next().unwrap_or_default().to_owned(),
        }
    }

    /// `f4 || f5`: one of the max users is the observer slot.
    fn observer_slot(&self) -> bool {
        self.observer_taken != 0 || self.observer_open != 0
    }

    /// The observer slot is open and empty.
    fn observer_free(&self) -> bool {
        self.observer_open != 0 && self.observer_taken == 0
    }
}

impl WolChannel {
    pub(crate) fn keyed(&self) -> bool {
        self.flags & CHANNEL_KEYED != 0
    }

    pub(crate) fn ex_info(&self) -> ExInfo {
        ExInfo::parse(&self.ex_info)
    }

    /// `atol(name + 7)` of a `Lob_41_` channel.
    pub(crate) fn lobby_number(&self) -> Option<i32> {
        self.name.strip_prefix(LOBBY_PREFIX).map(crt_atoi)
    }

    /// The shown player count and capacity (`cur - f4 + f3`, `max - (f4||f5)`).
    pub(crate) fn shown_capacity(&self) -> (i32, i32) {
        let info = self.ex_info();
        (
            self.users - info.observer_taken + info.ai_players,
            self.max_users - i32::from(info.observer_slot()),
        )
    }

    /// `0x00781480`'s full test, which also greys the name: with the
    /// observer preference off the shown count equals the capacity, with it
    /// on the observer slot is not free.
    pub(crate) fn full(&self, observer: bool) -> bool {
        if observer {
            !self.ex_info().observer_free()
        } else {
            let (shown, capacity) = self.shown_capacity();
            shown == capacity
        }
    }

    /// `0x00789960`: the widest screen this game admits.
    pub(crate) fn screen_limit(&self) -> i32 {
        if self.game_flags & GAME_LIMIT_800 != 0 {
            800
        } else if self.game_flags & GAME_LIMIT_640 != 0 {
            640
        } else {
            1024
        }
    }
}

/// One channel user (`0x6C` bytes in `0xB77908`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WolMember {
    /// `+0x00`: [`MEMBER_OPERATOR`], [`MEMBER_VOICE`]; `0x8000` marks the local user.
    pub(crate) flags: u32,
    /// `+0x24`.
    pub(crate) name: String,
    /// `+0x2E`: the clan column's tooltip.
    pub(crate) clan_name: String,
    /// `+0x57`.
    pub(crate) clan_tag: String,
}

pub(crate) const MEMBER_OPERATOR: u32 = 1;
pub(crate) const MEMBER_VOICE: u32 = 2;
pub(crate) const MEMBER_LOCAL: u32 = 0x8000;

impl WolMember {
    fn operator(&self) -> bool {
        self.flags & MEMBER_OPERATOR != 0
    }
}

/// The game list's sort mode `[0xB77E44]` (BSS 0), process-wide.
pub(crate) type GameSort = i32;
/// The player list's sort mode `[0xB77E48]` (BSS 0), process-wide.
pub(crate) type PlayerSort = i32;

/// The sort header buttons' toggles (`0x0078AC10`).
pub(crate) fn toggle_game_sort(control: u16, sort: GameSort) -> Option<GameSort> {
    Some(match control {
        super::SORT_ALPHA => i32::from(sort == 0),
        super::SORT_PING => {
            if sort == 0xE {
                0xF
            } else {
                0xE
            }
        }
        super::SORT_RESLOCK => match sort {
            6 => 7,
            7 => 4,
            4 => 5,
            _ => 6,
        },
        super::SORT_PASSWORD => {
            if sort == 2 {
                3
            } else {
                2
            }
        }
        super::SORT_GAMETYPE => match sort {
            0xC | 0xD => 8,
            8 | 9 => 0xA,
            _ => 0xC,
        },
        _ => return None,
    })
}

pub(crate) fn toggle_player_sort(control: u16, sort: PlayerSort) -> Option<PlayerSort> {
    Some(match control {
        super::PLAYER_SORT_ALPHA => i32::from(sort == 0),
        super::PLAYER_SORT_RANK => {
            if sort == 2 {
                3
            } else {
                2
            }
        }
        super::PLAYER_SORT_CLAN => {
            if sort == 4 {
                5
            } else {
                4
            }
        }
        _ => return None,
    })
}

/// The type column's image (`0x00781480`, x=2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameTypeIcon {
    /// `gt18.pcx`, no tooltip.
    Plain,
    /// `wolclan.pcx`, `TXT_BATTLECLAN_GAME`.
    Clan,
    /// `woltrny.pcx`, `TXT_TOURNAMENT_GAME`.
    Tournament,
}

impl GameTypeIcon {
    pub(crate) fn file(self) -> &'static str {
        match self {
            Self::Plain => "gt18.pcx",
            Self::Clan => "wolclan.pcx",
            Self::Tournament => "woltrny.pcx",
        }
    }
}

/// One projected game row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolGameRow {
    /// The record's index in the service's game list (item data).
    pub(crate) record: usize,
    /// The item key `0x4C7` stores and `0x4C5` finds.
    pub(crate) name: String,
    /// `"%hs %d/%d"` (`0x0084A724`).
    pub(crate) text: String,
    /// `0x808080` instead of -1.
    pub(crate) greyed: bool,
    pub(crate) kind: GameTypeIcon,
    /// `wolpriv.pcx` with `TXT_GAME_PASSWORD`.
    pub(crate) keyed: bool,
    /// The resolution/observer column (x=0x24) image.
    pub(crate) limit_icon: Option<&'static str>,
    /// -1 shows `"???"`; the column tooltip is `WOL:GamePing` either way.
    pub(crate) ping: i32,
}

/// `0x00781130`: whether the filter pass skips this record.
fn filter_skips(channel: &WolChannel, info: &ExInfo, sort: GameSort, pass: usize) -> bool {
    let clan = channel.game_flags & GAME_CLAN != 0;
    let marked = match sort {
        2 | 3 => channel.keyed(),
        4 | 5 => info.observer_slot(),
        6 | 7 => channel.game_flags & (GAME_LIMIT_640 | GAME_LIMIT_800) != 0,
        8 | 9 => channel.tournament != 0,
        10 | 11 => clan,
        12 | 13 => channel.tournament != 0 || clan,
        _ => return false,
    };
    // Even modes 2..10 and 13 list the marked records first.
    let marked_first = matches!(sort, 2 | 4 | 6 | 8 | 10 | 13);
    marked != ((pass == 0) == marked_first)
}

fn filtered(sort: GameSort) -> bool {
    (2..=13).contains(&sort)
}

/// `0x00781380` (ascending, -1 last) and `0x007813B0` (descending).
fn compare_ping(a: i32, b: i32, descending: bool) -> Ordering {
    if descending {
        return b.cmp(&a);
    }
    match a.cmp(&b) {
        Ordering::Less if a == -1 => Ordering::Greater,
        Ordering::Greater if b == -1 => Ordering::Less,
        order => order,
    }
}

/// `0x00781480`: the rows in display order. Mode 0 keeps the service's
/// order, mode 1 reverses it, every other mode orders by ping (MSVC
/// `qsort`, which is not stable: equal pings keep the service order here,
/// a residual). Non-full rows come first, then full ones; filter modes run
/// each half twice through `0x00781130`. Ranked games are hidden.
pub(crate) fn game_rows(
    channels: &[WolChannel],
    sort: GameSort,
    observer: bool,
) -> Vec<WolGameRow> {
    let count = channels.len();
    let mut sorted: Vec<usize> = (0..count).collect();
    sorted.sort_by(|a, b| compare_ping(channels[*a].ping, channels[*b].ping, sort == 0xF));
    let order: Vec<usize> = match sort {
        0 => (0..count).collect(),
        1 => (0..count).rev().collect(),
        _ => sorted,
    };
    let passes = if filtered(sort) { 2 } else { 1 };
    let mut rows = Vec::new();
    for full_pass in [false, true] {
        for pass in 0..passes {
            for &record in &order {
                let channel = &channels[record];
                if channel.game_flags & GAME_RANKED != 0 {
                    continue;
                }
                let info = channel.ex_info();
                if channel.full(observer) != full_pass || filter_skips(channel, &info, sort, pass) {
                    continue;
                }
                let (shown, capacity) = channel.shown_capacity();
                let free = info.observer_free();
                rows.push(WolGameRow {
                    record,
                    name: channel.name.clone(),
                    text: format!("{} {shown}/{capacity}", channel.name),
                    greyed: full_pass,
                    kind: if channel.game_flags & GAME_CLAN != 0 {
                        GameTypeIcon::Clan
                    } else if channel.tournament != 0 {
                        GameTypeIcon::Tournament
                    } else {
                        GameTypeIcon::Plain
                    },
                    keyed: channel.keyed(),
                    limit_icon: if channel.game_flags & (GAME_LIMIT_640 | GAME_LIMIT_800) != 0 {
                        Some(if free {
                            "wolrslkob.pcx"
                        } else {
                            "wolreslk.pcx"
                        })
                    } else {
                        free.then_some("wolob.pcx")
                    },
                    ping: channel.ping,
                });
            }
        }
    }
    rows
}

/// One chat-lobby row (`0x007806C0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolLobbyRow {
    pub(crate) record: usize,
    pub(crate) name: String,
    pub(crate) number: i32,
    /// The base-name index `n % 8` (C remainder) into `TXT_LOB_1..8`, and
    /// the `_%d` suffix `n / 8 + 1` when the quotient is not 0.
    pub(crate) base: i32,
    pub(crate) suffix: Option<i32>,
    pub(crate) users: i32,
}

impl WolLobbyRow {
    /// `L"%s  (%d)"` / `L"%s_%d  (%d)"` over `TXT_LOB_<base+1>`. A negative
    /// number would index before the table (`[0xB73E1C]`); that read is not
    /// reproduced and shows the label key.
    pub(crate) fn text(&self, base_names: &[String; 8]) -> String {
        let base = usize::try_from(self.base)
            .ok()
            .and_then(|index| base_names.get(index))
            .map_or("", String::as_str);
        match self.suffix {
            None => format!("{base}  ({})", self.users),
            Some(suffix) => format!("{base}_{suffix}  ({})", self.users),
        }
    }

    /// `0x007A5E40`'s `TXT_JOINED_S` argument: `L"%s"` or `L"%s %d"`.
    pub(crate) fn joined_name(number: i32, base_names: &[String; 8]) -> String {
        let row = lobby_row(0, "", number, 0);
        let base = usize::try_from(row.base)
            .ok()
            .and_then(|index| base_names.get(index))
            .map_or("", String::as_str);
        match row.suffix {
            None => base.to_owned(),
            Some(suffix) => format!("{base} {suffix}"),
        }
    }
}

fn lobby_row(record: usize, name: &str, number: i32, users: i32) -> WolLobbyRow {
    let quotient = number / 8;
    WolLobbyRow {
        record,
        name: name.to_owned(),
        number,
        base: number % 8,
        suffix: (quotient != 0).then_some(quotient + 1),
        users,
    }
}

/// `0x007806C0`: only `Lob_41_` channels, each inserted before the first
/// row whose number is larger.
pub(crate) fn lobby_rows(lobbies: &[WolChannel]) -> Vec<WolLobbyRow> {
    let mut rows: Vec<WolLobbyRow> = Vec::new();
    for (record, channel) in lobbies.iter().enumerate() {
        let Some(number) = channel.lobby_number() else {
            continue;
        };
        let at = rows
            .iter()
            .position(|row| row.number > number)
            .unwrap_or(rows.len());
        rows.insert(at, lobby_row(record, &channel.name, number, channel.users));
    }
    rows
}

/// `0x00788180`: the chat lobby to join. `skip_full` (a lobby was joined
/// before this visit's runner and it is not the post-match entry) passes
/// over lobbies of 26 or more users, falling back to the least crowded.
/// Returns the record and the lobby number `[0xB77DD4]` becomes.
pub(crate) fn pick_lobby(
    lobbies: &[WolChannel],
    number: i32,
    skip_full: bool,
) -> Option<(usize, i32)> {
    let admits = |channel: &WolChannel| !skip_full || channel.users < LOBBY_CROWDED;
    let target = format!("{LOBBY_PREFIX}{number}");
    if let Some(record) = lobbies
        .iter()
        .position(|channel| channel.name == target && admits(channel))
    {
        return Some((record, number));
    }
    let mut least: Option<(usize, u32)> = None;
    for (record, channel) in lobbies.iter().enumerate() {
        let Some(number) = channel.lobby_number() else {
            continue;
        };
        if admits(channel) {
            return Some((record, number));
        }
        // Unsigned compare against the initial 10000.
        let users = channel.users as u32;
        if users < least.map_or(10000, |(_, fewest)| fewest) {
            least = Some((record, users));
        }
    }
    least.map(|(record, _)| (record, lobbies[record].lobby_number().unwrap_or_default()))
}

/// The lobby's own WOL context for [`admit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdmissionContext {
    /// `[0xA8B244]` is Quick Match 1, 2 or 3.
    pub(crate) quick_match: bool,
    /// `[0xB77DE4]`: the user's clan id; no clan service feeds VERA, so 0.
    pub(crate) clan_id: i32,
    /// Rules ^ AI ^ Art of `0x00476D80`; VERA has no owner of those INI
    /// CRCs, so the hash comparison is skipped while this is `None`.
    pub(crate) local_hash: Option<i32>,
}

/// `0x0079BA60`: may this game be joined (as an observer)? `Err(None)`
/// rejects silently; `Err(Some(key))` names the chat line shown when the
/// caller asks for messages.
pub(crate) fn admit(
    channel: &WolChannel,
    observer: bool,
    context: AdmissionContext,
) -> Result<(), Option<&'static str>> {
    if (channel.game_flags & GAME_RANKED != 0) != context.quick_match {
        return Err(None);
    }
    if channel.game_flags & GAME_CLAN != 0 && context.clan_id < 1 {
        return Err(Some("TXT_NO_CLAN"));
    }
    let info = channel.ex_info();
    if info.version != EX_INFO_VERSION || context.local_hash.is_some_and(|hash| hash != info.hash) {
        return Err(Some("TXT_MISMATCH"));
    }
    if observer {
        return if info.observer_free() {
            Ok(())
        } else {
            Err(Some("TXT_OBSERVER_FULL"))
        };
    }
    let (shown, capacity) = channel.shown_capacity();
    if shown < capacity {
        Ok(())
    } else {
        Err(Some("WOL:GameFull"))
    }
}

/// `0x00789960` after the Yes of `WOL:ResolutionLimited800/640`: the
/// options' screen size becomes 800x600 or 640x480.
pub(crate) fn limited_screen(limit: i32) -> (i32, i32) {
    if limit == 800 { (800, 600) } else { (640, 480) }
}

/// `0x78DB70`/`0x78E030` enable OK only while the password contains none of
/// `" ,;"` (`0x0084BA18`).
pub(crate) fn password_accepted(units: &[u16]) -> bool {
    !units.iter().any(|unit| matches!(*unit, 0x20 | 0x2C | 0x3B))
}

/// The membership insert of the player list's sort mode (`0x007A5E40`'s
/// dispatch): operators stay in a leading block. Alpha (mode 0 ascending,
/// 1 descending) compares `_strcmpi` names; rank (2/3) without ladder
/// records orders names ascending; clan (4 ascending, 5 descending) orders
/// by tag, empty tags last, then names ascending within a tag.
pub(crate) fn insert_member(members: &mut Vec<WolMember>, member: WolMember, sort: PlayerSort) {
    let operators = members.iter().take_while(|m| m.operator()).count();
    let (start, end) = if member.operator() {
        (0, operators)
    } else {
        (operators, members.len())
    };
    let name_before = |existing: &WolMember| crt_strcmpi(&existing.name, &member.name).is_ge();
    let at = match sort {
        1 => (start..end)
            .find(|i| crt_strcmpi(&members[*i].name, &member.name).is_le())
            .unwrap_or(end),
        4 | 5 => {
            // 0x00782BB0 phase 1: the first tag position.
            let tag_at = (start..end)
                .find(|i| {
                    let existing = &members[*i].clan_tag;
                    existing.is_empty()
                        || (!member.clan_tag.is_empty()
                            && if sort == 4 {
                                crt_strcmpi(existing, &member.clan_tag).is_ge()
                            } else {
                                crt_strcmpi(&member.clan_tag, existing).is_ge()
                            })
                })
                .unwrap_or(end);
            // Phase 2: past same-tag names that sort before this one.
            (tag_at..end)
                .find(|i| {
                    let existing = &members[*i];
                    let same_tag = if existing.clan_tag.is_empty() || member.clan_tag.is_empty() {
                        existing.clan_tag.is_empty() && member.clan_tag.is_empty()
                    } else {
                        crt_strcmpi(&member.clan_tag, &existing.clan_tag).is_eq()
                    };
                    !same_tag || name_before(existing)
                })
                .unwrap_or(end)
        }
        // 0 and the rank modes without ladder records (0x00782320).
        _ => (start..end)
            .find(|i| name_before(&members[*i]))
            .unwrap_or(end),
    };
    members.insert(at, member);
}

/// A sort button re-inserts every member in the current order.
pub(crate) fn resort_members(members: &mut Vec<WolMember>, sort: PlayerSort) {
    for member in std::mem::take(members) {
        insert_member(members, member, sort);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(name: &str, users: i32, max: i32, ex: &str) -> WolChannel {
        WolChannel {
            kind: GAME_CHANNEL,
            max_users: max,
            users,
            ping: -1,
            name: name.into(),
            ex_info: ex.into(),
            ..Default::default()
        }
    }

    fn member(name: &str, flags: u32, tag: &str) -> WolMember {
        WolMember {
            flags,
            name: name.into(),
            clan_tag: tag.into(),
            ..Default::default()
        }
    }

    #[test]
    fn ex_info_reads_crt_tokens_and_missing_fields_read_zero() {
        assert_eq!(
            ExInfo::parse("25,-7,2,0,1,yrmap.map"),
            ExInfo {
                version: 25,
                hash: -7,
                ai_players: 2,
                observer_taken: 0,
                observer_open: 1,
                scenario: "yrmap.map".into(),
            }
        );
        // strtok collapses empty tokens; missing ones read 0.
        let info = ExInfo::parse("25,,3");
        assert_eq!((info.version, info.hash, info.ai_players), (25, 3, 0));
        assert_eq!(info.scenario, "");
    }

    #[test]
    fn game_rows_show_counts_grey_full_games_and_list_them_last() {
        let channels = vec![
            // 3 humans + 1 AI of 4: full.
            game("Full's_game", 3, 4, "25,0,1,0,0,a.map"),
            // Observer slot open: capacity 7 - 1.
            game("Open's_game", 2, 7, "25,0,0,0,1,b.map"),
        ];
        let rows = game_rows(&channels, 0, false);
        assert_eq!(
            rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>(),
            ["Open's_game 2/6", "Full's_game 4/4"]
        );
        assert_eq!(
            rows.iter().map(|row| row.greyed).collect::<Vec<_>>(),
            [false, true]
        );
        assert_eq!(rows[0].limit_icon, Some("wolob.pcx"));
        assert_eq!(rows[1].limit_icon, None);
        // With the observer preference only a free observer slot counts.
        let rows = game_rows(&channels, 0, true);
        assert_eq!(rows[0].name, "Open's_game");
        assert!(!rows[0].greyed && rows[1].greyed);
    }

    #[test]
    fn game_icons_follow_flags_and_ranked_games_are_hidden() {
        let mut clan = game("c", 1, 4, "25,0,0,0,0,x");
        clan.game_flags = GAME_CLAN | GAME_LIMIT_800;
        clan.flags = CHANNEL_KEYED;
        let mut tournament = game("t", 1, 4, "25,0,0,0,1,x");
        tournament.tournament = 1;
        tournament.game_flags = GAME_LIMIT_640;
        let mut ranked = game("r", 1, 4, "25,0,0,0,0,x");
        ranked.game_flags = 0x800;
        let rows = game_rows(&[clan, tournament, ranked], 0, false);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, GameTypeIcon::Clan);
        assert!(rows[0].keyed);
        assert_eq!(rows[0].limit_icon, Some("wolreslk.pcx"));
        assert_eq!(rows[1].kind, GameTypeIcon::Tournament);
        assert_eq!(rows[1].limit_icon, Some("wolrslkob.pcx"));
    }

    #[test]
    fn sort_modes_order_by_server_reverse_ping_and_filter_passes() {
        let mut channels: Vec<WolChannel> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| game(name, 1, 4, "25,0,0,0,0,x"))
            .collect();
        for (channel, ping) in channels.iter_mut().zip([90, -1, 30, 60]) {
            channel.ping = ping;
        }
        channels[3].flags = CHANNEL_KEYED;
        let names = |sort| -> Vec<String> {
            game_rows(&channels, sort, false)
                .into_iter()
                .map(|row| row.name)
                .collect()
        };
        assert_eq!(names(0), ["a", "b", "c", "d"]);
        assert_eq!(names(1), ["d", "c", "b", "a"]);
        assert_eq!(names(0xE), ["c", "d", "a", "b"]);
        assert_eq!(names(0xF), ["a", "d", "c", "b"]);
        // Password first (2), then last (3); each pass keeps the ping order.
        assert_eq!(names(2), ["d", "c", "a", "b"]);
        assert_eq!(names(3), ["c", "a", "b", "d"]);
    }

    #[test]
    fn sort_buttons_toggle_the_native_mode_sequences() {
        use super::super::*;
        assert_eq!(toggle_game_sort(SORT_ALPHA, 0), Some(1));
        assert_eq!(toggle_game_sort(SORT_ALPHA, 1), Some(0));
        assert_eq!(toggle_game_sort(SORT_ALPHA, 0xE), Some(0));
        assert_eq!(toggle_game_sort(SORT_PING, 0xE), Some(0xF));
        assert_eq!(toggle_game_sort(SORT_PING, 3), Some(0xE));
        let mut sort = 0;
        let mut seen = Vec::new();
        for _ in 0..5 {
            sort = toggle_game_sort(SORT_RESLOCK, sort).unwrap();
            seen.push(sort);
        }
        assert_eq!(seen, [6, 7, 4, 5, 6]);
        assert_eq!(toggle_game_sort(SORT_GAMETYPE, 0), Some(0xC));
        assert_eq!(toggle_game_sort(SORT_GAMETYPE, 0xD), Some(8));
        assert_eq!(toggle_game_sort(SORT_GAMETYPE, 9), Some(0xA));
        assert_eq!(toggle_game_sort(SORT_PASSWORD, 2), Some(3));
        assert_eq!(toggle_player_sort(PLAYER_SORT_ALPHA, 4), Some(0));
        assert_eq!(toggle_player_sort(PLAYER_SORT_RANK, 2), Some(3));
        assert_eq!(toggle_player_sort(PLAYER_SORT_CLAN, 0), Some(4));
    }

    #[test]
    fn lobby_rows_keep_lob_channels_in_number_order() {
        let lobby = |name: &str, users| WolChannel {
            name: name.into(),
            users,
            ..Default::default()
        };
        let rows = lobby_rows(&[
            lobby("Lob_41_9", 3),
            lobby("#chat", 1),
            lobby("Lob_41_0", 12),
            lobby("Lob_41_2", 0),
        ]);
        let bases: [String; 8] = std::array::from_fn(|i| format!("L{}", i + 1));
        assert_eq!(
            rows.iter().map(|row| row.text(&bases)).collect::<Vec<_>>(),
            ["L1  (12)", "L3  (0)", "L2_2  (3)"]
        );
        assert_eq!(rows[2].record, 0);
        assert_eq!(WolLobbyRow::joined_name(9, &bases), "L2 2");
        assert_eq!(WolLobbyRow::joined_name(2, &bases), "L3");
    }

    #[test]
    fn picker_rejoins_its_number_or_skips_crowded_lobbies() {
        let lobby = |name: &str, users| WolChannel {
            name: name.into(),
            users,
            ..Default::default()
        };
        let lobbies = [
            lobby("Lob_41_0", 30),
            lobby("Lob_41_1", 26),
            lobby("Lob_41_2", 5),
        ];
        assert_eq!(pick_lobby(&lobbies, 1, false), Some((1, 1)));
        assert_eq!(pick_lobby(&lobbies, 1, true), Some((2, 2)));
        assert_eq!(pick_lobby(&lobbies, 7, false), Some((0, 0)));
        let crowded = [lobby("Lob_41_0", 30), lobby("Lob_41_1", 27)];
        assert_eq!(pick_lobby(&crowded, 0, true), Some((1, 1)));
        assert_eq!(pick_lobby(&[lobby("#x", 0)], 0, false), None);
    }

    #[test]
    fn admission_checks_rank_clan_version_then_capacity() {
        let context = AdmissionContext {
            quick_match: false,
            clan_id: 0,
            local_hash: None,
        };
        let open = game("g", 2, 4, "25,0,0,0,1,x");
        assert_eq!(admit(&open, false, context), Ok(()));
        assert_eq!(admit(&open, true, context), Ok(()));
        let mut ranked = open.clone();
        ranked.game_flags = 0x1000;
        assert_eq!(admit(&ranked, false, context), Err(None));
        let mut clan = open.clone();
        clan.game_flags = GAME_CLAN;
        clan.ex_info = "24".into();
        assert_eq!(admit(&clan, false, context), Err(Some("TXT_NO_CLAN")));
        let old = game("g", 1, 4, "24,0,0,0,0,x");
        assert_eq!(admit(&old, false, context), Err(Some("TXT_MISMATCH")));
        let hashed = AdmissionContext {
            local_hash: Some(5),
            ..context
        };
        assert_eq!(admit(&open, false, hashed), Err(Some("TXT_MISMATCH")));
        // 3 humans of 4 with the observer slot counted: full.
        let full = game("g", 3, 4, "25,0,0,0,1,x");
        assert_eq!(admit(&full, false, context), Err(Some("WOL:GameFull")));
        let taken = game("g", 2, 4, "25,0,0,1,1,x");
        assert_eq!(admit(&taken, true, context), Err(Some("TXT_OBSERVER_FULL")));
    }

    #[test]
    fn resolution_limits_and_password_gate() {
        let mut channel = game("g", 1, 4, "");
        assert_eq!(channel.screen_limit(), 1024);
        channel.game_flags = GAME_LIMIT_640;
        assert_eq!(channel.screen_limit(), 640);
        channel.game_flags = GAME_LIMIT_640 | GAME_LIMIT_800;
        assert_eq!(channel.screen_limit(), 800);
        assert_eq!(limited_screen(800), (800, 600));
        assert_eq!(limited_screen(640), (640, 480));
        let units = |text: &str| text.encode_utf16().collect::<Vec<_>>();
        assert!(password_accepted(&units("abc_12")));
        for bad in ["a b", "a,b", "a;b"] {
            assert!(!password_accepted(&units(bad)), "{bad}");
        }
    }

    #[test]
    fn member_inserts_keep_operators_first_and_follow_the_sort_mode() {
        let names = |members: &[WolMember]| -> Vec<String> {
            members.iter().map(|m| m.name.clone()).collect()
        };
        let incoming = [
            member("bob", 0, "XX"),
            member("Zed", 1, ""),
            member("alice", 0, ""),
            member("Carl", 0, "AA"),
            member("abe", 1, ""),
            member("dora", 0, "XX"),
        ];
        let mut members = Vec::new();
        for m in incoming.iter().cloned() {
            insert_member(&mut members, m, 0);
        }
        assert_eq!(
            names(&members),
            ["abe", "Zed", "alice", "bob", "Carl", "dora"]
        );
        resort_members(&mut members, 1);
        assert_eq!(
            names(&members),
            ["Zed", "abe", "dora", "Carl", "bob", "alice"]
        );
        resort_members(&mut members, 3);
        assert_eq!(
            names(&members),
            ["abe", "Zed", "alice", "bob", "Carl", "dora"]
        );
        resort_members(&mut members, 4);
        assert_eq!(
            names(&members),
            ["abe", "Zed", "Carl", "bob", "dora", "alice"]
        );
        resort_members(&mut members, 5);
        assert_eq!(
            names(&members),
            ["abe", "Zed", "bob", "dora", "Carl", "alice"]
        );
    }
}
