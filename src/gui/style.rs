use rune::Cx;
use rune_kit::Kit;

use crate::utils::ConfiguredTheme;

fn is_dark(theme: ConfiguredTheme, system_dark: bool) -> bool {
    match theme {
        ConfiguredTheme::Auto => system_dark,
        ConfiguredTheme::Light => false,
        ConfiguredTheme::Dark => true,
    }
}

pub(super) fn kit<S: 'static>(theme: ConfiguredTheme, cx: &mut Cx<S>) -> Kit {
    Kit::neutral(is_dark(theme, cx.system_dark()))
}
