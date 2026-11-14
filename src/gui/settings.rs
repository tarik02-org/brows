use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use rune::tok::{self, type_};
use rune::{
    App, AppHandle, AppSpec, Color, Cx, El, PopupItem, PopupMenu, ScrollSpec, TextEdit, TextStyle, Tx,
    WindowHandle,
    WindowOptions, WindowSpec, col, el, row, spacer, text, text_input,
};
use tracing::info;

use super::model::{SettingsTab, UIBrowser, UIProfileAndIncognito, UISettingsRule, UIState};
use super::picker::{Picker, is_dark};
use crate::MessageToMain;
use crate::paths;
use crate::utils::ConfiguredTheme;

const SETTINGS_SIZE: (f32, f32) = (680.0, 500.0);
const ABOUT_SIZE: (f32, f32) = (380.0, 280.0);
const SOURCE_URL: &str = "https://github.com/tarik02-org/browsers-gpui";

/// The Settings and About windows, created hidden with the picker and shown on request.
pub(super) struct AuxWindows {
    settings: AppHandle<SettingsUi>,
    settings_window: WindowHandle,
    settings_open: Arc<AtomicBool>,
    about_window: WindowHandle,
    about_open: Arc<AtomicBool>,
}

impl AuxWindows {
    pub(super) fn create(
        app: &mut App<Picker>,
        state: &UIState,
        main_sender: Sender<MessageToMain>,
        picker: AppHandle<Picker>,
        persistent: bool,
    ) -> Self {
        let hidden = WindowOptions {
            visible: false,
            ..Default::default()
        };

        let settings_open = Arc::new(AtomicBool::new(false));
        let mut settings_state = SettingsUi {
            ui: None,
            rules: Vec::new(),
            main_sender,
            picker,
            open: settings_open.clone(),
            persistent,
        };
        settings_state.load(state.clone());
        let settings_app = App::new(AppSpec {
            min_size: Some((600.0, 420.0)),
            on_key: Some(close_on_escape),
            ..AppSpec::new("Browsers Settings", SETTINGS_SIZE, || unreachable!(), settings_view)
        })
        .init(move || settings_state)
        .on_close(|s, tx| {
            s.open.store(false, Ordering::Relaxed);
            if !s.persistent {
                tx.quit();
            }
        });
        let (settings, settings_window) =
            app.window(WindowSpec::from_app("settings", settings_app).options(hidden.clone()));

        let about_open = Arc::new(AtomicBool::new(false));
        let about_state = About {
            open: about_open.clone(),
            persistent,
        };
        let about_app = App::new(AppSpec {
            min_size: Some(ABOUT_SIZE),
            on_key: Some(close_on_escape),
            ..AppSpec::new("About Browsers", ABOUT_SIZE, || unreachable!(), about_view)
        })
        .init(move || about_state)
        .on_close(|s, tx| {
            s.open.store(false, Ordering::Relaxed);
            if !s.persistent {
                tx.quit();
            }
        });
        let (_, about_window) = app.window(
            WindowSpec::from_app("about", about_app).options(WindowOptions {
                resizable: false,
                ..hidden
            }),
        );

        Self {
            settings,
            settings_window,
            settings_open,
            about_window,
            about_open,
        }
    }

    pub(super) fn any_open(&self) -> bool {
        self.settings_open.load(Ordering::Relaxed) || self.about_open.load(Ordering::Relaxed)
    }

    pub(super) fn show_settings(&self, state: &UIState) {
        let state = state.clone();
        self.settings.update(move |s, _| s.load(state));
        self.settings_open.store(true, Ordering::Relaxed);
        self.settings_window.open();
    }

    pub(super) fn show_about(&self) {
        self.about_open.store(true, Ordering::Relaxed);
        self.about_window.open();
    }
}

fn close_on_escape<S>(_: &mut S, event: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if event.key == rune::Key::Escape {
        tx.close_window();
        return true;
    }
    false
}

