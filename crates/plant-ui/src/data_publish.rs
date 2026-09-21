//! 三维数据发布任务窗。
//!
//! 本模块只维护表单与预览状态；真正的发布由宿主处理 `Cmd::SubmitDataPublish`。

use egui::{Align, ComboBox, CornerRadius, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit};

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, radius, space};
use crate::style::widgets;
use crate::vm::{PropsVm, TreeVm, WorkbenchVm};
use crate::{Cmd, RefU64};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PublishCategory {
    #[default]
    Process,
    Electrical,
    Instrumentation,
    Ventilation,
    Room,
}

impl PublishCategory {
    pub const ALL: [Self; 5] = [
        Self::Process,
        Self::Electrical,
        Self::Instrumentation,
        Self::Ventilation,
        Self::Room,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Process => "工艺专业-全量数据发布",
            Self::Electrical => "电气专业-全量数据发布",
            Self::Instrumentation => "仪控专业-全量数据发布",
            Self::Ventilation => "通风专业-全量数据发布",
            Self::Room => "设备房间位置信息发布",
        }
    }

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Process => "/get_gy_bran_data",
            Self::Electrical => "/get_dq_bran_data",
            Self::Instrumentation => "/get_yk_bran_data",
            Self::Ventilation => "/get_tf_bran_data",
            // 设备房间位置发布走与工艺发布相同的数据中心契约。
            Self::Room => "/get_bran_data_room_code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesignPhase {
    #[default]
    Preliminary,
    Detailed,
}

