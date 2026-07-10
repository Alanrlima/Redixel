use std::collections::HashMap;

use redixel::prelude::{
    ClientId, Color, Game, GameContext, KeyCode, MouseButton, NetworkChannel, NetworkEvent, SERVER_ID, Vec2,
};

use crate::{
    effects::{Effects, ParticleProps},
    proto::{
        ARENA_H, ARENA_W, AgentState, BASE_COOLDOWN, ENTITY_SIZE, Effect, EffectBatch, EffectKind, POWERUP_SIZE,
        PlayerInput, RAPID_FIRE_COOLDOWN, Snapshot, V2, Weapon, player_color, weapon_color,
    },
};

/// How often a dashing agent leaves an after-image behind, in seconds.
const AFTERIMAGE_INTERVAL: f32 = 0.05;

/// How often each bullet emits a trail particle, in seconds.
const TRAIL_INTERVAL: f32 = 0.05;

/// The client's input action set, bound to keyboard and mouse in `on_start`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Shoot,
    Dash,
    Exit,
}

/// The render transform from fixed arena space to the (possibly differently
/// sized) window: a uniform scale plus a centering letterbox offset.
fn viewport(w: f32, h: f32) -> (f32, Vec2) {
    let scale: f32 = (w / ARENA_W).min(h / ARENA_H);
    let offset: Vec2 = Vec2::new((w - ARENA_W * scale) * 0.5, (h - ARENA_H * scale) * 0.5);
    (scale, offset)
}

/// Maps a server [`Effect`] to a particle burst, choosing density and spread by
/// kind while honoring the server-baked color.
fn effect_props(effect: &Effect, seed: f32) -> ParticleProps {
    let (count, speed, life, size): (usize, f32, f32, f32) = match effect.kind {
        EffectKind::Hit => (12, 250.0, 0.4, 6.0),
        EffectKind::Death => (40, 400.0, 0.8, 10.0),
        EffectKind::Powerup => (30, 350.0, 0.6, 8.0),
        EffectKind::PowerupSpawn => (20, 150.0, 0.5, 4.0),
        EffectKind::BulletImpact => (6, 150.0, 0.3, 4.0),
    };
    ParticleProps {
        pos: effect.pos.vec(),
        color: effect.color,
        count,
        speed,
        seed,
        life,
        size,
    }
}

/// A render-direct client: it samples input and renders the exact world the
/// server sends, holding only local cosmetic state (particles, shake, timers).
pub struct Client {
    latest: Option<Snapshot>,
    last_tick: Option<u64>,
    fx: Effects,
    time: f32,
    local: Option<ClientId>,
    prev_dashing: HashMap<ClientId, bool>,
    prev_health: HashMap<ClientId, i32>,
    local_shoot_cooldown: f32,
    afterimage_clock: f32,
    trail_clock: f32,
}

impl Client {
    /// Creates a fresh, unconnected client.
    pub fn new() -> Self {
        Self {
            latest: None,
            last_tick: None,
            fx: Effects::new(),
            time: 0.0,
            local: None,
            prev_dashing: HashMap::new(),
            prev_health: HashMap::new(),
            local_shoot_cooldown: 0.0,
            afterimage_clock: 0.0,
            trail_clock: 0.0,
        }
    }

    /// Returns a copy of the local player's agent state from the latest snapshot.
    fn local_agent(&self) -> Option<AgentState> {
        let id: ClientId = self.local?;
        self.latest
            .as_ref()?
            .agents
            .iter()
            .find(|a: &&AgentState| a.owner == id)
            .copied()
    }

    /// The shoot recoil magnitude for `weapon`, matching the server's impulse scale.
    fn recoil_for(weapon: Weapon) -> f32 {
        match weapon {
            Weapon::Pistol => 100.0,
            Weapon::Shotgun => 400.0,
            Weapon::Flamethrower => 25.0,
            Weapon::Homing => 150.0,
        }
    }

