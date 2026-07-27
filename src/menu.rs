use bevy::{
    input_focus::{
        InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui::UiScale,
    window::PrimaryWindow,
};
use crownline_core::{
    Action, ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES,
    scenario::Player,
};

use crate::{
    config::{CameraBindingsSettings, CameraKey, ClientSettings},
    guided_play::GuidedRuntime,
    help::HelpState,
    lifecycle::{
        ClientFlow, LocalClockRuntime, LocalSetup, ScenarioCatalog, SeatController, apply_control,
        start_fresh_match, validate_names,
    },
    local_ai::AiCancellationEpoch,
    local_persistence::{
        LocalPersistenceStatus, has_readable_local_save, load_slot, local_save_slot_summaries,
        save_slot,
    },
    online_lobby::{LobbyScreen, OnlineLobby},
    panels::PanelSurface,
    rendering::{
        DisplayedGame, FogPresentation, LocalTransitionEventQueue, LocalTransitionNoticeLog,
        OverlaySelection,
    },
};

const MENU_BACKGROUND: Color = Color::srgba(0.018, 0.026, 0.045, 0.985);
const CONTROL_IDLE: Color = Color::srgb(0.09, 0.13, 0.2);
const CONTROL_HOVERED: Color = Color::srgb(0.14, 0.22, 0.31);
const CONTROL_PRESSED: Color = Color::srgb(0.2, 0.34, 0.43);
pub(crate) const CONTROL_DISABLED: Color = Color::srgb(0.07, 0.075, 0.09);
const CONTROL_SELECTED: Color = Color::srgb(0.16, 0.3, 0.38);
const CONTROL_DESTRUCTIVE: Color = Color::srgb(0.35, 0.1, 0.12);
pub(crate) const CONTROL_EXIT: Color = Color::srgb(0.27, 0.16, 0.12);
pub(crate) const READONLY_BACKGROUND: Color = Color::srgb(0.055, 0.075, 0.105);
pub(crate) const READONLY_BORDER: Color = Color::srgb(0.2, 0.28, 0.36);
pub(crate) const INPUT_BACKGROUND: Color = Color::srgb(0.075, 0.105, 0.085);
pub(crate) const INPUT_BORDER: Color = Color::srgb(0.34, 0.58, 0.42);
const BORDER_IDLE: Color = Color::srgb(0.28, 0.35, 0.46);
const BORDER_FOCUSED: Color = Color::srgb(0.82, 0.91, 1.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MenuRoute {
    #[default]
    Home,
    LocalSetup,
    Guided,
    Online,
    Settings,
    Pause,
    Saves,
    Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuAction {
    Open(MenuRoute),
    Back,
    Confirm,
    Cancel,
    OpenHelp,
    Quit,
    PreviousScenario,
    NextScenario,
    CycleController(Player),
    SwapSides,
    ToggleClock,
    DecreaseBase,
    IncreaseBase,
    DecreaseIncrement,
    IncreaseIncrement,
    StartLocal,
    SelectSettingsTab(SettingsTab),
    NextResolution,
    DecreaseUiScale,
    IncreaseUiScale,
    ToggleReducedMotion,
    CaptureCameraBinding(CameraBindingSlot),
    ResetCameraBindings,
    ApplySettings,
    CancelSettings,
    ForgetSavedSeat,
    ResumeMatch,
    SaveSlot(u8),
    LoadSlot(u8),
    OfferDraw,
    AcceptDraw,
    DeclineDraw,
    RequestResign,
    Rematch,
    ReturnHome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    #[default]
    Display,
    Accessibility,
    Controls,
    Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CameraBindingSlot {
    PanUp,
    PanDown,
    PanLeft,
    PanRight,
    ZoomIn,
    ZoomOut,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuModal {
    ForgetSavedSeat,
    OverwriteSlot(u8),
    LoadSlot(u8),
    Resign,
    AbandonMatch,
}

#[derive(Debug, Clone, Resource, Default)]
pub(crate) struct MenuState {
    pub(crate) route: Option<MenuRoute>,
    pub(crate) previous: Vec<MenuRoute>,
    pub(crate) modal: Option<MenuModal>,
}

impl MenuState {
    pub(crate) fn open(&mut self, route: MenuRoute) {
        if let Some(current) = self.route
            && current != route
        {
            self.previous.push(current);
        }
        self.route = Some(route);
    }

    pub(crate) fn replace(&mut self, route: MenuRoute) {
        self.route = Some(route);
    }

    pub(crate) fn back(&mut self) {
        self.route = self.previous.pop();
        self.modal = None;
    }

    pub(crate) fn close(&mut self) {
        self.route = None;
        self.previous.clear();
        self.modal = None;
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.route.is_some()
    }
}

#[derive(Debug, Clone, Resource, Default)]
pub(crate) struct MenuIntent(pub(crate) Option<MenuAction>);

impl MenuIntent {
    pub(crate) fn send(&mut self, action: MenuAction) {
        self.0 = Some(action);
    }

    pub(crate) fn take(&mut self) -> Option<MenuAction> {
        self.0.take()
    }

    const fn peek(&self) -> Option<MenuAction> {
        self.0
    }
}

#[derive(Component)]
pub(crate) struct MenuRoot;

#[derive(Component)]
pub(crate) struct MenuContent;

#[derive(Component)]
pub(crate) struct MenuTitle;

#[derive(Component)]
pub(crate) struct MenuFooter;

#[derive(Component)]
struct MenuNameInput(Player);

#[derive(Debug, Clone, Copy, Component)]
enum SettingsTextInput {
    WindowWidth,
    WindowHeight,
    ServerUrl,
}

#[derive(Debug, Clone, Resource, Default)]
struct SettingsMenuState {
    original: Option<ClientSettings>,
    draft: Option<ClientSettings>,
    tab: SettingsTab,
    capturing: Option<CameraBindingSlot>,
    message: String,
}

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct MenuButton {
    pub(crate) action: MenuAction,
    pub(crate) availability: MenuAvailability,
    pub(crate) emphasis: MenuEmphasis,
}

#[derive(Debug, Clone, Copy, Component)]
struct MenuTabOrder(i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuAvailability {
    Enabled,
    Disabled,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuEmphasis {
    Normal,
    Selected,
    Destructive,
    Exit,
}

impl MenuButton {
    pub(crate) const fn new(action: MenuAction) -> Self {
        Self {
            action,
            availability: MenuAvailability::Enabled,
            emphasis: match action {
                MenuAction::Back
                | MenuAction::Cancel
                | MenuAction::CancelSettings
                | MenuAction::Quit => MenuEmphasis::Exit,
                _ => MenuEmphasis::Normal,
            },
        }
    }

    pub(crate) const fn disabled(mut self) -> Self {
        self.availability = MenuAvailability::Disabled;
        self
    }

    pub(crate) const fn selected(mut self) -> Self {
        self.emphasis = MenuEmphasis::Selected;
        self
    }

    #[allow(dead_code)]
    pub(crate) const fn pending(mut self) -> Self {
        self.availability = MenuAvailability::Pending;
        self
    }

    pub(crate) const fn destructive(mut self) -> Self {
        self.emphasis = MenuEmphasis::Destructive;
        self
    }

    const fn can_activate(self) -> bool {
        matches!(self.availability, MenuAvailability::Enabled)
    }
}

pub(crate) fn menu_button(
    label: impl Into<String>,
    action: MenuAction,
    tab_index: i32,
) -> impl Bundle {
    menu_button_bundle(label, action, tab_index, false)
}

fn row_menu_button(label: impl Into<String>, action: MenuAction, tab_index: i32) -> impl Bundle {
    menu_button_bundle(label, action, tab_index, true)
}

fn menu_button_bundle(
    label: impl Into<String>,
    action: MenuAction,
    tab_index: i32,
    in_row: bool,
) -> impl Bundle {
    (
        Button,
        Node {
            width: if in_row { auto() } else { percent(100) },
            flex_basis: if in_row { px(0) } else { auto() },
            flex_grow: if in_row { 1.0 } else { 0.0 },
            min_height: px(42),
            border: UiRect::all(px(2)),
            padding: UiRect::axes(px(12), px(8)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(CONTROL_IDLE),
        BorderColor::all(BORDER_IDLE),
        Outline::new(px(2), px(2), Color::NONE),
        TabIndex(tab_index),
        MenuTabOrder(tab_index),
        MenuButton::new(action),
        PanelSurface,
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(Color::srgb(0.92, 0.95, 1.0)),
            TextLayout::justify(Justify::Center),
        )],
    )
}

#[derive(Component)]
struct MenuRow;

fn menu_row() -> impl Bundle {
    (
        Node {
            width: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            column_gap: px(8),
            ..default()
        },
        MenuRow,
    )
}

#[derive(Component)]
struct ReadonlyPane;

#[derive(Component)]
struct MenuTextInput;

pub(crate) fn section_heading(label: impl Into<String>) -> impl Bundle {
    (
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::srgb(0.72, 0.9, 1.0)),
    )
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .init_resource::<MenuIntent>()
            .init_resource::<SettingsMenuState>()
            .add_systems(Startup, (spawn_menu_shell, open_home_on_startup).chain())
            .add_systems(
                Update,
                (
                    dispatch_pointer_actions,
                    dispatch_focused_action,
                    dispatch_local_setup_accelerators,
                    capture_camera_binding,
                    dispatch_match_menu,
                    dispatch_escape,
                    handle_navigation_action,
                    handle_local_setup_intent,
                    handle_settings_intent,
                    handle_match_menu_intent,
                    clear_menu_intent,
                    restore_home_after_submenu,
                    sync_menu_shell,
                    rebuild_menu_page,
                    style_menu_controls,
                )
                    .chain(),
            );
    }
}

fn spawn_menu_shell(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(percent(8), percent(5)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.005, 0.008, 0.015, 0.82)),
            GlobalZIndex(200),
            Interaction::default(),
            TabGroup::modal(),
            PanelSurface,
            MenuRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100),
                    max_width: px(1120),
                    height: percent(100),
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12),
                    padding: UiRect::all(px(18)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(MENU_BACKGROUND),
                BorderColor::all(BORDER_IDLE),
                PanelSurface,
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("CROWNLINES"),
                    TextFont {
                        font_size: FontSize::Px(28.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.95, 1.0)),
                    TextLayout::justify(Justify::Center),
                    MenuTitle,
                ));
                panel.spawn((
                    Node {
                        width: percent(100),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    Interaction::default(),
                    PanelSurface,
                    MenuContent,
                ));
                panel.spawn((
                    Node {
                        width: percent(100),
                        min_height: px(34),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    MenuFooter,
                    children![(
                        Text::new("Tab/Shift-Tab navigate - Enter/Space select - Esc back"),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.62, 0.68, 0.76)),
                    )],
                ));
            });
        });
}

