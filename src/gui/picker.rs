use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use rune::{
    AppHandle, Color, Cx, El, Image, ImageCache, Key, KeyEvent, Mods, PhysicalKeyCode,
    PointerPress, Tx, Waker, col, el, row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, Keybind, Kit, Menu, MenuItem, context_menu};
use tracing::{info, warn};

use super::model::{UIBrowser, UIState, get_filtered_browsers};
use super::placement;
use super::settings::AuxWindows;
use super::style;
use crate::{MessageToMain, MessageToUi, MoveTo};

pub(super) const PICKER_WIDTH: f32 = 296.0;
const ROW_HEIGHT: f32 = 36.0;
const FOOTER_HEIGHT: f32 = 30.0;
const PADDING: f32 = 4.0;
const ICON_SIZE: f32 = 24.0;
const PROFILE_ICON_SIZE: f32 = 12.0;
const MENU_WIDTH: f32 = 220.0;
const MENU_ROW_HEIGHT: f32 = 30.0;
const MENU_SEPARATOR_HEIGHT: f32 = 9.0;
const MENU_PADDING: f32 = 16.0;

pub(super) struct Picker {
    pub(super) state: UIState,
    main_sender: Sender<MessageToMain>,
    persistent: bool,
    me: AppHandle<Picker>,
    pub(super) aux: AuxWindows,
    icons: Option<Icons>,
    menu: Option<OpenMenu>,
    /// A native macOS menu is tracking; the panel may lose key status meanwhile.
    native_menu_open: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    Options,
    Browser(usize),
}

/// A menu drawn inside the picker window, which grows while it's open so the menu fits.
#[derive(Clone, Copy)]
struct OpenMenu {
    kind: MenuKind,
    at: Option<PointerPress>,
}

