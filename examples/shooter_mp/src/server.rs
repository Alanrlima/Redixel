use std::mem::take;

use redixel::prelude::{ClientId, Game, GameContext, NetworkChannel, NetworkEvent, Vec2};

use crate::proto::{
    ARENA_H, ARENA_W, AgentState, BASE_COOLDOWN, BULLET_SIZE, BULLET_SPEED, BulletState, ENTITY_SIZE, Effect,
    EffectKind, PLAYER_SPEED, POWERUP_DURATION, POWERUP_SIZE, PlayerInput, PowerupState, RAPID_FIRE_COOLDOWN, Snapshot,
    V2, Weapon, overlaps, rotate_vec, weapon_color,
};

/// An authoritative agent owned by one connected client. Holds only simulation
/// state; cosmetics live entirely on the client.
struct ServerAgent {
    owner: ClientId,
    pos: Vec2,
    vel: Vec2,
    health: i32,
    shoot_cooldown: f32,
    rapid_fire_timer: f32,
    dash_cooldown: f32,
    dash_timer: f32,
    weapon: Weapon,
    input: PlayerInput,
}

impl ServerAgent {
    /// Spawns a fresh agent for `id` at `(x, y)` with the default pistol loadout.
    fn new(id: ClientId, x: f32, y: f32) -> Self {
        Self {
            owner: id,
            pos: Vec2::new(x, y),
            vel: Vec2::ZERO,
            health: 100,
            shoot_cooldown: 0.0,
            rapid_fire_timer: 0.0,
            dash_cooldown: 0.0,
            dash_timer: 0.0,
            weapon: Weapon::Pistol,
            input: PlayerInput::default(),
        }
    }

    /// Resets the agent to full health and relocates it across the arena.
    fn respawn(&mut self) {
        self.health = 100;
        self.rapid_fire_timer = 0.0;
        self.dash_cooldown = 0.0;
        self.dash_timer = 0.0;
        self.weapon = Weapon::Pistol;
        self.vel = Vec2::ZERO;
        self.pos.x = (self.pos.x + 400.0) % (ARENA_W - ENTITY_SIZE);
        self.pos.y = (self.pos.y + 300.0) % (ARENA_H - ENTITY_SIZE);
    }

    /// The current shot cooldown, shortened while rapid-fire is active.
    fn current_cooldown_max(&self) -> f32 {
        if self.rapid_fire_timer > 0.0 {
            RAPID_FIRE_COOLDOWN
        } else {
            BASE_COOLDOWN
        }
    }
}

/// One authoritative bullet in flight.
struct ServerBullet {
    pos: Vec2,
    vel: Vec2,
    owner: ClientId,
    weapon: Weapon,
    life: f32,
    destroyed: bool,
    size: f32,
}

/// The single roaming weapon pickup.
struct ServerPowerup {
    pos: Vec2,
    active: bool,
    weapon: Weapon,
}

/// The authoritative server: owns the whole world and broadcasts it each tick.
/// Drives everything from `on_fixed_update`; the render callbacks never fire in
/// headless mode.
pub struct Server {
    agents: Vec<ServerAgent>,
    bullets: Vec<ServerBullet>,
    powerup: ServerPowerup,
    time: f32,
    effects: Vec<Effect>,
}