struct RuleEditor {
    source_app: TextEdit,
    url_pattern: TextEdit,
    opener: Option<UIProfileAndIncognito>,
}

struct SettingsUi {
    ui: Option<UIState>,
    rules: Vec<RuleEditor>,
    main_sender: Sender<MessageToMain>,
    picker: AppHandle<Picker>,
    open: Arc<AtomicBool>,
    persistent: bool,
}

impl SettingsUi {
    fn load(&mut self, state: UIState) {
        self.rules = state
            .ui_settings
            .rules
            .iter()
            .filter(|rule| !rule.deleted)
            .map(|rule| RuleEditor {
                source_app: TextEdit::new(rule.source_app.clone()),
                url_pattern: TextEdit::new(rule.url_pattern.clone()),
                opener: rule.opener.clone(),
            })
            .collect();
        self.ui = Some(state);
    }

    fn send(&self, message: MessageToMain) {
        self.main_sender.send(message).ok();
    }

    fn ui(&mut self) -> &mut UIState {
        self.ui.as_mut().expect("settings shown before load")
    }

    fn save_visual_settings(&mut self) {
        let visual = self.ui().ui_settings.visual_settings.clone();
        let for_picker = visual.clone();
        self.picker
            .update(move |p, _| p.state.ui_settings.visual_settings = for_picker);
        self.send(MessageToMain::SaveConfigUISettings(visual));
    }

    fn set_unwrap_urls(&mut self, unwrap: bool) {
        self.ui().ui_settings.behavioral_settings.unwrap_urls = unwrap;
        let behavior = self.ui().ui_settings.behavioral_settings.clone();
        self.picker
            .update(move |p, _| p.state.ui_settings.behavioral_settings.unwrap_urls = unwrap);
        self.send(MessageToMain::SaveConfigUIBehavioralSettings(behavior));
    }

    fn save_rules(&mut self) {
        let rules: Vec<UISettingsRule> = self
            .rules
            .iter()
            .enumerate()
            .map(|(index, editor)| UISettingsRule {
                index,
                saved: true,
                deleted: false,
                source_app: editor.source_app.text.clone(),
                url_pattern: editor.url_pattern.text.clone(),
                opener: editor.opener.clone(),
            })
            .collect();
        self.ui().ui_settings.rules = Arc::new(rules.clone());
        let for_picker = rules.clone();
        self.picker
            .update(move |p, _| p.state.ui_settings.rules = Arc::new(for_picker));
        info!("Saving {} rules", rules.len());
        self.send(MessageToMain::SaveConfigRules(rules));
    }

    fn set_default_opener(&mut self, opener: Option<UIProfileAndIncognito>) {
        self.ui().ui_settings.default_opener = opener.clone();
        let for_picker = opener.clone();
        self.picker
            .update(move |p, _| p.state.ui_settings.default_opener = for_picker);
        self.send(MessageToMain::SaveConfigDefaultOpener(opener));
    }
}

/// `rune::tok` styles, plus the accent for primary controls.
struct Style {
    t: tok::Theme,
    accent: Color,
}