    /// Drains inbound events, learning the local id and dispatching each message
    /// by the channel it arrived on: continuous world state unreliably, one-shot
    /// cosmetics reliably.
    fn receive(&mut self, ctx: &mut dyn GameContext<Action>) {
        if self.local.is_none()
            && let Some(id) = ctx.network().local_client()
        {
            self.local = Some(id);
            log::info!("Connected as client {id}.");
        }

        while let Some(event) = ctx.network().poll() {
            match event {
                NetworkEvent::Connected(..) => {}
                NetworkEvent::Disconnected(..) => {
                    log::warn!("Disconnected from server.");
                }
                NetworkEvent::Message(_from, NetworkChannel::UnreliableSequenced, payload) => {
                    if let Ok(snapshot) = postcard::from_bytes::<Snapshot>(payload) {
                        self.apply_snapshot(snapshot);
                    }
                }
                NetworkEvent::Message(_from, NetworkChannel::ReliableOrdered, payload) => {
                    if let Ok(batch) = postcard::from_bytes::<EffectBatch>(payload) {
                        self.play_effects(&batch.effects);
                    }
                }
            }
        }
    }

    /// Spawns a particle burst for each one-shot cosmetic the server sent.
    fn play_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
            let props: ParticleProps = effect_props(effect, self.time);
            self.fx.spawn_burst(props);
        }
    }

    /// Triggers cosmetics from a snapshot (dash trails/bursts, hit shake, baked
    /// effects), then stores it as the world to render.
    ///
    /// Snapshots ride an unreliable channel, so one can arrive out of order or
    /// be replayed. Applying a stale one would snap the world backwards, so
    /// anything not strictly newer than the last applied tick is dropped.
    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        if let Some(last) = self.last_tick
            && snapshot.tick <= last
        {
            return;
        }
        self.last_tick = Some(snapshot.tick);

        for agent in snapshot.agents.iter() {
            let color: (u8, u8, u8) = if agent.rapid_fire {
                (255, 200, 50)
            } else {
                player_color(agent.owner)
            };
            let center: Vec2 = agent.pos.vec() + Vec2::splat(ENTITY_SIZE / 2.0);
            let was_dashing: bool = self.prev_dashing.get(&agent.owner).copied().unwrap_or(false);

            if !was_dashing && agent.dashing {
                self.fx.spawn_burst(ParticleProps {
                    pos: center,
                    color,
                    count: 15,
                    speed: 250.0,
                    seed: self.time,
                    life: 0.4,
                    size: 6.0,
                });
                if Some(agent.owner) == self.local {
                    self.fx.add_shake(5.0);
                }
            }
            self.prev_dashing.insert(agent.owner, agent.dashing);

            let prev_hp: i32 = self.prev_health.get(&agent.owner).copied().unwrap_or(agent.health);
            if agent.health < prev_hp && Some(agent.owner) == self.local {
                self.fx.add_shake(12.0);
            }
            self.prev_health.insert(agent.owner, agent.health);
        }

        self.latest = Some(snapshot);
    }

    /// The screen-space center of the local player's body, if known.
    fn own_center(&self) -> Option<Vec2> {
        let local: ClientId = self.local?;
        let snapshot: &Snapshot = self.latest.as_ref()?;
        snapshot
            .agents
            .iter()
            .find(|a: &&AgentState| a.owner == local)
            .map(|a: &AgentState| -> Vec2 { a.pos.vec() + Vec2::splat(ENTITY_SIZE / 2.0) })
    }

    /// The unit aim direction from the local player to the cursor in arena space.
    fn aim_direction(&self, ctx: &dyn GameContext<Action>, scale: f32, offset: Vec2) -> Vec2 {
        let mouse: Vec2 = match ctx.input().mouse_position() {
            Some(p) => p,
            None => return Vec2::ZERO,
        };
        let center: Vec2 = match self.own_center() {
            Some(c) => c,
            None => return Vec2::ZERO,
        };
        if scale <= 0.0 {
            return Vec2::ZERO;
        }
        let raw: Vec2 = (mouse - offset) / scale - center;
        if raw.length_sq() > 0.0 {
            raw.normalise()
        } else {
            Vec2::ZERO
        }
    }

    /// Samples movement/aim/shoot/dash and sends one `PlayerInput` to the server.
    fn send_input(&mut self, ctx: &mut dyn GameContext<Action>) {
        if !ctx.network().is_connected() {
            return;
        }

        let (scale, offset): (f32, Vec2) = viewport(ctx.surface_width() as f32, ctx.surface_height() as f32);

        let mut move_dir: Vec2 = Vec2::ZERO;
        if ctx.input().held(Action::Left) {
            move_dir -= Vec2::X;
        }
        if ctx.input().held(Action::Right) {
            move_dir += Vec2::X;
        }
        if ctx.input().held(Action::Up) {
            move_dir -= Vec2::Y;
        }
        if ctx.input().held(Action::Down) {
            move_dir += Vec2::Y;
        }

        let shooting: bool = ctx.input().held(Action::Shoot);
        let input: PlayerInput = PlayerInput {
            move_dir: V2::of(move_dir),
            aim: V2::of(self.aim_direction(ctx, scale, offset)),
            shoot: shooting,
            dash: ctx.input().held(Action::Dash),
        };

        if shooting
            && self.local_shoot_cooldown <= 0.0
            && let Some(agent) = self.local_agent()
        {
            let cooldown: f32 = if agent.rapid_fire {
                RAPID_FIRE_COOLDOWN
            } else {
                BASE_COOLDOWN
            };
            self.local_shoot_cooldown = cooldown;
            self.fx.add_shake(Self::recoil_for(agent.weapon) * 0.015);
        }

        match postcard::to_stdvec(&input) {
            Ok(bytes) => ctx
                .network()
                .send(SERVER_ID, NetworkChannel::UnreliableSequenced, &bytes),
            Err(e) => log::error!("Failed to encode input: {e}"),
        }
    }

    /// Draws the arena grid in arena space through `to_screen`, with a fractional
    /// `parallax` offset that makes the grid drift as the local player moves.
    fn draw_grid(
        &self,
        ctx: &mut dyn GameContext<Action>,
        to_screen: impl Fn(Vec2) -> Vec2,
        scale: f32,
        parallax: Vec2,
    ) {
        let grid_color: Color = Color::rgb(0.13, 0.13, 0.16);

        let mut gx: f32 = parallax.x.rem_euclid(60.0);
        while gx <= ARENA_W {
            ctx.draw_rect(to_screen(Vec2::new(gx, 0.0)), Vec2::new(scale, ARENA_H * scale), grid_color);
            gx += 60.0;
        }

        let mut gy: f32 = parallax.y.rem_euclid(60.0);
        while gy <= ARENA_H {
            ctx.draw_rect(to_screen(Vec2::new(0.0, gy)), Vec2::new(ARENA_W * scale, scale), grid_color);
            gy += 60.0;
        }
    }

    /// Draws every agent: colored body, gold rapid-fire tint, core pulse, hp bar.
    fn draw_agents(
        &self,
        ctx: &mut dyn GameContext<Action>,
        snapshot: &Snapshot,
        to_screen: impl Fn(Vec2) -> Vec2,
        scale: f32,
    ) {
        for agent in snapshot.agents.iter() {
            let (r, g, b): (u8, u8, u8) = player_color(agent.owner);
            let body: Color = if agent.rapid_fire {
                Color::from_rgba8(255, 200, 50, 255)
            } else {
                Color::from_rgba8(r, g, b, 255)
            };
            let pos: Vec2 = agent.pos.vec();

            ctx.draw_rect(
                to_screen(pos + Vec2::splat(4.0)),
                Vec2::splat(ENTITY_SIZE * scale),
                Color::from_rgba8(0, 0, 0, 150),
            );
            ctx.draw_rect(to_screen(pos), Vec2::splat(ENTITY_SIZE * scale), body);

            let inner_size: f32 = ENTITY_SIZE * 0.4;
            let inner_offset: Vec2 = Vec2::splat((ENTITY_SIZE - inner_size) / 2.0);
            let pulse_core: f32 = (self.time * 3.0 + agent.owner as f32).sin().abs() * 0.5 + 0.5;
            ctx.draw_rect(
                to_screen(pos + inner_offset),
                Vec2::splat(inner_size * scale),
                Color::from_rgba8(255, 255, 255, (pulse_core * 200.0) as u8),
            );

            let hp_percent: f32 = (agent.health.max(0) as f32) / 100.0;
            let hp_pos: Vec2 = pos - Vec2::new(0.0, 10.0);
            ctx.draw_rect(
                to_screen(hp_pos),
                Vec2::new(ENTITY_SIZE * scale, 4.0 * scale),
                Color::rgb(0.7, 0.1, 0.1),
            );
            ctx.draw_rect(
                to_screen(hp_pos),
                Vec2::new(ENTITY_SIZE * hp_percent * scale, 4.0 * scale),
                Color::rgb(0.1, 0.8, 0.2),
            );
        }
    }

    /// Draws every bullet with its weapon color and a soft glow.
    fn draw_bullets(
        &self,
        ctx: &mut dyn GameContext<Action>,
        snapshot: &Snapshot,
        to_screen: impl Fn(Vec2) -> Vec2,
        scale: f32,
    ) {
        for bullet in snapshot.bullets.iter() {
            let (r, g, b): (u8, u8, u8) = weapon_color(bullet.weapon);
            let core: Color = Color::from_rgba8(r, g, b, 255);
            let glow: Color = Color::from_rgba8(r, g, b, 50);
            let pos: Vec2 = bullet.pos.vec();

            ctx.draw_rect(
                to_screen(pos + Vec2::splat(3.0)),
                Vec2::splat(bullet.size * scale),
                Color::from_rgba8(0, 0, 0, 100),
            );
            ctx.draw_rect(
                to_screen(pos - Vec2::splat(4.0)),
                Vec2::splat((bullet.size + 8.0) * scale),
                glow,
            );
            ctx.draw_rect(to_screen(pos), Vec2::splat(bullet.size * scale), core);
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for Client {
    type Action = Action;

    fn on_start(&mut self, ctx: &mut dyn GameContext<Action>) {
        ctx.input_mut().bind(Action::Up, KeyCode::KeyW.into());
        ctx.input_mut().bind(Action::Down, KeyCode::KeyS.into());
        ctx.input_mut().bind(Action::Left, KeyCode::KeyA.into());
        ctx.input_mut().bind(Action::Right, KeyCode::KeyD.into());
        ctx.input_mut().bind(Action::Dash, KeyCode::Space.into());
        ctx.input_mut().bind(Action::Exit, KeyCode::Escape.into());
        ctx.input_mut().bind(Action::Shoot, MouseButton::Left.into());
        log::info!("Client started. Connecting to server...");
    }

    fn on_fixed_update(&mut self, ctx: &mut dyn GameContext<Action>) {
        let dt: f32 = ctx.fixed_delta() as f32;
        self.local_shoot_cooldown -= dt;
        self.afterimage_clock -= dt;

        self.receive(ctx);

        if self.afterimage_clock <= 0.0 {
            self.afterimage_clock = AFTERIMAGE_INTERVAL;

            if let Some(snapshot) = self.latest.as_ref() {
                for agent in snapshot.agents.iter() {
                    if agent.dashing {
                        let color: (u8, u8, u8) = if agent.rapid_fire {
                            (255, 200, 50)
                        } else {
                            player_color(agent.owner)
                        };
                        self.fx.spawn_afterimage(agent.pos.vec(), ENTITY_SIZE, color);
                    }
                }
            }
        }

        self.send_input(ctx);
    }

    fn on_update(&mut self, ctx: &mut dyn GameContext<Action>) {
        let dt: f32 = ctx.delta_time() as f32;
        self.time = (self.time + dt) % 3600.0;
        self.trail_clock -= dt;
        self.fx.update(dt);

        if self.trail_clock <= 0.0 {
            self.trail_clock = TRAIL_INTERVAL;

            if let Some(snapshot) = self.latest.as_ref() {
                for bullet in snapshot.bullets.iter() {
                    let (r, g, b_c): (u8, u8, u8) = weapon_color(bullet.weapon);
                    self.fx.spawn_burst(ParticleProps {
                        pos: bullet.pos.vec() + Vec2::splat(bullet.size / 2.0),
                        color: (r, g, b_c),
                        count: 1,
                        speed: 20.0,
                        seed: self.time + bullet.pos.x,
                        life: 0.2,
                        size: bullet.size * 0.8,
                    });
                }
            }
        }

        if ctx.input().held(Action::Exit) {
            ctx.exit();
        }
    }

    fn on_render(&mut self, ctx: &mut dyn GameContext<Action>) {
        let w: f32 = ctx.surface_width() as f32;
        let h: f32 = ctx.surface_height() as f32;
        let (scale, offset): (f32, Vec2) = viewport(w, h);
        let shake: Vec2 = self.fx.shake_offset(self.time);
        let to_screen = move |p: Vec2| -> Vec2 { offset + (p + shake) * scale };

        let parallax: Vec2 = self
            .local_agent()
            .map(|a: AgentState| -> Vec2 {
                let p: Vec2 = a.pos.vec();
                Vec2::new(p.x * -0.05, p.y * -0.05)
            })
            .unwrap_or(Vec2::ZERO);

        ctx.clear_color(Color::rgb(0.08, 0.08, 0.11));
        ctx.draw_rect(
            to_screen(Vec2::ZERO),
            Vec2::new(ARENA_W, ARENA_H) * scale,
            Color::rgb(0.10, 0.10, 0.13),
        );
        self.draw_grid(ctx, to_screen, scale, parallax);

        let snapshot: &Snapshot = match self.latest.as_ref() {
            Some(s) => s,
            None => {
                let pulse: f32 = (self.time * 3.0).sin().abs() * 0.5 + 0.5;
                let size: f32 = ENTITY_SIZE * 2.0;
                let center: Vec2 = Vec2::new(ARENA_W / 2.0, ARENA_H / 2.0) - Vec2::splat(size / 2.0);
                ctx.draw_rect(
                    to_screen(center),
                    Vec2::splat(size * scale),
                    Color::from_rgba8(80, 80, 120, (pulse * 200.0) as u8),
                );
                return;
            }
        };

        self.fx.draw(ctx, to_screen, scale);

        if let Some(powerup) = snapshot.powerup {
            let pulse: f32 = (self.time * 5.0).sin().abs();
            let p_size: f32 = POWERUP_SIZE + pulse * 6.0;
            let p_draw_pos: Vec2 = powerup.pos.vec() - Vec2::splat((p_size - POWERUP_SIZE) / 2.0);
            let (r, g, b): (u8, u8, u8) = weapon_color(powerup.weapon);
            let color: Color = Color::from_rgba8(r, g, b, (150.0 + pulse * 100.0) as u8);

            ctx.draw_rect(
                to_screen(p_draw_pos + Vec2::splat(4.0)),
                Vec2::splat(p_size * scale),
                Color::from_rgba8(0, 0, 0, 150),
            );
            ctx.draw_rect(to_screen(p_draw_pos), Vec2::splat(p_size * scale), color);
        }

        self.draw_agents(ctx, snapshot, to_screen, scale);
        self.draw_bullets(ctx, snapshot, to_screen, scale);

        if let Some(mouse) = ctx.input().mouse_position() {
            let cross: Vec2 = mouse + shake * (scale * 0.5);
            ctx.draw_rect(cross - Vec2::new(2.0, 12.0), Vec2::new(4.0, 24.0), Color::WHITE);
            ctx.draw_rect(cross - Vec2::new(12.0, 2.0), Vec2::new(24.0, 4.0), Color::WHITE);
        }
    }
}
