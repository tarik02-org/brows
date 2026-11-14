use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use rune::{
    App, AppHandle, AppSpec, Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, WindowHandle,
    WindowOptions, WindowSpec, col, el, row, spacer, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Kit, SegmentedControl, Select, Switch, TextInput,
};
use tracing::info;

use super::model::{SettingsTab, UIBrowser, UIProfileAndIncognito, UISettingsRule, UIState};
use super::picker::Picker;
use super::style;
use crate::MessageToMain;
use crate::paths;
use crate::utils::ConfiguredTheme;

const SETTINGS_SIZE: (f32, f32) = (680.0, 500.0);
const ABOUT_SIZE: (f32, f32) = (380.0, 280.0);
const SOURCE_URL: &str = "https://github.com/tarik02-org/brows";
const ASK_EVERY_TIME: &str = "Ask every time";

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
            open_select: None,
            main_sender,
            picker,
            open: settings_open.clone(),
            persistent,
        };
        settings_state.load(state.clone());
        let settings_app = App::new(AppSpec {
            min_size: Some((600.0, 420.0)),
            on_key: Some(close_on_escape),
            ..AppSpec::new("brows Settings", SETTINGS_SIZE, || unreachable!(), settings_view)
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
            theme: state.ui_settings.visual_settings.theme,
        };
        let about_app = App::new(AppSpec {
            min_size: Some(ABOUT_SIZE),
            on_key: Some(close_on_escape),
            ..AppSpec::new("About brows", ABOUT_SIZE, || unreachable!(), about_view)
        })
        .init(move || about_state)
        .on_close(|s, tx| {
            s.open.store(false, Ordering::Relaxed);
            if !s.persistent {
                tx.quit();
            }
        });
        let (_, about_window) =
            app.window(WindowSpec::from_app("about", about_app).options(WindowOptions {
                resizable: false,
                ..hidden
            }));

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

fn close_on_escape<S>(_: &mut S, event: &KeyEvent, tx: &mut Tx) -> bool {
    if event.key == Key::Escape {
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
    /// The key of the opener select whose list is showing.
    open_select: Option<String>,
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
        self.open_select = None;
        self.ui = Some(state);
    }

    fn send(&self, message: MessageToMain) {
        self.main_sender.send(message).ok();
    }

    fn ui(&mut self) -> &mut UIState {
        self.ui.as_mut().expect("settings are loaded at creation")
    }

    fn toggle_select(&mut self, key: &str) {
        self.open_select = match self.open_select.as_deref() {
            Some(open) if open == key => None,
            _ => Some(key.to_string()),
        };
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

    /// The profile a select's label names; labels are profiles' full names.
    fn profile_named(&mut self, label: &str) -> Option<String> {
        self.ui()
            .browsers
            .iter()
            .find(|browser| browser.get_full_name() == label)
            .map(|browser| browser.unique_id.clone())
    }
}

fn section_title(k: &Kit, title: &str) -> El {
    text(title).font(k.ty.ui_medium).color(k.c.text_base)
}

fn setting_row(k: &Kit, title: &str, description: &str, control: El) -> El {
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
                .child(section_title(k, title))
                .child(text(description).font(k.ty.small).color(k.c.text_muted)),
        )
        .child(control)
}

fn card(k: &Kit) -> El {
    col()
        .w_full()
        .padding(10.0)
        .gap(8.0)
        .radius(k.radius.md)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_muted)
}

fn opener_label(opener: &Option<UIProfileAndIncognito>, browsers: &[UIBrowser]) -> Option<String> {
    let opener = opener.as_ref()?;
    browsers
        .iter()
        .find(|browser| browser.unique_id == opener.profile)
        .map(UIBrowser::get_full_name)
}

fn settings_view(s: &SettingsUi, cx: &mut Cx<SettingsUi>) -> El {
    let Some(ui) = &s.ui else {
        return el();
    };
    let k = style::kit(ui.ui_settings.visual_settings.theme, cx);
    let mut layer = Vec::new();

    let tab_index = match ui.ui_settings.tab {
        SettingsTab::General => 0,
        SettingsTab::Rules => 1,
        SettingsTab::Advanced => 2,
    };
    let tabs = SegmentedControl::new(&["General", "Rules", "Advanced"], Some(tab_index))
        .label("Settings section")
        .fit()
        .on_pick(|s: &mut SettingsUi, index, _: &mut Tx| {
            s.open_select = None;
            s.ui().ui_settings.tab = match index {
                1 => SettingsTab::Rules,
                2 => SettingsTab::Advanced,
                _ => SettingsTab::General,
            };
        })
        .build(&k)
        .key("tabs");

    let content = match ui.ui_settings.tab {
        SettingsTab::General => general_tab(ui, &k),
        SettingsTab::Rules => rules_tab(s, ui, &k, &mut layer),
        SettingsTab::Advanced => advanced_tab(ui, &k),
    };

    let body = col()
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .child(row().h(48.0).w_full().px(12.0).center_y().child(tabs))
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .padding(12.0)
                .child(content),
        );
    rune_kit::root(body, layer)
}