impl Style {
    fn new<S: 'static>(theme: ConfiguredTheme, cx: &mut Cx<S>) -> Self {
        let dark = is_dark(theme, cx.system_dark());
        Self {
            t: tok::theme(dark),
            accent: tok::accent(0, dark),
        }
    }

    fn button(&self, key: impl Into<String>, label: impl Into<String>, primary: bool) -> El {
        let (fill, color) = if primary {
            (self.accent, Color::WHITE)
        } else {
            (self.t.control_off, self.t.text)
        };
        row()
            .key(key)
            .h(26.0)
            .px(10.0)
            .radius(6.0)
            .center()
            .fill(fill)
            .hover(fill.alpha(0.85))
            .child(text(label).font(type_::LABEL).color(color))
    }

    fn switch(&self, key: impl Into<String>, on: bool, enabled: bool) -> El {
        let knob = el()
            .size(16.0, 16.0)
            .radius(8.0)
            .fill(self.t.knob)
            .absolute(if on { 16.0 } else { 2.0 }, 2.0);
        el().key(key)
            .role("switch")
            .checked(on)
            .size(34.0, 20.0)
            .radius(10.0)
            .fill(if on { self.accent } else { self.t.control_off })
            .opacity(if enabled { 1.0 } else { 0.4 })
            .child(knob)
    }

    fn setting_row(&self, title: &str, description: &str, control: El) -> El {
        row()
            .w_full()
            .py(6.0)
            .gap(12.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(title).font(type_::TITLE).color(self.t.text))
                    .child(text(description).font(type_::LABEL).color(self.t.text_secondary)),
            )
            .child(control)
    }

    fn segmented<S: 'static>(
        &self,
        key: &str,
        labels: &[&str],
        selected: usize,
        pick: fn(&mut S, usize),
    ) -> El {
        let mut segments = row()
            .key(key.to_string())
            .h(28.0)
            .padding(2.0)
            .gap(2.0)
            .radius(7.0)
            .fill(self.t.control_track);
        for (index, label) in labels.iter().enumerate() {
            let active = index == selected;
            segments = segments.child(
                row()
                    .key(format!("{key}-{index}"))
                    .h_full()
                    .px(12.0)
                    .radius(5.0)
                    .center()
                    .fill(if active { self.t.text.alpha(0.14) } else { Color::TRANSPARENT })
                    .child(text(*label).font(type_::LABEL).color(self.t.text))
                    .on_press(move |s: &mut S| pick(s, index)),
            );
        }
        segments
    }
}

fn settings_view(s: &SettingsUi, cx: &mut Cx<SettingsUi>) -> El {
    let Some(ui) = &s.ui else {
        return el();
    };
    let st = Style::new(ui.ui_settings.visual_settings.theme, cx);
    let tab = ui.ui_settings.tab;

    let tab_index = match tab {
        SettingsTab::General => 0,
        SettingsTab::Rules => 1,
        SettingsTab::Advanced => 2,
    };
    let tabs = st.segmented(
        "tabs",
        &["General", "Rules", "Advanced"],
        tab_index,
        |s: &mut SettingsUi, index| {
            s.ui().ui_settings.tab = match index {
                1 => SettingsTab::Rules,
                2 => SettingsTab::Advanced,
                _ => SettingsTab::General,
            };
        },
    );

    let content = match tab {
        SettingsTab::General => general_tab(ui, &st),
        SettingsTab::Rules => rules_tab(s, ui, &st),
        SettingsTab::Advanced => advanced_tab(ui, &st),
    };

    col()
        .w_full()
        .h_full()
        .fill(st.t.bg)
        .child(
            row()
                .h(44.0)
                .w_full()
                .px(12.0)
                .center_y()
                .child(tabs),
        )
        .child(el().h(1.0).w_full().fill(st.t.separator))
        .child(col().w_full().grow(1.0).min_h(0.0).padding(12.0).child(content))
}

