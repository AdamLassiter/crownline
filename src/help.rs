use bevy::prelude::*;
use crownline_core::scenario::ScenarioDefinition;

use crate::{
    panels::PanelSurface,
    rendering::{DisplayedGame, OverlayLegend, overlay_legend_symbol},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HelpSection {
    #[default]
    Overview,
    Movement,
    Realm,
    Match,
    Legend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub(crate) struct HelpLink(pub HelpSection);

#[derive(Resource, Default)]
struct HelpState {
    open: bool,
    section: HelpSection,
}

#[derive(Component)]
struct HelpRoot;

#[derive(Component)]
struct HelpContent;

#[derive(Component)]
struct HelpClose;

pub struct RulesHelpPlugin;

impl Plugin for RulesHelpPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HelpState>()
            .add_systems(Startup, spawn_help)
            .add_systems(Update, (handle_help_controls, sync_help).chain());
    }
}

#[allow(clippy::too_many_lines)]
fn spawn_help(mut commands: Commands) {
    commands.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            left: percent(45),
            bottom: px(8),
            min_width: px(110),
            min_height: px(32),
            padding: UiRect::axes(px(8), px(5)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.1, 0.2, 0.28)),
        HelpLink(HelpSection::Overview),
        PanelSurface,
        children![(
            Text::new("F1 · Rules & legend"),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::srgb(0.7, 0.94, 1.0)),
        )],
    ));

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(10),
                top: percent(6),
                width: percent(80),
                height: percent(86),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(12)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.035, 0.055, 0.98)),
            BorderColor::all(Color::srgb(0.5, 0.7, 0.84)),
            GlobalZIndex(100),
            Interaction::default(),
            PanelSurface,
            HelpRoot,
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: percent(100),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(6),
                ..default()
            })
            .with_children(|navigation| {
                for (label, section) in [
                    ("Overview", HelpSection::Overview),
                    ("Movement & terrain", HelpSection::Movement),
                    ("Realm systems", HelpSection::Realm),
                    ("Match rules", HelpSection::Match),
                    ("Board legend", HelpSection::Legend),
                ] {
                    navigation.spawn(help_button(label, section));
                }
                navigation.spawn((
                    Button,
                    Node {
                        min_height: px(30),
                        padding: UiRect::axes(px(8), px(5)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.35, 0.12, 0.14)),
                    HelpClose,
                    PanelSurface,
                    children![(
                        Text::new("Close · Esc"),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    )],
                ));
            });
            root.spawn((
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                Interaction::default(),
                PanelSurface,
                children![(
                    Text::new("Loading rules help…"),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.94, 0.98)),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                    HelpContent,
                )],
            ));
        });
}

fn help_button(label: &str, section: HelpSection) -> impl Bundle {
    (
        Button,
        Node {
            min_height: px(30),
            padding: UiRect::axes(px(8), px(5)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.1, 0.18, 0.28)),
        HelpLink(section),
        PanelSurface,
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.9, 1.0)),
        )],
    )
}

#[allow(clippy::needless_pass_by_value)]
fn handle_help_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<HelpState>,
    links: Query<(&Interaction, &HelpLink), Changed<Interaction>>,
    closes: Query<&Interaction, (With<HelpClose>, Changed<Interaction>)>,
) {
    if keys.just_pressed(KeyCode::F1) {
        state.open = !state.open;
        state.section = HelpSection::Overview;
    }
    if keys.just_pressed(KeyCode::Escape) && state.open {
        state.open = false;
    }
    for (interaction, link) in &links {
        if *interaction == Interaction::Pressed {
            open_help(&mut state, link.0);
        }
    }
    if closes
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        state.open = false;
    }
}

fn open_help(state: &mut HelpState, section: HelpSection) {
    state.open = true;
    state.section = section;
}

