use super::*;

impl UnitesWar {
    fn draw_quad(ctx: &mut dyn GameContext<Action>, p1: Vec2, p2: Vec2, p3: Vec2, p4: Vec2, color: Color) {
        ctx.draw_triangle(p1, p2, p3, color);
        ctx.draw_triangle(p1, p3, p4, color);
    }

    /// Draws an extruded rectangle using the engine's existing triangle API.
    /// The offset represents the projected depth of the back face.
    fn draw_prism(ctx: &mut dyn GameContext<Action>, pos: Vec2, size: Vec2, depth: Vec2, color: Color) {
        let top_left: Vec2 = pos;
        let top_right: Vec2 = pos + Vec2::new(size.x, 0.0);
        let bottom_right: Vec2 = pos + size;
        let bottom_left: Vec2 = pos + Vec2::new(0.0, size.y);

        Self::draw_quad(
            ctx,
            top_left + depth,
            top_right + depth,
            top_right,
            top_left,
            color.lerp(Color::WHITE, 0.24),
        );
        Self::draw_quad(
            ctx,
            top_right + depth,
            bottom_right + depth,
            bottom_right,
            top_right,
            color.lerp(Color::BLACK, 0.28),
        );
        Self::draw_quad(ctx, top_left, top_right, bottom_right, bottom_left, color);
    }

    fn draw_shadow(ctx: &mut dyn GameContext<Action>, center: Vec2, width: f32, depth: f32) {
        let left: Vec2 = center + Vec2::new(-width * 0.5, 0.0);
        let top: Vec2 = center + Vec2::new(0.0, -depth * 0.5);
        let right: Vec2 = center + Vec2::new(width * 0.5, 0.0);
        let bottom: Vec2 = center + Vec2::new(0.0, depth * 0.5);
        Self::draw_quad(ctx, left, top, right, bottom, Color::from_rgba8(20, 24, 22, 105));
    }

    fn draw_bar(ctx: &mut dyn GameContext<Action>, pos: Vec2, size: Vec2, ratio: f32, fill: Color) {
        ctx.draw_rect(pos, size, Color::from_rgba8(28, 31, 35, 255));
        let inset: f32 = 3.0_f32.min(size.x * 0.25).min(size.y * 0.25);
        ctx.draw_rect(
            pos + Vec2::splat(inset),
            Vec2::new((size.x - inset * 2.0) * ratio.clamp(0.0, 1.0), size.y - inset * 2.0),
            fill,
        );
    }

