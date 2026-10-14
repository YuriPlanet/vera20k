//! Live front-end session owner: gamemd's `SessionClass` instance at
//! `0x00A8B238`, whose first field is the `GameMode` discriminator every
//! `GameMode == 0` / `!= 0` branch and the modal pump read.
//!
//! Native writers of `GameMode`:
//! - process start `0x0077B532` (and the sibling exits of `FUN_0077B2A0`): 0.
//! - main-menu shell `FUN_0052D9A0`: `0x0052DD7F` Network -> 3 (LAN),
//!   `0x0052DD61` Internet -> 4 (WOL), `0x0052E10F` Skirmish -> 5 before
//!   `SessionClass__ReadMultiPlayerSettings @ 0x006980C0`, `0x0052E175`
//!   Skirmish Back -> 0 (campaign).
//! - `Load_Game @ 0x0067E440` (called from the load dialog at `0x00559DC5`):
//!   `0x0067E4E8` restores the value the save header carries at `+0x3F0`
//!   (`0x006812D0`).
//! - `Main_Game @ 0x0048CCC0`: `0x0048CF94` resets it to 0 on the flagged
//!   return path; VERA's shell resume route (`ShellRoute`) carries that
//!   decision instead.
//!
//! VERA has no IPX/WOL writers yet: the Network route still bounces to the
//! main menu (`bounce_network_to_main_menu`) and the lobby of a later stage
//! writes `Lan`. The owner is read ONLY by the app loop, never by `sim/`
//! (the layering rule); the simulation keeps its own persisted
//! zero/nonzero classification (`ScenarioSession::game_mode_nonzero`), set
//! from the same launch descriptor at construction.

/// Front-end session mode, as the modal pump reads it to decide whether the
/// simulation advances behind an open modal dialog. Mirrors gamemd's
/// `GameMode` discriminator; the values are writer-proofed — the active
/// engine only ever writes 0/3/4/5, so any other raw value is a legacy
/// (modem/serial) or uninitialized mode the pump treats conservatively as
/// non-advancing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "VERA launches only offline campaign and skirmish; the LAN/WOL lobbies of later networking stages construct the network modes"
    )
)]
pub(crate) enum SessionMode {
    /// Campaign / single-player. The modal pump freezes the world.
    Campaign,
    /// LAN / IPX network. The modal pump keeps the world advancing.
    Lan,
    /// WOL / Internet network. The modal pump keeps the world advancing.
    Wol,
    /// Offline skirmish. The modal pump freezes the world.
    Skirmish,
    /// Any raw value the active engine never writes (legacy modem/serial, or
    /// uninitialized). Treated as non-advancing.
    Other,
}

impl SessionMode {
    /// Map gamemd's raw `GameMode` int to a session mode. Writer-proofed:
    /// 0=Campaign, 3=Lan, 4=Wol, 5=Skirmish; every other value is `Other`.
    #[cfg(test)]
    pub(crate) fn from_game_mode(raw: i32) -> Self {
        match raw {
            0 => SessionMode::Campaign,
            3 => SessionMode::Lan,
            4 => SessionMode::Wol,
            5 => SessionMode::Skirmish,
            _ => SessionMode::Other,
        }
    }

    /// Whether this is a network session (LAN/WOL). Only network sessions keep the
    /// simulation advancing behind an open modal; offline modes freeze it.
    pub(crate) fn is_network(self) -> bool {
        matches!(self, SessionMode::Lan | SessionMode::Wol)
    }
}

/// One human participant of the session: native `NodeNameType` as far as
/// the offline launch fills it. The network lobby of a later stage adds the
/// endpoint; house index, side, colour and start spot stay in the launch
/// descriptor until the multi-human bootstrap consumes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionPlayer {
    /// The handle this participant plays under: the session name skirmish
    /// setup copies into the human house's UI name (`0x00688094`).
    pub(crate) name: String,
}

/// The live session: mode, human participant table, the local participant's
/// index and whether this client owns the match setup (seed and player list).
///
/// Process start matches native: `GameMode` 0 with an empty participant list
/// (`0x0077B532`). Fresh launches and saved-game restores are the only
/// writers; the value persists after a match exactly as the native global
/// does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatchSession {
    mode: SessionMode,
    players: Vec<SessionPlayer>,
    local_player: usize,
    host: bool,
}

impl Default for MatchSession {
    fn default() -> Self {
        Self {
            mode: SessionMode::Campaign,
            players: Vec::new(),
            local_player: 0,
            host: false,
        }
    }
}