enum MenuLine {
    Item {
        id: String,
        label: String,
        enabled: bool,
    },
    Separator,
    Label(String),
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
            menu: None,
            native_menu_open: false,
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
        self.menu = None;
        if self.persistent {
            tx.close_window();
        } else {
            tx.quit();
        }
    }

    /// Hides the picker behind Settings or About; their windows end the app when they close.
    fn hide_for_aux(&mut self) {
        self.menu = None;
        self.me.hide();
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
                self.menu = None;
                if self.persistent {
                    self.show_at_cursor(activation_token);
                }
                self.send(MessageToMain::LinkOpenedFromBundle(source_bundle_id, url));
            }
            MessageToUi::BrowsersUpdated(browsers) => {
                self.state.browsers = Arc::new(browsers);
                self.state.filtered_browsers =
                    Arc::new(get_filtered_browsers(&self.state.url, &self.state.browsers));
                self.menu = None;
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
            && !self.native_menu_open
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
        if self.menu.is_some() {
            if event.key == Key::Escape {
                self.menu = None;
                return true;
            }
            return false;
        }

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
            Key::Command(',') => {
                self.aux.show_settings(&self.state);
                self.hide_for_aux();
            }
            Key::Command('c') => tx.copy(self.state.url.clone()),
            _ => return false,
        }
        true
    }

    fn menu_lines(&self, kind: MenuKind) -> Vec<MenuLine> {
        let item = |id: &str, label: &str, enabled: bool| MenuLine::Item {
            id: id.to_string(),
            label: label.to_string(),
            enabled,
        };
        let mut lines = Vec::new();
        match kind {
            MenuKind::Browser(index) => {
                let Some(browser) = self.state.filtered_browsers.get(index) else {
                    return lines;
                };
                if !browser.has_priority_ordering() {
                    lines.push(item("top", "Move to top", !browser.is_first));
                    lines.push(item("up", "Move up", !browser.is_first));
                    lines.push(item("down", "Move down", !browser.is_last));
                    lines.push(item("bottom", "Move to bottom", !browser.is_last));
                    lines.push(MenuLine::Separator);
                }
                lines.push(item("hide", "Hide profile", true));
                if browser.supports_profiles {
                    lines.push(item("hide-all", "Hide all profiles", true));
                }
            }
            MenuKind::Options => {
                lines.push(item("refresh", "Refresh applications", true));
                if self.state.show_set_as_default {
                    lines.push(item("make-default", "Make brows default", true));
                }
                if !self.state.restorable_app_profiles.is_empty() {
                    lines.push(MenuLine::Separator);
                    lines.push(MenuLine::Label("Restore hidden profiles".to_string()));
                    for profile in self.state.restorable_app_profiles.iter() {
                        lines.push(item(
                            &format!("restore:{}", profile.unique_id),
                            &profile.get_full_name(),
                            true,
                        ));
                    }
                }
                lines.push(MenuLine::Separator);
                lines.push(item("settings", "Settings…", true));
                lines.push(item("about", "About brows", true));
                lines.push(item("quit", "Quit", true));
            }
        }
        lines
    }

    fn pick(&mut self, kind: MenuKind, id: &str, tx: &mut Tx) {
        self.menu = None;
        match kind {
            MenuKind::Browser(index) => {
                let Some(browser) = self.state.filtered_browsers.get(index) else {
                    return;
                };
                let profile_id = browser.unique_id.clone();
                let message = match id {
                    "top" => MessageToMain::MoveAppProfile(profile_id, MoveTo::TOP),
                    "up" => MessageToMain::MoveAppProfile(profile_id, MoveTo::UP),
                    "down" => MessageToMain::MoveAppProfile(profile_id, MoveTo::DOWN),
                    "bottom" => MessageToMain::MoveAppProfile(profile_id, MoveTo::BOTTOM),
                    "hide" => MessageToMain::HideAppProfile(profile_id),
                    "hide-all" => MessageToMain::HideAllProfiles(browser.unique_app_id.clone()),
                    _ => return,
                };
                self.send(message);
            }
            MenuKind::Options => match id {
                "refresh" => self.send(MessageToMain::Refresh),
                "make-default" => self.send(MessageToMain::SetBrowsersAsDefaultBrowser),
                "settings" => {
                    self.aux.show_settings(&self.state);
                    self.hide_for_aux();
                }
                "about" => {
                    self.aux.show_about();
                    self.hide_for_aux();
                }
                "quit" => tx.quit(),
                _ => {
                    if let Some(profile_id) = id.strip_prefix("restore:") {
                        self.send(MessageToMain::RestoreAppProfile(profile_id.to_string()));
                    }
                }
            },
        }
    }

    /// macOS has good-looking native menus; elsewhere the menu is drawn in the picker window.
    fn open_menu(&mut self, kind: MenuKind, at: Option<PointerPress>, tx: &mut Tx) {
        if cfg!(target_os = "macos") {
            self.open_native_menu(kind, at, tx);
        } else {
            self.menu = Some(OpenMenu { kind, at });
        }
    }

    fn open_native_menu(&mut self, kind: MenuKind, at: Option<PointerPress>, tx: &mut Tx) {
        let mut menu = rune::PopupMenu::new();
        for line in self.menu_lines(kind) {
            menu = match line {
                MenuLine::Item { id, label, enabled } => {
                    menu.item(rune::PopupItem::new(id, label).enabled(enabled))
                }
                MenuLine::Separator => menu.separator(),
                MenuLine::Label(label) => menu.item(rune::PopupItem::new("", label).enabled(false)),
            };
        }
        self.native_menu_open = true;
        let on_pick = move |s: &mut Picker, picked: Option<&str>, tx: &mut Tx| {
            s.native_menu_open = false;
            if let Some(id) = picked.filter(|id| !id.is_empty()) {
                s.pick(kind, id, tx);
            }
        };
        match at {
            Some(press) => tx.popup_menu(menu, press, on_pick),
            None => tx.popup_menu(menu, "options", on_pick),
        }
    }

    fn open_menu_of(&self, kind: MenuKind) -> Option<OpenMenu> {
        self.menu.filter(|menu| menu.kind == kind)
    }

    /// The menu's entries; selecting one runs its action.
    fn kit_menu(&self, menu: Menu, kind: MenuKind) -> Menu {
        let mut menu = menu
            .min_width(MENU_WIDTH)
            .on_close(|s: &mut Picker| s.menu = None);
        for line in self.menu_lines(kind) {
            menu = match line {
                MenuLine::Item { id, label, enabled } => menu.item(
                    MenuItem::new(label)
                        .disabled(!enabled)
                        .on_select_tx(move |s: &mut Picker, tx: &mut Tx| s.pick(kind, &id, tx)),
                ),
                MenuLine::Separator => menu.separator(),
                MenuLine::Label(label) => menu.group_label(label),
            };
        }
        menu
    }

    /// Extra transparent room under the panel so an in-window menu fits in the window.
    fn menu_room(&self) -> f32 {
        let Some(menu) = self.menu else {
            return 0.0;
        };
        let menu_height: f32 = self
            .menu_lines(menu.kind)
            .iter()
            .map(|line| match line {
                MenuLine::Item { .. } => MENU_ROW_HEIGHT,
                MenuLine::Separator => MENU_SEPARATOR_HEIGHT,
                MenuLine::Label(_) => MENU_ROW_HEIGHT,
            })
            .sum::<f32>()
            + MENU_PADDING;
        let panel_height = picker_height(&self.state);
        // A context menu drops from the pointer; the options menu rises from the footer.
        let bottom = match menu.at {
            Some(press) => press.y + menu_height,
            None => menu_height + FOOTER_HEIGHT,
        };
        (bottom - panel_height).max(0.0)
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
    let k = style::kit(s.state.ui_settings.visual_settings.theme, cx);
    let mut layer = Vec::new();

    let mut list = col().key("browsers").w_full();
    for (index, browser) in s.state.filtered_browsers.iter().enumerate() {
        let browser_row = browser_row(s, index, browser, &k);
        list = list.child(match s.open_menu_of(MenuKind::Browser(index)) {
            Some(open) => s
                .kit_menu(
                    context_menu(format!("browser-menu-{index}"), browser_row, open.at),
                    MenuKind::Browser(index),
                )
                .build(&k, &mut layer),
            None => browser_row,
        });
    }

    let url = text(s.state.url.clone())
        .key("url")
        .font(k.ty.small)
        .color(k.c.text_muted)
        .truncate()
        .grow(1.0)
        .min_w(0.0)
        .on_press_tx(|s: &mut Picker, tx: &mut Tx| tx.copy(s.state.url.clone()));
    let options = Button::new("•••")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Regular)
        .on_press_tx(|s: &mut Picker, tx: &mut Tx| s.open_menu(MenuKind::Options, None, tx))
        .build(&k)
        .key("options");
    let options = match s.open_menu_of(MenuKind::Options) {
        Some(_) => s
            .kit_menu(
                Menu::new("options-menu").trigger(options).open(true),
                MenuKind::Options,
            )
            .build(&k, &mut layer),
        None => options,
    };
    let footer = row()
        .key("footer")
        .h(FOOTER_HEIGHT)
        .w_full()
        .px(6.0)
        .gap(4.0)
        .center_y()
        .child(url)
        .child(options);

    let panel = col()
        .key("picker")
        .w_full()
        .padding(PADDING)
        .radius(10.0)
        .clip_rounded(10.0)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .child(list)
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(footer);

    let body = col()
        .w_full()
        .child(panel)
        .child(el().key("menu-room").h(s.menu_room()));
    rune_kit::root(body, layer)
}

