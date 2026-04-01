#![no_std]
#![no_main]

extern crate alloc;
extern crate user_lib;

use alloc::{vec, vec::Vec};
use core::cmp::{max, min};
use user_lib::{
    FramebufferInfo, INPUT_EVENT_KEY, INPUT_VALUE_RELEASE, OpenFlags, close, exec, exit,
    framebuffer_get_info, framebuffer_present, get_time, input_poll, key, open, read, sched_yield,
};

const INTERNAL_W: usize = 160;
const INTERNAL_H: usize = 100;
const FIX_SHIFT: i32 = 12;
const FIX_ONE: i32 = 1 << FIX_SHIFT;
const HALF_FIX: i32 = FIX_ONE / 2;
const MOVE_SPEED: i32 = FIX_ONE / 7;
const STRAFE_SPEED: i32 = FIX_ONE / 8;
const ENEMY_SPEED: i32 = FIX_ONE / 18;
const TURN_COS: i32 = 4074;
const TURN_SIN: i32 = 428;
const FIRE_COOLDOWN_FRAMES: i32 = 7;
const FRAME_MS: isize = 33;
const MAX_INPUT_CODE: usize = 128;
const TEX_SIZE: usize = 16;
const MAX_FRAME_PIXELS: usize = INTERNAL_W * INTERNAL_H;

const COLOR_BLACK: u32 = 0xff00_0000;
const COLOR_WHITE: u32 = 0xffff_ffff;
const COLOR_SKY: u32 = 0xff16_1c_30;
const COLOR_FLOOR: u32 = 0xff28_1811;
const COLOR_PANEL: u32 = 0xff1a_0c_08;
const COLOR_HUD: u32 = 0xffd5_ba_7a;
const COLOR_BLOOD: u32 = 0xffaa_2a_2a;
const COLOR_AMMO: u32 = 0xfff0_c2_4d;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Title,
    Playing,
    Dead,
    Victory,
}

#[derive(Clone, Copy)]
struct Enemy {
    x: i32,
    y: i32,
    hp: i32,
    cooldown: i32,
}

impl Enemy {
    fn alive(&self) -> bool {
        self.hp > 0
    }
}

struct Level {
    width: usize,
    height: usize,
    tiles: Vec<u8>,
    start_x: i32,
    start_y: i32,
    enemies: Vec<Enemy>,
}

impl Level {
    fn tile(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return 1;
        }
        self.tiles[y as usize * self.width + x as usize]
    }

    fn is_wall_fixed(&self, x: i32, y: i32) -> bool {
        self.tile(x >> FIX_SHIFT, y >> FIX_SHIFT) != 0
    }
}

#[derive(Clone, Copy)]
struct Player {
    x: i32,
    y: i32,
    dir_x: i32,
    dir_y: i32,
    plane_x: i32,
    plane_y: i32,
    hp: i32,
    ammo: i32,
    flash: i32,
    fire_cooldown: i32,
}

impl Player {
    fn new(level: &Level) -> Self {
        Self {
            x: level.start_x,
            y: level.start_y,
            dir_x: FIX_ONE,
            dir_y: 0,
            plane_x: 0,
            plane_y: (FIX_ONE * 2) / 3,
            hp: 100,
            ammo: 40,
            flash: 0,
            fire_cooldown: 0,
        }
    }
}

struct Frame {
    pixels: Vec<u32>,
}

impl Frame {
    fn new() -> Self {
        Self {
            pixels: vec![0; MAX_FRAME_PIXELS],
        }
    }

    fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    fn set(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x as usize >= INTERNAL_W || y as usize >= INTERNAL_H {
            return;
        }
        self.pixels[y as usize * INTERNAL_W + x as usize] = color;
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let x0 = max(0, x);
        let y0 = max(0, y);
        let x1 = min(INTERNAL_W as i32, x + w);
        let y1 = min(INTERNAL_H as i32, y + h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for yy in y0..y1 {
            let row = yy as usize * INTERNAL_W;
            for xx in x0..x1 {
                self.pixels[row + xx as usize] = color;
            }
        }
    }

    fn draw_vline(&mut self, x: i32, y0: i32, y1: i32, color: u32) {
        let x = x.clamp(0, INTERNAL_W as i32 - 1);
        let start = y0.clamp(0, INTERNAL_H as i32 - 1);
        let end = y1.clamp(0, INTERNAL_H as i32 - 1);
        if start > end {
            return;
        }
        for y in start..=end {
            self.set(x, y, color);
        }
    }

    fn draw_text(&mut self, mut x: i32, y: i32, text: &str, scale: i32, color: u32) {
        for ch in text.bytes() {
            if ch == b' ' {
                x += 4 * scale;
                continue;
            }
            draw_glyph(self, x, y, ch, scale, color);
            x += 4 * scale;
        }
    }
}

struct Game {
    level: Level,
    frame: Frame,
    mode: Mode,
    player: Player,
    enemies: Vec<Enemy>,
    depth: [i32; INTERNAL_W],
    keys: [bool; MAX_INPUT_CODE],
    prev_fire: bool,
    prev_enter: bool,
    prev_escape: bool,
    frame_counter: usize,
    kills: usize,
}

impl Game {
    fn new(level: Level) -> Self {
        let player = Player::new(&level);
        let enemies = level.enemies.clone();
        Self {
            level,
            frame: Frame::new(),
            mode: Mode::Playing,
            player,
            enemies,
            depth: [0; INTERNAL_W],
            keys: [false; MAX_INPUT_CODE],
            prev_fire: false,
            prev_enter: false,
            prev_escape: false,
            frame_counter: 0,
            kills: 0,
        }
    }

    fn reset_run(&mut self) {
        self.player = Player::new(&self.level);
        self.enemies.clone_from(&self.level.enemies);
        self.kills = 0;
        self.frame_counter = 0;
        self.mode = Mode::Playing;
    }

    fn poll_input(&mut self) {
        let mut event = user_lib::InputEvent::default();
        loop {
            let ret = input_poll(&mut event);
            if ret <= 0 {
                break;
            }
            if event.event_type == INPUT_EVENT_KEY {
                let code = event.code as usize;
                if code < self.keys.len() {
                    self.keys[code] = event.value != INPUT_VALUE_RELEASE;
                }
            }
        }
    }

    fn key(&self, code: u16) -> bool {
        self.keys[code as usize]
    }

    fn update(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);
        let fire_now = self.key(key::SPACE);
        let enter_now = self.key(key::ENTER);
        let esc_now = self.key(key::ESC);
        let fire_pressed = just_pressed(fire_now, &mut self.prev_fire);
        let enter_pressed = just_pressed(enter_now, &mut self.prev_enter);
        let esc_pressed = just_pressed(esc_now, &mut self.prev_escape);

        match self.mode {
            Mode::Title => {
                if enter_pressed || fire_pressed || self.frame_counter > 45 {
                    self.reset_run();
                }
            }
            Mode::Dead | Mode::Victory => {
                if enter_pressed || fire_pressed {
                    self.reset_run();
                }
            }
            Mode::Playing => {
                if esc_pressed {
                    self.mode = Mode::Title;
                    return;
                }
                if self.player.fire_cooldown > 0 {
                    self.player.fire_cooldown -= 1;
                }
                if self.player.flash > 0 {
                    self.player.flash -= 1;
                }
                self.update_player(fire_pressed);
                self.update_enemies();
                if self.player.hp <= 0 {
                    self.mode = Mode::Dead;
                } else if self.kills == self.enemies.len() {
                    self.mode = Mode::Victory;
                }
            }
        }
    }

