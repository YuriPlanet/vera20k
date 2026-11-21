//! Which lobby this computer talks to and the sign-in it remembers there:
//! the remembered name, the credential a sign-in uses, and remembering or
//! forgetting it in `Account.json`.

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
use super::DEFAULT_LOBBY_URL;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use super::lobby_base_url;
use super::{LiveService, http};
use crate::app::frontend::state::FrontendState;
use crate::app::persistence::account;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::shell_wol::WolVisit;
use crate::util::config::GameConfig;

/// The configured lobby's origin, as [`LiveService::open`] takes it without an
/// external launch.
fn configured_lobby() -> Option<String> {
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let lobby = lobby_base_url().ok()?;
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    let lobby = DEFAULT_LOBBY_URL.to_owned();
    Some(lobby.trim_end_matches('/').to_owned())
}

/// The name this computer signs in to without its password on this visit's
/// lobby (docs/lobby-protocol.md "Accounts").
pub(in crate::app::shell_wol) fn remembered_name(
    wol: Option<&WolVisit>,
    game_config: Option<&GameConfig>,
) -> Option<String> {
    let dir = game_config?.paths.settings_dir();
    let lobby = match wol?.live_service.as_ref() {
        Some(service) => service.lobby.clone(),
        None => configured_lobby()?,
    };
    account::load(dir, &lobby).map(|remembered| remembered.name)
}

/// The folder of VERA.INI, which holds `Account.json`.
pub(super) fn settings_dir(game_config: Option<&GameConfig>) -> Option<&std::path::Path> {
    game_config.map(|config| config.paths.settings_dir())
}

/// Guest entry sends no credential. Account entry sends the typed password
/// or the remembered token for this name. If that token disappeared since
/// the dialog opened, the account must enter its password.
pub(super) fn sign_in_credential(
    settings: Option<&std::path::Path>,
    service: &LiveService,
    nickname: &str,
    password: String,
    remember: bool,
) -> Option<http::Credential> {
    if password.is_empty() && !remember {
        return Some(http::Credential::Guest);
    }
    let remembered = settings
        .and_then(|dir| account::load(dir, &service.lobby))
        .filter(|remembered| remembered.name.eq_ignore_ascii_case(nickname));
    if password.is_empty() {
        return remembered.map(|entry| http::Credential::Remembered(entry.sign_in_token));
    }
    if remembered.is_some() {
        forget_sign_in(settings, service);
    }
    Some(http::Credential::Password { password, remember })
}

/// Drop this computer's remembered sign-in, here and at the lobby.
pub(super) fn forget_sign_in(settings: Option<&std::path::Path>, service: &LiveService) {
    let Some(dir) = settings else {
        return;
    };
    match account::forget(dir, &service.lobby, None) {
        Ok(Some(remembered)) => {
            let _ = service.requests.send(http::Request::ForgetSignIn {
                name: remembered.name,
                sign_in_token: remembered.sign_in_token,
            });
        }
        Ok(None) => {}
        Err(error) => log::warn!("Could not forget the remembered sign-in: {error}"),
    }
}

/// The lobby has deleted this account. Forget only credentials and profile
/// entries that still name it, even if the original form has closed.
pub(super) fn forget_deleted_account(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    options_profile: &mut RetailOptionsProfile,
    lobby: &str,
    name: &str,
) {
    if let Some(dir) = settings_dir(game_config)
        && let Err(error) = account::forget(dir, lobby, Some(name))
    {
        log::warn!("Could not forget the deleted account's sign-in: {error}");
    }
    if options_profile.online_name.eq_ignore_ascii_case(name) {
        remember_online_name(game_config, options_profile, "");
    }
    let globals = &mut frontend.wol_account;
    if globals.nickname.eq_ignore_ascii_case(name) {
        globals.nickname.clear();
        globals.password.clear();
        globals.remember = false;
    }
    if let Some(super::WolPage::Login(login)) = frontend.wol.as_mut().map(|visit| &mut visit.page) {
        login.account_deleted(&mut frontend.shell_controller, name);
    }
}

/// VERA: keep the name the lobby accepted as `[VERA] OnlineName=`, so the
/// next visit's login starts with it
/// ([`crate::ui::wol_login::WolLoginState::open_online`]).
pub(super) fn remember_online_name(
    game_config: Option<&GameConfig>,
    options_profile: &mut RetailOptionsProfile,
    nickname: &str,
) {
    if options_profile.online_name == nickname {
        return;
    }
    options_profile.online_name = nickname.to_owned();
    if let Some(config) = game_config
        && let Err(error) = options_profile.commit_online_name(config.paths.settings_dir())
    {
        log::warn!("Could not save the online name: {error}");
    }
}

/// Keep the sign-in the lobby gave a password sign-in with Remember password,
/// in place of another account's, which the lobby then forgets.
pub(super) fn remember_sign_in(
    settings: Option<&std::path::Path>,
    service: &LiveService,
    name: &str,
    token: String,
) {
    let Some(dir) = settings else {
        return;
    };
    let remembered = account::Remembered {
        name: name.to_owned(),
        sign_in_token: token,
    };
    if !account::valid_token(&remembered.sign_in_token) {
        log::warn!("The lobby's remembered sign-in for {name} is malformed");
        return;
    }
    forget_sign_in(settings, service);
    if let Err(error) = account::save(dir, &service.lobby, remembered) {
        log::warn!("Could not remember the sign-in of {name}: {error}");
    }
}
