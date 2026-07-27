#![allow(dead_code)] // Routes and controls are populated incrementally by Tasks 07.04.02-07.

use bevy::{
    input_focus::{
        InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    prelude::*,
};

use crate::{
    lifecycle::{ClientFlow, LocalSetup},
    local_persistence::has_readable_local_save,
    panels::PanelSurface,
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
                    dispatch_escape,
                    handle_menu_intent,
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
fn handle_menu_intent(
    mut state: ResMut<MenuState>,
    mut intent: ResMut<MenuIntent>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(action) = intent.take() else {
        return;
    };
    match action {
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
        MenuAction::Confirm | MenuAction::Cancel | MenuAction::OpenHelp => {}
    }
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
            MenuRoute::LocalSetup => spawn_placeholder(
                page,
                "New Local Match",
                "Scenario, player, AI, and clock controls are grouped here.",
            ),
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
}
