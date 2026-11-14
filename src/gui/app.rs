use std::sync::mpsc::Sender;

use flume::Receiver;
use rune::{App, AppSpec, WindowOptions};
use tracing::warn;

use super::model::UIState;
use super::picker::{self, PICKER_WIDTH, Picker, picker_height};
use super::placement;
use super::settings::AuxWindows;
use crate::utils::BehavioralConfig;
use crate::{MessageToMain, MessageToUi};

/// Run the UI on the current thread. A persistent (daemon) app starts hidden and shows the
/// picker for every URL the backend forwards; otherwise the picker shows once and quits.
pub fn run(
    state: UIState,
    main_sender: Sender<MessageToMain>,
    ui_receiver: Receiver<MessageToUi>,
    persistent: bool,
) {
    let size = (PICKER_WIDTH, picker_height(&state));
    let show_initial_picker = !state.url.is_empty();

    let spec = AppSpec {
        min_size: Some((PICKER_WIDTH, 0.0)),
        setup: Some(Picker::setup),
        on_key: Some(Picker::on_key),
        ..AppSpec::new("brows", size, || unreachable!(), picker::view)
    };
    let mut app = App::new(spec)
        .fit_content_height()
        .on_focus(Picker::on_focus)
        .on_modifiers(Picker::on_modifiers)
        .on_open_url(|s: &mut Picker, url, _| {
            s.send(MessageToMain::UrlPassedToMain(
                String::new(),
                url.to_string(),
                BehavioralConfig {
                    unwrap_urls: s.state.ui_settings.behavioral_settings.unwrap_urls,
                },
            ));
        });

    app = if persistent {
        app.windowless()
            .keep_running()
            .configured_options(WindowOptions::launcher_panel())
    } else {
        app.window_options(WindowOptions {
            borderless: true,
            transparent: true,
            always_on_top: true,
            resizable: false,
            min_size: Some((PICKER_WIDTH, 0.0)),
            position: placement::initial_position(size),
            ..Default::default()
        })
    };

    let handle = app.handle();
    let aux = AuxWindows::create(&mut app, &state, main_sender.clone(), handle.clone(), persistent);
    let me = handle.clone();
    app = app.init(move || Picker::new(state, main_sender, persistent, me, aux));

    let forward = handle.clone();
    std::thread::spawn(move || {
        while let Ok(message) = ui_receiver.recv() {
            forward.update(move |s, tx| s.handle_ui_event(message, tx));
        }
        warn!("UI event channel disconnected");
    });

    if persistent && show_initial_picker {
        handle.update(|s, _| s.show_at_cursor(None));
    }

    app.run();
}