fn general_tab(ui: &UIState, st: &Style) -> El {
    let visual = &ui.ui_settings.visual_settings;
    let theme_index = match visual.theme {
        ConfiguredTheme::Auto => 0,
        ConfiguredTheme::Light => 1,
        ConfiguredTheme::Dark => 2,
    };
    let theme = st.segmented(
        "theme",
        &["System", "Light", "Dark"],
        theme_index,
        |s: &mut SettingsUi, index| {
            s.ui().ui_settings.visual_settings.theme = match index {
                1 => ConfiguredTheme::Light,
                2 => ConfiguredTheme::Dark,
                _ => ConfiguredTheme::Auto,
            };
            s.save_visual_settings();
        },
    );

    col()
        .w_full()
        .gap(6.0)
        .child(st.setting_row(
            "Appearance",
            "Follow the system or choose a fixed theme",
            theme,
        ))
        .child(st.setting_row(
            "Keyboard hints",
            "Show number shortcuts in the browser picker",
            st.switch("show-hotkeys", visual.show_hotkeys, true)
                .on_press(|s: &mut SettingsUi| {
                    let visual = &mut s.ui().ui_settings.visual_settings;
                    visual.show_hotkeys = !visual.show_hotkeys;
                    s.save_visual_settings();
                }),
        ))
        .child(st.setting_row(
            "Close when focus is lost",
            "Dismiss the picker after switching to another app",
            st.switch("quit-on-blur", visual.quit_on_lost_focus, true)
                .on_press(|s: &mut SettingsUi| {
                    let visual = &mut s.ui().ui_settings.visual_settings;
                    visual.quit_on_lost_focus = !visual.quit_on_lost_focus;
                    s.save_visual_settings();
                }),
        ))
        .child(st.setting_row(
            "Unwrap tracking links",
            "Open the destination behind Outlook Safe Links and Messenger redirects",
            st.switch(
                "unwrap-urls",
                ui.ui_settings.behavioral_settings.unwrap_urls,
                true,
            )
            .on_press(|s: &mut SettingsUi| {
                let unwrap = !s.ui().ui_settings.behavioral_settings.unwrap_urls;
                s.set_unwrap_urls(unwrap);
            }),
        ))
}

fn opener_label(opener: &Option<UIProfileAndIncognito>, browsers: &[UIBrowser]) -> String {
    opener
        .as_ref()
        .and_then(|opener| {
            browsers
                .iter()
                .find(|browser| browser.unique_id == opener.profile)
                .map(|browser| {
                    let suffix = if opener.incognito { " · Private" } else { "" };
                    format!("{}{}", browser.get_full_name(), suffix)
                })
        })
        .unwrap_or_else(|| "Choose an opener".to_string())
}

fn opener_menu(browsers: &[UIBrowser], with_ask: bool) -> PopupMenu {
    let mut menu = PopupMenu::new();
    if with_ask {
        menu = menu.item(PopupItem::new("", "Ask every time")).separator();
    }
    for browser in browsers {
        menu = menu.item(PopupItem::new(browser.unique_id.clone(), browser.get_full_name()));
    }
    menu
}

fn rules_tab(s: &SettingsUi, ui: &UIState, st: &Style) -> El {
    let default_opener = &ui.ui_settings.default_opener;
    let default_private = default_opener.as_ref().is_some_and(|opener| opener.incognito);

    let fallback = row()
        .w_full()
        .padding(8.0)
        .gap(12.0)
        .radius(8.0)
        .center_y()
        .fill(st.t.card)
        .child(
            col()
                .grow(1.0)
                .gap(2.0)
                .child(text("Fallback opener").font(type_::TITLE).color(st.t.text))
                .child(text("Used when no rule matches").font(type_::LABEL).color(st.t.text_secondary)),
        )
        .child(
            st.button("default-opener", opener_label(default_opener, &ui.browsers), false)
                .on_press_tx(|s: &mut SettingsUi, tx: &mut Tx| {
                    let menu = opener_menu(&s.ui().browsers.clone(), true);
                    tx.popup_menu(menu, "default-opener", |s: &mut SettingsUi, picked, _| {
                        let Some(picked) = picked else { return };
                        let incognito = s
                            .ui()
                            .ui_settings
                            .default_opener
                            .as_ref()
                            .is_some_and(|opener| opener.incognito);
                        let opener = (!picked.is_empty()).then(|| UIProfileAndIncognito {
                            profile: picked.to_string(),
                            incognito,
                        });
                        s.set_default_opener(opener);
                    });
                }),
        )
        .child(text("Private").font(type_::LABEL).color(st.t.text_secondary))
        .child(
            st.switch("default-private", default_private, default_opener.is_some())
                .on_press(|s: &mut SettingsUi| {
                    if let Some(mut opener) = s.ui().ui_settings.default_opener.clone() {
                        opener.incognito = !opener.incognito;
                        s.set_default_opener(Some(opener));
                    }
                }),
        );

    let header = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(text("Opening rules").font(type_::TITLE).color(st.t.text))
        .child(text(format!("{} · First match wins", s.rules.len())).font(type_::LABEL).color(st.t.text_secondary))
        .child(spacer())
        .child(st.button("add-rule", "Add rule", false).on_press(|s: &mut SettingsUi| {
            s.rules.push(RuleEditor {
                source_app: TextEdit::new(""),
                url_pattern: TextEdit::new(""),
                opener: None,
            });
        }))
        .child(st.button("save-rules", "Save changes", true).on_press(|s: &mut SettingsUi| s.save_rules()));

    let mut list = col().key("rules").w_full().gap(8.0).scroll(ScrollSpec { stick_bottom: false });
    if s.rules.is_empty() {
        list = list.child(
            col()
                .w_full()
                .padding(24.0)
                .gap(4.0)
                .center()
                .radius(8.0)
                .border(1.0, st.t.separator)
                .child(text("No opening rules").font(type_::TITLE).color(st.t.text_secondary))
                .child(text("Add one to route matching links automatically").font(type_::LABEL).color(st.t.text_secondary)),
        );
    }
    for (index, rule) in s.rules.iter().enumerate() {
        list = list.child(rule_card(index, rule, ui, st));
    }

    col()
        .w_full()
        .h_full()
        .gap(12.0)
        .child(fallback)
        .child(header)
        .child(col().w_full().grow(1.0).min_h(0.0).child(list))
}