    fn update_player(&mut self, fire_pressed: bool) {
        let turn_left = self.key(key::LEFT) || self.key(key::A);
        let turn_right = self.key(key::RIGHT) || self.key(key::D);
        let forward = self.key(key::W) || self.key(key::UP);
        let backward = self.key(key::S) || self.key(key::DOWN);
        let strafe_left = self.key(key::Q);
        let strafe_right = self.key(key::E);
        let auto_demo =
            !turn_left && !turn_right && !forward && !backward && !strafe_left && !strafe_right && !fire_pressed;

        if turn_left {
            rotate_player(&mut self.player, TURN_COS, -TURN_SIN);
        }
        if turn_right {
            rotate_player(&mut self.player, TURN_COS, TURN_SIN);
        }
        if auto_demo && self.frame_counter % 4 == 0 {
            rotate_player(&mut self.player, TURN_COS, TURN_SIN);
        }

        let mut move_x = 0;
        let mut move_y = 0;
        if forward {
            move_x += mul_fixed(self.player.dir_x, MOVE_SPEED);
            move_y += mul_fixed(self.player.dir_y, MOVE_SPEED);
        }
        if backward {
            move_x -= mul_fixed(self.player.dir_x, MOVE_SPEED);
            move_y -= mul_fixed(self.player.dir_y, MOVE_SPEED);
        }
        if strafe_left {
            move_x += mul_fixed(-self.player.dir_y, STRAFE_SPEED);
            move_y += mul_fixed(self.player.dir_x, STRAFE_SPEED);
        }
        if strafe_right {
            move_x -= mul_fixed(-self.player.dir_y, STRAFE_SPEED);
            move_y -= mul_fixed(self.player.dir_x, STRAFE_SPEED);
        }
        if auto_demo {
            move_x += mul_fixed(self.player.dir_x, MOVE_SPEED / 2);
            move_y += mul_fixed(self.player.dir_y, MOVE_SPEED / 2);
        }
        try_move(
            &self.level,
            &mut self.player.x,
            &mut self.player.y,
            move_x,
            move_y,
        );

        if fire_pressed && self.player.fire_cooldown == 0 && self.player.ammo > 0 {
            self.player.ammo -= 1;
            self.player.fire_cooldown = FIRE_COOLDOWN_FRAMES;
            self.player.flash = 3;
            self.fire_hitscan();
        }
    }

    fn fire_hitscan(&mut self) {
        let mut best_idx = None;
        let mut best_score = i32::MAX;
        for (idx, enemy) in self.enemies.iter().enumerate() {
            if !enemy.alive() {
                continue;
            }
            let rel_x = enemy.x - self.player.x;
            let rel_y = enemy.y - self.player.y;
            let forward = mul_fixed(rel_x, self.player.dir_x) + mul_fixed(rel_y, self.player.dir_y);
            if forward <= FIX_ONE / 2 {
                continue;
            }
            let side = mul_fixed(rel_x, -self.player.dir_y) + mul_fixed(rel_y, self.player.dir_x);
            if side.abs() * 4 > forward {
                continue;
            }
            if wall_between(
                &self.level,
                self.player.x,
                self.player.y,
                enemy.x,
                enemy.y,
            ) {
                continue;
            }
            let score = side.abs() + (forward >> 1);
            if score < best_score {
                best_score = score;
                best_idx = Some(idx);
            }
        }
        if let Some(idx) = best_idx {
            let enemy = &mut self.enemies[idx];
            enemy.hp -= 1;
            if enemy.hp <= 0 {
                self.kills += 1;
                self.player.ammo = min(99, self.player.ammo + 4);
                self.player.hp = min(100, self.player.hp + 5);
            }
        }
    }

    fn update_enemies(&mut self) {
        for idx in 0..self.enemies.len() {
            if !self.enemies[idx].alive() {
                continue;
            }
            if self.enemies[idx].cooldown > 0 {
                self.enemies[idx].cooldown -= 1;
            }
            let dx = self.player.x - self.enemies[idx].x;
            let dy = self.player.y - self.enemies[idx].y;
            let dist2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
            if dist2 < ((FIX_ONE * 3 / 4) as i64) * ((FIX_ONE * 3 / 4) as i64) {
                if self.enemies[idx].cooldown == 0 {
                    self.player.hp -= 8;
                    self.enemies[idx].cooldown = 16;
                }
                continue;
            }
            if dist2 > ((FIX_ONE * 9) as i64) * ((FIX_ONE * 9) as i64) {
                continue;
            }
            let step_x = clamp_signed(dx, ENEMY_SPEED);
            let step_y = clamp_signed(dy, ENEMY_SPEED);
            let mut x = self.enemies[idx].x;
            let mut y = self.enemies[idx].y;
            try_move(&self.level, &mut x, &mut y, step_x, step_y);
            self.enemies[idx].x = x;
            self.enemies[idx].y = y;
        }
    }

    fn render(&mut self) {
        match self.mode {
            Mode::Title => self.render_title(),
            Mode::Playing => self.render_world(),
            Mode::Dead => self.render_end(false),
            Mode::Victory => self.render_end(true),
        }
    }

