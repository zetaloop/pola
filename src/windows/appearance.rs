use windows_reactor::*;

use crate::{
    locale::{self, tr},
    mode::Mode,
    schedule::Event,
};

pub fn view(
    mode: &Result<Mode, String>,
    next: Option<&Event>,
    changed: Callback<Option<usize>>,
) -> View {
    let next = next.map_or_else(View::empty, |event| {
        TextBlock::new()
            .text(locale::next(event).unwrap_or_else(|error| error))
            .text_wrapping(TextWrapping::Wrap)
            .into()
    });
    StackPanel::new()
        .horizontal_alignment(HorizontalAlignment::Center)
        .vertical_alignment(VerticalAlignment::Center)
        .margin(24.0)
        .spacing(16.0)
        .children((
            RadioButtons::new()
                .header(tr!("Appearance"))
                .items_source([tr!("Light"), tr!("Dark")])
                .max_columns(2)
                .selected_index(
                    mode.as_ref()
                        .ok()
                        .map(|mode| usize::from(*mode == Mode::Dark)),
                )
                .on_selection_changed(changed),
            next,
        ))
}
