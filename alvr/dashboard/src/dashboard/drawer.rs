use super::{Tab, TABS};
use crate::icons;
use alvr_gui_common::theme::{self, Palette};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Layout, Margin, Response, RichText, Sense,
    SidePanel, Stroke, TextureHandle, TopBottomPanel, Ui,
};

const TITLE_BAR_HEIGHT: f32 = 48.0;
const DRAWER_WIDTH: f32 = 240.0;
const MINI_DRAWER_WIDTH: f32 = 72.0;
const NAVIGATION_ROW_HEIGHT: f32 = 48.0;
const STATUS_CHIP_WIDTH: f32 = 132.0;

pub enum TitleBarAction {
    ToggleExpanded,
    StartDrag,
    #[cfg(not(target_arch = "wasm32"))]
    Minimize,
    #[cfg(not(target_arch = "wasm32"))]
    ToggleMaximize,
    #[cfg(not(target_arch = "wasm32"))]
    Close,
}

pub enum NavigationAction {
    SelectTab(Tab),
    #[cfg(not(target_arch = "wasm32"))]
    RestartSteamvr,
    #[cfg(not(target_arch = "wasm32"))]
    LaunchSteamvr,
}

pub fn show_title_bar(
    context: &egui::Context,
    logo: &TextureHandle,
    expanded: bool,
    connected_to_server: bool,
    palette: Palette,
) -> Vec<TitleBarAction> {
    let mut actions = Vec::new();
    let maximized = context.input(|input| input.viewport().maximized.unwrap_or(false));

    TopBottomPanel::top("material_title_bar")
        .exact_height(TITLE_BAR_HEIGHT)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(palette.navigation)
                .inner_margin(Margin::same(0))
                .stroke(Stroke::NONE)
                .shadow(theme::app_bar_shadow(context)),
        )
        .show(context, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if title_bar_button(
                    ui,
                    icons::MENU,
                    if expanded {
                        "收起导航抽屉"
                    } else {
                        "展开导航抽屉"
                    },
                    false,
                    palette,
                )
                .clicked()
                {
                    actions.push(TitleBarAction::ToggleExpanded);
                }

                ui.add_space(10.0);
                ui.add(egui::Image::new((logo.id(), egui::vec2(24.0, 24.0))));
                ui.add_space(8.0);
                ui.label(
                    RichText::new("ALVR 华为专版")
                        .size(17.0)
                        .strong()
                        .color(palette.foreground),
                );

                let window_controls_width = if cfg!(target_arch = "wasm32") {
                    0.0
                } else {
                    3.0 * TITLE_BAR_HEIGHT
                };
                let controls_gutter = if cfg!(target_arch = "wasm32") {
                    0.0
                } else {
                    8.0
                };
                let drag_width = (ui.available_width()
                    - STATUS_CHIP_WIDTH
                    - controls_gutter
                    - window_controls_width)
                    .max(16.0);
                let drag_response = ui.allocate_response(
                    egui::vec2(drag_width, TITLE_BAR_HEIGHT),
                    Sense::click_and_drag(),
                );
                if drag_response.double_clicked() {
                    #[cfg(not(target_arch = "wasm32"))]
                    actions.push(TitleBarAction::ToggleMaximize);
                } else if drag_response.drag_started() {
                    actions.push(TitleBarAction::StartDrag);
                }

                status_chip(ui, connected_to_server, palette);

                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.add_space(controls_gutter);
                    if title_bar_button(ui, icons::REMOVE, "最小化", false, palette).clicked() {
                        actions.push(TitleBarAction::Minimize);
                    }
                    let maximize_icon = if maximized {
                        icons::FILTER_NONE
                    } else {
                        icons::CROP_SQUARE
                    };
                    if title_bar_button(ui, maximize_icon, "最大化/还原", false, palette).clicked()
                    {
                        actions.push(TitleBarAction::ToggleMaximize);
                    }
                    if title_bar_button(ui, icons::CLOSE, "关闭", true, palette).clicked() {
                        actions.push(TitleBarAction::Close);
                    }
                }
            });
        });

    actions
}