#[allow(clippy::needless_pass_by_value)]
fn sync_help(
    state: Res<HelpState>,
    game: Res<DisplayedGame>,
    legend: Res<OverlayLegend>,
    mut roots: Query<&mut Node, With<HelpRoot>>,
    mut content: Query<&mut Text, With<HelpContent>>,
) {
    if let Ok(mut root) = roots.single_mut() {
        root.display = if state.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !state.is_changed() && !game.is_changed() {
        return;
    }
    if let Ok(mut text) = content.single_mut() {
        text.0 = help_text(state.section, &game.scenario, &legend);
    }
}

fn help_text(
    section: HelpSection,
    scenario: &ScenarioDefinition,
    legend: &OverlayLegend,
) -> String {
    match section {
        HelpSection::Overview => format!(
            "CROWNLINES RULES\n\n{}\n\n{}\n\n{}\n\nOpen Board legend for every visual mark.",
            movement_help(scenario),
            realm_help(scenario),
            match_help()
        ),
        HelpSection::Movement => movement_help(scenario),
        HelpSection::Realm => format!("{}\n\n{}", realm_help(scenario), legend_help(legend)),
        HelpSection::Match => match_help(),
        HelpSection::Legend => legend_help(legend),
    }
}

fn movement_help(scenario: &ScenarioDefinition) -> String {
    format!(
        "CHESS MOVEMENT & TERRAIN\nKing: one square; may castle only on an authored clear, unattacked route.\nQueen: orthogonal or diagonal rays. Rook: orthogonal rays. Bishop: diagonal rays. Knight: L-jump. Pawn: forward movement and diagonal capture in its army direction.\nPawn double-step: {}. En passant: {}. Authored castling routes: {}.\n\nOpen terrain has no extra blocker. Forest stops sliding rays beyond the forest tile. Mountain blocks sliders and Pawn placement; Knights jump over intervening terrain. Road is visually distinct but does not add a movement exception.\nRiver and Wall edges block crossing. Bridge, Ford, and Gate edges reopen their boundary. Knights ignore intervening edge barriers; diagonal movement must cross both component boundaries legally.",
        enabled(scenario.rules.allow_pawn_double_step),
        enabled(scenario.rules.allow_en_passant),
        scenario.castling_routes.len(),
    )
}

fn realm_help(scenario: &ScenarioDefinition) -> String {
    format!(
        "GOVERNANCE, DEVELOPMENT, PRODUCTION & PROMOTION\nKings, Queens, Rooks, and Bishops geometrically govern a settlement along an unblocked attack line; Knights and Pawns do not. A friendly founder on the endpoint does not block governance.\nAn owned settlement develops after {} continuous owner-turn cycles with its founder present, no enemy occupant, and at least one governor. Interruption {} progress in this scenario.\nAn established settlement produces after {} eligible cycles. Production queues a mandatory adjacent Pawn placement; Move and Hold remain disabled until every queued choice is resolved.\nA Pawn on a promotion site becomes eligible after {} surviving cycles. Promotion is mandatory when queued and offers Queen, Rook, Bishop, or Knight.",
        scenario.rules.establishment_cycles,
        if scenario.rules.development_resets_when_interrupted {
            "resets"
        } else {
            "pauses"
        },
        scenario.rules.production_cycles,
        scenario.rules.promotion_cycles,
    )
}

fn match_help() -> String {
    "COMMANDS, CLOCKS & OUTCOMES\nEach completed turn contains exactly one legal Move or Hold after mandatory choices. Hold preserves occupancy, is unavailable in check, and lets a player end a non-check no-move turn.\nWhen clocks are enabled, only the active player's clock runs, including during mandatory choices. Time is charged before action validation; expiration at the deadline wins. Increment is added only after an accepted Move or Hold.\nA match ends by checkmate, timeout, resignation, accepted draw, or automatic third repetition of the complete gameplay state. Terminal state rejects further gameplay actions.\nPreviews explain consequences but never rank or recommend moves."
        .to_owned()
}

fn legend_help(legend: &OverlayLegend) -> String {
    let mut lines = vec![
        "BOARD LEGEND".to_owned(),
        "Tiles: alternating light/dark parity remains visible under every tint. Open = neutral sand/stone; Forest = green; Mountain = gray; Road = ochre.".to_owned(),
        "Pieces: Unicode chess silhouette on a contrasting backplate. North uses pale glyphs on a dark upright plate; South uses dark glyphs on a pale rotated plate.".to_owned(),
        "Features: inset blue/orange tile = North/South Keep; square ring with ·/N/S = neutral/North/South settlement; purple X = promotion site; T = fortification tower.".to_owned(),
        "Edges: blue band = River; brown thick band with = = Bridge; light-blue band with ·· = Ford; dark band = Wall; gold band with / = Gate.".to_owned(),
        "Overlays:".to_owned(),
    ];
    lines.extend(
        legend
            .entries
            .iter()
            .map(|(kind, description)| format!("{} — {description}", overlay_legend_symbol(*kind))),
    );
    lines.join("\n")
}

const fn enabled(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario() -> ScenarioDefinition {
        ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap()
    }

    #[test]
    fn help_uses_active_scenario_switches_and_thresholds() {
        let scenario = scenario();
        let text = format!("{}\n{}", movement_help(&scenario), realm_help(&scenario));
        assert!(text.contains("Pawn double-step: enabled"));
        assert!(text.contains("En passant: enabled"));
        assert!(text.contains("after 3 continuous"));
        assert!(text.contains("after 3 eligible"));
        assert!(text.contains("after 2 surviving"));
        assert!(text.contains("pauses progress"));
    }

    #[test]
    fn legend_names_every_terrain_edge_feature_and_overlay() {
        let legend = OverlayLegend::default();
        let text = legend_help(&legend);
        for name in [
            "Open",
            "Forest",
            "Mountain",
            "Road",
            "River",
            "Bridge",
            "Ford",
            "Wall",
            "Gate",
            "Keep",
            "settlement",
            "promotion site",
            "fortification",
        ] {
            assert!(text.contains(name), "missing {name}");
        }
        for (_, description) in &legend.entries {
            assert!(text.contains(description));
        }
    }

    #[test]
    fn generated_help_does_not_claim_out_of_scope_modes() {
        let text = help_text(
            HelpSection::Overview,
            &scenario(),
            &OverlayLegend::default(),
        );
        for unsupported in [" AI ", "faction", "campaign"] {
            assert!(!text.to_lowercase().contains(&unsupported.to_lowercase()));
        }
    }

    #[test]
    fn context_link_opens_relevant_help_without_changing_match_state() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(crate::rendering::BoardRenderingPlugin)
            .add_plugins(crate::panels::InformationPanelsPlugin)
            .add_plugins(RulesHelpPlugin);
        app.update();
        let before = app
            .world()
            .resource::<DisplayedGame>()
            .state
            .canonical_hash()
            .unwrap();
        let link_entity = {
            let world = app.world_mut();
            let mut links = world.query::<(Entity, &HelpLink)>();
            links
                .iter(world)
                .find(|(_, link)| link.0 == HelpSection::Realm)
                .map(|(entity, _)| entity)
                .unwrap()
        };
        *app.world_mut()
            .entity_mut(link_entity)
            .get_mut::<Interaction>()
            .unwrap() = Interaction::Pressed;
        app.update();
        let state = app.world().resource::<HelpState>();
        assert!(state.open);
        assert_eq!(state.section, HelpSection::Realm);
        assert_eq!(
            app.world()
                .resource::<DisplayedGame>()
                .state
                .canonical_hash()
                .unwrap(),
            before
        );
    }
}