fn rule_card(index: usize, rule: &RuleEditor, ui: &UIState, st: &Style) -> El {
    let field = |edit: &TextEdit, key: String, placeholder: &str| {
        // A line as tall as the field centres the value and the placeholder in it.
        text_input(edit, type_::BODY.lh(26.0))
            .key(key)
            .single_line()
            .placeholder_font(type_::BODY.lh(26.0))
            .px(8.0)
            .radius(6.0)
            .fill(st.t.control_track)
            .border(1.0, st.t.separator)
            .color(st.t.text)
            .placeholder(placeholder, st.t.text_secondary)
    };
    let source = field(&rule.source_app, format!("rule-{index}-source"), "Source application (optional)")
        .w(220.0)
        .on_edit(move |s: &mut SettingsUi| &mut s.rules[index].source_app);
    let pattern = field(&rule.url_pattern, format!("rule-{index}-pattern"), "URL pattern, e.g. example.com/**")
        .grow(1.0)
        .on_edit(move |s: &mut SettingsUi| &mut s.rules[index].url_pattern);

    let opener_key = format!("rule-{index}-opener");
    let private = rule.opener.as_ref().is_some_and(|opener| opener.incognito);
    let badge = |label: &str, fill: Color| {
        row()
            .w(48.0)
            .h(24.0)
            .radius(5.0)
            .center()
            .fill(fill)
            .child(text(label).font(type_::SECTION).color(Color::WHITE))
    };

    col()
        .key(format!("rule-{index}"))
        .w_full()
        .padding(8.0)
        .gap(8.0)
        .radius(8.0)
        .fill(st.t.card)
        .child(
            row()
                .w_full()
                .center_y()
                .child(text(format!("RULE {:02}", index + 1)).font(type_::SECTION).color(st.t.text_secondary))
                .child(spacer())
                .child(
                    st.button(format!("rule-{index}-delete"), "Delete", false)
                        .on_press(move |s: &mut SettingsUi| {
                            if index < s.rules.len() {
                                s.rules.remove(index);
                                s.save_rules();
                            }
                        }),
                ),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(badge("WHEN", st.t.indicator))
                .child(source)
                .child(text("AND").font(type_::LABEL).color(st.t.text_secondary))
                .child(pattern),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(badge("THEN", st.accent))
                .child(
                    st.button(opener_key.clone(), opener_label(&rule.opener, &ui.browsers), false)
                        .on_press_tx(move |s: &mut SettingsUi, tx: &mut Tx| {
                            let menu = opener_menu(&s.ui().browsers.clone(), false);
                            tx.popup_menu(menu, opener_key.as_str(), move |s: &mut SettingsUi, picked, _| {
                                let (Some(picked), Some(rule)) = (picked, s.rules.get_mut(index)) else {
                                    return;
                                };
                                let incognito = rule.opener.as_ref().is_some_and(|opener| opener.incognito);
                                rule.opener = Some(UIProfileAndIncognito {
                                    profile: picked.to_string(),
                                    incognito,
                                });
                            });
                        }),
                )
                .child(text("Private").font(type_::LABEL).color(st.t.text_secondary))
                .child(
                    st.switch(format!("rule-{index}-private"), private, rule.opener.is_some())
                        .on_press(move |s: &mut SettingsUi| {
                            if let Some(opener) = s.rules.get_mut(index).and_then(|rule| rule.opener.as_mut()) {
                                opener.incognito = !opener.incognito;
                            }
                        }),
                ),
        )
}