    fn render_title(&mut self) {
        self.frame.clear(COLOR_BLACK);
        for y in 0..INTERNAL_H as i32 {
            let shade = 40 + (y * 100 / INTERNAL_H as i32) as u8;
            self.frame.fill_rect(
                0,
                y,
                INTERNAL_W as i32,
                1,
                shade_color(0xff601010, shade),
            );
        }
        draw_doom_logo(&mut self.frame, 20, 16, 6);
        self.frame
            .draw_text(24, 60, "RAYCASTER", 2, shade_color(COLOR_WHITE, 210));
        self.frame
            .draw_text(14, 72, "ENTER OR SPACE", 2, COLOR_HUD);
        self.frame.draw_text(26, 82, "TO START", 2, COLOR_HUD);
    }

    fn render_end(&mut self, win: bool) {
        self.render_world();
        self.frame.fill_rect(10, 24, 140, 48, shade_color(COLOR_BLACK, 220));
        if win {
            self.frame.draw_text(30, 34, "VICTORY", 3, COLOR_AMMO);
            self.frame.draw_text(22, 52, "ALL DEMONS", 2, COLOR_WHITE);
            self.frame.draw_text(32, 60, "DOWN", 2, COLOR_WHITE);
        } else {
            self.frame.draw_text(28, 36, "YOU DIED", 3, COLOR_BLOOD);
        }
        self.frame.draw_text(18, 76, "PRESS ENTER", 2, COLOR_HUD);
    }

    fn render_world(&mut self) {
        self.frame.fill_rect(0, 0, INTERNAL_W as i32, INTERNAL_H as i32 / 2, COLOR_SKY);
        self.frame.fill_rect(
            0,
            INTERNAL_H as i32 / 2,
            INTERNAL_W as i32,
            INTERNAL_H as i32 / 2,
            COLOR_FLOOR,
        );

        for x in 0..INTERNAL_W {
            let camera_x = ((2 * x as i32) << FIX_SHIFT) / INTERNAL_W as i32 - FIX_ONE;
            let ray_dir_x = self.player.dir_x + mul_fixed(self.player.plane_x, camera_x);
            let ray_dir_y = self.player.dir_y + mul_fixed(self.player.plane_y, camera_x);
            let mut map_x = self.player.x >> FIX_SHIFT;
            let mut map_y = self.player.y >> FIX_SHIFT;

            let delta_x = inv_abs(ray_dir_x);
            let delta_y = inv_abs(ray_dir_y);
            let (step_x, mut side_x) = if ray_dir_x < 0 {
                (
                    -1,
                    mul_fixed(self.player.x - (map_x << FIX_SHIFT), delta_x),
                )
            } else {
                (
                    1,
                    mul_fixed(((map_x + 1) << FIX_SHIFT) - self.player.x, delta_x),
                )
            };
            let (step_y, mut side_y) = if ray_dir_y < 0 {
                (
                    -1,
                    mul_fixed(self.player.y - (map_y << FIX_SHIFT), delta_y),
                )
            } else {
                (
                    1,
                    mul_fixed(((map_y + 1) << FIX_SHIFT) - self.player.y, delta_y),
                )
            };

            let (wall, side) = loop {
                if side_x < side_y {
                    side_x += delta_x;
                    map_x += step_x;
                    let tile = self.level.tile(map_x, map_y);
                    if tile != 0 {
                        break (tile, 0);
                    }
                } else {
                    side_y += delta_y;
                    map_y += step_y;
                    let tile = self.level.tile(map_x, map_y);
                    if tile != 0 {
                        break (tile, 1);
                    }
                }
            };

            let dist = max(
                1,
                if side == 0 { side_x - delta_x } else { side_y - delta_y },
            );
            self.depth[x] = dist;
            let line_h = max(1, ((INTERNAL_H as i32 * FIX_ONE) / dist) as i32);
            let draw_start = max(0, INTERNAL_H as i32 / 2 - line_h / 2);
            let draw_end = min(INTERNAL_H as i32 - 1, INTERNAL_H as i32 / 2 + line_h / 2);
            let hit = if side == 0 {
                self.player.y + mul_fixed(dist, ray_dir_y)
            } else {
                self.player.x + mul_fixed(dist, ray_dir_x)
            };
            let mut tex_x = (((hit & (FIX_ONE - 1)) * TEX_SIZE as i32) >> FIX_SHIFT) as usize;
            if side == 0 && ray_dir_x > 0 {
                tex_x = TEX_SIZE - tex_x - 1;
            }
            if side == 1 && ray_dir_y < 0 {
                tex_x = TEX_SIZE - tex_x - 1;
            }
            for y in draw_start..=draw_end {
                let tex_y = (((y - draw_start) * TEX_SIZE as i32) / max(1, draw_end - draw_start + 1))
                    as usize;
                let mut color = wall_texture(wall, tex_x, tex_y);
                if side == 1 {
                    color = shade_color(color, 170);
                }
                let fog = distance_fog(dist);
                color = shade_color(color, fog);
                self.frame.set(x as i32, y, color);
            }
        }

        self.render_sprites();
        self.render_weapon();
        self.render_crosshair();
        self.render_hud();
        self.render_minimap();
    }