fn open_home_on_startup(flow: Option<Res<ClientFlow>>, mut state: ResMut<MenuState>) {
    if flow.is_some_and(|flow| *flow == ClientFlow::Setup) {
        state.open(MenuRoute::Home);
    }
}

fn dispatch_pointer_actions(
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    mut intent: ResMut<MenuIntent>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed && button.can_activate() {
            intent.send(button.action);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn dispatch_focused_action(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    buttons: Query<&MenuButton>,
    mut intent: ResMut<MenuIntent>,
) {
    if !(keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space)) {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    if let Ok(button) = buttons.get(focused)
        && button.can_activate()
    {
        intent.send(button.action);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn dispatch_local_setup_accelerators(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<MenuState>,
    focus: Res<InputFocus>,
    editable: Query<(), With<EditableText>>,
    mut intent: ResMut<MenuIntent>,
) {
    if state.route != Some(MenuRoute::LocalSetup) || state.modal.is_some() {
        return;
    }
    let editing = focus
        .get()
        .is_some_and(|entity| editable.get(entity).is_ok());
    if let Some(action) = local_setup_accelerator(&keys, editing) {
        intent.send(action);
    }
}

fn local_setup_accelerator(keys: &ButtonInput<KeyCode>, editing: bool) -> Option<MenuAction> {
    if keys.just_pressed(KeyCode::F2) {
        Some(MenuAction::StartLocal)
    } else if keys.just_pressed(KeyCode::PageUp) {
        Some(MenuAction::PreviousScenario)
    } else if keys.just_pressed(KeyCode::PageDown) {
        Some(MenuAction::NextScenario)
    } else if keys.just_pressed(KeyCode::F7) {
        Some(MenuAction::CycleController(Player::North))
    } else if keys.just_pressed(KeyCode::F8) {
        Some(MenuAction::CycleController(Player::South))
    } else if !editing && keys.just_pressed(KeyCode::KeyX) {
        Some(MenuAction::SwapSides)
    } else if !editing && keys.just_pressed(KeyCode::KeyC) {
        Some(MenuAction::ToggleClock)
    } else if !editing && keys.just_pressed(KeyCode::Minus) {
        Some(MenuAction::DecreaseBase)
    } else if !editing && keys.just_pressed(KeyCode::Equal) {
        Some(MenuAction::IncreaseBase)
    } else if !editing && keys.just_pressed(KeyCode::Comma) {
        Some(MenuAction::DecreaseIncrement)
    } else if !editing && keys.just_pressed(KeyCode::Period) {
        Some(MenuAction::IncreaseIncrement)
    } else {
        None
    }
}

#[allow(clippy::needless_pass_by_value)]
fn capture_camera_binding(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<SettingsMenuState>,
) {
    let Some(slot) = settings.capturing else {
        return;
    };
    let Some(key) = supported_camera_keys()
        .into_iter()
        .find(|(_, key_code)| keys.just_pressed(*key_code))
        .map(|(key, _)| key)
    else {
        return;
    };
    let Some(draft) = settings.draft.as_mut() else {
        settings.capturing = None;
        return;
    };
    let previous = camera_binding(&draft.camera_bindings, slot);
    set_camera_binding(&mut draft.camera_bindings, slot, key);
    if let Err(error) = draft.validate() {
        set_camera_binding(&mut draft.camera_bindings, slot, previous);
        settings.message = error.to_string();
    } else {
        settings.message = format!("Captured Shift+{key:?}. Apply to save.");
    }
    settings.capturing = None;
}

const fn supported_camera_keys() -> [(CameraKey, KeyCode); 13] {
    [
        (CameraKey::W, KeyCode::KeyW),
        (CameraKey::A, KeyCode::KeyA),
        (CameraKey::S, KeyCode::KeyS),
        (CameraKey::D, KeyCode::KeyD),
        (CameraKey::Q, KeyCode::KeyQ),
        (CameraKey::E, KeyCode::KeyE),
        (CameraKey::F, KeyCode::KeyF),
        (CameraKey::Up, KeyCode::ArrowUp),
        (CameraKey::Down, KeyCode::ArrowDown),
        (CameraKey::Left, KeyCode::ArrowLeft),
        (CameraKey::Right, KeyCode::ArrowRight),
        (CameraKey::Minus, KeyCode::Minus),
        (CameraKey::Equal, KeyCode::Equal),
    ]
}

const fn camera_binding(bindings: &CameraBindingsSettings, slot: CameraBindingSlot) -> CameraKey {
    match slot {
        CameraBindingSlot::PanUp => bindings.pan_up,
        CameraBindingSlot::PanDown => bindings.pan_down,
        CameraBindingSlot::PanLeft => bindings.pan_left,
        CameraBindingSlot::PanRight => bindings.pan_right,
        CameraBindingSlot::ZoomIn => bindings.zoom_in,
        CameraBindingSlot::ZoomOut => bindings.zoom_out,
        CameraBindingSlot::Reset => bindings.reset,
    }
}

const fn set_camera_binding(
    bindings: &mut CameraBindingsSettings,
    slot: CameraBindingSlot,
    key: CameraKey,
) {
    match slot {
        CameraBindingSlot::PanUp => bindings.pan_up = key,
        CameraBindingSlot::PanDown => bindings.pan_down = key,
        CameraBindingSlot::PanLeft => bindings.pan_left = key,
        CameraBindingSlot::PanRight => bindings.pan_right = key,
        CameraBindingSlot::ZoomIn => bindings.zoom_in = key,
        CameraBindingSlot::ZoomOut => bindings.zoom_out = key,
        CameraBindingSlot::Reset => bindings.reset = key,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn dispatch_match_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut flow: Option<ResMut<ClientFlow>>,
    mut state: ResMut<MenuState>,
) {
    if state.is_open() {
        return;
    }
    let Some(flow) = flow.as_deref_mut() else {
        return;
    };
    match *flow {
        ClientFlow::Paused => state.open(MenuRoute::Pause),
        ClientFlow::Outcome => state.open(MenuRoute::Outcome),
        ClientFlow::Playing if keys.just_pressed(KeyCode::KeyP) => {
            *flow = ClientFlow::Paused;
            state.open(MenuRoute::Pause);
        }
        ClientFlow::OnlinePlaying if keys.just_pressed(KeyCode::KeyP) => {
            state.open(MenuRoute::Pause);
        }
        _ => {}
    }
}

#[allow(clippy::needless_pass_by_value)]
fn dispatch_escape(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<MenuState>,
    mut intent: ResMut<MenuIntent>,
) {
    if keys.just_pressed(KeyCode::Escape) && state.is_open() {
        intent.send(if state.modal.is_some() {
            MenuAction::Cancel
        } else if state.route == Some(MenuRoute::Settings) {
            MenuAction::CancelSettings
        } else {
            MenuAction::Back
        });
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn handle_navigation_action(
    mut state: ResMut<MenuState>,
    intent: Res<MenuIntent>,
    mut app_exit: MessageWriter<AppExit>,
    mut flow: Option<ResMut<ClientFlow>>,
    mut guided: Option<ResMut<GuidedRuntime>>,
    mut lobby: Option<ResMut<OnlineLobby>>,
    mut help: Option<ResMut<HelpState>>,
) {
    let Some(action) = intent.peek() else {
        return;
    };
    match action {
        MenuAction::Open(MenuRoute::Guided) => {
            if let Some(guided) = guided.as_deref_mut() {
                guided.open_browser();
                state.close();
            } else {
                state.open(MenuRoute::Guided);
            }
        }
        MenuAction::Open(MenuRoute::Online) => {
            if let (Some(lobby), Some(flow)) = (lobby.as_deref_mut(), flow.as_deref_mut()) {
                lobby.open_menu();
                *flow = ClientFlow::OnlineLobby;
                state.close();
            } else {
                state.open(MenuRoute::Online);
            }
        }
        MenuAction::Open(MenuRoute::Settings) => {}
        MenuAction::Open(route) => state.open(route),
        MenuAction::Back if state.route != Some(MenuRoute::Home) => state.back(),
        MenuAction::Quit => {
            app_exit.write(AppExit::Success);
        }
        MenuAction::Cancel if state.modal.is_some() => state.modal = None,
        MenuAction::OpenHelp => {
            if let Some(help) = help.as_deref_mut() {
                help.open_overview();
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn handle_local_setup_intent(
    intent: Res<MenuIntent>,
    catalog: Option<Res<ScenarioCatalog>>,
    mut setup: Option<ResMut<LocalSetup>>,
    mut flow: Option<ResMut<ClientFlow>>,
    mut game: Option<ResMut<DisplayedGame>>,
    mut selection: Option<ResMut<OverlaySelection>>,
    mut history: Option<ResMut<LocalTransitionNoticeLog>>,
    mut ai_epoch: Option<ResMut<AiCancellationEpoch>>,
    mut names: Query<(&mut EditableText, &MenuNameInput)>,
    mut state: ResMut<MenuState>,
) {
    let Some(
        action @ (MenuAction::PreviousScenario
        | MenuAction::NextScenario
        | MenuAction::CycleController(_)
        | MenuAction::SwapSides
        | MenuAction::ToggleClock
        | MenuAction::DecreaseBase
        | MenuAction::IncreaseBase
        | MenuAction::DecreaseIncrement
        | MenuAction::IncreaseIncrement
        | MenuAction::StartLocal),
    ) = intent.peek()
    else {
        return;
    };
    let (Some(catalog), Some(setup), Some(flow), Some(game), Some(selection), Some(history)) = (
        catalog.as_deref(),
        setup.as_deref_mut(),
        flow.as_deref_mut(),
        game.as_deref_mut(),
        selection.as_deref_mut(),
        history.as_deref_mut(),
    ) else {
        return;
    };
    handle_local_setup_action(
        action,
        catalog,
        setup,
        flow,
        game,
        selection,
        history,
        ai_epoch.as_deref_mut(),
        &mut names,
        &mut state,
    );
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn handle_settings_intent(
    intent: Res<MenuIntent>,
    mut menu: ResMut<SettingsMenuState>,
    mut settings: Option<ResMut<ClientSettings>>,
    mut ui_scale: Option<ResMut<UiScale>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    fields: Query<(&EditableText, &SettingsTextInput)>,
    mut state: ResMut<MenuState>,
) {
    let Some(action) = intent.peek() else {
        return;
    };
    if action == MenuAction::Open(MenuRoute::Settings) {
        if let Some(settings) = settings.as_deref() {
            menu.original = Some(settings.clone());
            menu.draft = Some(settings.clone());
            menu.tab = SettingsTab::Display;
            menu.capturing = None;
            menu.message.clear();
        }
        state.open(MenuRoute::Settings);
        return;
    }
    if action == MenuAction::Confirm && state.modal == Some(MenuModal::ForgetSavedSeat) {
        if let Some(draft) = menu.draft.as_mut() {
            draft.saved_online_seat = None;
            "Saved seat will be forgotten when settings are applied.".clone_into(&mut menu.message);
        }
        state.modal = None;
        return;
    }
    if let MenuAction::SelectSettingsTab(tab) = action {
        menu.tab = tab;
        return;
    }
    if matches!(
        action,
        MenuAction::NextResolution
            | MenuAction::DecreaseUiScale
            | MenuAction::IncreaseUiScale
            | MenuAction::ToggleReducedMotion
            | MenuAction::CaptureCameraBinding(_)
            | MenuAction::ResetCameraBindings
            | MenuAction::ApplySettings
            | MenuAction::CancelSettings
            | MenuAction::ForgetSavedSeat
    ) {
        handle_settings_action(
            action,
            &mut menu,
            settings.as_deref_mut(),
            ui_scale.as_deref_mut(),
            &mut windows,
            &fields,
            &mut state,
        );
    }
}

fn clear_menu_intent(mut intent: ResMut<MenuIntent>) {
    let _ = intent.take();
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_local_setup_action(
    action: MenuAction,
    catalog: &ScenarioCatalog,
    setup: &mut LocalSetup,
    flow: &mut ClientFlow,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    history: &mut LocalTransitionNoticeLog,
    ai_epoch: Option<&mut AiCancellationEpoch>,
    names: &mut Query<(&mut EditableText, &MenuNameInput)>,
    menu: &mut MenuState,
) {
    let mut north = setup.north_name.clone();
    let mut south = setup.south_name.clone();
    for (input, player) in names.iter_mut() {
        match player.0 {
            Player::North => north = input.value().to_string(),
            Player::South => south = input.value().to_string(),
        }
    }
    match action {
        MenuAction::PreviousScenario => {
            setup.selected_scenario =
                (setup.selected_scenario + catalog.0.len() - 1) % catalog.0.len();
        }
        MenuAction::NextScenario => {
            setup.selected_scenario = (setup.selected_scenario + 1) % catalog.0.len();
        }
        MenuAction::CycleController(player) => match player {
            Player::North => setup.north_controller = setup.north_controller.next(),
            Player::South => setup.south_controller = setup.south_controller.next(),
        },
        MenuAction::SwapSides => {
            std::mem::swap(&mut north, &mut south);
            std::mem::swap(&mut setup.north_controller, &mut setup.south_controller);
            for (mut input, player) in names.iter_mut() {
                input.editor_mut().set_text(match player.0 {
                    Player::North => &north,
                    Player::South => &south,
                });
            }
            setup.north_name.clone_from(&north);
            setup.south_name.clone_from(&south);
        }
        MenuAction::ToggleClock => {
            setup.clock = setup.clock.is_none().then_some(ClockSettings {
                base_minutes: 10,
                increment_seconds: 0,
            });
        }
        MenuAction::DecreaseBase => {
            if let Some(clock) = setup.clock.as_mut() {
                clock.base_minutes = clock.base_minutes.saturating_sub(1).max(MIN_BASE_MINUTES);
            }
        }
        MenuAction::IncreaseBase => {
            if let Some(clock) = setup.clock.as_mut() {
                clock.base_minutes = clock.base_minutes.saturating_add(1).min(MAX_BASE_MINUTES);
            }
        }
        MenuAction::DecreaseIncrement => {
            if let Some(clock) = setup.clock.as_mut() {
                clock.increment_seconds = clock.increment_seconds.saturating_sub(1);
            }
        }
        MenuAction::IncreaseIncrement => {
            if let Some(clock) = setup.clock.as_mut() {
                clock.increment_seconds = clock
                    .increment_seconds
                    .saturating_add(1)
                    .min(MAX_INCREMENT_SECONDS);
            }
        }
        MenuAction::StartLocal => match validate_names(&north, &south) {
            Ok((north, south)) => {
                let scenario = &catalog.0[setup.selected_scenario];
                if scenario.rules.fog.is_some()
                    && (setup.north_controller.profile().is_some()
                        || setup.south_controller.profile().is_some())
                {
                    "AI seats currently require a perfect-information scenario."
                        .clone_into(&mut setup.error);
                    return;
                }
                setup.north_name = north;
                setup.south_name = south;
                setup.error.clear();
                start_fresh_match(scenario, setup, game, selection, history);
                if let Some(epoch) = ai_epoch {
                    epoch.cancel_pending();
                }
                *flow = ClientFlow::Playing;
                menu.close();
            }
            Err(error) => error.clone_into(&mut setup.error),
        },
        _ => {}
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::cast_precision_loss
)]
fn handle_settings_action(
    action: MenuAction,
    menu: &mut SettingsMenuState,
    settings: Option<&mut ClientSettings>,
    ui_scale: Option<&mut UiScale>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    fields: &Query<(&EditableText, &SettingsTextInput)>,
    state: &mut MenuState,
) {
    let Some(draft) = menu.draft.as_mut() else {
        return;
    };
    match action {
        MenuAction::NextResolution => {
            const PRESETS: [(u32, u32); 6] = [
                (800, 600),
                (1280, 720),
                (1280, 800),
                (1600, 900),
                (1920, 1080),
                (2560, 1440),
            ];
            let current = PRESETS
                .iter()
                .position(|preset| *preset == (draft.window_width, draft.window_height))
                .unwrap_or(1);
            let next = PRESETS[(current + 1) % PRESETS.len()];
            draft.window_width = next.0;
            draft.window_height = next.1;
        }
        MenuAction::DecreaseUiScale => {
            draft.ui_scale = (draft.ui_scale - 0.05).max(0.75);
            preview_accessibility(draft, settings, ui_scale);
        }
        MenuAction::IncreaseUiScale => {
            draft.ui_scale = (draft.ui_scale + 0.05).min(2.5);
            preview_accessibility(draft, settings, ui_scale);
        }
        MenuAction::ToggleReducedMotion => {
            draft.reduced_motion = !draft.reduced_motion;
            preview_accessibility(draft, settings, ui_scale);
        }
        MenuAction::CaptureCameraBinding(slot) => {
            menu.capturing = Some(slot);
            menu.message = "Press one supported key; camera controls always require Shift.".into();
        }
        MenuAction::ResetCameraBindings => {
            draft.camera_bindings = CameraBindingsSettings::default();
            "Camera bindings reset in the draft. Apply to save.".clone_into(&mut menu.message);
        }
        MenuAction::ForgetSavedSeat => {
            state.modal = Some(MenuModal::ForgetSavedSeat);
        }
        MenuAction::ApplySettings => {
            for (value, field) in fields {
                match field {
                    SettingsTextInput::WindowWidth => {
                        let Ok(width) = value.value().to_string().parse() else {
                            "Window width must be a whole number.".clone_into(&mut menu.message);
                            return;
                        };
                        draft.window_width = width;
                    }
                    SettingsTextInput::WindowHeight => {
                        let Ok(height) = value.value().to_string().parse() else {
                            "Window height must be a whole number.".clone_into(&mut menu.message);
                            return;
                        };
                        draft.window_height = height;
                    }
                    SettingsTextInput::ServerUrl => {
                        value
                            .value()
                            .to_string()
                            .trim()
                            .clone_into(&mut draft.server_url);
                    }
                }
            }
            if let Err(error) = draft.validate() {
                menu.message = error.to_string();
                return;
            }
            if let Err(error) = draft.save() {
                menu.message = format!("Settings were not saved; previous values remain. {error}");
                return;
            }
            if let Some(settings) = settings {
                settings.clone_from(draft);
            }
            if let Some(ui_scale) = ui_scale {
                ui_scale.0 = draft.ui_scale;
            }
            if let Ok(mut window) = windows.single_mut() {
                window
                    .resolution
                    .set(draft.window_width as f32, draft.window_height as f32);
            }
            menu.original = Some(draft.clone());
            "Settings applied.".clone_into(&mut menu.message);
            state.back();
        }
        MenuAction::CancelSettings => {
            if let Some(original) = menu.original.take() {
                if let Some(settings) = settings {
                    settings.clone_from(&original);
                }
                if let Some(ui_scale) = ui_scale {
                    ui_scale.0 = original.ui_scale;
                }
            }
            menu.draft = None;
            menu.capturing = None;
            menu.message.clear();
            state.back();
        }
        _ => {}
    }
}

fn preview_accessibility(
    draft: &ClientSettings,
    settings: Option<&mut ClientSettings>,
    ui_scale: Option<&mut UiScale>,
) {
    if let Some(settings) = settings {
        settings.ui_scale = draft.ui_scale;
        settings.reduced_motion = draft.reduced_motion;
    }
    if let Some(ui_scale) = ui_scale {
        ui_scale.0 = draft.ui_scale;
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]
fn handle_match_menu_intent(
    intent: Res<MenuIntent>,
    mut state: ResMut<MenuState>,
    mut flow: Option<ResMut<ClientFlow>>,
    mut setup: Option<ResMut<LocalSetup>>,
    mut clock_runtime: Option<ResMut<LocalClockRuntime>>,
    mut game: Option<ResMut<DisplayedGame>>,
    mut history: Option<ResMut<LocalTransitionNoticeLog>>,
    mut events: Option<ResMut<LocalTransitionEventQueue>>,
    mut selection: Option<ResMut<OverlaySelection>>,
    mut fog: Option<ResMut<FogPresentation>>,
    mut ai_epoch: Option<ResMut<AiCancellationEpoch>>,
    mut persistence: Option<ResMut<LocalPersistenceStatus>>,
) {
    let Some(action) = intent.peek() else {
        return;
    };
    if action == MenuAction::ResumeMatch {
        if let Some(flow) = flow.as_deref_mut()
            && *flow == ClientFlow::Paused
        {
            *flow = ClientFlow::Playing;
        }
        state.close();
        return;
    }
    if action == MenuAction::ReturnHome {
        state.modal = Some(MenuModal::AbandonMatch);
        return;
    }
    if action == MenuAction::RequestResign {
        state.modal = Some(MenuModal::Resign);
        return;
    }
    if let MenuAction::SaveSlot(slot) = action {
        let occupied = local_save_slot_summaries()
            .into_iter()
            .find(|summary| summary.slot == slot)
            .is_some_and(|summary| summary.occupied);
        if occupied {
            state.modal = Some(MenuModal::OverwriteSlot(slot));
        } else if let (Some(game), Some(setup), Some(history)) =
            (game.as_deref(), setup.as_deref(), history.as_deref())
        {
            record_save_result(
                slot,
                save_slot(slot, game, setup, &history.entries),
                persistence.as_deref_mut(),
            );
        }
        return;
    }
    if let MenuAction::LoadSlot(slot) = action {
        let replacing_match = flow.as_deref().is_some_and(|flow| {
            matches!(
                *flow,
                ClientFlow::Playing | ClientFlow::Paused | ClientFlow::Outcome
            )
        });
        if replacing_match {
            state.modal = Some(MenuModal::LoadSlot(slot));
            return;
        }
        restore_slot(
            slot,
            flow.as_deref_mut(),
            setup.as_deref_mut(),
            clock_runtime.as_deref_mut(),
            game.as_deref_mut(),
            history.as_deref_mut(),
            events.as_deref_mut(),
            selection.as_deref_mut(),
            fog.as_deref_mut(),
            ai_epoch.as_deref_mut(),
            persistence.as_deref_mut(),
            &mut state,
        );
        return;
    }
    if action == MenuAction::Confirm {
        match state.modal {
            Some(MenuModal::OverwriteSlot(slot)) => {
                if let (Some(game), Some(setup), Some(history)) =
                    (game.as_deref(), setup.as_deref(), history.as_deref())
                {
                    record_save_result(
                        slot,
                        save_slot(slot, game, setup, &history.entries),
                        persistence.as_deref_mut(),
                    );
                }
                state.modal = None;
            }
            Some(MenuModal::LoadSlot(slot)) => {
                state.modal = None;
                restore_slot(
                    slot,
                    flow.as_deref_mut(),
                    setup.as_deref_mut(),
                    clock_runtime.as_deref_mut(),
                    game.as_deref_mut(),
                    history.as_deref_mut(),
                    events.as_deref_mut(),
                    selection.as_deref_mut(),
                    fog.as_deref_mut(),
                    ai_epoch.as_deref_mut(),
                    persistence.as_deref_mut(),
                    &mut state,
                );
            }
            Some(MenuModal::Resign) => {
                if let (Some(game), Some(events), Some(flow)) = (
                    game.as_deref_mut(),
                    events.as_deref_mut(),
                    flow.as_deref_mut(),
                ) {
                    apply_control(
                        &Action::Resign {
                            player: game.state.active_player,
                        },
                        game,
                        events,
                    );
                    *flow = ClientFlow::Outcome;
                    state.modal = None;
                    state.replace(MenuRoute::Outcome);
                }
            }
            Some(MenuModal::AbandonMatch) => {
                if let Some(flow) = flow.as_deref_mut() {
                    *flow = ClientFlow::Setup;
                }
                if let Some(selection) = selection.as_deref_mut() {
                    selection.piece = None;
                }
                state.modal = None;
                state.previous.clear();
                state.replace(MenuRoute::Home);
            }
            _ => {}
        }
        return;
    }
    let (Some(game), Some(events)) = (game.as_deref_mut(), events.as_deref_mut()) else {
        return;
    };
    match action {
        MenuAction::OfferDraw => apply_control(
            &Action::OfferDraw {
                player: game.state.active_player,
            },
            game,
            events,
        ),
        MenuAction::AcceptDraw | MenuAction::DeclineDraw => apply_control(
            &Action::RespondToDraw {
                player: game.state.active_player,
                accept: action == MenuAction::AcceptDraw,
            },
            game,
            events,
        ),
        MenuAction::Rematch => {
            let (Some(setup), Some(flow), Some(selection), Some(history)) = (
                setup.as_deref_mut(),
                flow.as_deref_mut(),
                selection.as_deref_mut(),
                history.as_deref_mut(),
            ) else {
                return;
            };
            let scenario = game.scenario.clone();
            start_fresh_match(&scenario, setup, game, selection, history);
            if let Some(epoch) = ai_epoch.as_deref_mut() {
                epoch.cancel_pending();
            }
            *flow = ClientFlow::Playing;
            state.close();
        }
        _ => {}
    }
}

fn record_save_result(
    slot: u8,
    result: Result<std::path::PathBuf, String>,
    status: Option<&mut LocalPersistenceStatus>,
) {
    let Some(status) = status else {
        return;
    };
    status.slot = slot;
    status.message = match result {
        Ok(path) => format!("Saved slot {slot} safely to {}.", path.display()),
        Err(error) => format!("Save failed; the previous slot was preserved. {error}"),
    };
}

#[allow(clippy::too_many_arguments)]
fn restore_slot(
    slot: u8,
    flow: Option<&mut ClientFlow>,
    setup: Option<&mut LocalSetup>,
    runtime: Option<&mut LocalClockRuntime>,
    game: Option<&mut DisplayedGame>,
    history: Option<&mut LocalTransitionNoticeLog>,
    events: Option<&mut LocalTransitionEventQueue>,
    selection: Option<&mut OverlaySelection>,
    fog: Option<&mut FogPresentation>,
    ai_epoch: Option<&mut AiCancellationEpoch>,
    status: Option<&mut LocalPersistenceStatus>,
    menu: &mut MenuState,
) {
    let (
        Some(flow),
        Some(setup),
        Some(runtime),
        Some(game),
        Some(history),
        Some(events),
        Some(selection),
        Some(fog),
    ) = (flow, setup, runtime, game, history, events, selection, fog)
    else {
        return;
    };
    match load_slot(slot) {
        Ok(document) => {
            game.scenario = ron::from_str(&document.scenario_ron)
                .expect("decoded save scenario was already validated");
            game.state = document.core.state;
            history.entries = document.history;
            setup.selected_scenario = document.selected_scenario;
            setup.session_id = document.session_id;
            setup.north_name = document.north_name;
            setup.south_name = document.south_name;
            setup.clock = document.clock;
            setup.north_controller = document.north_controller;
            setup.south_controller = document.south_controller;
            setup.error.clear();
            runtime.sub_millisecond_nanos = 0;
            selection.piece = None;
            events.mark_local_discontinuity();
            fog.require_handoff(game);
            if let Some(epoch) = ai_epoch {
                epoch.cancel_pending();
            }
            *flow = if game.state.outcome.is_some() {
                ClientFlow::Outcome
            } else {
                ClientFlow::Playing
            };
            if let Some(status) = status {
                status.slot = slot;
                status.message = format!(
                    "Loaded slot {slot}. Offline time was not charged; canonical revision {} restored.",
                    game.state.revision
                );
            }
            menu.close();
        }
        Err(error) => {
            if let Some(status) = status {
                status.message = format!("Load failed; the slot was unchanged. {error}");
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn restore_home_after_submenu(
    flow: Option<Res<ClientFlow>>,
    guided: Option<Res<GuidedRuntime>>,
    lobby: Option<Res<OnlineLobby>>,
    mut state: ResMut<MenuState>,
) {
    if state.is_open()
        || !flow.is_some_and(|flow| *flow == ClientFlow::Setup)
        || guided.is_some_and(|guided| guided.browser_is_open())
        || lobby.is_some_and(|lobby| lobby.screen != LobbyScreen::Closed)
    {
        return;
    }
    state.replace(MenuRoute::Home);
}

#[allow(clippy::needless_pass_by_value)]
fn sync_menu_shell(
    state: Res<MenuState>,
    mut roots: Query<&mut Node, With<MenuRoot>>,
    mut titles: Query<&mut Text, With<MenuTitle>>,
) {
    if !state.is_changed() {
        return;
    }
    for mut root in &mut roots {
        root.display = if state.is_open() {
            Display::Flex
        } else {
            Display::None
        };
    }
    let title = match state.route {
        Some(MenuRoute::Home) | None => "CROWNLINES",
        Some(MenuRoute::LocalSetup) => "NEW LOCAL MATCH",
        Some(MenuRoute::Guided) => "GUIDED PLAY",
        Some(MenuRoute::Online) => "ONLINE PLAY",
        Some(MenuRoute::Settings) => "SETTINGS",
        Some(MenuRoute::Pause) => "MATCH MENU",
        Some(MenuRoute::Saves) => "LOCAL SAVES",
        Some(MenuRoute::Outcome) => "MATCH COMPLETE",
    };
    for mut text in &mut titles {
        title.clone_into(&mut text.0);
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn rebuild_menu_page(
    state: Res<MenuState>,
    setup: Option<Res<LocalSetup>>,
    catalog: Option<Res<ScenarioCatalog>>,
    settings: Res<SettingsMenuState>,
    flow: Option<Res<ClientFlow>>,
    game: Option<Res<DisplayedGame>>,
    guided: Option<Res<GuidedRuntime>>,
    mut commands: Commands,
    content: Query<Entity, With<MenuContent>>,
) {
    if !state.is_changed()
        && !settings.is_changed()
        && !setup
            .as_ref()
            .is_some_and(bevy::prelude::DetectChanges::is_changed)
    {
        return;
    }
    let Ok(content) = content.single() else {
        return;
    };
    commands.entity(content).despawn_related::<Children>();
    let Some(route) = state.route else {
        return;
    };
    commands.entity(content).with_children(|page| {
        if let Some(modal) = state.modal {
            let (heading, copy, confirm) = match modal {
                MenuModal::ForgetSavedSeat => (
                    "FORGET SAVED ONLINE SEAT?",
                    "The local reconnect credential will be removed. This cannot be undone.",
                    "Forget seat",
                ),
                MenuModal::OverwriteSlot(_) => (
                    "OVERWRITE LOCAL SAVE?",
                    "The existing slot will be atomically replaced only after the new save validates.",
                    "Overwrite slot",
                ),
                MenuModal::LoadSlot(_) => (
                    "REPLACE ACTIVE MATCH?",
                    "Loading replaces the active local match with the selected validated save.",
                    "Load",
                ),
                MenuModal::Resign => (
                    "CONFIRM RESIGNATION?",
                    "The active player will immediately lose this local match.",
                    "Resign",
                ),
                MenuModal::AbandonMatch => (
                    "RETURN TO HOME?",
                    "Unsaved local match progress will be discarded.",
                    "Return Home",
                ),
            };
            page.spawn(section_heading(heading));
            page.spawn(body_text(copy));
            page.spawn(menu_row()).with_children(|row| {
                row.spawn(row_menu_button("Cancel [Esc]", MenuAction::Cancel, 0));
                row.spawn(row_menu_button(confirm, MenuAction::Confirm, 1))
                    .insert(MenuButton::new(MenuAction::Confirm).destructive());
            });
            return;
        }
        match route {
            MenuRoute::Home => spawn_home_page(page),
            MenuRoute::LocalSetup => {
                if let (Some(setup), Some(catalog)) = (setup.as_deref(), catalog.as_deref()) {
                    spawn_local_setup_page(page, setup, catalog);
                }
            }
            MenuRoute::Guided => spawn_placeholder(
                page,
                "Guided Play",
                "Tutorials and challenge scenarios are grouped here.",
            ),
            MenuRoute::Online => spawn_placeholder(
                page,
                "Online Play",
                "Host, Join, reconnect, and waiting-room controls are grouped here.",
            ),
            MenuRoute::Settings => spawn_settings_page(page, &settings),
            MenuRoute::Saves => spawn_saves_page(page, &state, flow.as_deref()),
            MenuRoute::Pause => spawn_pause_page(
                page,
                flow.as_deref(),
                game.as_deref(),
                guided.as_deref().is_some_and(GuidedRuntime::is_active),
            ),
            MenuRoute::Outcome => spawn_outcome_page(page, game.as_deref()),
        }
    });
}

fn spawn_pause_page(
    page: &mut ChildSpawnerCommands,
    flow: Option<&ClientFlow>,
    game: Option<&DisplayedGame>,
    guided: bool,
) {
    let online = flow.is_some_and(|flow| *flow == ClientFlow::OnlinePlaying);
    page.spawn(body_text(if online {
        "ONLINE MATCH MENU\nBoard input is blocked while this menu is open. The authoritative server clock continues."
    } else {
        "LOCAL MATCH PAUSED\nBoard input and the local clock are paused."
    }));
    page.spawn(menu_button("Resume Match [P]", MenuAction::ResumeMatch, 0));
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button(
            "Settings",
            MenuAction::Open(MenuRoute::Settings),
            1,
        ));
        row.spawn(row_menu_button(
            "Rules & Legend [F1]",
            MenuAction::OpenHelp,
            2,
        ));
    });
    if online {
        page.spawn(body_text(
            "Manual save/load is unavailable online. Draw, resignation, rematch, and leave actions remain authoritative and are available in the online match controls.",
        ));
        return;
    }
    if guided {
        page.spawn(body_text(
            "Guided attempts save progress automatically in separate storage; ordinary save slots are unavailable. Use the guided objective controls to retry or leave.",
        ));
        return;
    }
    page.spawn(menu_button(
        "Save / Load Game",
        MenuAction::Open(MenuRoute::Saves),
        3,
    ));
    if let Some(game) = game {
        match game.state.outstanding_draw_offer {
            None => {
                page.spawn(menu_button("Offer Draw", MenuAction::OfferDraw, 4));
            }
            Some(offering) if offering != game.state.active_player => {
                page.spawn(menu_row()).with_children(|row| {
                    row.spawn(row_menu_button("Accept Draw", MenuAction::AcceptDraw, 4));
                    row.spawn(row_menu_button("Decline Draw", MenuAction::DeclineDraw, 5));
                });
            }
            Some(_) => {
                page.spawn(body_text("Draw offer sent; waiting for the opponent."));
            }
        }
    }
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button("Resign", MenuAction::RequestResign, 6))
            .insert(MenuButton::new(MenuAction::RequestResign).destructive());
        row.spawn(row_menu_button("Return to Home", MenuAction::ReturnHome, 7))
            .insert(MenuButton::new(MenuAction::ReturnHome).destructive());
    });
}

fn spawn_saves_page(page: &mut ChildSpawnerCommands, state: &MenuState, flow: Option<&ClientFlow>) {
    let can_save = state.previous.last() == Some(&MenuRoute::Pause)
        && flow.is_some_and(|flow| *flow == ClientFlow::Paused);
    page.spawn(body_text(if can_save {
        "Choose one of three local slots. Occupied slots require overwrite confirmation."
    } else {
        "Choose a readable local slot to continue."
    }));
    for summary in local_save_slot_summaries() {
        page.spawn(section_heading(format!(
            "SLOT {} - {}",
            summary.slot, summary.description
        )));
        if can_save {
            page.spawn(menu_row()).with_children(|row| {
                row.spawn(row_menu_button(
                    if summary.occupied {
                        format!("Overwrite slot {}", summary.slot)
                    } else {
                        format!("Save to slot {}", summary.slot)
                    },
                    MenuAction::SaveSlot(summary.slot),
                    i32::from(summary.slot) * 2,
                ));
                row.spawn(row_menu_button(
                    if summary.readable {
                        format!("Load slot {}", summary.slot)
                    } else {
                        format!("Load slot {} - unavailable", summary.slot)
                    },
                    MenuAction::LoadSlot(summary.slot),
                    i32::from(summary.slot) * 2 + 1,
                ))
                .insert(if summary.readable {
                    MenuButton::new(MenuAction::LoadSlot(summary.slot))
                } else {
                    MenuButton::new(MenuAction::LoadSlot(summary.slot)).disabled()
                });
            });
        } else {
            page.spawn(menu_button(
                if summary.readable {
                    format!("Load slot {}", summary.slot)
                } else {
                    format!("Load slot {} - unavailable", summary.slot)
                },
                MenuAction::LoadSlot(summary.slot),
                i32::from(summary.slot) * 2 + 1,
            ))
            .insert(if summary.readable {
                MenuButton::new(MenuAction::LoadSlot(summary.slot))
            } else {
                MenuButton::new(MenuAction::LoadSlot(summary.slot)).disabled()
            });
        }
    }
    page.spawn(menu_button("Back [Esc]", MenuAction::Back, 20));
}

fn spawn_outcome_page(page: &mut ChildSpawnerCommands, game: Option<&DisplayedGame>) {
    if let Some(outcome) = game.and_then(|game| game.state.outcome) {
        page.spawn(section_heading("MATCH COMPLETE"));
        page.spawn(body_text(format!(
            "Winner: {}\nReason: {:?}",
            outcome
                .winner
                .map_or_else(|| "Draw".to_owned(), |winner| format!("{winner:?}")),
            outcome.reason
        )));
    }
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button("Rematch", MenuAction::Rematch, 0));
        row.spawn(row_menu_button("Rules & Legend", MenuAction::OpenHelp, 1));
    });
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button("Return to Home", MenuAction::ReturnHome, 2));
        row.spawn(row_menu_button("Quit", MenuAction::Quit, 3));
    });
}

#[allow(clippy::too_many_lines)]
fn spawn_settings_page(page: &mut ChildSpawnerCommands, menu: &SettingsMenuState) {
    let Some(draft) = menu.draft.as_ref() else {
        page.spawn(error_text("Settings could not be loaded."));
        page.spawn(menu_button("Back [Esc]", MenuAction::CancelSettings, 0));
        return;
    };
    page.spawn(body_text(
        "Changes remain a draft until Apply. UI scale and reduced motion preview immediately; Cancel restores them.",
    ));
    page.spawn(menu_row()).with_children(|row| {
        for (index, (label, tab)) in [
            ("Display", SettingsTab::Display),
            ("Accessibility", SettingsTab::Accessibility),
            ("Controls", SettingsTab::Controls),
            ("Online", SettingsTab::Online),
        ]
        .into_iter()
        .enumerate()
        {
            row.spawn(row_menu_button(
                label,
                MenuAction::SelectSettingsTab(tab),
                i32::try_from(index).unwrap(),
            ))
            .insert(if menu.tab == tab {
                MenuButton::new(MenuAction::SelectSettingsTab(tab)).selected()
            } else {
                MenuButton::new(MenuAction::SelectSettingsTab(tab))
            });
        }
    });
    match menu.tab {
        SettingsTab::Display => {
            page.spawn(section_heading("DISPLAY"));
            page.spawn(body_text(format!(
                "Window size: {}x{}. Presets cycle through supported desktop sizes; custom values are validated on Apply.",
                draft.window_width, draft.window_height
            )));
            page.spawn(menu_button(
                "Next window-size preset",
                MenuAction::NextResolution,
                10,
            ));
            page.spawn(menu_row()).with_children(|row| {
                row.spawn(settings_text_input(
                    "Custom window width",
                    draft.window_width.to_string(),
                    SettingsTextInput::WindowWidth,
                    11,
                    true,
                ));
                row.spawn(settings_text_input(
                    "Custom window height",
                    draft.window_height.to_string(),
                    SettingsTextInput::WindowHeight,
                    12,
                    true,
                ));
            });
        }
        SettingsTab::Accessibility => {
            page.spawn(section_heading("ACCESSIBILITY"));
            page.spawn(body_text(format!("UI scale: {:.2}", draft.ui_scale)));
            page.spawn(menu_row()).with_children(|row| {
                row.spawn(row_menu_button(
                    "Decrease UI scale",
                    MenuAction::DecreaseUiScale,
                    10,
                ));
                row.spawn(row_menu_button(
                    "Increase UI scale",
                    MenuAction::IncreaseUiScale,
                    11,
                ));
            });
            page.spawn(menu_button(
                format!("Reduced motion: {}", enabled_label(draft.reduced_motion)),
                MenuAction::ToggleReducedMotion,
                12,
            ));
        }
        SettingsTab::Controls => {
            page.spawn(section_heading("CAMERA CONTROLS"));
            page.spawn(body_text(
                "Camera bindings always use Shift, keeping them separate from menu and match commands.",
            ));
            for (index, (label, slot)) in [
                ("Pan up", CameraBindingSlot::PanUp),
                ("Pan down", CameraBindingSlot::PanDown),
                ("Pan left", CameraBindingSlot::PanLeft),
                ("Pan right", CameraBindingSlot::PanRight),
                ("Zoom in", CameraBindingSlot::ZoomIn),
                ("Zoom out", CameraBindingSlot::ZoomOut),
                ("Reset", CameraBindingSlot::Reset),
            ]
            .into_iter()
            .enumerate()
            {
                let binding = camera_binding(&draft.camera_bindings, slot);
                let capture = menu.capturing == Some(slot);
                page.spawn(menu_button(
                    if capture {
                        format!("{label}: press a key...")
                    } else {
                        format!("{label}: Shift+{binding:?}")
                    },
                    MenuAction::CaptureCameraBinding(slot),
                    10 + i32::try_from(index).unwrap(),
                ));
            }
            page.spawn(menu_button(
                "Reset camera bindings",
                MenuAction::ResetCameraBindings,
                17,
            ));
        }
        SettingsTab::Online => {
            page.spawn(section_heading("ONLINE"));
            page.spawn(settings_text_input(
                "Default server URL",
                &draft.server_url,
                SettingsTextInput::ServerUrl,
                10,
                false,
            ));
            page.spawn(menu_button(
                if draft.saved_online_seat.is_some() {
                    "Forget saved online seat"
                } else {
                    "Forget saved online seat - none stored"
                },
                MenuAction::ForgetSavedSeat,
                11,
            ))
            .insert(if draft.saved_online_seat.is_some() {
                MenuButton::new(MenuAction::ForgetSavedSeat).destructive()
            } else {
                MenuButton::new(MenuAction::ForgetSavedSeat).disabled()
            });
        }
    }
    if !menu.message.is_empty() {
        page.spawn(body_text(&menu.message));
    }
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button(
            "Cancel [Esc]",
            MenuAction::CancelSettings,
            30,
        ));
        row.spawn(row_menu_button(
            "Apply Settings",
            MenuAction::ApplySettings,
            31,
        ));
    });
}

fn settings_text_input(
    label: &str,
    value: impl AsRef<str>,
    field: SettingsTextInput,
    tab_index: i32,
    in_row: bool,
) -> impl Bundle {
    (
        Node {
            width: if in_row { auto() } else { percent(100) },
            flex_basis: if in_row { px(0) } else { auto() },
            flex_grow: if in_row { 1.0 } else { 0.0 },
            min_height: px(42),
            border: UiRect::all(px(2)),
            padding: UiRect::axes(px(10), px(7)),
            ..default()
        },
        BorderColor::all(INPUT_BORDER),
        BackgroundColor(INPUT_BACKGROUND),
        EditableText::new(value),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.92, 0.95, 1.0)),
        TextCursorStyle::default(),
        TabIndex(tab_index),
        field,
        MenuTextInput,
        PanelSurface,
        Name::new(label.to_owned()),
    )
}

#[allow(clippy::too_many_lines)]
fn spawn_local_setup_page(
    page: &mut ChildSpawnerCommands,
    setup: &LocalSetup,
    catalog: &ScenarioCatalog,
) {
    let scenario = &catalog.0[setup.selected_scenario];
    page.spawn(section_heading("SCENARIO"));
    page.spawn(body_text(format!(
        "{} - {}x{} - {}-{} minutes\nFog: {} - Pawn double-step: {} - en passant: {} - castling routes: {}",
        scenario.metadata.name,
        scenario.board.width,
        scenario.board.height,
        scenario.metadata.expected_minutes.0,
        scenario.metadata.expected_minutes.1,
        enabled_label(scenario.rules.fog.is_some()),
        enabled_label(scenario.rules.allow_pawn_double_step),
        enabled_label(scenario.rules.allow_en_passant),
        scenario.castling_routes.len(),
    )));
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button(
            "Previous scenario [PageUp]",
            MenuAction::PreviousScenario,
            0,
        ));
        row.spawn(row_menu_button(
            "Next scenario [PageDown]",
            MenuAction::NextScenario,
            1,
        ));
    });

    page.spawn(section_heading("PLAYERS"));
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(menu_name_input(
            "North player",
            &setup.north_name,
            Player::North,
            2,
            true,
        ));
        row.spawn(row_menu_button(
            format!(
                "North controller: {} [F7]",
                controller_label(setup.north_controller)
            ),
            MenuAction::CycleController(Player::North),
            3,
        ));
    });
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(menu_name_input(
            "South player",
            &setup.south_name,
            Player::South,
            4,
            true,
        ));
        row.spawn(row_menu_button(
            format!(
                "South controller: {} [F8]",
                controller_label(setup.south_controller)
            ),
            MenuAction::CycleController(Player::South),
            5,
        ));
    });
    page.spawn(menu_button(
        "Swap player names and controllers [X]",
        MenuAction::SwapSides,
        6,
    ));

    page.spawn(section_heading("TIME CONTROL"));
    page.spawn(menu_button(
        setup
            .clock
            .map_or("Untimed - enable clock [C]".to_owned(), |clock| {
                format!(
                    "Timed: {} min + {} sec - disable clock [C]",
                    clock.base_minutes, clock.increment_seconds
                )
            }),
        MenuAction::ToggleClock,
        7,
    ));
    if setup.clock.is_some() {
        page.spawn(menu_row()).with_children(|row| {
            row.spawn(row_menu_button(
                "Decrease base time [-]",
                MenuAction::DecreaseBase,
                8,
            ));
            row.spawn(row_menu_button(
                "Increase base time [+]",
                MenuAction::IncreaseBase,
                9,
            ));
        });
        page.spawn(menu_row()).with_children(|row| {
            row.spawn(row_menu_button(
                "Decrease increment [,]",
                MenuAction::DecreaseIncrement,
                10,
            ));
            row.spawn(row_menu_button(
                "Increase increment [.]",
                MenuAction::IncreaseIncrement,
                11,
            ));
        });
    }
    if !setup.error.is_empty() {
        page.spawn(error_text(&setup.error));
    }
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button("Back [Esc]", MenuAction::Back, 12));
        row.spawn(row_menu_button(
            "Start Local Match [F2]",
            MenuAction::StartLocal,
            13,
        ));
    });
}

