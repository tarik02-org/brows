use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use rune::tok::{self, type_};
use rune::{
    AppHandle, Color, Cx, El, Image, ImageCache, Key, KeyEvent, Mods, PhysicalKeyCode, PopupItem,
    PopupMenu, PointerPress, Tx, Waker, col, el, row, text,
};
use tracing::{info, warn};

use super::model::{UIBrowser, UIState, get_filtered_browsers};
use super::placement;
use super::settings::AuxWindows;
use crate::utils::ConfiguredTheme;
use crate::{MessageToMain, MessageToUi, MoveTo};

pub(super) const PICKER_WIDTH: f32 = 296.0;
const ROW_HEIGHT: f32 = 36.0;
const FOOTER_HEIGHT: f32 = 30.0;
const PADDING: f32 = 4.0;
const ICON_SIZE: f32 = 24.0;
const PROFILE_ICON_SIZE: f32 = 12.0;

pub(super) struct Picker {
    pub(super) state: UIState,
    main_sender: Sender<MessageToMain>,
    persistent: bool,
    me: AppHandle<Picker>,
    pub(super) aux: AuxWindows,
    icons: Option<Icons>,
    menu_open: bool,
}

struct Icons {
    cache: ImageCache,
    images: HashMap<String, Image>,
}

impl Icons {
    /// Keeps handles for icons still shown and requests new ones; the cache only shares live
    /// handles, so dropping the map would re-rasterize every icon on the next sync.
    fn sync(&mut self, browsers: &[UIBrowser]) {
        let mut images = HashMap::new();
        for browser in browsers {
            for (source, size) in [
                (browser_icon_source(browser), ICON_SIZE),
                (browser.profile_icon_path.clone(), PROFILE_ICON_SIZE),
            ] {
                if source.is_empty() || images.contains_key(&source) {
                    continue;
                }
                let image = self.images.remove(&source).unwrap_or_else(|| {
                    if cfg!(target_os = "windows") && size == ICON_SIZE {
                        self.cache.app_icon(&source, size)
                    } else {
                        self.cache.file(&source, size, 2.0)
                    }
                });
                images.insert(source, image);
            }
        }
        self.images = images;
    }
}

/// Windows has no extracted icon files; Rune reads the executable's icon directly.
fn browser_icon_source(browser: &UIBrowser) -> String {
    if cfg!(target_os = "windows") {
        browser.unique_app_id.clone()
    } else {
        browser.icon_path.clone()
    }
}

pub(super) fn picker_height(state: &UIState) -> f32 {
    PADDING * 2.0 + ROW_HEIGHT * state.filtered_browsers.len().max(1) as f32 + 1.0 + FOOTER_HEIGHT
}

pub(super) fn is_dark(theme: ConfiguredTheme, system_dark: bool) -> bool {
    match theme {
        ConfiguredTheme::Auto => system_dark,
        ConfiguredTheme::Light => false,
        ConfiguredTheme::Dark => true,
    }
}

impl Picker {
    pub(super) fn new(
        state: UIState,
        main_sender: Sender<MessageToMain>,
        persistent: bool,
        me: AppHandle<Picker>,
        aux: AuxWindows,
    ) -> Self {
        Self {
            state,
            main_sender,
            persistent,
            me,
            aux,
            icons: None,
            menu_open: false,
        }
    }

    pub(super) fn setup(&mut self, waker: Waker) {
        let mut icons = Icons {
            cache: ImageCache::new(waker),
            images: HashMap::new(),
        };
        icons.sync(&self.state.browsers);
        self.icons = Some(icons);
    }

    fn icon(&self, source: &str) -> Option<&Image> {
        self.icons.as_ref()?.images.get(source)
    }

    pub(super) fn send(&self, message: MessageToMain) {
        if let Err(error) = self.main_sender.send(message) {
            warn!("Could not send message to backend: {error}");
        }
    }

    fn open_filtered(&self, index: usize, incognito: bool) {
        if let Some(browser) = self.state.filtered_browsers.get(index) {
            self.send(MessageToMain::OpenLink(
                browser.unique_id.clone(),
                incognito && browser.supports_incognito,
                self.state.url.clone(),
            ));
        }
    }