    fn render_sprites(&mut self) {
        let mut order: Vec<(usize, i64)> = self
            .enemies
            .iter()
            .enumerate()
            .filter(|(_, enemy)| enemy.alive())
            .map(|(idx, enemy)| {
                let dx = enemy.x - self.player.x;
                let dy = enemy.y - self.player.y;
                (idx, (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64))
            })
            .collect();
        order.sort_by(|a, b| b.1.cmp(&a.1));

        let det = mul_fixed(self.player.plane_x, self.player.dir_y)
            - mul_fixed(self.player.dir_x, self.player.plane_y);
        if det == 0 {
            return;
        }
        let inv_det = ((FIX_ONE as i64 * FIX_ONE as i64) / det as i64) as i32;

        for (idx, _) in order {
            let enemy = self.enemies[idx];
            let rel_x = enemy.x - self.player.x;
            let rel_y = enemy.y - self.player.y;
            let transform_x =
                mul_fixed(inv_det, mul_fixed(self.player.dir_y, rel_x) - mul_fixed(self.player.dir_x, rel_y));
            let transform_y = mul_fixed(
                inv_det,
                -mul_fixed(self.player.plane_y, rel_x) + mul_fixed(self.player.plane_x, rel_y),
            );
            if transform_y <= FIX_ONE / 5 {
                continue;
            }
            let screen_x = ((INTERNAL_W as i32 / 2) * (FIX_ONE + fixed_div(transform_x, transform_y)))
                >> FIX_SHIFT;
            let sprite_h = max(4, (INTERNAL_H as i32 * FIX_ONE / transform_y) as i32);
            let sprite_w = sprite_h;
            let draw_start_y = max(0, INTERNAL_H as i32 / 2 - sprite_h / 2);
            let draw_end_y = min(INTERNAL_H as i32 - 1, INTERNAL_H as i32 / 2 + sprite_h / 2);
            let draw_start_x = max(0, screen_x - sprite_w / 2);
            let draw_end_x = min(INTERNAL_W as i32 - 1, screen_x + sprite_w / 2);

            for x in draw_start_x..=draw_end_x {
                let tex_x =
                    (((x - (screen_x - sprite_w / 2)) * TEX_SIZE as i32) / max(1, sprite_w)) as usize;
                if transform_y >= self.depth[x as usize] {
                    continue;
                }
                for y in draw_start_y..=draw_end_y {
                    let tex_y = (((y - draw_start_y) * TEX_SIZE as i32)
                        / max(1, draw_end_y - draw_start_y + 1)) as usize;
                    if let Some(mut color) = enemy_texture(tex_x, tex_y, enemy.hp) {
                        color = shade_color(color, distance_fog(transform_y));
                        self.frame.set(x, y, color);
                    }
                }
            }
        }
    }

    fn render_crosshair(&mut self) {
        let cx = INTERNAL_W as i32 / 2;
        let cy = INTERNAL_H as i32 / 2;
        self.frame.draw_vline(cx, cy - 3, cy + 3, COLOR_WHITE);
        self.frame.fill_rect(cx - 3, cy, 7, 1, COLOR_WHITE);
    }

    fn render_weapon(&mut self) {
        let base_x = INTERNAL_W as i32 / 2 - 18;
        let base_y = INTERNAL_H as i32 - 20;
        self.frame.fill_rect(base_x, base_y, 36, 16, 0xff4d_3530);
        self.frame.fill_rect(base_x + 6, base_y + 2, 24, 8, 0xff6c_5b_56);
        self.frame.fill_rect(base_x + 12, base_y + 10, 10, 6, 0xff2f_1f_1b);
        if self.player.flash > 0 {
            self.frame.fill_rect(base_x + 14, base_y - 6, 8, 8, COLOR_AMMO);
            self.frame.fill_rect(base_x + 10, base_y - 2, 16, 4, shade_color(COLOR_AMMO, 210));
        }
    }