impl Server {
    /// Creates a server with no players and the first powerup ready.
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            bullets: Vec::new(),
            powerup: ServerPowerup {
                active: true,
                weapon: Weapon::Shotgun,
                pos: Vec2::new((ARENA_W - POWERUP_SIZE) / 2.0, 100.0),
            },
            time: 0.0,
            effects: Vec::new(),
        }
    }

    /// Adds an agent for a newly connected client at a deterministic spawn.
    fn add_player(&mut self, id: ClientId) {
        let slot: f32 = (id % 12) as f32;
        let x: f32 = 80.0 + (slot * 173.0) % (ARENA_W - ENTITY_SIZE - 160.0);
        let y: f32 = 80.0 + (slot * 113.0) % (ARENA_H - ENTITY_SIZE - 160.0);
        self.agents.push(ServerAgent::new(id, x, y));
        log::info!("Player {id} joined ({} total).", self.agents.len());
    }

    /// Removes the disconnected client's agent.
    fn remove_player(&mut self, id: ClientId) {
        self.agents.retain(|a: &ServerAgent| a.owner != id);
        log::info!("Player {id} left ({} remain).", self.agents.len());
    }

    /// Spawns the bullets for one shot and returns the recoil impulse magnitude.
    fn spawn_bullets(
        bullets: &mut Vec<ServerBullet>,
        owner: ClientId,
        weapon: Weapon,
        center: Vec2,
        dir_vec: Vec2,
        time: f32,
    ) -> f32 {
        match weapon {
            Weapon::Pistol => {
                bullets.push(ServerBullet {
                    pos: center,
                    vel: dir_vec * BULLET_SPEED,
                    owner,
                    weapon: Weapon::Pistol,
                    life: 3.0,
                    destroyed: false,
                    size: BULLET_SIZE,
                });
                100.0
            }
            Weapon::Shotgun => {
                let angles: [f32; 5] = [-0.3, -0.15, 0.0, 0.15, 0.3];
                for angle in angles {
                    bullets.push(ServerBullet {
                        pos: center,
                        vel: rotate_vec(dir_vec, angle) * BULLET_SPEED,
                        owner,
                        weapon: Weapon::Shotgun,
                        life: 0.6,
                        destroyed: false,
                        size: BULLET_SIZE,
                    });
                }
                400.0
            }
            Weapon::Flamethrower => {
                let offsets: [f32; 6] = [-0.4, -0.2, -0.05, 0.05, 0.2, 0.4];
                let speeds: [f32; 6] = [0.85, 1.0, 1.1, 0.95, 1.05, 0.9];
                let sizes: [f32; 6] = [12.0, 18.0, 24.0, 20.0, 16.0, 14.0];
                let mut i: usize = 0;
                while i < 6 {
                    let wobble: f32 = (time * 25.0 + i as f32).sin() * 0.15;
                    let angle: f32 = offsets[i] + wobble;
                    bullets.push(ServerBullet {
                        pos: center - Vec2::splat(sizes[i] / 2.0),
                        vel: rotate_vec(dir_vec, angle) * (BULLET_SPEED * speeds[i]),
                        owner,
                        weapon: Weapon::Flamethrower,
                        life: 0.9,
                        destroyed: false,
                        size: sizes[i],
                    });
                    i += 1;
                }
                25.0
            }
            Weapon::Homing => {
                bullets.push(ServerBullet {
                    pos: center,
                    vel: dir_vec * (BULLET_SPEED * 0.6),
                    owner,
                    weapon: Weapon::Homing,
                    life: 4.0,
                    destroyed: false,
                    size: BULLET_SIZE,
                });
                150.0
            }
        }
    }

    /// Reads inbound connection and input events into the world.
    fn ingest_events(&mut self, ctx: &mut dyn GameContext<()>) {
        while let Some(event) = ctx.network().poll() {
            match event {
                NetworkEvent::Connected(id) => self.add_player(id),
                NetworkEvent::Disconnected(id) => self.remove_player(id),
                NetworkEvent::Message(id, _channel, payload) => {
                    if let Ok(input) = postcard::from_bytes::<PlayerInput>(payload)
                        && let Some(agent) = self.agents.iter_mut().find(|a: &&mut ServerAgent| a.owner == id)
                    {
                        agent.input = input;
                    }
                }
            }
        }
    }

    /// Respawns the roaming powerup on a fixed cadence, cycling its weapon.
    fn update_powerup(&mut self, dt: f32) {
        if !self.powerup.active && self.time % 10.0 < dt {
            self.powerup.active = true;
            self.powerup.pos = Vec2::new((self.time * 100.0) % ARENA_W, (self.time * 50.0) % ARENA_H);
            let weapon_cycle: usize = ((self.time / 10.0) as usize) % 4;
            self.powerup.weapon = match weapon_cycle {
                0 => Weapon::Shotgun,
                1 => Weapon::Flamethrower,
                2 => Weapon::Homing,
                _ => Weapon::Pistol,
            };
            self.effects.push(Effect {
                kind: EffectKind::PowerupSpawn,
                pos: V2::of(self.powerup.pos + Vec2::splat(POWERUP_SIZE / 2.0)),
                color: (255, 255, 255),
            });
        }
    }

    /// Applies each agent's latest input, integrates motion, fires weapons, and
    /// resolves wall bounces and powerup pickups.
    fn step_agents(&mut self, dt: f32) {
        let agents_len: usize = self.agents.len();
        for i in 0..agents_len {
            let agent: &mut ServerAgent = &mut self.agents[i];

            agent.shoot_cooldown -= dt;
            agent.dash_cooldown -= dt;
            if agent.dash_timer > 0.0 {
                agent.dash_timer -= dt;
            }

            if agent.rapid_fire_timer > 0.0 {
                agent.rapid_fire_timer -= dt;
                if agent.rapid_fire_timer <= 0.0 {
                    agent.weapon = Weapon::Pistol;
                }
            }

            let input: PlayerInput = agent.input;
            let mut dir: Vec2 = input.move_dir.vec();
            if dir.length_sq() > 0.0 {
                dir = dir.normalise();
            }

            let target_vel: Vec2 = dir * PLAYER_SPEED;
            agent.vel = agent.vel.lerp(target_vel, dt * 15.0);

            if input.dash && agent.dash_cooldown <= 0.0 && dir.length_sq() > 0.0 {
                agent.vel += dir * 1500.0;
                agent.dash_cooldown = 1.2;
                agent.dash_timer = 0.4;
            }

            if input.shoot && agent.shoot_cooldown <= 0.0 {
                let aim: Vec2 = input.aim.vec();
                if aim.length_sq() > 0.0 {
                    let dir_vec: Vec2 = aim.normalise();
                    let center: Vec2 = agent.pos + Vec2::splat(ENTITY_SIZE / 2.0);
                    agent.shoot_cooldown = agent.current_cooldown_max();
                    let recoil: f32 =
                        Self::spawn_bullets(&mut self.bullets, agent.owner, agent.weapon, center, dir_vec, self.time);
                    agent.vel -= dir_vec * recoil;
                }
            }

            agent.pos += agent.vel * dt;

            if agent.pos.x < 0.0 {
                agent.pos.x = 0.0;
                agent.vel.x *= -0.5;
            }
            if agent.pos.x > ARENA_W - ENTITY_SIZE {
                agent.pos.x = ARENA_W - ENTITY_SIZE;
                agent.vel.x *= -0.5;
            }
            if agent.pos.y < 0.0 {
                agent.pos.y = 0.0;
                agent.vel.y *= -0.5;
            }
            if agent.pos.y > ARENA_H - ENTITY_SIZE {
                agent.pos.y = ARENA_H - ENTITY_SIZE;
                agent.vel.y *= -0.5;
            }

            if self.powerup.active && overlaps(agent.pos, ENTITY_SIZE, self.powerup.pos, POWERUP_SIZE) {
                agent.weapon = self.powerup.weapon;
                agent.rapid_fire_timer = POWERUP_DURATION;
                self.powerup.active = false;
                let color: (u8, u8, u8) = weapon_color(agent.weapon);
                self.effects.push(Effect {
                    kind: EffectKind::Powerup,
                    pos: V2::of(agent.pos + Vec2::splat(ENTITY_SIZE / 2.0)),
                    color,
                });
            }
        }
    }

    /// Separates overlapping agents with a soft symmetric push.
    fn resolve_agent_overlap(&mut self) {
        let agents_len: usize = self.agents.len();
        let mut pushes: Vec<Vec2> = vec![Vec2::ZERO; agents_len];

        for i in 0..agents_len {
            for j in (i + 1)..agents_len {
                let dir_raw: Vec2 = self.agents[i].pos - self.agents[j].pos;
                let dist_sq: f32 = dir_raw.length_sq();
                let min_dist: f32 = ENTITY_SIZE;
                if dist_sq < min_dist * min_dist && dist_sq > 0.001 {
                    let dist: f32 = dist_sq.sqrt();
                    let push_vec: Vec2 = (dir_raw / dist) * ((min_dist - dist) * 0.5);
                    pushes[i] += push_vec;
                    pushes[j] -= push_vec;
                }
            }
        }

        for (agent, push) in self.agents.iter_mut().zip(pushes.iter()) {
            agent.pos += *push;
            agent.pos.x = agent.pos.x.clamp(0.0, (ARENA_W - ENTITY_SIZE).max(0.0));
            agent.pos.y = agent.pos.y.clamp(0.0, (ARENA_H - ENTITY_SIZE).max(0.0));
        }
    }

    /// Steers homing bullets, advances all bullets, and expires the dead ones.
    fn step_bullets(&mut self, dt: f32) {
        let bullets_len: usize = self.bullets.len();
        for i in 0..bullets_len {
            let bullet: &mut ServerBullet = &mut self.bullets[i];

            if bullet.weapon == Weapon::Homing && !bullet.destroyed {
                let mut closest_dist: f32 = f32::MAX;
                let mut target_pos: Vec2 = Vec2::ZERO;
                let mut found: bool = false;
                for other in self.agents.iter() {
                    if other.owner != bullet.owner {
                        let dist_sq: f32 = (other.pos - bullet.pos).length_sq();
                        if dist_sq < closest_dist {
                            closest_dist = dist_sq;
                            target_pos = other.pos;
                            found = true;
                        }
                    }
                }
                if found && closest_dist < 60000.0 {
                    let target_dir: Vec2 = (target_pos - bullet.pos).normalise();
                    bullet.vel = (bullet.vel + target_dir * 25.0).normalise() * (BULLET_SPEED * 0.6);
                }
            }

            bullet.pos += bullet.vel * dt;
            bullet.life -= dt;

            if bullet.life <= 0.0
                || bullet.pos.x < -bullet.size
                || bullet.pos.x > ARENA_W
                || bullet.pos.y < -bullet.size
                || bullet.pos.y > ARENA_H
            {
                bullet.destroyed = true;
            }
        }
    }

    /// Resolves bullet-vs-agent and bullet-vs-bullet collisions, applying damage,
    /// knockback, respawns, and the matching cosmetic effects.
    fn resolve_collisions(&mut self) {
        let bullets_len: usize = self.bullets.len();

        for i in 0..bullets_len {
            if self.bullets[i].destroyed {
                continue;
            }
            let pos_i: Vec2 = self.bullets[i].pos;
            let size_i: f32 = self.bullets[i].size;
            let owner_i: ClientId = self.bullets[i].owner;
            let vel_i: Vec2 = self.bullets[i].vel;

            let mut hit_index: Option<usize> = None;
            for k in 0..self.agents.len() {
                let agent: &ServerAgent = &self.agents[k];
                if agent.owner != owner_i && overlaps(pos_i, size_i, agent.pos, ENTITY_SIZE) {
                    hit_index = Some(k);
                    break;
                }
            }

            if let Some(k) = hit_index {
                let (hit_pos, dead): (Vec2, bool) = {
                    let agent: &mut ServerAgent = &mut self.agents[k];
                    agent.health -= 25;
                    agent.vel += vel_i.normalise() * 300.0;
                    (agent.pos + Vec2::splat(ENTITY_SIZE / 2.0), agent.health <= 0)
                };

                self.effects.push(Effect {
                    kind: EffectKind::Hit,
                    pos: V2::of(hit_pos),
                    color: (255, 50, 50),
                });

                if dead {
                    self.effects.push(Effect {
                        kind: EffectKind::Death,
                        pos: V2::of(hit_pos),
                        color: (255, 30, 30),
                    });
                    self.agents[k].respawn();
                }

                self.bullets[i].destroyed = true;
            }
        }

        for i in 0..bullets_len {
            if self.bullets[i].destroyed {
                continue;
            }
            let pos_i: Vec2 = self.bullets[i].pos;
            let size_i: f32 = self.bullets[i].size;
            let owner_i: ClientId = self.bullets[i].owner;

            for j in (i + 1)..bullets_len {
                if self.bullets[j].destroyed || owner_i == self.bullets[j].owner {
                    continue;
                }
                if overlaps(pos_i, size_i, self.bullets[j].pos, self.bullets[j].size) {
                    self.bullets[i].destroyed = true;
                    self.bullets[j].destroyed = true;
                    self.effects.push(Effect {
                        kind: EffectKind::BulletImpact,
                        pos: V2::of(pos_i + Vec2::splat(size_i / 2.0)),
                        color: (255, 255, 100),
                    });
                    break;
                }
            }
        }

        self.bullets.retain(|b: &ServerBullet| !b.destroyed);
    }

    /// Builds the broadcast snapshot for `tick`, draining this tick's effects.
    fn build_snapshot(&mut self, tick: u64) -> Snapshot {
        let agents: Vec<AgentState> = self
            .agents
            .iter()
            .map(|a: &ServerAgent| -> AgentState {
                AgentState {
                    owner: a.owner,
                    pos: V2::of(a.pos),
                    health: a.health,
                    weapon: a.weapon,
                    rapid_fire: a.rapid_fire_timer > 0.0,
                    dashing: a.dash_timer > 0.0,
                }
            })
            .collect();

        let bullets: Vec<BulletState> = self
            .bullets
            .iter()
            .map(|b: &ServerBullet| -> BulletState {
                BulletState {
                    pos: V2::of(b.pos),
                    size: b.size,
                    weapon: b.weapon,
                }
            })
            .collect();

        let powerup: Option<PowerupState> = if self.powerup.active {
            Some(PowerupState {
                pos: V2::of(self.powerup.pos),
                weapon: self.powerup.weapon,
            })
        } else {
            None
        };

        Snapshot {
            tick,
            agents,
            bullets,
            powerup,
            effects: take(&mut self.effects),
        }
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for Server {
    type Action = ();

    fn on_start(&mut self, _ctx: &mut dyn GameContext<()>) {
        log::info!("Authoritative shooter server ready. Waiting for players...");
    }

    fn on_fixed_update(&mut self, ctx: &mut dyn GameContext<()>) {
        let dt: f32 = ctx.fixed_delta() as f32;
        self.time += dt;

        self.ingest_events(ctx);
        self.update_powerup(dt);
        self.step_agents(dt);
        self.resolve_agent_overlap();
        self.step_bullets(dt);
        self.resolve_collisions();

        let snapshot: Snapshot = self.build_snapshot(ctx.fixed_tick());
        match postcard::to_stdvec(&snapshot) {
            Ok(bytes) => ctx.network().broadcast(NetworkChannel::UnreliableSequenced, &bytes),
            Err(e) => log::error!("Failed to encode snapshot: {e}"),
        }
    }

    fn on_update(&mut self, _ctx: &mut dyn GameContext<()>) {}

    fn on_render(&mut self, _ctx: &mut dyn GameContext<()>) {}
}