impl MatchSession {
    /// The shell's launch write: campaign (`0x0052E175`) or skirmish
    /// (`0x0052E10F`). An offline session has at most the local human and is
    /// its own host. A campaign has no participant table.
    pub(crate) fn begin_offline(&mut self, mode: SessionMode, local_player_name: Option<String>) {
        debug_assert!(
            !mode.is_network(),
            "offline launch cannot start a network session"
        );
        self.mode = mode;
        self.players = local_player_name
            .map(|name| SessionPlayer { name })
            .into_iter()
            .collect();
        self.local_player = 0;
        self.host = true;
    }

    /// `Load_Game @ 0x0067E4E8`: the mode the save header carries. VERA's
    /// save persists the native zero/nonzero classification only
    /// (`ScenarioSession::game_mode_nonzero`), and no network save exists
    /// yet, so nonzero maps to `Skirmish` exactly for every save VERA writes.
    pub(crate) fn restore_from_save(
        &mut self,
        game_mode_nonzero: bool,
        local_player_name: Option<&str>,
    ) {
        let mode = if game_mode_nonzero {
            SessionMode::Skirmish
        } else {
            SessionMode::Campaign
        };
        self.begin_offline(mode, local_player_name.map(str::to_owned));
    }

    pub(crate) fn mode(&self) -> SessionMode {
        self.mode
    }

    /// Human participants in native player order.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the multi-human match bootstrap of a later networking stage builds houses from this table"
        )
    )]
    pub(crate) fn players(&self) -> &[SessionPlayer] {
        &self.players
    }

    /// This client's participant, when the session has one (campaigns do not).
    pub(crate) fn local_player(&self) -> Option<&SessionPlayer> {
        self.players.get(self.local_player)
    }

    /// The local participant's handle; `None` for launches without one.
    pub(crate) fn local_player_name(&self) -> Option<&str> {
        self.local_player().map(|player| player.name.as_str())
    }

    /// Whether this client owns the match setup: it reads the seed and fixes
    /// the participant list. Every offline session is its own host.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the lockstep transport and multi-human bootstrap of later networking stages read the host flag"
        )
    )]
    pub(crate) fn host(&self) -> bool {
        self.host
    }
}

#[cfg(test)]
mod tests {
    use super::{MatchSession, SessionMode, SessionPlayer};

    #[test]
    fn process_start_is_campaign_with_no_participants() {
        let session = MatchSession::default();
        assert_eq!(session.mode(), SessionMode::Campaign);
        assert!(session.players().is_empty());
        assert_eq!(session.local_player(), None);
        assert_eq!(session.local_player_name(), None);
        assert!(!session.host());
    }

    #[test]
    fn offline_skirmish_launch_seats_the_local_player_as_host() {
        let mut session = MatchSession::default();
        session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
        assert_eq!(session.mode(), SessionMode::Skirmish);
        assert!(!session.mode().is_network());
        assert_eq!(
            session.players(),
            &[SessionPlayer {
                name: "Commander".to_owned()
            }]
        );
        assert_eq!(session.local_player_name(), Some("Commander"));
        assert!(session.host());
    }

    #[test]
    fn campaign_launch_replaces_an_outgoing_skirmish_table() {
        let mut session = MatchSession::default();
        session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
        session.begin_offline(SessionMode::Campaign, None);
        assert_eq!(session.mode(), SessionMode::Campaign);
        assert!(session.players().is_empty());
        assert_eq!(session.local_player_name(), None);
        assert!(session.host());
    }

    #[test]
    fn saved_game_restore_follows_the_persisted_classification() {
        let mut session = MatchSession::default();
        session.restore_from_save(true, Some("Saved"));
        assert_eq!(session.mode(), SessionMode::Skirmish);
        assert_eq!(session.local_player_name(), Some("Saved"));
        session.restore_from_save(false, None);
        assert_eq!(session.mode(), SessionMode::Campaign);
        assert_eq!(session.local_player_name(), None);
    }

    #[test]
    fn raw_game_mode_mapping_is_writer_proofed() {
        assert_eq!(SessionMode::from_game_mode(0), SessionMode::Campaign);
        assert_eq!(SessionMode::from_game_mode(3), SessionMode::Lan);
        assert_eq!(SessionMode::from_game_mode(4), SessionMode::Wol);
        assert_eq!(SessionMode::from_game_mode(5), SessionMode::Skirmish);
        for raw in [1, 2, -1, 99] {
            assert_eq!(SessionMode::from_game_mode(raw), SessionMode::Other);
        }
        assert!(SessionMode::Lan.is_network());
        assert!(SessionMode::Wol.is_network());
        assert!(!SessionMode::Campaign.is_network());
        assert!(!SessionMode::Skirmish.is_network());
        assert!(!SessionMode::Other.is_network());
    }
}