fn browser_row(s: &Picker, index: usize, browser: &UIBrowser, k: &Kit) -> El {
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
        .child(text(name).font(k.ty.body).color(k.c.text_base));
    if browser.supports_profiles {
        label = label.child(
            text(format!("· {}", browser.profile_name))
                .font(k.ty.small)
                .color(k.c.text_muted)
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
        .fill(if selected {
            k.c.hover
        } else {
            Color::TRANSPARENT
        })
        .on_press_tx(move |s: &mut Picker, tx: &mut Tx| s.open_filtered(index, tx.mods().shift))
        .on_hover_change(move |s: &mut Picker, hovered, _| {
            if hovered {
                s.state.focused_index = Some(index);
            }
        })
        .on_secondary_press(move |s: &mut Picker, press, tx: &mut Tx| {
            s.open_menu(MenuKind::Browser(index), Some(press), tx)
        })
        .child(browser_icon(s, browser, k))
        .child(label);

    if s.state.ui_settings.visual_settings.show_hotkeys && index < 10 {
        let hotkey = if index == 9 { 0 } else { index + 1 };
        browser_row = browser_row.child(Keybind::new(&[hotkey.to_string()]).build(k));
    }
    browser_row
}

fn browser_icon(s: &Picker, browser: &UIBrowser, k: &Kit) -> El {
    let icon = match s.icon(&browser_icon_source(browser)) {
        Some(image) if image.error().is_none() => {
            el().size(ICON_SIZE, ICON_SIZE).pixels(image.pixels())
        }
        _ => {
            let initial = browser.browser_name.chars().next().unwrap_or('?');
            el().size(ICON_SIZE, ICON_SIZE)
                .radius(6.0)
                .fill(k.c.layer_02)
                .center()
                .child(
                    text(initial.to_string())
                        .font(k.ty.small_medium)
                        .color(k.c.text_muted),
                )
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