fn menu_name_input(
    label: &str,
    value: &str,
    player: Player,
    tab_index: i32,
    in_row: bool,
) -> impl Bundle {
    (
        Node {
            width: if in_row { auto() } else { percent(100) },
            flex_basis: if in_row { px(0) } else { auto() },
            flex_grow: if in_row { 1.0 } else { 0.0 },
            min_height: px(42),
            border: UiRect::all(px(2)),
            padding: UiRect::axes(px(10), px(7)),
            ..default()
        },
        BorderColor::all(INPUT_BORDER),
        BackgroundColor(INPUT_BACKGROUND),
        EditableText::new(value),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.92, 0.95, 1.0)),
        TextCursorStyle::default(),
        TabIndex(tab_index),
        MenuNameInput(player),
        MenuTextInput,
        PanelSurface,
        Name::new(label.to_owned()),
    )
}

fn error_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.68, 0.62)),
    )
}

const fn enabled_label(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

const fn controller_label(controller: SeatController) -> &'static str {
    match controller {
        SeatController::Human => "Human",
        SeatController::Ai(crownline_ai::DifficultyProfile::Apprentice) => "AI Apprentice",
        SeatController::Ai(crownline_ai::DifficultyProfile::Steward) => "AI Steward",
        SeatController::Ai(crownline_ai::DifficultyProfile::Warden) => "AI Warden",
    }
}

