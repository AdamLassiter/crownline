#![allow(dead_code)] // Routes and controls are populated incrementally by Tasks 07.04.02-07.

use bevy::{
    input_focus::{
        InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    prelude::*,
    text::{EditableText, TextCursorStyle},
};
use crownline_core::{
    ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES, scenario::Player,
};

use crate::{
    guided_play::GuidedRuntime,
    help::HelpState,
    lifecycle::{
        ClientFlow, LocalSetup, ScenarioCatalog, SeatController, start_fresh_match, validate_names,
    },
    local_ai::AiCancellationEpoch,
    local_persistence::has_readable_local_save,
    online_lobby::{LobbyScreen, OnlineLobby},
    panels::PanelSurface,
    rendering::{DisplayedGame, LocalTransitionNoticeLog, OverlaySelection},
};

const MENU_BACKGROUND: Color = Color::srgba(0.018, 0.026, 0.045, 0.985);
const CONTROL_IDLE: Color = Color::srgb(0.09, 0.13, 0.2);
const CONTROL_HOVERED: Color = Color::srgb(0.14, 0.22, 0.31);
const CONTROL_PRESSED: Color = Color::srgb(0.2, 0.34, 0.43);
const CONTROL_DISABLED: Color = Color::srgb(0.07, 0.075, 0.09);
const CONTROL_SELECTED: Color = Color::srgb(0.16, 0.3, 0.38);
const CONTROL_DESTRUCTIVE: Color = Color::srgb(0.35, 0.1, 0.12);
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
    Close,
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
}

#[derive(Debug, Clone, Resource, Default)]
pub(crate) struct MenuState {
    pub(crate) route: Option<MenuRoute>,
    pub(crate) previous: Vec<MenuRoute>,
    pub(crate) modal: bool,
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
        self.modal = false;
    }

    pub(crate) fn close(&mut self) {
        self.route = None;
        self.previous.clear();
        self.modal = false;
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
pub(crate) struct MenuButton {
    pub(crate) action: MenuAction,
    pub(crate) availability: MenuAvailability,
    pub(crate) emphasis: MenuEmphasis,
}

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
}