    fn render_hud(&mut self) {
        self.frame.fill_rect(0, INTERNAL_H as i32 - 12, INTERNAL_W as i32, 12, COLOR_PANEL);
        draw_bar(&mut self.frame, 6, INTERNAL_H as i32 - 9, 42, 4, self.player.hp, 100, COLOR_BLOOD);
        draw_bar(&mut self.frame, 58, INTERNAL_H as i32 - 9, 34, 4, self.player.ammo, 99, COLOR_AMMO);
        self.frame.draw_text(6, INTERNAL_H as i32 - 18, "HP", 2, COLOR_HUD);
        self.frame.draw_text(58, INTERNAL_H as i32 - 18, "AMMO", 2, COLOR_HUD);
        self.frame.draw_text(108, INTERNAL_H as i32 - 18, "KILLS", 2, COLOR_HUD);

        let mut buf = [0u8; 12];
        let hp_str = write_num(&mut buf, self.player.hp.max(0));
        self.frame.draw_text(26, INTERNAL_H as i32 - 18, hp_str, 2, COLOR_WHITE);
        let ammo_str = write_num(&mut buf, self.player.ammo.max(0));
        self.frame
            .draw_text(90, INTERNAL_H as i32 - 18, ammo_str, 2, COLOR_WHITE);
        let kill_str = write_num(&mut buf, self.kills as i32);
        self.frame
            .draw_text(136, INTERNAL_H as i32 - 18, kill_str, 2, COLOR_WHITE);
    }

    fn render_minimap(&mut self) {
        let scale = 3;
        let ox = 6;
        let oy = 6;
        for y in 0..self.level.height {
            for x in 0..self.level.width {
                let tile = self.level.tiles[y * self.level.width + x];
                let color = if tile == 0 {
                    shade_color(0xff1a_1a_1a, 180)
                } else {
                    wall_texture(tile, 0, 0)
                };
                self.frame.fill_rect(
                    ox + x as i32 * scale,
                    oy + y as i32 * scale,
                    scale,
                    scale,
                    color,
                );
            }
        }
        for enemy in self.enemies.iter().filter(|enemy| enemy.alive()) {
            self.frame.fill_rect(
                ox + ((enemy.x >> FIX_SHIFT) * scale),
                oy + ((enemy.y >> FIX_SHIFT) * scale),
                scale,
                scale,
                COLOR_AMMO,
            );
        }
        self.frame.fill_rect(
            ox + ((self.player.x >> FIX_SHIFT) * scale),
            oy + ((self.player.y >> FIX_SHIFT) * scale),
            scale,
            scale,
            COLOR_WHITE,
        );
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut info = FramebufferInfo::default();
    if framebuffer_get_info(&mut info) != 0 {
        user_lib::println!("doom: framebuffer unavailable, switching to shell");
        let _ = exec("user_shell");
        return 1;
    }
    let level = match load_level("doom_level.txt\0") {
        Ok(level) => level,
        Err(err) => {
            user_lib::println!("doom: {err}");
            let _ = exec("user_shell");
            return 1;
        }
    };

    let mut game = Game::new(level);
    let mut next_frame = get_time();
    loop {
        game.poll_input();
        let now = get_time();
        if now < next_frame {
            sched_yield();
            continue;
        }
        let mut steps = 0;
        while next_frame <= now && steps < 3 {
            game.update();
            next_frame += FRAME_MS;
            steps += 1;
        }
        game.render();
        if framebuffer_present(&game.frame.pixels, INTERNAL_W, INTERNAL_H) != 0 {
            user_lib::println!("doom: framebuffer present failed");
            exit(1);
        }
    }
}

fn load_level(path: &str) -> Result<Level, &'static str> {
    let fd = open(path, OpenFlags::RDONLY);
    if fd < 0 {
        return Err("failed to open doom_level.txt");
    }
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let len = read(fd as usize, &mut chunk);
        if len < 0 {
            let _ = close(fd as usize);
            return Err("failed to read doom_level.txt");
        }
        if len == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..len as usize]);
    }
    let _ = close(fd as usize);
    let text = core::str::from_utf8(bytes.as_slice()).map_err(|_| "level is not valid utf-8")?;
    parse_level(text)
}