impl DesignPhase {
    pub const ALL: [Self; 2] = [Self::Preliminary, Self::Detailed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Preliminary => "初设阶段",
            Self::Detailed => "施设阶段",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishElement {
    pub refno: RefU64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublishRequest {
    pub title: String,
    pub category: PublishCategory,
    pub design_phase: DesignPhase,
    pub elements: Vec<PublishElement>,
    pub delete_refnos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct DeletionCandidate {
    pub id: String,
    pub name: String,
    #[serde(skip, default = "default_checked")]
    pub selected: bool,
}

fn default_checked() -> bool {
    true
}

#[derive(Debug, Default)]
pub struct State {
    pub open: bool,
    pub preview_open: bool,
    pub delete_confirmation_open: bool,
    pub deletion_candidates: Vec<DeletionCandidate>,
    pub pending_request: Option<PublishRequest>,
    /// 从首次点击提交起，到宿主收到接口回执为止。
    pub submitting: bool,
    pub request: PublishRequest,
    pub selected_refno: Option<RefU64>,
}

impl State {
    fn start_lookup(&mut self) -> Cmd {
        let request = self.request.clone();
        self.pending_request = Some(request.clone());
        self.submitting = true;
        self.delete_confirmation_open = false;
        self.deletion_candidates.clear();
        Cmd::LookupDataPublishDeletions(request)
    }

    fn confirm_publish(&mut self) -> Option<Cmd> {
        if self.submitting {
            return None;
        }
        let mut request = self.pending_request.clone()?;
        request.delete_refnos = self
            .deletion_candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .map(|candidate| candidate.id.clone())
            .collect();
        self.submitting = true;
        Some(Cmd::SubmitDataPublish(request))
    }

    fn add_element(&mut self, element: PublishElement) -> bool {
        if self
            .request
            .elements
            .iter()
            .any(|existing| existing.refno == element.refno)
        {
            return false;
        }
        self.request.elements.push(element);
        true
    }

    fn remove_selected(&mut self) -> bool {
        let Some(selected_refno) = self.selected_refno.take() else {
            return false;
        };
        let original_len = self.request.elements.len();
        self.request
            .elements
            .retain(|element| element.refno != selected_refno);
        self.request.elements.len() != original_len
    }

    fn clear_elements(&mut self) {
        self.request.elements.clear();
        self.selected_refno = None;
    }
}

pub fn show(
    ctx: &egui::Context,
    tokens: &Tokens,
    density: Density,
    workbench: &WorkbenchVm,
    state: &mut State,
) -> Vec<Cmd> {
    if !state.open {
        state.preview_open = false;
        state.delete_confirmation_open = false;
        state.pending_request = None;
        return Vec::new();
    }

    let mut commands = Vec::new();
    let mut window_open = state.open;
    egui::Window::new("三维数据接口")
        .id(egui::Id::new("data-publish-window"))
        .open(&mut window_open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(560.0), density.px(400.0)])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
            let field_width = density.px(360.0);

            egui::Grid::new("data-publish-form")
                .num_columns(2)
                .spacing([space::S3, space::S2])
                .show(ui, |ui| {
                    form_label(ui, tokens, density, "标题：");
                    ui.add_sized(
                        [field_width, density.btn_h()],
                        TextEdit::singleline(&mut state.request.title).hint_text("请输入发布标题"),
                    );
                    ui.end_row();

                    form_label(ui, tokens, density, "选择提资内容：");
                    ComboBox::from_id_salt("data-publish-category")
                        .selected_text(state.request.category.label())
                        .width(field_width)
                        .show_ui(ui, |ui| {
                            for category in PublishCategory::ALL {
                                ui.selectable_value(
                                    &mut state.request.category,
                                    category,
                                    category.label(),
                                );
                            }
                        });
                    ui.end_row();
                });

            ui.add_space(space::S1);
            form_label(ui, tokens, density, "添加提资元素：");

            let current_element = selected_workbench_element(workbench);
            let already_added = current_element.as_ref().is_some_and(|candidate| {
                state
                    .request
                    .elements
                    .iter()
                    .any(|element| element.refno == candidate.refno)
            });

            ui.horizontal_top(|ui| {
                ui.set_height(density.px(176.0));
                ui.vertical(|ui| {
                    ui.set_width(density.px(76.0));
                    let add_hint = if current_element.is_none() {
                        "请先在模型树或三维视图中选择元素"
                    } else {
                        "当前元素已经在提资列表中"
                    };
                    let add_response = ui
                        .add_enabled(
                            current_element.is_some() && !already_added,
                            widgets::button(tokens, density, "添加").full_width(),
                        )
                        .on_disabled_hover_text(add_hint);
                    if add_response.clicked()
                        && let Some(element) = current_element.clone()
                    {
                        state.add_element(element);
                    }

                    let remove_response = ui.add_enabled(
                        state.selected_refno.is_some(),
                        widgets::button(tokens, density, "移除").full_width(),
                    );
                    if remove_response.clicked() {
                        state.remove_selected();
                    }

                    let clear_response = ui.add_enabled(
                        !state.request.elements.is_empty(),
                        widgets::button(tokens, density, "清空").full_width(),
                    );
                    if clear_response.clicked() {
                        state.clear_elements();
                    }
                });

                egui::Frame::new()
                    .fill(tokens.bg_input)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(CornerRadius::same(radius::MD))
                    .inner_margin(Margin::same(space::S2 as i8))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(density.px(430.0), density.px(160.0)));
                        ScrollArea::vertical()
                            .id_salt("data-publish-elements")
                            .auto_shrink([false, false])
                            .max_height(density.px(160.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if state.request.elements.is_empty() {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(
                                            RichText::new("尚未添加提资元素")
                                                .font(Font::meta(density))
                                                .color(tokens.text_muted),
                                        );
                                    });
                                    return;
                                }

                                for element in &state.request.elements {
                                    let selected = state.selected_refno == Some(element.refno);
                                    let response = ui.selectable_label(
                                        selected,
                                        format!("{}    {}", element.name, element.refno),
                                    );
                                    if response.clicked() {
                                        state.selected_refno = Some(element.refno);
                                    }
                                }
                            });
                    });
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can_submit = !state.submitting
                    && !state.delete_confirmation_open
                    && !state.request.elements.is_empty();
                let submit = ui.add_enabled(
                    can_submit,
                    widgets::button(tokens, density, "提交")
                        .primary()
                        .loading(state.submitting),
                );
                if !can_submit {
                    submit
                        .clone()
                        .on_disabled_hover_text(if state.delete_confirmation_open {
                            "请先关闭确认删除提资窗口"
                        } else if state.submitting {
                            "正在提交，请稍候"
                        } else {
                            "请至少添加一个元素"
                        });
                }
                if submit.clicked() {
                    commands.push(state.start_lookup());
                }
                if ui.add(widgets::button(tokens, density, "预览")).clicked() {
                    state.preview_open = true;
                }
            });
        });

    state.open = window_open || (state.submitting && state.delete_confirmation_open);
    if !state.open {
        state.preview_open = false;
        state.delete_confirmation_open = false;
        state.pending_request = None;
    } else if state.preview_open {
        show_preview(
            ctx,
            tokens,
            density,
            &state.request,
            &mut state.preview_open,
        );
    }

    if state.open && state.delete_confirmation_open {
        if let Some(command) = show_delete_confirmation(ctx, tokens, density, state) {
            commands.push(command);
        }
    }

    commands
}