    pub(super) fn dismiss(&mut self, tx: &mut Tx) {
        if self.persistent {
            tx.close_window();
        } else {
            tx.quit();
        }
    }

    pub(super) fn show_at_cursor(&self, activation_token: Option<String>) {
        placement::place_at_cursor(&self.me, (PICKER_WIDTH, picker_height(&self.state)));
        match activation_token {
            Some(token) => self.me.activate(token),
            None => self.me.open(),
        }
    }

    pub(super) fn handle_ui_event(&mut self, message: MessageToUi, tx: &mut Tx) {
        match message {
            MessageToUi::OpenLinkCompleted => {
                info!("Link opened; dismissing picker");
                self.dismiss(tx);
            }
            MessageToUi::BehaviorUpdated(behavior) => {
                self.state.ui_settings.behavioral_settings.unwrap_urls = behavior.unwrap_urls;
            }
            MessageToUi::UrlOpened {
                source_bundle_id,
                url,
                activation_token,
            } => {
                self.state.set_url(url.clone());
                self.state.incognito_mode = false;
                if self.persistent {
                    self.show_at_cursor(activation_token);
                }
                self.send(MessageToMain::LinkOpenedFromBundle(source_bundle_id, url));
            }
            MessageToUi::BrowsersUpdated(browsers) => {
                self.state.browsers = Arc::new(browsers);
                self.state.filtered_browsers =
                    Arc::new(get_filtered_browsers(&self.state.url, &self.state.browsers));
                if let Some(icons) = &mut self.icons {
                    icons.sync(&self.state.browsers);
                }
            }
            MessageToUi::HiddenBrowsersUpdated(browsers) => {
                self.state.restorable_app_profiles = Arc::new(browsers);
            }
        }
    }

    pub(super) fn on_focus(&mut self, focused: bool, tx: &mut Tx) {
        if !focused
            && !self.menu_open
            && !self.aux.any_open()
            && self.state.ui_settings.visual_settings.quit_on_lost_focus
        {
            info!("Picker lost focus; dismissing");
            self.dismiss(tx);
        }
    }

    pub(super) fn on_modifiers(&mut self, mods: Mods, _: &mut Tx) {
        self.state.incognito_mode = mods.shift;
    }

    pub(super) fn on_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        self.state.incognito_mode = event.mods.shift;
        let incognito = event.mods.shift;
        let focused = self.state.focused_index.unwrap_or(0);
        let count = self.state.filtered_browsers.len();

        // Shift turns digits into symbols, so hotkeys match where the key sits instead.
        if !event.mods.cmd
            && let Some(slot) = tx.physical_key().and_then(digit_slot)
        {
            self.open_filtered(slot, incognito);
            return true;
        }