fn parse_level(text: &str) -> Result<Level, &'static str> {
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let width = lines.first().map(|line| line.len()).ok_or("empty level file")?;
    let height = lines.len();
    let mut tiles = vec![0u8; width * height];
    let mut start_x = None;
    let mut start_y = None;
    let mut enemies = Vec::new();

    for (y, line) in lines.iter().enumerate() {
        if line.len() != width {
            return Err("inconsistent level width");
        }
        for (x, ch) in line.bytes().enumerate() {
            let idx = y * width + x;
            match ch {
                b'.' => {}
                b'S' => {
                    start_x = Some(((x as i32) << FIX_SHIFT) + HALF_FIX);
                    start_y = Some(((y as i32) << FIX_SHIFT) + HALF_FIX);
                }
                b'E' => enemies.push(Enemy {
                    x: ((x as i32) << FIX_SHIFT) + HALF_FIX,
                    y: ((y as i32) << FIX_SHIFT) + HALF_FIX,
                    hp: 2,
                    cooldown: 0,
                }),
                b'1'..=b'9' => tiles[idx] = ch - b'0',
                b'#' => tiles[idx] = 1,
                _ => return Err("unsupported level character"),
            }
        }
    }

    Ok(Level {
        width,
        height,
        tiles,
        start_x: start_x.ok_or("missing S start tile")?,
        start_y: start_y.ok_or("missing S start tile")?,
        enemies,
    })
}

fn rotate_player(player: &mut Player, cos: i32, sin: i32) {
    let dir_x = player.dir_x;
    let dir_y = player.dir_y;
    player.dir_x = mul_fixed(dir_x, cos) - mul_fixed(dir_y, sin);
    player.dir_y = mul_fixed(dir_x, sin) + mul_fixed(dir_y, cos);
    let plane_x = player.plane_x;
    let plane_y = player.plane_y;
    player.plane_x = mul_fixed(plane_x, cos) - mul_fixed(plane_y, sin);
    player.plane_y = mul_fixed(plane_x, sin) + mul_fixed(plane_y, cos);
}

fn try_move(level: &Level, x: &mut i32, y: &mut i32, dx: i32, dy: i32) {
    let radius = FIX_ONE / 5;
    let nx = *x + dx;
    if !level.is_wall_fixed(nx - radius, *y)
        && !level.is_wall_fixed(nx + radius, *y)
        && !level.is_wall_fixed(nx, *y - radius)
        && !level.is_wall_fixed(nx, *y + radius)
    {
        *x = nx;
    }
    let ny = *y + dy;
    if !level.is_wall_fixed(*x - radius, ny)
        && !level.is_wall_fixed(*x + radius, ny)
        && !level.is_wall_fixed(*x, ny - radius)
        && !level.is_wall_fixed(*x, ny + radius)
    {
        *y = ny;
    }
}

fn wall_between(level: &Level, ax: i32, ay: i32, bx: i32, by: i32) -> bool {
    let dx = bx - ax;
    let dy = by - ay;
    let steps = max(dx.abs(), dy.abs()) / (FIX_ONE / 4);
    let steps = max(1, steps);
    let step_x = dx / steps;
    let step_y = dy / steps;
    let mut x = ax;
    let mut y = ay;
    for _ in 0..steps {
        x += step_x;
        y += step_y;
        if level.is_wall_fixed(x, y) {
            return true;
        }
    }
    false
}

fn mul_fixed(a: i32, b: i32) -> i32 {
    (((a as i64) * (b as i64)) >> FIX_SHIFT) as i32
}

fn fixed_div(a: i32, b: i32) -> i32 {
    (((a as i64) << FIX_SHIFT) / max(1, b as i64)) as i32
}

fn inv_abs(v: i32) -> i32 {
    if v == 0 {
        i32::MAX / 4
    } else {
        (((FIX_ONE as i64) * (FIX_ONE as i64)) / v.abs() as i64) as i32
    }
}

fn clamp_signed(delta: i32, max_step: i32) -> i32 {
    if delta > max_step {
        max_step
    } else if delta < -max_step {
        -max_step
    } else {
        delta
    }
}

fn just_pressed(now: bool, prev: &mut bool) -> bool {
    let pressed = now && !*prev;
    *prev = now;
    pressed
}