impl MenuButton {
    pub(crate) const fn new(action: MenuAction) -> Self {
        Self {
            action,
            availability: MenuAvailability::Enabled,
            emphasis: MenuEmphasis::Normal,
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
    (
        Button,
        Node {
            width: percent(100),
            min_height: px(42),
            border: UiRect::all(px(2)),
            padding: UiRect::axes(px(12), px(8)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(CONTROL_IDLE),
        BorderColor::all(BORDER_IDLE),
        TabIndex(tab_index),
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
            .add_systems(Startup, (spawn_menu_shell, open_home_on_startup).chain())
            .add_systems(
                Update,
                (
                    dispatch_pointer_actions,
                    dispatch_focused_action,
                    dispatch_local_setup_accelerators,
                    dispatch_escape,
                    handle_menu_intent,
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
    if state.route != Some(MenuRoute::LocalSetup) || state.modal {
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
fn dispatch_escape(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<MenuState>,
    mut intent: ResMut<MenuIntent>,
) {
    if keys.just_pressed(KeyCode::Escape) && state.is_open() {
        intent.send(if state.modal {
            MenuAction::Cancel
        } else {
            MenuAction::Back
        });
    }
}

#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_arguments)]
fn handle_menu_intent(
    mut state: ResMut<MenuState>,
    mut intent: ResMut<MenuIntent>,
    mut app_exit: MessageWriter<AppExit>,
    catalog: Option<Res<ScenarioCatalog>>,
    mut setup: Option<ResMut<LocalSetup>>,
    mut flow: Option<ResMut<ClientFlow>>,
    mut game: Option<ResMut<DisplayedGame>>,
    mut selection: Option<ResMut<OverlaySelection>>,
    mut history: Option<ResMut<LocalTransitionNoticeLog>>,
    mut ai_epoch: Option<ResMut<AiCancellationEpoch>>,
    mut names: Query<(&mut EditableText, &MenuNameInput)>,
    mut guided: Option<ResMut<GuidedRuntime>>,
    mut lobby: Option<ResMut<OnlineLobby>>,
    mut help: Option<ResMut<HelpState>>,
) {
    let Some(action) = intent.take() else {
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
        MenuAction::Open(route) => state.open(route),
        MenuAction::Back => {
            if state.route != Some(MenuRoute::Home) {
                state.back();
            }
        }
        MenuAction::Close => state.close(),
        MenuAction::Quit => state.modal = true,
        MenuAction::Confirm if state.modal && state.route == Some(MenuRoute::Home) => {
            app_exit.write(AppExit::Success);
        }
        MenuAction::Cancel if state.modal => state.modal = false,
        MenuAction::PreviousScenario
        | MenuAction::NextScenario
        | MenuAction::CycleController(_)
        | MenuAction::SwapSides
        | MenuAction::ToggleClock
        | MenuAction::DecreaseBase
        | MenuAction::IncreaseBase
        | MenuAction::DecreaseIncrement
        | MenuAction::IncreaseIncrement
        | MenuAction::StartLocal => {
            let (
                Some(catalog),
                Some(setup),
                Some(flow),
                Some(game),
                Some(selection),
                Some(history),
            ) = (
                catalog.as_deref(),
                setup.as_deref_mut(),
                flow.as_deref_mut(),
                game.as_deref_mut(),
                selection.as_deref_mut(),
                history.as_deref_mut(),
            )
            else {
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
        MenuAction::OpenHelp => {
            if let Some(help) = help.as_deref_mut() {
                help.open_overview();
            }
        }
        MenuAction::Confirm | MenuAction::Cancel => {}
    }
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

#[allow(clippy::needless_pass_by_value)]
fn rebuild_menu_page(
    state: Res<MenuState>,
    setup: Option<Res<LocalSetup>>,
    catalog: Option<Res<ScenarioCatalog>>,
    mut commands: Commands,
    content: Query<Entity, With<MenuContent>>,
) {
    if !state.is_changed()
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
        if state.modal {
            page.spawn(section_heading("QUIT CROWNLINES?"));
            page.spawn(body_text(
                "Any unsaved local match progress will be lost. Choose Quit to close the application.",
            ));
            page.spawn(menu_button("Cancel [Esc]", MenuAction::Cancel, 0));
            page.spawn(menu_button("Quit", MenuAction::Confirm, 1))
                .insert(MenuButton::new(MenuAction::Confirm).destructive());
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
            MenuRoute::Settings => spawn_placeholder(
                page,
                "Settings",
                "Display, Accessibility, Controls, and Online settings are grouped here.",
            ),
            MenuRoute::Saves => spawn_placeholder(
                page,
                "Continue / Load Game",
                "The three local save slots are grouped here.",
            ),
            MenuRoute::Pause => spawn_placeholder(
                page,
                "Match Menu",
                "Resume, persistence, rules, and match controls are grouped here.",
            ),
            MenuRoute::Outcome => spawn_placeholder(
                page,
                "Match Complete",
                "Outcome, rematch, and return actions are grouped here.",
            ),
        }
    });
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
    page.spawn(menu_button(
        "Previous scenario [PageUp]",
        MenuAction::PreviousScenario,
        0,
    ));
    page.spawn(menu_button(
        "Next scenario [PageDown]",
        MenuAction::NextScenario,
        1,
    ));

    page.spawn(section_heading("PLAYERS"));
    page.spawn(menu_name_input(
        "North player",
        &setup.north_name,
        Player::North,
        2,
    ));
    page.spawn(menu_button(
        format!(
            "North controller: {} [F7]",
            controller_label(setup.north_controller)
        ),
        MenuAction::CycleController(Player::North),
        3,
    ));
    page.spawn(menu_name_input(
        "South player",
        &setup.south_name,
        Player::South,
        4,
    ));
    page.spawn(menu_button(
        format!(
            "South controller: {} [F8]",
            controller_label(setup.south_controller)
        ),
        MenuAction::CycleController(Player::South),
        5,
    ));
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
        page.spawn(menu_button(
            "Decrease base time [-]",
            MenuAction::DecreaseBase,
            8,
        ));
        page.spawn(menu_button(
            "Increase base time [+]",
            MenuAction::IncreaseBase,
            9,
        ));
        page.spawn(menu_button(
            "Decrease increment [,]",
            MenuAction::DecreaseIncrement,
            10,
        ));
        page.spawn(menu_button(
            "Increase increment [.]",
            MenuAction::IncreaseIncrement,
            11,
        ));
    }
    if !setup.error.is_empty() {
        page.spawn(error_text(&setup.error));
    }
    page.spawn(menu_button("Back [Esc]", MenuAction::Back, 12));
    page.spawn(menu_button(
        "Start Local Match [F2]",
        MenuAction::StartLocal,
        13,
    ));
}

fn menu_name_input(label: &str, value: &str, player: Player, tab_index: i32) -> impl Bundle {
    (
        Node {
            width: percent(100),
            min_height: px(42),
            border: UiRect::all(px(2)),
            padding: UiRect::axes(px(10), px(7)),
            ..default()
        },
        BorderColor::all(BORDER_IDLE),
        BackgroundColor(CONTROL_IDLE),
        EditableText::new(value),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.92, 0.95, 1.0)),
        TextCursorStyle::default(),
        TabIndex(tab_index),
        MenuNameInput(player),
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
    for (index, (label, action)) in [
        ("New Local Match", MenuAction::Open(MenuRoute::LocalSetup)),
        ("Guided Play", MenuAction::Open(MenuRoute::Guided)),
        ("Online Play", MenuAction::Open(MenuRoute::Online)),
        ("Settings", MenuAction::Open(MenuRoute::Settings)),
        ("Rules & Legend [F1]", MenuAction::OpenHelp),
    ]
    .into_iter()
    .enumerate()
    {
        page.spawn(menu_button(label, action, i32::try_from(index).unwrap()));
    }
    let readable_save = has_readable_local_save();
    page.spawn(menu_button(
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
    page.spawn(menu_button("Quit", MenuAction::Quit, 6))
        .insert(MenuButton::new(MenuAction::Quit).destructive());
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
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.82, 0.86, 0.92)),
        TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
    )
}

type MenuControlStyleQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Interaction,
        &'static MenuButton,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
    ),
    Or<(Changed<Interaction>, Changed<MenuButton>)>,
>;

#[allow(clippy::needless_pass_by_value)]
fn style_menu_controls(
    focus: Res<InputFocus>,
    mut commands: Commands,
    mut buttons: MenuControlStyleQuery,
) {
    let focus_changed = focus.is_changed();
    for (entity, interaction, button, mut background, mut border) in &mut buttons {
        let focused = focus.get() == Some(entity);
        background.0 = if !button.can_activate() {
            CONTROL_DISABLED
        } else if *interaction == Interaction::Pressed {
            CONTROL_PRESSED
        } else if *interaction == Interaction::Hovered {
            CONTROL_HOVERED
        } else if button.emphasis == MenuEmphasis::Destructive {
            CONTROL_DESTRUCTIVE
        } else if button.emphasis == MenuEmphasis::Selected {
            CONTROL_SELECTED
        } else {
            CONTROL_IDLE
        };
        *border = BorderColor::all(if focused { BORDER_FOCUSED } else { BORDER_IDLE });
        if focused {
            commands.entity(entity).insert(Outline {
                color: BORDER_FOCUSED,
                width: px(2),
                offset: px(2),
            });
        } else if focus_changed {
            commands.entity(entity).remove::<Outline>();
        }
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
}