fn show_delete_confirmation(
    ctx: &egui::Context,
    tokens: &Tokens,
    density: Density,
    state: &mut State,
) -> Option<Cmd> {
    let mut open = state.delete_confirmation_open;
    let mut close_requested = false;
    let mut confirmed = false;
    egui::Window::new("确认删除提资")
        .id(egui::Id::new("data-publish-delete-confirmation"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(420.0), density.px(300.0)])
        .show(ctx, |ui| {
            ui.add_enabled_ui(!state.submitting, |ui| {
                let widgets = &mut ui.visuals_mut().widgets;
                for visuals in [
                    &mut widgets.inactive,
                    &mut widgets.hovered,
                    &mut widgets.active,
                ] {
                    visuals.corner_radius = CornerRadius::ZERO;
                }

                let any_selected = state.deletion_candidates.iter().any(|item| item.selected);
                let mut all_selected = !state.deletion_candidates.is_empty()
                    && state.deletion_candidates.iter().all(|item| item.selected);
                let partially_selected = any_selected && !all_selected;
                if ui
                    .add(
                        egui::Checkbox::new(&mut all_selected, "全选")
                            .indeterminate(partially_selected),
                    )
                    .changed()
                {
                    for candidate in &mut state.deletion_candidates {
                        candidate.selected = all_selected;
                    }
                }

                egui::Frame::new()
                    .fill(tokens.bg_input)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(CornerRadius::same(radius::MD))
                    .inner_margin(Margin::same(space::S2 as i8))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(density.px(375.0), density.px(160.0)));
                        ScrollArea::vertical()
                            .id_salt("data-publish-delete-candidates")
                            .auto_shrink([false, false])
                            .max_height(density.px(160.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if state.deletion_candidates.is_empty() {
                                    ui.label("没有待删除的提资");
                                }
                                for candidate in &mut state.deletion_candidates {
                                    ui.horizontal_top(|ui| {
                                        ui.add(egui::Checkbox::without_text(
                                            &mut candidate.selected,
                                        ));
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&candidate.name)
                                                    .font(Font::body(density))
                                                    .color(tokens.text_secondary),
                                            )
                                            .wrap(),
                                        );
                                    });
                                }
                            });
                    });
            });
            ui.add_space((ui.available_height() - density.btn_h()).max(0.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let confirm = ui.add_enabled(
                    !state.submitting,
                    widgets::button(tokens, density, "确认")
                        .primary()
                        .loading(state.submitting),
                );
                if confirm.clicked() {
                    confirmed = true;
                }
                if ui
                    .add_enabled(!state.submitting, widgets::button(tokens, density, "取消"))
                    .clicked()
                {
                    close_requested = true;
                }
            });
        });
    state.delete_confirmation_open = state.submitting || (open && !close_requested);
    if confirmed {
        return state.confirm_publish();
    }
    if !state.delete_confirmation_open {
        state.pending_request = None;
    }
    None
}

fn form_label(ui: &mut egui::Ui, tokens: &Tokens, density: Density, text: &str) {
    ui.label(
        RichText::new(text)
            .font(Font::meta(density))
            .color(tokens.text_muted),
    );
}

fn selected_workbench_element(workbench: &WorkbenchVm) -> Option<PublishElement> {
    let refno = workbench.selection.primary()?;
    let name_from_props = match &workbench.props {
        PropsVm::Ready(props) if props.refno == refno && !props.name.trim().is_empty() => {
            Some(props.name.clone())
        }
        _ => None,
    };
    let name_from_tree = match &workbench.tree {
        TreeVm::Ready(rows) => rows
            .iter()
            .find(|row| row.refno == refno)
            .map(|row| row.name.clone()),
        TreeVm::Loading | TreeVm::Failed(_) => None,
    };
    Some(PublishElement {
        refno,
        name: name_from_props
            .or(name_from_tree)
            .unwrap_or_else(|| refno.to_string()),
    })
}