        match &event.key {
            Key::Escape => self.dismiss(tx),
            Key::Enter => self.open_filtered(focused, incognito),
            Key::Text(typed) if typed == " " => self.open_filtered(focused, incognito),
            Key::Up if count > 0 => {
                self.state.focused_index = Some(if focused == 0 { count - 1 } else { focused - 1 });
            }
            Key::Down if count > 0 => {
                self.state.focused_index = Some((focused + 1) % count);
            }
            Key::Command(',') => self.aux.show_settings(&self.state),
            Key::Command('c') => tx.copy(self.state.url.clone()),
            _ => return false,
        }
        true
    }

    fn open_browser_menu(&mut self, index: usize, press: PointerPress, tx: &mut Tx) {
        let Some(browser) = self.state.filtered_browsers.get(index).cloned() else {
            return;
        };

        let mut menu = PopupMenu::new();
        if !browser.has_priority_ordering() {
            menu = menu
                .item(PopupItem::new("top", "Move to top").enabled(!browser.is_first))
                .item(PopupItem::new("up", "Move up").enabled(!browser.is_first))
                .item(PopupItem::new("down", "Move down").enabled(!browser.is_last))
                .item(PopupItem::new("bottom", "Move to bottom").enabled(!browser.is_last))
                .separator();
        }
        menu = menu.item(PopupItem::new("hide", "Hide profile"));
        if browser.supports_profiles {
            menu = menu.item(PopupItem::new("hide-all", "Hide all profiles"));
        }

        self.menu_open = true;
        tx.popup_menu(menu, press, move |s: &mut Picker, picked, _| {
            s.menu_open = false;
            let profile_id = browser.unique_id.clone();
            let message = match picked {
                Some("top") => MessageToMain::MoveAppProfile(profile_id, MoveTo::TOP),
                Some("up") => MessageToMain::MoveAppProfile(profile_id, MoveTo::UP),
                Some("down") => MessageToMain::MoveAppProfile(profile_id, MoveTo::DOWN),
                Some("bottom") => MessageToMain::MoveAppProfile(profile_id, MoveTo::BOTTOM),
                Some("hide") => MessageToMain::HideAppProfile(profile_id),
                Some("hide-all") => MessageToMain::HideAllProfiles(browser.unique_app_id.clone()),
                _ => return,
            };
            s.send(message);
        });
    }

    fn open_options_menu(&mut self, tx: &mut Tx) {
        let mut menu = PopupMenu::new().item(PopupItem::new("refresh", "Refresh applications"));
        if self.state.show_set_as_default {
            menu = menu.item(PopupItem::new("make-default", "Make Browsers default"));
        }
        if !self.state.restorable_app_profiles.is_empty() {
            let mut hidden = PopupMenu::new();
            for profile in self.state.restorable_app_profiles.iter() {
                hidden = hidden.item(PopupItem::new(
                    format!("restore:{}", profile.unique_id),
                    profile.get_full_name(),
                ));
            }
            menu = menu.separator().submenu("Restore hidden profiles", hidden);
        }
        menu = menu
            .separator()
            .item(PopupItem::new("settings", "Settings…"))
            .item(PopupItem::new("about", "About Browsers"))
            .item(PopupItem::new("quit", "Quit"));

        self.menu_open = true;
        tx.popup_menu(menu, "options", |s: &mut Picker, picked, tx| {
            s.menu_open = false;
            match picked {
                Some("refresh") => s.send(MessageToMain::Refresh),
                Some("make-default") => s.send(MessageToMain::SetBrowsersAsDefaultBrowser),
                Some("settings") => s.aux.show_settings(&s.state),
                Some("about") => s.aux.show_about(),
                Some("quit") => tx.quit(),
                Some(id) => {
                    if let Some(profile_id) = id.strip_prefix("restore:") {
                        s.send(MessageToMain::RestoreAppProfile(profile_id.to_string()));
                    }
                }
                None => {}
            }
        });
    }
}

fn digit_slot(key: PhysicalKeyCode) -> Option<usize> {
    use PhysicalKeyCode::*;
    let digit = match key {
        Digit0 | Numpad0 => 0,
        Digit1 | Numpad1 => 1,
        Digit2 | Numpad2 => 2,
        Digit3 | Numpad3 => 3,
        Digit4 | Numpad4 => 4,
        Digit5 | Numpad5 => 5,
        Digit6 | Numpad6 => 6,
        Digit7 | Numpad7 => 7,
        Digit8 | Numpad8 => 8,
        Digit9 | Numpad9 => 9,
        _ => return None,
    };
    // "1" opens the first row and "0" the tenth.
    Some(if digit == 0 { 9 } else { digit - 1 })
}