fn general_tab(ui: &UIState, k: &Kit) -> El {
    let visual = &ui.ui_settings.visual_settings;
    let theme_index = match visual.theme {
        ConfiguredTheme::Auto => 0,
        ConfiguredTheme::Light => 1,
        ConfiguredTheme::Dark => 2,
    };
    let theme = SegmentedControl::new(&["System", "Light", "Dark"], Some(theme_index))
        .label("Appearance")
        .fit()
        .on_pick(|s: &mut SettingsUi, index, _: &mut Tx| {
            s.ui().ui_settings.visual_settings.theme = match index {
                1 => ConfiguredTheme::Light,
                2 => ConfiguredTheme::Dark,
                _ => ConfiguredTheme::Auto,
            };
            s.save_visual_settings();
        })
        .build(k)
        .key("theme");

    col()
        .w_full()
        .gap(6.0)
        .child(setting_row(
            k,
            "Appearance",
            "Follow the system or choose a fixed theme",
            theme,
        ))
        .child(setting_row(
            k,
            "Keyboard hints",
            "Show number shortcuts in the browser picker",
            Switch::new(visual.show_hotkeys)
                .label("Keyboard hints")
                .hide_label()
                .on_toggle(|s: &mut SettingsUi| {
                    let visual = &mut s.ui().ui_settings.visual_settings;
                    visual.show_hotkeys = !visual.show_hotkeys;
                    s.save_visual_settings();
                })
                .build(k)
                .key("show-hotkeys"),
        ))
        .child(setting_row(
            k,
            "Close when focus is lost",
            "Dismiss the picker after switching to another app",
            Switch::new(visual.quit_on_lost_focus)
                .label("Close when focus is lost")
                .hide_label()
                .on_toggle(|s: &mut SettingsUi| {
                    let visual = &mut s.ui().ui_settings.visual_settings;
                    visual.quit_on_lost_focus = !visual.quit_on_lost_focus;
                    s.save_visual_settings();
                })
                .build(k)
                .key("quit-on-blur"),
        ))
        .child(setting_row(
            k,
            "Unwrap tracking links",
            "Open the destination behind Outlook Safe Links and Messenger redirects",
            Switch::new(ui.ui_settings.behavioral_settings.unwrap_urls)
                .label("Unwrap tracking links")
                .hide_label()
                .on_toggle(|s: &mut SettingsUi| {
                    let unwrap = !s.ui().ui_settings.behavioral_settings.unwrap_urls;
                    s.set_unwrap_urls(unwrap);
                })
                .build(k)
                .key("unwrap-urls"),
        ))
}

fn private_switch(
    k: &Kit,
    key: String,
    on: bool,
    enabled: bool,
    toggle: impl Fn(&mut SettingsUi) + 'static,
) -> El {
    Switch::new(on)
        .label("Private")
        .disabled(!enabled)
        .on_toggle(toggle)
        .build(k)
        .key(key)
}

