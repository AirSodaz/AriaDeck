use gpui::{Context, FontWeight, IntoElement, Render, Window, div, prelude::*, px};

use crate::Theme;

/// Initial application frame used while the live workspace is being built.
pub struct AppShell {
    theme: Theme,
}

impl AppShell {
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }
}

impl Render for AppShell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text_primary)
            .child(
                div()
                    .h(px(56.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .border_b_1()
                    .border_color(colors.border)
                    .bg(colors.surface)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("AriaDeck"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.text_muted)
                                    .child("Downloads"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .text_sm()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_color(colors.text_secondary)
                                    .child("Down  0 B/s")
                                    .child("Up  0 B/s"),
                            )
                            .child(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(colors.elevated_surface)
                                    .text_color(colors.text_muted)
                                    .child("Offline"),
                            )
                            .child(
                                div()
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(colors.accent)
                                    .text_color(colors.text_inverse)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Add download"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .w(px(220.0))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .border_r_1()
                            .border_color(colors.border)
                            .bg(colors.surface)
                            .p_3()
                            .child(div().flex().flex_col().gap_1().children([
                                sidebar_item("All", "0", true, colors),
                                sidebar_item("Active", "0", false, colors),
                                sidebar_item("Waiting", "0", false, colors),
                                sidebar_item("Paused", "0", false, colors),
                                sidebar_item("Completed", "0", false, colors),
                                sidebar_item("Failed", "0", false, colors),
                            ]))
                            .child(div().flex().flex_col().gap_1().children([
                                sidebar_item("Profiles", "", false, colors),
                                sidebar_item("Settings", "", false, colors),
                            ])),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("No downloads yet"),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(colors.text_muted)
                                            .child("Connect to aria2 to load your queue."),
                                    ),
                            ),
                    ),
            )
    }
}

fn sidebar_item(
    label: &'static str,
    count: &'static str,
    selected: bool,
    colors: crate::ThemeColors,
) -> gpui::Div {
    let background = if selected {
        colors.surface_active
    } else {
        colors.surface
    };
    let text = if selected {
        colors.text_primary
    } else {
        colors.text_secondary
    };

    div()
        .h(px(34.0))
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .rounded_md()
        .bg(background)
        .text_sm()
        .text_color(text)
        .child(label)
        .child(
            div()
                .text_color(colors.text_muted)
                .font_weight(FontWeight::MEDIUM)
                .child(count),
        )
}