fn wall_texture(tile: u8, tx: usize, ty: usize) -> u32 {
    let base = match tile {
        1 => 0xff7c_332d,
        2 => 0xff32_5f_8f,
        3 => 0xff3d_7f_40,
        4 => 0xff91_7c_35,
        _ => 0xff55_5555,
    };
    if tx == 0 || tx == TEX_SIZE - 1 || ty == 0 || ty == TEX_SIZE - 1 {
        return shade_color(base, 105);
    }
    let checker = ((tx / 4) + (ty / 4) + tile as usize) & 1;
    if checker == 0 {
        shade_color(base, 220)
    } else {
        shade_color(base, 170)
    }
}

fn enemy_texture(tx: usize, ty: usize, hp: i32) -> Option<u32> {
    let body = if hp > 1 { 0xffb0_2f_2f } else { 0xff7a_2020 };
    let face = 0xffd4_b070;
    let eye = COLOR_WHITE;
    let in_head = (4..=11).contains(&tx) && (2..=9).contains(&ty);
    let in_body = (3..=12).contains(&tx) && (8..=15).contains(&ty);
    if !in_head && !in_body {
        return None;
    }
    if in_head && (tx == 6 || tx == 9) && (4..=5).contains(&ty) {
        return Some(eye);
    }
    if in_head {
        return Some(if ty < 7 { face } else { shade_color(face, 190) });
    }
    Some(if tx == 3 || tx == 12 {
        shade_color(body, 160)
    } else {
        body
    })
}

fn distance_fog(dist: i32) -> u8 {
    let fog = 255 - min(170, (dist >> 5) as usize) as i32;
    fog.max(80) as u8
}

fn shade_color(color: u32, amount: u8) -> u32 {
    let r = ((color >> 16) & 0xff) as u32 * amount as u32 / 255;
    let g = ((color >> 8) & 0xff) as u32 * amount as u32 / 255;
    let b = (color & 0xff) as u32 * amount as u32 / 255;
    0xff00_0000 | (r << 16) | (g << 8) | b
}

fn draw_bar(frame: &mut Frame, x: i32, y: i32, w: i32, h: i32, value: i32, max_value: i32, color: u32) {
    frame.fill_rect(x - 1, y - 1, w + 2, h + 2, shade_color(COLOR_WHITE, 80));
    frame.fill_rect(x, y, w, h, shade_color(COLOR_BLACK, 180));
    let fill = max(0, min(w, w * value.max(0) / max_value.max(1)));
    frame.fill_rect(x, y, fill, h, color);
}

fn draw_doom_logo(frame: &mut Frame, x: i32, y: i32, scale: i32) {
    frame.draw_text(x, y, "DOOM", scale, 0xffe2_9f_3c);
    frame.draw_text(x + 2, y + 2, "DOOM", scale, shade_color(0xff6a_1d_0f, 180));
}

fn draw_glyph(frame: &mut Frame, x: i32, y: i32, ch: u8, scale: i32, color: u32) {
    let glyph = glyph_3x5(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..3 {
            if (bits >> (2 - col)) & 1 == 1 {
                frame.fill_rect(
                    x + col * scale,
                    y + row as i32 * scale,
                    scale,
                    scale,
                    color,
                );
            }
        }
    }
}

fn glyph_3x5(ch: u8) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        b'A' => [0b111, 0b101, 0b111, 0b101, 0b101],
        b'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        b'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        b'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        b'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        b'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        b'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        b'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        b'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        b'J' => [0b001, 0b001, 0b001, 0b101, 0b111],
        b'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        b'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        b'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        b'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        b'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        b'Q' => [0b111, 0b101, 0b101, 0b111, 0b001],
        b'R' => [0b111, 0b101, 0b111, 0b110, 0b101],
        b'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        b'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        b'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        b'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        b'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        b'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        b'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        b'-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        _ => [0b000, 0b000, 0b000, 0b000, 0b000],
    }
}

fn write_num<'a>(buf: &'a mut [u8; 12], mut value: i32) -> &'a str {
    if value == 0 {
        buf[0] = b'0';
        return core::str::from_utf8(&buf[..1]).unwrap();
    }
    let neg = value < 0;
    if neg {
        value = -value;
    }
    let mut tmp = [0u8; 12];
    let mut len = 0;
    while value > 0 {
        tmp[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    let mut out = 0;
    if neg {
        buf[out] = b'-';
        out += 1;
    }
    while len > 0 {
        len -= 1;
        buf[out] = tmp[len];
        out += 1;
    }
    core::str::from_utf8(&buf[..out]).unwrap()
}