fn spawn_home_page(page: &mut ChildSpawnerCommands) {
    page.spawn(body_text(
        "Choose a mode. Every action is available by pointer or keyboard; shown shortcuts are optional.",
    ));
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button(
            "New Local Match",
            MenuAction::Open(MenuRoute::LocalSetup),
            0,
        ));
        row.spawn(row_menu_button(
            "Guided Play",
            MenuAction::Open(MenuRoute::Guided),
            1,
        ));
    });
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button(
            "Online Play",
            MenuAction::Open(MenuRoute::Online),
            2,
        ));
        row.spawn(row_menu_button(
            "Settings",
            MenuAction::Open(MenuRoute::Settings),
            3,
        ));
    });
    let readable_save = has_readable_local_save();
    page.spawn(menu_row()).with_children(|row| {
        row.spawn(row_menu_button(
            "Rules & Legend [F1]",
            MenuAction::OpenHelp,
            4,
        ));
        row.spawn(row_menu_button(
            if readable_save {
                "Continue / Load Game"
            } else {
                "Continue / Load Game - no readable saves"
            },
            MenuAction::Open(MenuRoute::Saves),
            5,
        ))
        .insert(if readable_save {
            MenuButton::new(MenuAction::Open(MenuRoute::Saves))
        } else {
            MenuButton::new(MenuAction::Open(MenuRoute::Saves)).disabled()
        });
    });
    page.spawn(menu_button("Quit", MenuAction::Quit, 6));
    page.spawn((
        Text::new(format!(
            "Crownlines {} - native desktop client",
            env!("CARGO_PKG_VERSION")
        )),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(Color::srgb(0.58, 0.64, 0.72)),
        TextLayout::justify(Justify::Center),
    ));
}

