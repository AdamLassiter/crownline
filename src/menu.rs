#![allow(dead_code)] // Routes and controls are populated incrementally by Tasks 07.04.02-07.

use bevy::{
    input_focus::{
        InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    prelude::*,
};

use crate::panels::PanelSurface;

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
            .add_systems(Startup, spawn_menu_shell)
            .add_systems(
                Update,
                (
                    dispatch_pointer_actions,
                    dispatch_focused_action,
                    sync_menu_shell,
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