fn advanced_tab(ui: &UIState, st: &Style) -> El {
    let mut content = col()
        .w_full()
        .gap(6.0)
        .child(st.setting_row(
            "Default browser",
            "Register Browsers as the system handler for web links",
            st.button("make-default", "Make default", true).on_press(|s: &mut SettingsUi| {
                s.send(MessageToMain::SetBrowsersAsDefaultBrowser)
            }),
        ))
        .child(st.setting_row(
            "Installed applications",
            "Rescan browser profiles and desktop URL handlers",
            st.button("refresh-apps", "Refresh", false)
                .on_press(|s: &mut SettingsUi| s.send(MessageToMain::Refresh)),
        ));

    if !ui.restorable_app_profiles.is_empty() {
        let mut hidden = col()
            .w_full()
            .gap(4.0)
            .child(text("Hidden profiles").font(type_::TITLE).color(st.t.text));
        for profile in ui.restorable_app_profiles.iter() {
            let id = profile.unique_id.clone();
            hidden = hidden.child(
                row()
                    .w_full()
                    .py(2.0)
                    .center_y()
                    .child(text(profile.get_full_name()).font(type_::BODY).color(st.t.text).grow(1.0))
                    .child(
                        st.button(format!("restore-{id}"), "Restore", false)
                            .on_press(move |s: &mut SettingsUi| {
                                s.send(MessageToMain::RestoreAppProfile(id.clone()))
                            }),
                    ),
            );
        }
        content = content.child(hidden);
    }

    let path_line = |label: &str, path: std::path::PathBuf| {
        text(format!("{label}: {}", path.display()))
            .font(type_::LABEL)
            .color(st.t.text_secondary)
            .selectable(true)
    };
    content
        .child(el().h(1.0).w_full().fill(st.t.separator))
        .child(path_line("Configuration", paths::get_config_json_path()))
        .child(path_line("Cache", paths::get_cache_root_dir()))
        .child(path_line("Logs", paths::get_logs_root_dir()))
}

struct About {
    open: Arc<AtomicBool>,
    persistent: bool,
}

fn about_view(_: &About, cx: &mut Cx<About>) -> El {
    let st = Style::new(ConfiguredTheme::Auto, cx);
    col()
        .w_full()
        .h_full()
        .padding(16.0)
        .gap(8.0)
        .center()
        .fill(st.t.bg)
        .child(text("Browsers").font(TextStyle::new(22.0, 600.0)).color(st.t.text))
        .child(
            text(format!("Version {}", env!("CARGO_PKG_VERSION")))
                .font(type_::LABEL)
                .color(st.t.text_secondary),
        )
        .child(text("Open the right browser at the right time.").font(type_::BODY).color(st.t.text))
        .child(
            row()
                .gap(8.0)
                .mt(8.0)
                .child(st.button("source", "Source code", false).on_press_tx(|_: &mut About, tx: &mut Tx| {
                    tx.open_url(SOURCE_URL).ok();
                }))
                .child(st.button("close", "Close", true).on_press_tx(|_: &mut About, tx: &mut Tx| {
                    tx.close_window()
                })),
        )
}