fn spawn_placeholder(page: &mut ChildSpawnerCommands, heading: &str, description: &str) {
    page.spawn(section_heading(heading));
    page.spawn(body_text(description));
    page.spawn(menu_button("Back [Esc]", MenuAction::Back, 0));
}

fn body_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        Node {
            width: percent(100),
            border: UiRect::all(px(1)),
            padding: UiRect::axes(px(10), px(7)),
            ..default()
        },
        BackgroundColor(READONLY_BACKGROUND),
        BorderColor::all(READONLY_BORDER),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.82, 0.86, 0.92)),
        TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
        ReadonlyPane,
    )
}

type MenuControlStyleQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Interaction,
        &'static MenuButton,
        &'static MenuTabOrder,
        &'static mut TabIndex,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        &'static mut Outline,
    ),
>;

#[allow(clippy::needless_pass_by_value)]
fn style_menu_controls(mut focus: ResMut<InputFocus>, mut buttons: MenuControlStyleQuery) {
    for (
        entity,
        interaction,
        button,
        menu_order,
        mut tab_index,
        mut background,
        mut border,
        mut outline,
    ) in &mut buttons
    {
        let mut focused = focus.get() == Some(entity);
        if focused && !button.can_activate() {
            focus.clear();
            focused = false;
        }
        tab_index.0 = if button.can_activate() {
            menu_order.0
        } else {
            -1
        };
        background.0 = if !button.can_activate() {
            CONTROL_DISABLED
        } else if *interaction == Interaction::Pressed {
            CONTROL_PRESSED
        } else if *interaction == Interaction::Hovered {
            CONTROL_HOVERED
        } else if button.emphasis == MenuEmphasis::Destructive {
            CONTROL_DESTRUCTIVE
        } else if button.emphasis == MenuEmphasis::Exit {
            CONTROL_EXIT
        } else if button.emphasis == MenuEmphasis::Selected {
            CONTROL_SELECTED
        } else {
            CONTROL_IDLE
        };
        *border = BorderColor::all(if focused { BORDER_FOCUSED } else { BORDER_IDLE });
        outline.color = if focused { BORDER_FOCUSED } else { Color::NONE };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_navigation_stack_is_bounded_and_deterministic() {
        let mut state = MenuState::default();
        state.open(MenuRoute::Home);
        state.open(MenuRoute::Settings);
        state.open(MenuRoute::Settings);
        assert_eq!(state.previous, vec![MenuRoute::Home]);
        state.back();
        assert_eq!(state.route, Some(MenuRoute::Home));
        state.close();
        assert!(!state.is_open());
        assert!(state.previous.is_empty());
    }

    #[test]
    fn shell_is_full_window_scrollable_and_hidden_until_opened() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .add_plugins(MenuPlugin);
        app.update();

        let world = app.world_mut();
        let mut roots = world.query_filtered::<&Node, With<MenuRoot>>();
        let root = roots.single(world).unwrap();
        assert_eq!(root.display, Display::None);
        assert_eq!(root.left, px(0));
        assert_eq!(root.right, px(0));

        let mut content = world.query_filtered::<&Node, With<MenuContent>>();
        assert_eq!(
            content.single(world).unwrap().overflow,
            Overflow::scroll_y()
        );
    }

    #[test]
    fn home_opens_for_setup_and_exposes_every_destination() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<ClientFlow>()
            .init_resource::<LocalSetup>()
            .add_plugins(MenuPlugin);
        app.update();
        app.update();

        assert_eq!(
            app.world().resource::<MenuState>().route,
            Some(MenuRoute::Home)
        );
        let world = app.world_mut();
        let mut labels = world.query::<&Text>();
        let copy: Vec<_> = labels.iter(world).map(|text| text.0.as_str()).collect();
        for expected in [
            "New Local Match",
            "Guided Play",
            "Online Play",
            "Settings",
            "Rules & Legend [F1]",
            "Quit",
        ] {
            assert!(copy.contains(&expected), "missing Home action {expected}");
        }
    }

    #[test]
    fn menus_group_related_controls_and_distinguish_readonly_and_editable_panes() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<ClientFlow>()
            .init_resource::<LocalSetup>()
            .init_resource::<ScenarioCatalog>()
            .add_plugins(MenuPlugin);
        app.update();
        app.world_mut()
            .resource_mut::<MenuState>()
            .replace(MenuRoute::LocalSetup);
        app.update();

        let world = app.world_mut();
        let mut rows = world.query_filtered::<Entity, With<MenuRow>>();
        assert!(rows.iter(world).count() >= 4);
        let mut readonly =
            world.query_filtered::<(&BackgroundColor, &BorderColor), With<ReadonlyPane>>();
        let (background, border) = readonly.iter(world).next().unwrap();
        assert_eq!(background.0, READONLY_BACKGROUND);
        assert_eq!(border.left, READONLY_BORDER);
        let mut inputs =
            world.query_filtered::<(&BackgroundColor, &BorderColor), With<MenuTextInput>>();
        let (background, border) = inputs.iter(world).next().unwrap();
        assert_eq!(background.0, INPUT_BACKGROUND);
        assert_eq!(border.left, INPUT_BORDER);
    }

    #[test]
    fn back_cancel_and_quit_share_exit_emphasis() {
        for action in [
            MenuAction::Back,
            MenuAction::Cancel,
            MenuAction::CancelSettings,
            MenuAction::Quit,
        ] {
            assert_eq!(MenuButton::new(action).emphasis, MenuEmphasis::Exit);
        }
    }

    #[test]
    fn pointer_and_keyboard_use_the_same_typed_intent() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<MenuIntent>()
            .add_systems(Update, (dispatch_pointer_actions, dispatch_focused_action));
        let button = app
            .world_mut()
            .spawn((
                Interaction::Pressed,
                MenuButton::new(MenuAction::Open(MenuRoute::Settings)),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world_mut().resource_mut::<MenuIntent>().take(),
            Some(MenuAction::Open(MenuRoute::Settings))
        );

        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(button, bevy::input_focus::FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            app.world_mut().resource_mut::<MenuIntent>().take(),
            Some(MenuAction::Open(MenuRoute::Settings))
        );
    }

    #[test]
    fn disabled_controls_leave_tab_order_and_never_show_focus() {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .add_systems(Update, style_menu_controls);
        let control = app
            .world_mut()
            .spawn((
                Interaction::None,
                MenuButton::new(MenuAction::Quit).disabled(),
                MenuTabOrder(4),
                TabIndex(4),
                BackgroundColor(CONTROL_IDLE),
                BorderColor::all(BORDER_IDLE),
                Outline::new(px(2), px(2), BORDER_FOCUSED),
            ))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(control, bevy::input_focus::FocusCause::Navigated);

        app.update();

        assert_eq!(app.world().get::<TabIndex>(control).unwrap().0, -1);
        assert_eq!(
            app.world().get::<Outline>(control).unwrap().color,
            Color::NONE
        );
        assert_eq!(app.world().resource::<InputFocus>().get(), None);
    }

    #[test]
    fn modal_precedence_blocks_route_accelerators() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<MenuIntent>()
            .insert_resource(MenuState {
                route: Some(MenuRoute::LocalSetup),
                modal: Some(MenuModal::AbandonMatch),
                ..default()
            })
            .add_systems(Update, dispatch_local_setup_accelerators);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);

        app.update();

        assert_eq!(app.world().resource::<MenuIntent>().0, None);
    }

    #[test]
    fn quit_exits_immediately_without_opening_a_confirmation() {
        let mut app = App::new();
        app.add_message::<AppExit>()
            .init_resource::<MenuState>()
            .insert_resource(MenuIntent(Some(MenuAction::Quit)))
            .add_systems(Update, handle_navigation_action);

        app.update();

        assert_eq!(app.world().resource::<MenuState>().modal, None);
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 1);
    }

    #[test]
    fn presentation_only_navigation_preserves_canonical_match_state() {
        let scenario = ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let game = DisplayedGame {
            state: crownline_core::MatchState::from_scenario(&scenario).unwrap(),
            scenario,
        };
        let before = game.state.canonical_hash().unwrap();
        let mut menu = MenuState::default();

        menu.open(MenuRoute::Home);
        menu.open(MenuRoute::Settings);
        menu.back();
        menu.close();

        assert_eq!(game.state.canonical_hash().unwrap(), before);
    }

    #[test]
    fn bug_023_setup_accelerators_map_to_visible_actions_and_respect_text_editing() {
        for (key, expected) in [
            (KeyCode::KeyX, MenuAction::SwapSides),
            (KeyCode::F2, MenuAction::StartLocal),
            (KeyCode::KeyC, MenuAction::ToggleClock),
            (KeyCode::Minus, MenuAction::DecreaseBase),
            (KeyCode::Equal, MenuAction::IncreaseBase),
            (KeyCode::PageUp, MenuAction::PreviousScenario),
            (KeyCode::PageDown, MenuAction::NextScenario),
            (KeyCode::F7, MenuAction::CycleController(Player::North)),
            (KeyCode::F8, MenuAction::CycleController(Player::South)),
        ] {
            let mut keys = ButtonInput::default();
            keys.press(key);
            assert_eq!(local_setup_accelerator(&keys, false), Some(expected));
        }

        for key in [KeyCode::KeyX, KeyCode::KeyC, KeyCode::Minus, KeyCode::Equal] {
            let mut keys = ButtonInput::default();
            keys.press(key);
            assert_eq!(local_setup_accelerator(&keys, true), None);
        }
        let mut function_key = ButtonInput::default();
        function_key.press(KeyCode::F2);
        assert_eq!(
            local_setup_accelerator(&function_key, true),
            Some(MenuAction::StartLocal)
        );
    }

    #[test]
    fn match_menu_pauses_local_time_but_not_authoritative_online_time() {
        for (initial, expected) in [
            (ClientFlow::Playing, ClientFlow::Paused),
            (ClientFlow::OnlinePlaying, ClientFlow::OnlinePlaying),
        ] {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<MenuState>()
                .insert_resource(initial)
                .add_systems(Update, dispatch_match_menu);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyP);
            app.update();
            assert_eq!(*app.world().resource::<ClientFlow>(), expected);
            assert_eq!(
                app.world().resource::<MenuState>().route,
                Some(MenuRoute::Pause)
            );
        }
    }
}