    pub(super) fn glyph_rows(character: char) -> [u8; 7] {
        match character.to_ascii_uppercase() {
            'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
            'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
            'C' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
            'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
            'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
            'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
            'G' => [0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
            'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
            'I' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
            'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
            'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
            'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
            'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
            'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
            'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
            'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
            'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
            'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
            'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
            'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
            'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
            'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
            'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
            'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
            'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
            'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
            '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
            '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
            '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
            '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
            '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
            '5' => [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110],
            '6' => [0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
            '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
            '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
            '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
            '-' => [0, 0, 0, 0b11111, 0, 0, 0],
            ':' => [0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0],
            '.' => [0, 0, 0, 0, 0, 0b00100, 0b00100],
            '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
            _ => [0; 7],
        }
    }

    pub(super) fn text_width(text: &str, scale: f32) -> f32 {
        let count: usize = text.chars().count();
        if count == 0 {
            0.0
        } else {
            (count as f32 * 6.0 - 1.0) * scale
        }
    }

    fn draw_text(ctx: &mut dyn GameContext<Action>, text: &str, pos: Vec2, scale: f32, color: Color) {
        for (character_index, character) in text.chars().enumerate() {
            let rows: [u8; 7] = Self::glyph_rows(character);
            for (row, bits) in rows.into_iter().enumerate() {
                for column in 0..5 {
                    if bits & (1 << (4 - column)) != 0 {
                        ctx.draw_rect(
                            pos + Vec2::new((character_index as f32 * 6.0 + column as f32) * scale, row as f32 * scale),
                            Vec2::splat(scale),
                            color,
                        );
                    }
                }
            }
        }
    }

    fn draw_text_centered(
        ctx: &mut dyn GameContext<Action>,
        text: &str,
        center_x: f32,
        y: f32,
        scale: f32,
        color: Color,
    ) {
        let x: f32 = center_x - Self::text_width(text, scale) * 0.5;
        Self::draw_text(ctx, text, Vec2::new(x, y), scale, color);
    }

    pub(super) fn draw_menu(&self, ctx: &mut dyn GameContext<Action>, width: f32, height: f32) {
        Self::draw_background(ctx, width, height);
        Self::draw_castle(ctx, &self.player_castle, Faction::Player, width, height);
        Self::draw_castle(ctx, &self.enemy_castle, Faction::Enemy, width, height);

        let panel_width: f32 = (width * 0.62).clamp(390.0, 700.0);
        let panel_height: f32 = (height * 0.76).clamp(440.0, 560.0);
        let panel_pos: Vec2 = Vec2::new((width - panel_width) * 0.5, (height - panel_height) * 0.43);
        Self::draw_prism(
            ctx,
            panel_pos,
            Vec2::new(panel_width, panel_height),
            Vec2::new(12.0, -10.0),
            Color::from_rgba8(31, 40, 39, 245),
        );

        let title_scale: f32 = (width / 260.0).clamp(3.2, 5.0);
        Self::draw_text_centered(
            ctx,
            "UNITES WAR",
            width * 0.5 + 3.0,
            panel_pos.y + 58.0,
            title_scale,
            Color::from_rgba8(34, 42, 39, 220),
        );
        Self::draw_text_centered(
            ctx,
            "UNITES WAR",
            width * 0.5,
            panel_pos.y + 54.0,
            title_scale,
            Color::from_rgba8(239, 197, 83, 255),
        );
        Self::draw_text_centered(
            ctx,
            "GUERRA ENTRE CLAS",
            width * 0.5,
            panel_pos.y + 111.0,
            2.0,
            Color::from_rgba8(174, 194, 177, 255),
        );
        Self::draw_text_centered(
            ctx,
            "MOEDAS POR TEMPO E ABATES",
            width * 0.5,
            panel_pos.y + 138.0,
            1.45,
            Color::from_rgba8(226, 190, 99, 255),
        );

        let mouse: Option<Vec2> = ctx.input().mouse_position();
        for (index, label) in ["JOGAR", "SAIR"].into_iter().enumerate() {
            let (button_pos, button_size): (Vec2, Vec2) = Self::menu_button_rect(index, width, height);
            let hovered: bool = mouse.is_some_and(|point: Vec2| Self::point_in_rect(point, button_pos, button_size));
            let button_color: Color = match (index, hovered) {
                (0, true) => Color::from_rgba8(74, 157, 83, 255),
                (0, false) => Color::from_rgba8(52, 111, 66, 255),
                (1, true) => Color::from_rgba8(171, 78, 62, 255),
                _ => Color::from_rgba8(112, 59, 54, 255),
            };
            Self::draw_prism(ctx, button_pos, button_size, Vec2::new(7.0, -6.0), button_color);
            Self::draw_text_centered(ctx, label, width * 0.5, button_pos.y + 18.0, 3.0, Color::WHITE);
        }

        Self::draw_text_centered(
            ctx,
            "ENTER PARA JOGAR",
            width * 0.5,
            height * 0.78,
            1.5,
            Color::from_rgba8(181, 190, 182, 255),
        );
        Self::draw_text_centered(
            ctx,
            "1-4 TROPAS   Q MAGIA   U MELHORIA",
            width * 0.5,
            height * 0.84,
            1.35,
            Color::from_rgba8(139, 158, 146, 255),
        );
    }

    pub(super) fn draw_background(ctx: &mut dyn GameContext<Action>, width: f32, height: f32) {
        let ground: f32 = Self::ground_y(height);
        let horizon: f32 = ground - 142.0;
        ctx.clear_color(Color::from_rgba8(41, 71, 79, 255));
        ctx.draw_rect(
            Vec2::new(0.0, ground - 190.0),
            Vec2::new(width, 190.0),
            Color::from_rgba8(74, 105, 86, 255),
        );
        ctx.draw_triangle(
            Vec2::new(0.0, ground - 190.0),
            Vec2::new(width * 0.28, ground - 315.0),
            Vec2::new(width * 0.52, ground - 190.0),
            Color::from_rgba8(55, 88, 77, 255),
        );
        ctx.draw_triangle(
            Vec2::new(width * 0.36, ground - 190.0),
            Vec2::new(width * 0.72, ground - 290.0),
            Vec2::new(width, ground - 190.0),
            Color::from_rgba8(49, 82, 73, 255),
        );

        Self::draw_quad(
            ctx,
            Vec2::new(0.0, horizon),
            Vec2::new(width, horizon),
            Vec2::new(width, height - TOOLBAR_HEIGHT),
            Vec2::new(0.0, height - TOOLBAR_HEIGHT),
            Color::from_rgba8(70, 91, 58, 255),
        );

        // The converging battlefield gives the side-view game a projected 3D floor.
        Self::draw_quad(
            ctx,
            Vec2::new(width * 0.38, horizon),
            Vec2::new(width * 0.62, horizon),
            Vec2::new(width + 70.0, height - TOOLBAR_HEIGHT),
            Vec2::new(-70.0, height - TOOLBAR_HEIGHT),
            Color::from_rgba8(91, 72, 49, 255),
        );
        Self::draw_quad(
            ctx,
            Vec2::new(width * 0.38, horizon),
            Vec2::new(width * 0.395, horizon),
            Vec2::new(35.0, height - TOOLBAR_HEIGHT),
            Vec2::new(0.0, height - TOOLBAR_HEIGHT),
            Color::from_rgba8(121, 101, 63, 255),
        );
        Self::draw_quad(
            ctx,
            Vec2::new(width * 0.605, horizon),
            Vec2::new(width * 0.62, horizon),
            Vec2::new(width, height - TOOLBAR_HEIGHT),
            Vec2::new(width - 35.0, height - TOOLBAR_HEIGHT),
            Color::from_rgba8(73, 57, 42, 255),
        );

        ctx.draw_rect(
            Vec2::new(0.0, ground),
            Vec2::new(width, 7.0),
            Color::from_rgba8(113, 145, 75, 255),
        );
    }

    pub(super) fn draw_castle(
        ctx: &mut dyn GameContext<Action>,
        castle: &Castle,
        faction: Faction,
        width: f32,
        height: f32,
    ) {
        let ground: f32 = Self::ground_y(height);
        let x: f32 = Self::castle_x(faction, width);
        let stone: Color = match faction {
            Faction::Player => Color::from_rgba8(77, 111, 91, 255),
            Faction::Enemy => Color::from_rgba8(125, 72, 64, 255),
        };
        let depth: Vec2 = Vec2::new(12.0, -9.0);
        let dark: Color = stone.lerp(Color::BLACK, 0.27);

        Self::draw_shadow(ctx, Vec2::new(x + 9.0, ground + 5.0), 128.0, 32.0);
        Self::draw_prism(
            ctx,
            Vec2::new(x - CASTLE_WIDTH * 0.5, ground - 126.0),
            Vec2::new(CASTLE_WIDTH, 126.0),
            depth,
            dark,
        );
        Self::draw_prism(ctx, Vec2::new(x - 38.0, ground - 153.0), Vec2::new(31.0, 53.0), depth, stone);
        Self::draw_prism(ctx, Vec2::new(x + 7.0, ground - 153.0), Vec2::new(31.0, 53.0), depth, stone);
        Self::draw_prism(
            ctx,
            Vec2::new(x - 32.0, ground - 91.0),
            Vec2::new(64.0, 91.0),
            Vec2::new(9.0, -7.0),
            stone,
        );

        Self::draw_prism(
            ctx,
            Vec2::new(x - 13.0, ground - 55.0),
            Vec2::new(26.0, 55.0),
            Vec2::new(4.0, -3.0),
            Color::from_rgba8(35, 35, 31, 255),
        );

        // Projected battlements and a flag make the castle read as a low-poly model.
        for battlement in [-34.0_f32, -12.0, 10.0, 32.0] {
            Self::draw_prism(
                ctx,
                Vec2::new(x + battlement - 7.0, ground - 137.0),
                Vec2::new(14.0, 17.0),
                Vec2::new(5.0, -4.0),
                stone.lerp(Color::WHITE, 0.06),
            );
        }
        let flag_direction: f32 = faction.direction();
        let flag_x: f32 = x - flag_direction * 18.0;
        ctx.draw_rect(
            Vec2::new(flag_x - 2.0, ground - 206.0),
            Vec2::new(4.0, 61.0),
            Color::from_rgba8(64, 51, 38, 255),
        );
        ctx.draw_triangle(
            Vec2::new(flag_x, ground - 204.0),
            Vec2::new(flag_x + flag_direction * 35.0, ground - 191.0),
            Vec2::new(flag_x, ground - 178.0),
            if faction == Faction::Player {
                Color::from_rgba8(77, 181, 96, 255)
            } else {
                Color::from_rgba8(202, 68, 59, 255)
            },
        );

        for level in 0..castle.level {
            let marker_x: f32 = if faction == Faction::Player {
                x - 31.0 + level as f32 * 18.0
            } else {
                x + 19.0 - level as f32 * 18.0
            };
            ctx.draw_rect(
                Vec2::new(marker_x, ground - 118.0),
                Vec2::new(12.0, 18.0),
                Color::from_rgba8(247, 195, 80, 255),
            );
        }

        Self::draw_bar(
            ctx,
            Vec2::new(x - 52.0, ground - 177.0),
            Vec2::new(104.0, 13.0),
            castle.health / castle.max_health,
            if faction == Faction::Player {
                Color::from_rgba8(91, 218, 103, 255)
            } else {
                Color::from_rgba8(231, 82, 67, 255)
            },
        );
    }

    pub(super) fn draw_unit(&self, ctx: &mut dyn GameContext<Action>, unit: &Unit) {
        let stats: UnitStats = unit.kind.stats();
        let bob: f32 = (self.battle_time * 5.5 + unit.pos.x * 0.035).sin() * 1.2;
        let top_left: Vec2 = unit.pos - stats.size * 0.5 + Vec2::new(0.0, bob);
        let body_color: Color = if unit.hit_flash > 0.0 {
            Color::WHITE
        } else {
            unit.kind.color(unit.faction)
        };
        let direction: f32 = unit.faction.direction();
        let model_depth: Vec2 = Vec2::new(5.0, -4.0);

        Self::draw_shadow(
            ctx,
            Vec2::new(unit.pos.x + direction * 3.0, unit.pos.y + stats.size.y * 0.5 + 3.0),
            stats.size.x * 1.35,
            stats.size.x * 0.48,
        );

        let leg_width: f32 = stats.size.x * 0.22;
        Self::draw_prism(
            ctx,
            top_left + Vec2::new(stats.size.x * 0.22, stats.size.y * 0.7),
            Vec2::new(leg_width, stats.size.y * 0.3),
            model_depth * 0.55,
            body_color.lerp(Color::BLACK, 0.28),
        );
        Self::draw_prism(
            ctx,
            top_left + Vec2::new(stats.size.x * 0.57, stats.size.y * 0.7),
            Vec2::new(leg_width, stats.size.y * 0.3),
            model_depth * 0.55,
            body_color.lerp(Color::BLACK, 0.2),
        );

        Self::draw_prism(
            ctx,
            top_left + Vec2::new(stats.size.x * 0.18, stats.size.y * 0.3),
            Vec2::new(stats.size.x * 0.64, stats.size.y * 0.7),
            model_depth,
            body_color,
        );
        Self::draw_prism(
            ctx,
            top_left + Vec2::new(stats.size.x * 0.23, 0.0),
            Vec2::new(stats.size.x * 0.54, stats.size.y * 0.42),
            model_depth * 0.8,
            body_color.lerp(Color::WHITE, 0.08),
        );
        let eye_x: f32 = if direction > 0.0 {
            top_left.x + stats.size.x * 0.61
        } else {
            top_left.x + stats.size.x * 0.3
        };
        ctx.draw_rect(
            Vec2::new(eye_x, top_left.y + stats.size.y * 0.12),
            Vec2::splat(3.0),
            Color::from_rgba8(242, 225, 108, 255),
        );

        match unit.kind {
            UnitKind::Runner => {
                let weapon_x: f32 = unit.pos.x + direction * stats.size.x * 0.45;
                ctx.draw_triangle(
                    Vec2::new(weapon_x, unit.pos.y - 4.0),
                    Vec2::new(weapon_x + direction * 13.0, unit.pos.y + 2.0),
                    Vec2::new(weapon_x, unit.pos.y + 5.0),
                    Color::from_rgba8(219, 215, 196, 255),
                );
            }
            UnitKind::Guard => {
                let shield_x: f32 = unit.pos.x + direction * stats.size.x * 0.5;
                Self::draw_prism(
                    ctx,
                    Vec2::new(shield_x - 5.0, unit.pos.y - 9.0),
                    Vec2::new(10.0, 20.0),
                    Vec2::new(3.0, -3.0),
                    Color::from_rgba8(87, 112, 129, 255),
                );
            }
            UnitKind::Archer => {
                let bow_x: f32 = unit.pos.x + direction * stats.size.x * 0.48;
                ctx.draw_triangle(
                    Vec2::new(bow_x, unit.pos.y - 12.0),
                    Vec2::new(bow_x + direction * 8.0, unit.pos.y),
                    Vec2::new(bow_x, unit.pos.y + 12.0),
                    Color::from_rgba8(111, 67, 37, 255),
                );
            }
            UnitKind::Brute => {
                Self::draw_prism(
                    ctx,
                    Vec2::new(unit.pos.x + direction * 12.0 - 5.0, top_left.y - 5.0),
                    Vec2::new(10.0, 31.0),
                    Vec2::new(4.0, -4.0),
                    Color::from_rgba8(87, 62, 42, 255),
                );
            }
        }

        Self::draw_bar(
            ctx,
            Vec2::new(top_left.x, top_left.y - 8.0),
            Vec2::new(stats.size.x, 5.0),
            unit.health / unit.max_health,
            if unit.faction == Faction::Player {
                Color::from_rgba8(89, 220, 98, 255)
            } else {
                Color::from_rgba8(235, 82, 65, 255)
            },
        );

        if unit.faction == Faction::Player {
            let passive: PassiveKind = match unit.kind {
                UnitKind::Runner => PassiveKind::Momentum,
                UnitKind::Guard => PassiveKind::Bulwark,
                UnitKind::Archer => PassiveKind::PiercingShot,
                UnitKind::Brute => PassiveKind::Rage,
            };
            if self.player_passives.contains(passive) {
                let active: bool = match passive {
                    PassiveKind::Momentum => unit.momentum_stacks > 0,
                    PassiveKind::Rage => unit.health / unit.max_health <= RAGE_HEALTH_THRESHOLD,
                    PassiveKind::Bulwark | PassiveKind::PiercingShot => true,
                };
                let indicator_color: Color = if active {
                    Color::from_rgba8(250, 202, 75, 255)
                } else {
                    Color::from_rgba8(103, 121, 116, 255)
                };
                ctx.draw_rect(
                    Vec2::new(top_left.x, top_left.y - 14.0),
                    Vec2::new(stats.size.x * 0.72, 3.0),
                    indicator_color,
                );
            }
        }
    }

    pub(super) fn draw_effects(&self, ctx: &mut dyn GameContext<Action>) {
        for effect in &self.effects {
            let progress: f32 = 1.0 - effect.life / effect.max_life;
            let pos: Vec2 = effect.start.lerp(effect.end, progress.clamp(0.0, 1.0));
            let size: f32 = if effect.heavy { 9.0 } else { 5.0 };
            Self::draw_prism(
                ctx,
                pos - Vec2::splat(size * 0.5),
                Vec2::splat(size),
                Vec2::new(size * 0.45, -size * 0.45),
                effect.color,
            );
            if effect.heavy {
                let midpoint: Vec2 = effect.start.lerp(effect.end, 0.5);
                ctx.draw_triangle(
                    effect.start,
                    midpoint + Vec2::new(7.0, 0.0),
                    effect.end,
                    effect.color.with_alpha(0.7),
                );
            }
        }
    }

    fn draw_coin(ctx: &mut dyn GameContext<Action>, center: Vec2, radius: f32) {
        let gold: Color = Color::from_rgba8(244, 190, 57, 255);
        Self::draw_quad(
            ctx,
            center + Vec2::new(-radius, 0.0),
            center + Vec2::new(0.0, -radius),
            center + Vec2::new(radius, 0.0),
            center + Vec2::new(0.0, radius),
            gold,
        );
        Self::draw_quad(
            ctx,
            center + Vec2::new(-radius * 0.45, 0.0),
            center + Vec2::new(0.0, -radius * 0.45),
            center + Vec2::new(radius * 0.45, 0.0),
            center + Vec2::new(0.0, radius * 0.45),
            Color::from_rgba8(255, 224, 111, 255),
        );
    }

    fn draw_unit_icon(ctx: &mut dyn GameContext<Action>, kind: UnitKind, pos: Vec2, size: Vec2, available: bool) {
        let color: Color = if available {
            kind.color(Faction::Player)
        } else {
            Color::from_rgba8(67, 69, 70, 255)
        };
        Self::draw_prism(
            ctx,
            pos + Vec2::new(size.x * 0.3, 12.0),
            Vec2::new(size.x * 0.4, 35.0),
            Vec2::new(4.0, -4.0),
            color,
        );
        Self::draw_prism(
            ctx,
            pos + Vec2::new(size.x * 0.37, 7.0),
            Vec2::new(size.x * 0.26, 17.0),
            Vec2::new(3.0, -3.0),
            color.lerp(Color::WHITE, 0.1),
        );
        let cost_text: String = format!("{}", kind.stats().cost as u32);
        Self::draw_coin(ctx, pos + Vec2::new(13.0, size.y - 9.0), 6.0);
        Self::draw_text(
            ctx,
            &cost_text,
            pos + Vec2::new(23.0, size.y - 14.0),
            1.35,
            Color::from_rgba8(248, 221, 139, 255),
        );
    }

    pub(super) fn draw_hud(&self, ctx: &mut dyn GameContext<Action>, width: f32, height: f32) {
        let player_panel_pos: Vec2 = Vec2::new(18.0, 16.0);
        let enemy_panel_pos: Vec2 = Vec2::new(width - 268.0, 16.0);
        let panel_size: Vec2 = Vec2::new(250.0, 39.0);
        Self::draw_prism(
            ctx,
            player_panel_pos,
            panel_size,
            Vec2::new(5.0, -4.0),
            Color::from_rgba8(34, 48, 45, 245),
        );
        Self::draw_prism(
            ctx,
            enemy_panel_pos,
            panel_size,
            Vec2::new(5.0, -4.0),
            Color::from_rgba8(55, 40, 38, 245),
        );
        Self::draw_coin(ctx, player_panel_pos + Vec2::new(18.0, 18.0), 10.0);
        Self::draw_coin(ctx, enemy_panel_pos + Vec2::new(18.0, 18.0), 10.0);
        Self::draw_text(
            ctx,
            &format!("MOEDAS {}", self.player_coins.floor() as u32),
            player_panel_pos + Vec2::new(36.0, 11.0),
            2.0,
            Color::from_rgba8(242, 226, 177, 255),
        );
        Self::draw_text(
            ctx,
            &format!("INIMIGO {}", self.enemy_coins.floor() as u32),
            enemy_panel_pos + Vec2::new(36.0, 11.0),
            2.0,
            Color::from_rgba8(242, 203, 166, 255),
        );
        Self::draw_bar(
            ctx,
            player_panel_pos + Vec2::new(3.0, 32.0),
            Vec2::new(panel_size.x - 6.0, 5.0),
            self.player_coins / COIN_METER_RANGE,
            Color::from_rgba8(244, 190, 57, 255),
        );
        Self::draw_bar(
            ctx,
            enemy_panel_pos + Vec2::new(3.0, 32.0),
            Vec2::new(panel_size.x - 6.0, 5.0),
            self.enemy_coins / COIN_METER_RANGE,
            Color::from_rgba8(226, 145, 62, 255),
        );
        Self::draw_bar(
            ctx,
            Vec2::new(18.0, 62.0),
            Vec2::new(160.0, 9.0),
            1.0 - self.spell_cooldown / SPELL_COOLDOWN,
            Color::from_rgba8(239, 226, 87, 255),
        );
        let spell_status: String = if self.spell_cooldown <= 0.0 {
            "MAGIA PRONTA".to_owned()
        } else {
            format!("MAGIA {}S", self.spell_cooldown.ceil() as u32)
        };
        Self::draw_text(
            ctx,
            &spell_status,
            Vec2::new(187.0, 61.0),
            1.35,
            Color::from_rgba8(239, 226, 122, 255),
        );

        if self.state == BattleState::Playing {
            let (skills_pos, skills_size): (Vec2, Vec2) = Self::skills_button_rect(width);
            let hovered: bool = ctx
                .input()
                .mouse_position()
                .is_some_and(|mouse: Vec2| Self::point_in_rect(mouse, skills_pos, skills_size));
            Self::draw_prism(
                ctx,
                skills_pos,
                skills_size,
                Vec2::new(5.0, -4.0),
                if hovered || self.skills_panel_open {
                    Color::from_rgba8(85, 112, 139, 255)
                } else {
                    Color::from_rgba8(55, 76, 96, 245)
                },
            );
            Self::draw_text_centered(
                ctx,
                "HABILIDADES",
                width * 0.5,
                skills_pos.y + 13.0,
                1.45,
                Color::from_rgba8(211, 226, 235, 255),
            );
        }

        ctx.draw_rect(
            Vec2::new(0.0, height - TOOLBAR_HEIGHT),
            Vec2::new(width, TOOLBAR_HEIGHT),
            Color::from_rgba8(28, 33, 34, 255),
        );
        ctx.draw_rect(
            Vec2::new(0.0, height - TOOLBAR_HEIGHT),
            Vec2::new(width, 4.0),
            Color::from_rgba8(154, 121, 61, 255),
        );

        for (index, kind) in UnitKind::ALL.into_iter().enumerate() {
            let (pos, size): (Vec2, Vec2) = Self::button_rect(index, width, height);
            let available: bool = self.player_coins >= kind.stats().cost && self.units.len() < MAX_UNITS;
            Self::draw_prism(
                ctx,
                pos,
                size,
                Vec2::new(5.0, -5.0),
                if available {
                    Color::from_rgba8(55, 64, 61, 255)
                } else {
                    Color::from_rgba8(39, 42, 42, 255)
                },
            );
            Self::draw_unit_icon(ctx, kind, pos, size, available);
        }

        let (upgrade_pos, upgrade_size): (Vec2, Vec2) = Self::button_rect(4, width, height);
        let can_upgrade: bool =
            self.player_castle.level < 3 && self.player_coins >= Self::upgrade_cost(self.player_castle.level);
        Self::draw_prism(
            ctx,
            upgrade_pos,
            upgrade_size,
            Vec2::new(5.0, -5.0),
            if can_upgrade {
                Color::from_rgba8(110, 82, 38, 255)
            } else {
                Color::from_rgba8(47, 43, 37, 255)
            },
        );
        for level in 0..3 {
            Self::draw_prism(
                ctx,
                upgrade_pos + Vec2::new(13.0 + level as f32 * 15.0, 28.0 - level as f32 * 7.0),
                Vec2::new(10.0, 25.0 + level as f32 * 7.0),
                Vec2::new(2.5, -2.5),
                if level < self.player_castle.level {
                    Color::from_rgba8(248, 195, 69, 255)
                } else {
                    Color::from_rgba8(106, 90, 62, 255)
                },
            );
        }
        if self.player_castle.level < 3 {
            let cost_text: String = format!("{}", Self::upgrade_cost(self.player_castle.level) as u32);
            Self::draw_text_centered(
                ctx,
                &cost_text,
                upgrade_pos.x + upgrade_size.x * 0.5,
                upgrade_pos.y + upgrade_size.y - 13.0,
                1.15,
                Color::from_rgba8(252, 218, 117, 255),
            );
        } else {
            Self::draw_text_centered(
                ctx,
                "MAX",
                upgrade_pos.x + upgrade_size.x * 0.5,
                upgrade_pos.y + upgrade_size.y - 13.0,
                1.15,
                Color::from_rgba8(252, 218, 117, 255),
            );
        }
    }

    pub(super) fn draw_skills_panel(&self, ctx: &mut dyn GameContext<Action>, width: f32, height: f32) {
        if !self.skills_panel_open {
            return;
        }

        ctx.draw_rect(Vec2::ZERO, Vec2::new(width, height), Color::from_rgba8(10, 14, 15, 205));
        let (panel_pos, panel_size): (Vec2, Vec2) = Self::skills_panel_rect(width, height);
        Self::draw_prism(
            ctx,
            panel_pos,
            panel_size,
            Vec2::new(11.0, -9.0),
            Color::from_rgba8(37, 49, 56, 255),
        );
        Self::draw_text_centered(
            ctx,
            "HABILIDADES PASSIVAS",
            width * 0.5,
            panel_pos.y + 27.0,
            2.35,
            Color::from_rgba8(228, 203, 111, 255),
        );
        Self::draw_text_centered(
            ctx,
            "COMPRE COM OURO - A BATALHA ESTA PAUSADA",
            width * 0.5,
            panel_pos.y + 63.0,
            1.15,
            Color::from_rgba8(157, 178, 186, 255),
        );

        let (close_pos, close_size): (Vec2, Vec2) = Self::skills_close_button_rect(width, height);
        let mouse: Option<Vec2> = ctx.input().mouse_position();
        for (index, passive) in PassiveKind::ALL.into_iter().enumerate() {
            let (slot_pos, slot_size): (Vec2, Vec2) = Self::passive_slot_rect(index, width, height);
            let unlocked: bool = self.player_passives.contains(passive);
            let affordable: bool = self.player_coins >= passive.cost();
            let hovered: bool = mouse.is_some_and(|mouse| Self::point_in_rect(mouse, slot_pos, slot_size));
            let slot_color: Color = if unlocked {
                Color::from_rgba8(43, 91, 64, 255)
            } else if affordable && hovered {
                Color::from_rgba8(116, 91, 45, 255)
            } else if affordable {
                Color::from_rgba8(76, 66, 43, 255)
            } else {
                Color::from_rgba8(27, 35, 40, 255)
            };
            Self::draw_prism(ctx, slot_pos, slot_size, Vec2::new(5.0, -4.0), slot_color);

            let icon_center: Vec2 = slot_pos + Vec2::new(28.0, 31.0);
            Self::draw_quad(
                ctx,
                icon_center + Vec2::new(-12.0, 0.0),
                icon_center + Vec2::new(0.0, -12.0),
                icon_center + Vec2::new(12.0, 0.0),
                icon_center + Vec2::new(0.0, 12.0),
                passive.unit_kind().color(Faction::Player),
            );
            Self::draw_text(
                ctx,
                passive.title(),
                slot_pos + Vec2::new(50.0, 22.0),
                1.5,
                if unlocked {
                    Color::from_rgba8(172, 240, 177, 255)
                } else {
                    Color::from_rgba8(231, 218, 175, 255)
                },
            );
            let description: [&str; 2] = passive.description();
            Self::draw_text(
                ctx,
                description[0],
                slot_pos + Vec2::new(16.0, slot_size.y * 0.48),
                1.05,
                Color::from_rgba8(166, 187, 194, 255),
            );
            Self::draw_text_centered(
                ctx,
                description[1],
                slot_pos.x + slot_size.x * 0.5,
                slot_pos.y + slot_size.y * 0.62,
                1.05,
                Color::from_rgba8(166, 187, 194, 255),
            );

            let status: String = if unlocked {
                "COMPRADA".to_owned()
            } else {
                format!("OURO {}", passive.cost() as u32)
            };
            Self::draw_text_centered(
                ctx,
                &status,
                slot_pos.x + slot_size.x * 0.5,
                slot_pos.y + slot_size.y - 22.0,
                1.35,
                if unlocked {
                    Color::from_rgba8(119, 231, 142, 255)
                } else if affordable {
                    Color::from_rgba8(250, 205, 83, 255)
                } else {
                    Color::from_rgba8(112, 121, 123, 255)
                },
            );
        }

        let close_hovered: bool = ctx
            .input()
            .mouse_position()
            .is_some_and(|mouse: Vec2| Self::point_in_rect(mouse, close_pos, close_size));
        Self::draw_prism(
            ctx,
            close_pos,
            close_size,
            Vec2::new(6.0, -5.0),
            if close_hovered {
                Color::from_rgba8(157, 77, 66, 255)
            } else {
                Color::from_rgba8(105, 59, 56, 255)
            },
        );
        Self::draw_text_centered(ctx, "FECHAR", width * 0.5, close_pos.y + 14.0, 2.0, Color::WHITE);
    }

    pub(super) fn draw_end_overlay(&self, ctx: &mut dyn GameContext<Action>, width: f32, height: f32) {
        if self.state == BattleState::Playing {
            return;
        }

        let color: Color = if self.state == BattleState::Victory {
            Color::from_rgba8(44, 132, 78, 225)
        } else {
            Color::from_rgba8(145, 55, 48, 225)
        };
        let panel_pos: Vec2 = Vec2::new(width * 0.25, height * 0.27);
        let panel_size: Vec2 = Vec2::new(width * 0.5, height * 0.34);
        ctx.draw_rect(panel_pos, panel_size, color);

        let emblem_center: Vec2 = panel_pos + panel_size * 0.5;
        if self.state == BattleState::Victory {
            ctx.draw_triangle(
                emblem_center + Vec2::new(-48.0, 0.0),
                emblem_center + Vec2::new(-12.0, 38.0),
                emblem_center + Vec2::new(54.0, -44.0),
                Color::WHITE,
            );
        } else {
            ctx.draw_rect(emblem_center + Vec2::new(-48.0, -8.0), Vec2::new(96.0, 16.0), Color::WHITE);
        }
        ctx.draw_rect(
            Vec2::new(emblem_center.x - 54.0, panel_pos.y + panel_size.y - 42.0),
            Vec2::new(108.0, 18.0),
            Color::from_rgba8(35, 39, 38, 255),
        );
    }
}