fn rules_tab(s: &SettingsUi, ui: &UIState, k: &Kit, layer: &mut Vec<El>) -> El {
    let profile_labels: Vec<String> = ui.browsers.iter().map(UIBrowser::get_full_name).collect();
    let default_opener = &ui.ui_settings.default_opener;

    let mut default_choices = vec![ASK_EVERY_TIME.to_string()];
    default_choices.extend(profile_labels.iter().cloned());
    let default_current = opener_label(default_opener, &ui.browsers);
    let default_select = Select::new("default-opener", &default_choices)
        .label("Fallback opener")
        .width(240.0)
        .current(Some(default_current.as_deref().unwrap_or(ASK_EVERY_TIME)))
        .open(s.open_select.as_deref() == Some("default-opener"))
        .on_toggle(|s: &mut SettingsUi| s.toggle_select("default-opener"))
        .on_close(|s: &mut SettingsUi| s.open_select = None)
        .on_choose(|s: &mut SettingsUi, label: &str, _: &mut Tx| {
            s.open_select = None;
            let incognito = s
                .ui()
                .ui_settings
                .default_opener
                .as_ref()
                .is_some_and(|opener| opener.incognito);
            let opener = s
                .profile_named(label)
                .map(|profile| UIProfileAndIncognito { profile, incognito });
            s.set_default_opener(opener);
        })
        .build(k, layer);

    let fallback = card(k).child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .gap(2.0)
                    .child(section_title(k, "Fallback opener"))
                    .child(
                        text("Used when no rule matches")
                            .font(k.ty.small)
                            .color(k.c.text_muted),
                    ),
            )
            .child(default_select)
            .child(private_switch(
                k,
                "default-private".to_string(),
                default_opener
                    .as_ref()
                    .is_some_and(|opener| opener.incognito),
                default_opener.is_some(),
                |s: &mut SettingsUi| {
                    if let Some(mut opener) = s.ui().ui_settings.default_opener.clone() {
                        opener.incognito = !opener.incognito;
                        s.set_default_opener(Some(opener));
                    }
                },
            )),
    );

    let header = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(section_title(k, "Opening rules"))
        .child(
            text(format!("{} · First match wins", s.rules.len()))
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
        .child(spacer())
        .child(
            Button::new("Add rule")
                .control_size(ControlSize::Regular)
                .on_press(|s: &mut SettingsUi| {
                    s.rules.push(RuleEditor {
                        source_app: TextEdit::new(""),
                        url_pattern: TextEdit::new(""),
                        opener: None,
                    });
                })
                .build(k)
                .key("add-rule"),
        )
        .child(
            Button::new("Save changes")
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Regular)
                .on_press(|s: &mut SettingsUi| s.save_rules())
                .build(k)
                .key("save-rules"),
        );

    let mut list = col().key("rules").w_full().gap(8.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    if s.rules.is_empty() {
        list = list.child(
            col()
                .w_full()
                .padding(24.0)
                .gap(4.0)
                .center()
                .radius(k.radius.md)
                .border(1.0, k.c.border_muted)
                .child(
                    text("No opening rules")
                        .font(k.ty.ui_medium)
                        .color(k.c.text_muted),
                )
                .child(
                    text("Add one to route matching links automatically")
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                ),
        );
    }
    for (index, rule) in s.rules.iter().enumerate() {
        list = list.child(rule_card(s, index, rule, ui, &profile_labels, k, layer));
    }

    col()
        .w_full()
        .h_full()
        .gap(12.0)
        .child(fallback)
        .child(header)
        .child(col().w_full().grow(1.0).min_h(0.0).child(list))
}

fn rule_card(
    s: &SettingsUi,
    index: usize,
    rule: &RuleEditor,
    ui: &UIState,
    profile_labels: &[String],
    k: &Kit,
    layer: &mut Vec<El>,
) -> El {
    let source = TextInput::new(&rule.source_app)
        .placeholder("Source application (optional)")
        .label("Source application")
        .width(Some(220.0))
        .on_edit(move |s: &mut SettingsUi| &mut s.rules[index].source_app)
        .build(k)
        .key(format!("rule-{index}-source"));
    let pattern = el().grow(1.0).min_w(0.0).child(
        TextInput::new(&rule.url_pattern)
            .placeholder("URL pattern, e.g. example.com/**")
            .label("URL pattern")
            .width(None)
            .on_edit(move |s: &mut SettingsUi| &mut s.rules[index].url_pattern)
            .build(k)
            .key(format!("rule-{index}-pattern")),
    );

    let select_key = format!("rule-{index}-opener");
    let toggle_key = select_key.clone();
    let opener = Select::new(select_key.clone(), profile_labels)
        .label("Opener")
        .placeholder("Choose an opener")
        .width(240.0)
        .current(opener_label(&rule.opener, &ui.browsers).as_deref())
        .open(s.open_select.as_deref() == Some(select_key.as_str()))
        .on_toggle(move |s: &mut SettingsUi| s.toggle_select(&toggle_key))
        .on_close(|s: &mut SettingsUi| s.open_select = None)
        .on_choose(move |s: &mut SettingsUi, label: &str, _: &mut Tx| {
            s.open_select = None;
            let Some(profile) = s.profile_named(label) else {
                return;
            };
            if let Some(rule) = s.rules.get_mut(index) {
                let incognito = rule.opener.as_ref().is_some_and(|opener| opener.incognito);
                rule.opener = Some(UIProfileAndIncognito { profile, incognito });
            }
        })
        .build(k, layer);

    let badge = |label: &str, fill, ink| {
        row()
            .w(52.0)
            .h(24.0)
            .radius(k.radius.sm)
            .center()
            .fill(fill)
            .child(text(label).font(k.ty.text_xsmall_medium).color(ink))
    };

    card(k)
        .key(format!("rule-{index}"))
        .child(
            row()
                .w_full()
                .center_y()
                .child(
                    text(format!("RULE {:02}", index + 1))
                        .font(k.ty.text_xsmall_medium)
                        .color(k.c.text_muted),
                )
                .child(spacer())
                .child(
                    Button::new("Delete")
                        .variant(ButtonVariant::GhostMuted)
                        .control_size(ControlSize::Regular)
                        .on_press(move |s: &mut SettingsUi| {
                            if index < s.rules.len() {
                                s.rules.remove(index);
                                s.open_select = None;
                                s.save_rules();
                            }
                        })
                        .build(k)
                        .key(format!("rule-{index}-delete")),
                ),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(badge("WHEN", k.c.bg_contrast, k.c.text_contrast))
                .child(source)
                .child(text("AND").font(k.ty.small).color(k.c.text_muted))
                .child(pattern),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(badge("THEN", k.c.bg_accent, k.c.white))
                .child(opener)
                .child(private_switch(
                    k,
                    format!("rule-{index}-private"),
                    rule.opener.as_ref().is_some_and(|opener| opener.incognito),
                    rule.opener.is_some(),
                    move |s: &mut SettingsUi| {
                        if let Some(opener) =
                            s.rules.get_mut(index).and_then(|rule| rule.opener.as_mut())
                        {
                            opener.incognito = !opener.incognito;
                        }
                    },
                )),
        )
}