pub fn show_navigation_drawer(
    context: &egui::Context,
    expanded: bool,
    selected_tab: Tab,
    connected_to_server: bool,
    palette: Palette,
) -> Vec<NavigationAction> {
    let mut actions = Vec::new();

    SidePanel::left("material_navigation_drawer")
        .resizable(false)
        .show_separator_line(false)
        .exact_width(if expanded {
            DRAWER_WIDTH
        } else {
            MINI_DRAWER_WIDTH
        })
        .frame(
            Frame::new()
                .fill(palette.navigation)
                .inner_margin(Margin::same(0))
                .stroke(Stroke::NONE),
        )
        .show(context, |ui| {
            ui.add_space(8.0);
            for tab in TABS {
                let response = navigation_row(
                    ui,
                    expanded,
                    tab.icon(),
                    tab.label(),
                    selected_tab == *tab,
                    palette,
                    Sense::click(),
                );
                if response.clicked() {
                    actions.push(NavigationAction::SelectTab(*tab));
                }
            }

            #[cfg(not(target_arch = "wasm32"))]
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.add_space(6.0);
                let (action_icon, action_label, action) = if connected_to_server {
                    (
                        icons::RESTART_ALT,
                        "重启 SteamVR",
                        NavigationAction::RestartSteamvr,
                    )
                } else {
                    (
                        icons::PLAY_ARROW,
                        "启动 SteamVR",
                        NavigationAction::LaunchSteamvr,
                    )
                };
                if navigation_row(
                    ui,
                    expanded,
                    action_icon,
                    action_label,
                    false,
                    palette,
                    Sense::click(),
                )
                .clicked()
                {
                    actions.push(action);
                }
            });

            let divider = if ui.visuals().dark_mode {
                Color32::from_rgb(45, 45, 45)
            } else {
                Color32::from_rgb(238, 238, 238)
            };
            ui.painter().vline(
                ui.max_rect().right() - 0.5,
                ui.max_rect().y_range(),
                Stroke::new(1.0, divider),
            );
        });

    actions
}

fn title_bar_button(
    ui: &mut Ui,
    glyph: &str,
    tooltip: &str,
    danger: bool,
    palette: Palette,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(TITLE_BAR_HEIGHT, TITLE_BAR_HEIGHT),
        Sense::click(),
    );
    let fill = if response.is_pointer_button_down_on() {
        if danger {
            palette.error.gamma_multiply(0.28)
        } else {
            palette.foreground.gamma_multiply(0.14)
        }
    } else if response.hovered() {
        if danger {
            palette.error.gamma_multiply(0.20)
        } else {
            palette.raised_surface
        }
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::new(20.0, icons::family()),
        if danger && response.hovered() {
            palette.error
        } else {
            palette.foreground
        },
    );
    response.on_hover_text(tooltip)
}

fn status_chip(ui: &mut Ui, connected_to_server: bool, palette: Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(STATUS_CHIP_WIDTH, 28.0), Sense::hover());
    ui.painter().rect_filled(rect, 14.0, palette.raised_surface);
    let (status, status_color) = if connected_to_server {
        ("SteamVR 已连接", palette.ok)
    } else {
        ("SteamVR 未连接", palette.error)
    };
    ui.painter().text(
        egui::pos2(rect.left() + 17.0, rect.center().y),
        Align2::CENTER_CENTER,
        icons::RADIO_BUTTON_CHECKED,
        FontId::new(11.0, icons::family()),
        status_color,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 29.0, rect.center().y),
        Align2::LEFT_CENTER,
        status,
        FontId::proportional(12.0),
        status_color,
    );
}

fn navigation_row(
    ui: &mut Ui,
    expanded: bool,
    glyph: &str,
    label: &str,
    selected: bool,
    palette: Palette,
    sense: Sense,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), NAVIGATION_ROW_HEIGHT),
        sense,
    );
    let selected_fill =
        palette
            .accent
            .gamma_multiply(if ui.visuals().dark_mode { 0.28 } else { 0.12 });
    let fill = if selected {
        selected_fill
    } else if response.is_pointer_button_down_on() {
        palette.accent.gamma_multiply(0.18)
    } else if response.hovered() {
        palette.raised_surface
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0.0, fill);

    let color = if selected {
        palette.accent
    } else {
        palette.foreground
    };
    let icon_center_x = if expanded {
        rect.left() + 30.0
    } else {
        rect.center().x
    };
    ui.painter().text(
        egui::pos2(icon_center_x, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        FontId::new(22.0, icons::family()),
        color,
    );
    if expanded {
        ui.painter().text(
            egui::pos2(rect.left() + 56.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(15.0),
            color,
        );
    }

    if expanded {
        response
    } else {
        response.on_hover_text(label)
    }
}