fn show_preview(
    ctx: &egui::Context,
    tokens: &Tokens,
    density: Density,
    request: &PublishRequest,
    open: &mut bool,
) {
    let mut close_requested = false;
    egui::Window::new("预览数据")
        .id(egui::Id::new("data-publish-preview-window"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(620.0), density.px(420.0)])
        .show(ctx, |ui| {
            egui::Grid::new("data-publish-preview-summary")
                .num_columns(2)
                .spacing([space::S3, space::S2])
                .show(ui, |ui| {
                    preview_label(ui, tokens, density, "标题");
                    preview_value(
                        ui,
                        tokens,
                        density,
                        if request.title.trim().is_empty() {
                            "（未填写）"
                        } else {
                            &request.title
                        },
                    );
                    ui.end_row();
                    preview_label(ui, tokens, density, "提资内容");
                    preview_value(ui, tokens, density, request.category.label());
                    ui.end_row();
                    preview_label(ui, tokens, density, "接口");
                    preview_value(ui, tokens, density, request.category.endpoint());
                    ui.end_row();
                    preview_label(ui, tokens, density, "设计阶段");
                    preview_value(ui, tokens, density, request.design_phase.label());
                    ui.end_row();
                    preview_label(ui, tokens, density, "元素数量");
                    preview_value(ui, tokens, density, &request.elements.len().to_string());
                    ui.end_row();
                });

            ui.separator();
            ScrollArea::vertical()
                .id_salt("data-publish-preview-elements")
                .auto_shrink([false, false])
                .max_height(density.px(260.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if request.elements.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new("尚未添加提资元素")
                                    .font(Font::meta(density))
                                    .color(tokens.text_muted),
                            );
                        });
                        return;
                    }

                    egui::Grid::new("data-publish-preview-table")
                        .striped(true)
                        .min_col_width(density.px(220.0))
                        .spacing([space::S3, space::S2])
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("元素名称")
                                    .font(Font::strong(density))
                                    .color(tokens.text_primary),
                            );
                            ui.label(
                                RichText::new("参考号")
                                    .font(Font::strong(density))
                                    .color(tokens.text_primary),
                            );
                            ui.end_row();
                            for element in &request.elements {
                                ui.label(
                                    RichText::new(&element.name)
                                        .font(Font::body(density))
                                        .color(tokens.text_secondary),
                                );
                                ui.label(
                                    RichText::new(element.refno.to_string())
                                        .font(Font::mono_meta(density))
                                        .color(tokens.text_secondary),
                                );
                                ui.end_row();
                            }
                        });
                });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(widgets::button(tokens, density, "关闭")).clicked() {
                    close_requested = true;
                }
            });
        });
    if close_requested {
        *open = false;
    }
}

fn preview_label(ui: &mut egui::Ui, tokens: &Tokens, density: Density, text: &str) {
    ui.label(
        RichText::new(text)
            .font(Font::meta(density))
            .color(tokens.text_muted),
    );
}

fn preview_value(ui: &mut egui::Ui, tokens: &Tokens, density: Density, text: &str) {
    ui.label(
        RichText::new(text)
            .font(Font::body(density))
            .color(tokens.text_secondary),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_submits_the_original_request_snapshot() {
        let mut state = State::default();
        state.request.title = "原始标题".into();
        state.request.elements.push(PublishElement {
            refno: 1_u64.into(),
            name: "/PIPE-1".into(),
        });
        assert!(matches!(
            state.start_lookup(),
            Cmd::LookupDataPublishDeletions(_)
        ));
        state.deletion_candidates = vec![
            DeletionCandidate {
                id: "24383/66458".into(),
                name: "/1WCC1135".into(),
                selected: true,
            },
            DeletionCandidate {
                id: "24383/66457".into(),
                name: "/1WCC1134".into(),
                selected: false,
            },
        ];
        state.request.title = "之后修改的标题".into();
        state.submitting = false;

        let Some(Cmd::SubmitDataPublish(request)) = state.confirm_publish() else {
            panic!("确认后应提交原始提资请求");
        };
        assert_eq!(request.title, "原始标题");
        assert_eq!(request.delete_refnos, vec!["24383/66458"]);
        assert!(state.submitting);
        assert!(state.pending_request.is_some());
        assert!(state.confirm_publish().is_none());
    }

    #[test]
    fn state_keeps_unique_elements_and_stable_request_snapshots() {
        let first = PublishElement {
            refno: 1_u64.into(),
            name: "PIPE /P-1001".into(),
        };
        let second = PublishElement {
            refno: 2_u64.into(),
            name: "EQUI /V-1001".into(),
        };
        let mut state = State::default();
        state.request.title = "全量发布".into();
        state.request.category = PublishCategory::Electrical;
        state.request.design_phase = DesignPhase::Detailed;

        assert!(state.add_element(first.clone()));
        assert!(!state.add_element(first.clone()));
        assert!(state.add_element(second.clone()));
        assert_eq!(state.request.elements, vec![first.clone(), second.clone()]);

        let submitted = state.request.clone();
        state.selected_refno = Some(first.refno);
        assert!(state.remove_selected());
        assert_eq!(state.request.elements, vec![second]);

        state.clear_elements();
        assert!(state.request.elements.is_empty());
        assert_eq!(submitted.title, "全量发布");
        assert_eq!(submitted.category, PublishCategory::Electrical);
        assert_eq!(submitted.category.endpoint(), "/get_dq_bran_data");
        assert_eq!(submitted.design_phase, DesignPhase::Detailed);
        assert_eq!(PublishCategory::ALL.len(), 5);
        assert_eq!(PublishCategory::Room.label(), "房间数据发布");
        assert_eq!(PublishCategory::Room.endpoint(), "/get_gy_bran_data");
        assert_eq!(
            submitted.elements,
            vec![
                first,
                PublishElement {
                    refno: 2_u64.into(),
                    name: "EQUI /V-1001".into(),
                }
            ]
        );
    }
}