fn advanced_tab(ui: &UIState, k: &Kit) -> El {
    let mut content = col()
        .w_full()
        .gap(6.0)
        .child(setting_row(
            k,
            "Default browser",
            "Register brows as the system handler for web links",
            Button::new("Make default")
                .variant(ButtonVariant::Submit)
                .on_press(|s: &mut SettingsUi| s.send(MessageToMain::SetBrowsersAsDefaultBrowser))
                .build(k)
                .key("make-default"),
        ))
        .child(setting_row(
            k,
            "Installed applications",
            "Rescan browser profiles and desktop URL handlers",
            Button::new("Refresh")
                .on_press(|s: &mut SettingsUi| s.send(MessageToMain::Refresh))
                .build(k)
                .key("refresh-apps"),
        ));

    if !ui.restorable_app_profiles.is_empty() {
        let mut hidden = col()
            .w_full()
            .gap(4.0)
            .child(section_title(k, "Hidden profiles"));
        for profile in ui.restorable_app_profiles.iter() {
            let id = profile.unique_id.clone();
            hidden = hidden.child(
                row()
                    .w_full()
                    .py(2.0)
                    .center_y()
                    .child(
                        text(profile.get_full_name())
                            .font(k.ty.body)
                            .color(k.c.text_base)
                            .grow(1.0),
                    )
                    .child(
                        Button::new("Restore")
                            .control_size(ControlSize::Regular)
                            .on_press(move |s: &mut SettingsUi| {
                                s.send(MessageToMain::RestoreAppProfile(id.clone()))
                            })
                            .build(k)
                            .key(format!("restore-{}", profile.unique_id)),
                    ),
            );
        }
        content = content.child(hidden);
    }

    let path_line = |label: &str, path: std::path::PathBuf| {
        text(format!("{label}: {}", path.display()))
            .font(k.ty.small)
            .color(k.c.text_muted)
            .selectable(true)
    };
    content
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(path_line("Configuration", paths::get_config_json_path()))
        .child(path_line("Cache", paths::get_cache_root_dir()))
        .child(path_line("Logs", paths::get_logs_root_dir()))
}

struct About {
    open: Arc<AtomicBool>,
    persistent: bool,
    theme: ConfiguredTheme,
}

fn about_view(s: &About, cx: &mut Cx<About>) -> El {
    let k = style::kit(s.theme, cx);
    col()
        .w_full()
        .h_full()
        .padding(16.0)
        .gap(8.0)
        .center()
        .fill(k.c.bg_base)
        .child(
            text("brows")
                .font(rune::TextStyle::new(22.0, 600.0))
                .color(k.c.text_base),
        )
        .child(
            text(format!("Version {}", env!("CARGO_PKG_VERSION")))
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
        .child(
            text("Open the right browser at the right time.")
                .font(k.ty.body)
                .color(k.c.text_base),
        )
        .child(
            text("A fork of Browsers by Madis Liias.")
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
        .child(
            row()
                .gap(8.0)
                .mt(8.0)
                .child(
                    Button::new("Source code")
                        .on_press_tx(|_: &mut About, tx: &mut Tx| {
                            tx.open_url(SOURCE_URL).ok();
                        })
                        .build(&k)
                        .key("source"),
                )
                .child(
                    Button::new("Close")
                        .variant(ButtonVariant::Submit)
                        .on_press_tx(|_: &mut About, tx: &mut Tx| tx.close_window())
                        .build(&k)
                        .key("close"),
                ),
        )
}