pub(super) fn view(s: &Picker, cx: &mut Cx<Picker>) -> El {
    let dark = is_dark(s.state.ui_settings.visual_settings.theme, cx.system_dark());
    let t = tok::theme(dark);
    let highlight = t.text.alpha(if dark { 0.10 } else { 0.07 });

    let mut list = col().key("browsers").w_full();
    for (index, browser) in s.state.filtered_browsers.iter().enumerate() {
        list = list.child(browser_row(s, index, browser, &t, highlight));
    }

    let url = text(s.state.url.clone())
        .key("url")
        .font(type_::LABEL)
        .color(t.text_secondary)
        .truncate()
        .grow(1.0)
        .min_w(0.0)
        .on_press_tx(|s: &mut Picker, tx: &mut Tx| tx.copy(s.state.url.clone()));
    let options = row()
        .key("options")
        .h(24.0)
        .px(8.0)
        .radius(6.0)
        .center_y()
        .hover(highlight)
        .child(text("•••").font(type_::LABEL).color(t.text_secondary))
        .on_press_tx(|s: &mut Picker, tx: &mut Tx| s.open_options_menu(tx));
    let footer = row()
        .key("footer")
        .h(FOOTER_HEIGHT)
        .w_full()
        .px(6.0)
        .gap(4.0)
        .center_y()
        .child(url)
        .child(options);

    col()
        .key("picker")
        .w_full()
        .padding(PADDING)
        .radius(10.0)
        .clip_rounded(10.0)
        .fill(t.card)
        .border(1.0, t.text.alpha(0.12))
        .child(list)
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(footer)
}

fn browser_row(
    s: &Picker,
    index: usize,
    browser: &UIBrowser,
    t: &tok::Theme,
    highlight: Color,
) -> El {
    let selected = s.state.focused_index == Some(index);
    let name = if s.state.incognito_mode && browser.supports_incognito {
        format!("{} · Private", browser.browser_name)
    } else {
        browser.browser_name.clone()
    };

    let mut label = row()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .center_y()
        .child(text(name).font(type_::BODY).color(t.text));
    if browser.supports_profiles {
        label = label.child(
            text(format!("· {}", browser.profile_name))
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate()
                .shrink(),
        );
    }

    let mut browser_row = row()
        .key(format!("browser-{}", browser.unique_id))
        .h(ROW_HEIGHT)
        .w_full()
        .px(8.0)
        .gap(8.0)
        .radius(6.0)
        .center_y()
        .fill(if selected { highlight } else { Color::TRANSPARENT })
        .on_press_tx(move |s: &mut Picker, tx: &mut Tx| s.open_filtered(index, tx.mods().shift))
        .on_hover_change(move |s: &mut Picker, hovered, _| {
            if hovered {
                s.state.focused_index = Some(index);
            }
        })
        .on_secondary_press(move |s: &mut Picker, press, tx: &mut Tx| {
            s.open_browser_menu(index, press, tx)
        })
        .child(browser_icon(s, browser, t))
        .child(label);

    if s.state.ui_settings.visual_settings.show_hotkeys && index < 10 {
        let hotkey = if index == 9 { 0 } else { index + 1 };
        browser_row = browser_row.child(
            row()
                .h(18.0)
                .min_w(18.0)
                .px(4.0)
                .radius(4.0)
                .center()
                .border(1.0, t.text.alpha(0.15))
                .child(text(hotkey.to_string()).font(type_::LABEL).color(t.text_secondary)),
        );
    }
    browser_row
}

fn browser_icon(s: &Picker, browser: &UIBrowser, t: &tok::Theme) -> El {
    let icon = match s.icon(&browser_icon_source(browser)) {
        Some(image) if image.error().is_none() => {
            el().size(ICON_SIZE, ICON_SIZE).pixels(image.pixels())
        }
        _ => {
            let initial = browser.browser_name.chars().next().unwrap_or('?');
            el().size(ICON_SIZE, ICON_SIZE)
                .radius(6.0)
                .fill(t.control_off)
                .center()
                .child(text(initial.to_string()).font(type_::LABEL).color(t.text_secondary))
        }
    };

    let mut slot = el().size(ICON_SIZE, ICON_SIZE).child(icon);
    if let Some(profile_icon) = s.icon(&browser.profile_icon_path)
        && profile_icon.error().is_none()
    {
        slot = slot.child(
            el().size(PROFILE_ICON_SIZE, PROFILE_ICON_SIZE)
                .absolute(ICON_SIZE - 9.0, ICON_SIZE - 11.0)
                .pixels(profile_icon.pixels()),
        );
    }
    slot
}
