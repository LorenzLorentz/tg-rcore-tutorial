//! 图形化 PingPong 游戏内核模块。
//!
//! 本模块承担三类职责：
//! - 将 `tg-rcore-tutorial-gfx` 接到 ch5 内核页表/MMIO 环境
//! - 轮询 VirtIO 键盘输入，并在无键盘时回退到 UART
//! - 维护球、球拍、比分等游戏状态，并负责每帧渲染

use crate::{KERNEL_SPACE, Sv39, build_flags};
use alloc::alloc::{alloc_zeroed, dealloc};
use core::{alloc::Layout, ptr::NonNull};
use riscv::register::time;
use spin::{Lazy, Mutex};
use tg_console::log;
use tg_kernel_vm::page_table::{MmuMeta, VAddr, VmFlags};
use tg_rcore_tutorial_gfx::{Canvas, Color, DisplayDriver, Point, VIRTIO_GPU_MMIO_BASE};
use tg_task_manage::ProcId;
use virtio_drivers::{Hal, InputEvent, MmioTransport, VirtIOHeader, VirtIOInput};

const UART_BASE: usize = 0x1000_0000;
const UART_LSR: usize = UART_BASE + 5;
const VIRTIO_INPUT_MMIO_BASE: usize = VIRTIO_GPU_MMIO_BASE + 0x1000;

const INPUT_EVENT_KEY: u16 = 0x01;
const INPUT_VALUE_RELEASE: u32 = 0;

const KEY_Q: u16 = 16;
const KEY_W: u16 = 17;
const KEY_R: u16 = 19;
const KEY_O: u16 = 24;
const KEY_S: u16 = 31;
const KEY_L: u16 = 38;

const ROLE_HOST: usize = 0;
const ROLE_LEFT: usize = 1;
const ROLE_RIGHT: usize = 2;

/// 自定义 syscall：注册 host/player 角色。
pub const SYSCALL_ATTACH: usize = 0x500;
/// 自定义 syscall：推进 host/player 的一次 tick。
pub const SYSCALL_TICK: usize = 0x501;

/// `tick()` 返回值：等待玩家加入。
pub const PHASE_LOBBY: isize = 0;
/// `tick()` 返回值：正常运行中。
pub const PHASE_RUNNING: isize = 1;
/// `tick()` 返回值：本局结束，等待 `r` 重开或 `q` 退出。
pub const PHASE_GAME_OVER: isize = 2;
/// `tick()` 返回值：退出当前会话。
pub const PHASE_QUIT: isize = 3;

const LOGICAL_W: i32 = 320;
const LOGICAL_H: i32 = 180;
const COURT_MARGIN: i32 = 20;
const PADDLE_W: i32 = 5;
const PADDLE_H: i32 = 28;
const PADDLE_STEP: i32 = 6;
const BALL_RADIUS: i32 = 3;
const BALL_SPEED_X: i32 = 3 << 8;
const BALL_SPEED_Y: i32 = 2 << 8;
const WIN_SCORE: u8 = 7;

const BG: Color = Color::rgb(10, 18, 33);
const PANEL: Color = Color::rgb(22, 37, 64);
const GRID: Color = Color::rgb(46, 75, 120);
const LEFT_COLOR: Color = Color::rgb(255, 145, 87);
const RIGHT_COLOR: Color = Color::rgb(104, 230, 174);
const BALL_COLOR: Color = Color::rgb(255, 244, 132);
const WINNER: Color = Color::rgb(255, 214, 92);
const OFFLINE: Color = Color::rgb(74, 96, 132);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Lobby,
    Running,
    GameOver,
    Quit,
}

#[derive(Clone, Copy)]
struct PlayerSlot {
    pid: Option<ProcId>,
    up_key: u16,
    down_key: u16,
    up_pressed: bool,
    down_pressed: bool,
    paddle_y: i32,
}

impl PlayerSlot {
    const fn empty(default_y: i32) -> Self {
        Self {
            pid: None,
            up_key: 0,
            down_key: 0,
            up_pressed: false,
            down_pressed: false,
            paddle_y: default_y,
        }
    }

    fn clear(&mut self, default_y: i32) {
        *self = Self::empty(default_y);
    }

    fn attach(&mut self, pid: ProcId, up_key: u8, down_key: u8, default_y: i32) {
        self.pid = Some(pid);
        self.up_key = ascii_to_keycode(up_key);
        self.down_key = ascii_to_keycode(down_key);
        self.up_pressed = false;
        self.down_pressed = false;
        self.paddle_y = default_y;
    }
}

struct Keyboard {
    inner: VirtIOInput<KernelHal, MmioTransport>,
}

unsafe impl Send for Keyboard {}
unsafe impl Sync for Keyboard {}

struct PingPongKernel {
    host: Option<ProcId>,
    players: [PlayerSlot; 2],
    phase: Phase,
    left_score: u8,
    right_score: u8,
    winner: Option<usize>,
    ball_x_fp: i32,
    ball_y_fp: i32,
    ball_vx_fp: i32,
    ball_vy_fp: i32,
    last_host_tick_ms: usize,
}

impl PingPongKernel {
    const fn new() -> Self {
        let default_y = (LOGICAL_H - PADDLE_H) / 2;
        Self {
            host: None,
            players: [PlayerSlot::empty(default_y), PlayerSlot::empty(default_y)],
            phase: Phase::Lobby,
            left_score: 0,
            right_score: 0,
            winner: None,
            ball_x_fp: (LOGICAL_W / 2) << 8,
            ball_y_fp: (LOGICAL_H / 2) << 8,
            ball_vx_fp: BALL_SPEED_X,
            ball_vy_fp: BALL_SPEED_Y,
            last_host_tick_ms: 0,
        }
    }

    fn session_reset(&mut self, clear_players: bool) {
        let default_y = (LOGICAL_H - PADDLE_H) / 2;
        self.phase = Phase::Lobby;
        self.left_score = 0;
        self.right_score = 0;
        self.winner = None;
        self.last_host_tick_ms = 0;
        if clear_players {
            self.players[0].clear(default_y);
            self.players[1].clear(default_y);
        } else {
            self.players[0].paddle_y = default_y;
            self.players[1].paddle_y = default_y;
            self.players[0].up_pressed = false;
            self.players[0].down_pressed = false;
            self.players[1].up_pressed = false;
            self.players[1].down_pressed = false;
        }
        self.reset_round(true);
    }

    fn reset_round(&mut self, serve_left_to_right: bool) {
        self.ball_x_fp = (LOGICAL_W / 2) << 8;
        self.ball_y_fp = (LOGICAL_H / 2) << 8;
        self.ball_vx_fp = if serve_left_to_right {
            BALL_SPEED_X
        } else {
            -BALL_SPEED_X
        };
        self.ball_vy_fp = BALL_SPEED_Y;
    }

    fn attach_host(&mut self, pid: ProcId) -> isize {
        if let Some(host) = self.host {
            if host != pid {
                return -1;
            }
        } else {
            self.host = Some(pid);
            self.session_reset(true);
        }
        self.render();
        self.phase_code()
    }

    fn attach_player(&mut self, pid: ProcId, role: usize, up_key: u8, down_key: u8) -> isize {
        let Some(slot) = self.player_slot_mut(role) else {
            return -1;
        };
        if let Some(owner) = slot.pid {
            if owner != pid {
                return -1;
            }
        } else {
            slot.attach(pid, up_key, down_key, (LOGICAL_H - PADDLE_H) / 2);
        }
        self.render();
        self.phase_code()
    }

    fn tick_host(&mut self, pid: ProcId) -> isize {
        if self.host != Some(pid) {
            return -1;
        }
        self.poll_input();
        self.apply_player_motion();
        match self.phase {
            Phase::Quit => {}
            Phase::Lobby => {
                if self.players_ready() {
                    self.phase = Phase::Running;
                    self.last_host_tick_ms = now_ms();
                }
            }
            Phase::Running => self.update_ball(),
            Phase::GameOver => {}
        }
        self.render();
        self.phase_code()
    }

    fn tick_player(&mut self, pid: ProcId, role: usize) -> isize {
        let Some(slot) = self.player_slot_mut(role) else {
            return -1;
        };
        if slot.pid != Some(pid) {
            return -1;
        }
        self.phase_code()
    }

    fn detach(&mut self, pid: ProcId) {
        if self.host == Some(pid) {
            self.host = None;
            self.session_reset(true);
            self.render();
            return;
        }
        for slot in &mut self.players {
            if slot.pid == Some(pid) {
                slot.clear((LOGICAL_H - PADDLE_H) / 2);
                if self.phase == Phase::Quit {
                    return;
                }
                self.phase = Phase::Lobby;
                self.winner = None;
                self.last_host_tick_ms = 0;
                self.reset_round(true);
                self.render();
                return;
            }
        }
    }

    fn poll_input(&mut self) {
        if keyboard_available() {
            while let Some(event) = keyboard_pop_pending_event() {
                self.handle_keyboard_event(event);
            }
        } else {
            while let Some(ch) = uart_getchar_nonblocking() {
                self.handle_ascii_input(ascii_lower(ch));
            }
        }
    }

    fn handle_keyboard_event(&mut self, event: InputEvent) {
        if event.event_type != INPUT_EVENT_KEY {
            return;
        }
        let pressed = event.value != INPUT_VALUE_RELEASE;
        match event.code {
            KEY_Q if pressed => {
                self.phase = Phase::Quit;
                return;
            }
            KEY_R if pressed => {
                self.session_reset(false);
                return;
            }
            _ => {}
        }

        for slot in &mut self.players {
            if slot.pid.is_none() {
                continue;
            }
            if event.code == slot.up_key {
                slot.up_pressed = pressed;
            } else if event.code == slot.down_key {
                slot.down_pressed = pressed;
            }
        }
    }

    fn handle_ascii_input(&mut self, ch: u8) {
        match ch {
            b'q' => {
                self.phase = Phase::Quit;
                return;
            }
            b'r' => {
                self.session_reset(false);
                return;
            }
            _ => {}
        }

        for slot in &mut self.players {
            if slot.pid.is_none() {
                continue;
            }
            let delta = if ch == keycode_to_ascii(slot.up_key) {
                -PADDLE_STEP
            } else if ch == keycode_to_ascii(slot.down_key) {
                PADDLE_STEP
            } else {
                0
            };
            if delta != 0 {
                slot.paddle_y = (slot.paddle_y + delta).clamp(0, LOGICAL_H - PADDLE_H);
            }
        }
    }

    fn apply_player_motion(&mut self) {
        for slot in &mut self.players {
            if slot.pid.is_none() {
                continue;
            }
            let delta = match (slot.up_pressed, slot.down_pressed) {
                (true, false) => -PADDLE_STEP,
                (false, true) => PADDLE_STEP,
                _ => 0,
            };
            if delta != 0 {
                slot.paddle_y = (slot.paddle_y + delta).clamp(0, LOGICAL_H - PADDLE_H);
            }
        }
    }

    fn update_ball(&mut self) {
        if !self.players_ready() {
            self.phase = Phase::Lobby;
            return;
        }

        let now = now_ms();
        let ticks = if self.last_host_tick_ms == 0 {
            1
        } else {
            (now.saturating_sub(self.last_host_tick_ms) / 12).clamp(1, 4)
        };
        self.last_host_tick_ms = now;

        for _ in 0..ticks {
            self.ball_x_fp += self.ball_vx_fp;
            self.ball_y_fp += self.ball_vy_fp;

            let ball_y = self.ball_y_fp >> 8;
            if ball_y - BALL_RADIUS <= 0 {
                self.ball_y_fp = BALL_RADIUS << 8;
                self.ball_vy_fp = self.ball_vy_fp.abs();
            } else if ball_y + BALL_RADIUS >= LOGICAL_H {
                self.ball_y_fp = (LOGICAL_H - BALL_RADIUS) << 8;
                self.ball_vy_fp = -self.ball_vy_fp.abs();
            }

            let ball_x = self.ball_x_fp >> 8;
            let left_x = 12;
            let right_x = LOGICAL_W - 12 - PADDLE_W;

            if self.ball_vx_fp < 0
                && ball_x - BALL_RADIUS <= left_x + PADDLE_W
                && ball_x + BALL_RADIUS >= left_x
                && ball_y + BALL_RADIUS >= self.players[0].paddle_y
                && ball_y - BALL_RADIUS <= self.players[0].paddle_y + PADDLE_H
            {
                self.ball_x_fp = (left_x + PADDLE_W + BALL_RADIUS) << 8;
                self.ball_vx_fp = self.ball_vx_fp.abs() + (1 << 6);
                self.reflect_from_paddle(self.players[0].paddle_y);
            } else if self.ball_vx_fp > 0
                && ball_x + BALL_RADIUS >= right_x
                && ball_x - BALL_RADIUS <= right_x + PADDLE_W
                && ball_y + BALL_RADIUS >= self.players[1].paddle_y
                && ball_y - BALL_RADIUS <= self.players[1].paddle_y + PADDLE_H
            {
                self.ball_x_fp = (right_x - BALL_RADIUS) << 8;
                self.ball_vx_fp = -self.ball_vx_fp.abs() - (1 << 6);
                self.reflect_from_paddle(self.players[1].paddle_y);
            }

            let ball_x = self.ball_x_fp >> 8;
            if ball_x < -BALL_RADIUS {
                self.right_score = self.right_score.saturating_add(1);
                if self.right_score >= WIN_SCORE {
                    self.phase = Phase::GameOver;
                    self.winner = Some(1);
                } else {
                    self.reset_round(false);
                }
                break;
            } else if ball_x > LOGICAL_W + BALL_RADIUS {
                self.left_score = self.left_score.saturating_add(1);
                if self.left_score >= WIN_SCORE {
                    self.phase = Phase::GameOver;
                    self.winner = Some(0);
                } else {
                    self.reset_round(true);
                }
                break;
            }
        }
    }

    fn reflect_from_paddle(&mut self, paddle_y: i32) {
        let ball_center = self.ball_y_fp >> 8;
        let paddle_center = paddle_y + PADDLE_H / 2;
        let offset = (ball_center - paddle_center).clamp(-PADDLE_H / 2, PADDLE_H / 2);
        self.ball_vy_fp = offset * 24;
        if self.ball_vy_fp == 0 {
            self.ball_vy_fp = BALL_SPEED_Y;
        }
    }

    fn players_ready(&self) -> bool {
        self.players[0].pid.is_some() && self.players[1].pid.is_some()
    }

    fn player_slot_mut(&mut self, role: usize) -> Option<&mut PlayerSlot> {
        match role {
            ROLE_LEFT => Some(&mut self.players[0]),
            ROLE_RIGHT => Some(&mut self.players[1]),
            _ => None,
        }
    }

    fn phase_code(&self) -> isize {
        match self.phase {
            Phase::Lobby => PHASE_LOBBY,
            Phase::Running => PHASE_RUNNING,
            Phase::GameOver => PHASE_GAME_OVER,
            Phase::Quit => PHASE_QUIT,
        }
    }

    fn render(&self) {
        let mut display_guard = DISPLAY.lock();
        let Some(display) = display_guard.as_mut() else {
            return;
        };

        let width = display.width() as i32;
        let height = display.height() as i32;
        let scale = ((width - COURT_MARGIN * 2) / LOGICAL_W)
            .min((height - COURT_MARGIN * 2) / LOGICAL_H)
            .max(1);
        let court_w = LOGICAL_W * scale;
        let court_h = LOGICAL_H * scale;
        let origin = Point::new((width - court_w) / 2, (height - court_h) / 2);

        let mut canvas = display.canvas();
        canvas.clear(BG);
        canvas.fill_rect(
            Point::new(origin.x - 12, origin.y - 12),
            court_w + 24,
            court_h + 24,
            PANEL,
        );
        canvas.stroke_rect(origin, court_w, court_h, scale.max(1), GRID);

        for y in 0..9 {
            if y % 2 == 0 {
                canvas.fill_rect(
                    Point::new(
                        origin.x + court_w / 2 - scale / 2,
                        origin.y + 10 * scale + y * 18 * scale,
                    ),
                    scale.max(1),
                    10 * scale,
                    GRID,
                );
            }
        }

        draw_digit(
            &mut canvas,
            origin.x + court_w / 2 - 22 * scale,
            origin.y + 8 * scale,
            self.left_score,
            scale,
            if self.winner == Some(0) {
                WINNER
            } else {
                LEFT_COLOR
            },
        );
        draw_digit(
            &mut canvas,
            origin.x + court_w / 2 + 10 * scale,
            origin.y + 8 * scale,
            self.right_score,
            scale,
            if self.winner == Some(1) {
                WINNER
            } else {
                RIGHT_COLOR
            },
        );

        let left_status = if self.players[0].pid.is_some() {
            LEFT_COLOR
        } else {
            OFFLINE
        };
        let right_status = if self.players[1].pid.is_some() {
            RIGHT_COLOR
        } else {
            OFFLINE
        };
        canvas.fill_circle(
            Point::new(origin.x + 18 * scale, origin.y + 12 * scale),
            4 * scale,
            left_status,
        );
        canvas.fill_circle(
            Point::new(origin.x + court_w - 18 * scale, origin.y + 12 * scale),
            4 * scale,
            right_status,
        );

        draw_paddle(
            &mut canvas,
            origin,
            scale,
            12,
            self.players[0].paddle_y,
            LEFT_COLOR,
        );
        draw_paddle(
            &mut canvas,
            origin,
            scale,
            LOGICAL_W - 12 - PADDLE_W,
            self.players[1].paddle_y,
            RIGHT_COLOR,
        );

        if self.phase != Phase::Lobby || self.players_ready() {
            canvas.fill_circle(
                Point::new(
                    origin.x + ((self.ball_x_fp >> 8) * scale),
                    origin.y + ((self.ball_y_fp >> 8) * scale),
                ),
                BALL_RADIUS * scale,
                BALL_COLOR,
            );
        }

        if self.phase == Phase::GameOver {
            canvas.stroke_rect(
                Point::new(origin.x + 88 * scale, origin.y + 72 * scale),
                144 * scale,
                36 * scale,
                scale.max(2),
                WINNER,
            );
        }

        if let Err(err) = display.present() {
            log::error!("failed to present pingpong frame: {err:?}");
        }
    }
}

fn draw_paddle(canvas: &mut Canvas<'_>, origin: Point, scale: i32, x: i32, y: i32, color: Color) {
    canvas.fill_rect(
        Point::new(origin.x + x * scale, origin.y + y * scale),
        PADDLE_W * scale,
        PADDLE_H * scale,
        color,
    );
}

fn draw_digit(canvas: &mut Canvas<'_>, x: i32, y: i32, digit: u8, scale: i32, color: Color) {
    const SEGMENTS: [u8; 10] = [
        0b0011_1111,
        0b0000_0110,
        0b0101_1011,
        0b0100_1111,
        0b0110_0110,
        0b0110_1101,
        0b0111_1101,
        0b0000_0111,
        0b0111_1111,
        0b0110_1111,
    ];

    let mask = SEGMENTS[digit as usize % 10];
    let w = 10 * scale;
    let h = 20 * scale;
    let t = 2 * scale.max(1);

    if mask & 0b0000_0001 != 0 {
        canvas.fill_rect(Point::new(x, y), w, t, color);
    }
    if mask & 0b0000_0010 != 0 {
        canvas.fill_rect(Point::new(x + w - t, y), t, h / 2, color);
    }
    if mask & 0b0000_0100 != 0 {
        canvas.fill_rect(Point::new(x + w - t, y + h / 2), t, h / 2, color);
    }
    if mask & 0b0000_1000 != 0 {
        canvas.fill_rect(Point::new(x, y + h - t), w, t, color);
    }
    if mask & 0b0001_0000 != 0 {
        canvas.fill_rect(Point::new(x, y + h / 2), t, h / 2, color);
    }
    if mask & 0b0010_0000 != 0 {
        canvas.fill_rect(Point::new(x, y), t, h / 2, color);
    }
    if mask & 0b0100_0000 != 0 {
        canvas.fill_rect(Point::new(x, y + h / 2 - t / 2), w, t, color);
    }
}

#[repr(transparent)]
struct KernelHal;

impl Hal for KernelHal {
    fn dma_alloc(pages: usize) -> usize {
        unsafe {
            alloc_zeroed(Layout::from_size_align_unchecked(
                pages << Sv39::PAGE_BITS,
                1 << Sv39::PAGE_BITS,
            )) as usize
        }
    }

    fn dma_dealloc(paddr: usize, pages: usize) -> i32 {
        unsafe {
            dealloc(
                paddr as *mut u8,
                Layout::from_size_align_unchecked(pages << Sv39::PAGE_BITS, 1 << Sv39::PAGE_BITS),
            );
        }
        0
    }

    fn phys_to_virt(paddr: usize) -> usize {
        paddr
    }

    fn virt_to_phys(vaddr: usize) -> usize {
        const VALID: VmFlags<Sv39> = build_flags("__V");
        let ptr: NonNull<u8> = unsafe {
            KERNEL_SPACE
                .assume_init_ref()
                .translate(VAddr::new(vaddr), VALID)
                .unwrap()
        };
        ptr.as_ptr() as usize
    }
}

static DISPLAY: Lazy<Mutex<Option<DisplayDriver<KernelHal>>>> = Lazy::new(|| Mutex::new(None));
static INPUT: Lazy<Mutex<Option<Keyboard>>> = Lazy::new(|| Mutex::new(None));
static GAME: Lazy<Mutex<PingPongKernel>> = Lazy::new(|| Mutex::new(PingPongKernel::new()));

/// 需要映射到内核页表中的 MMIO 范围。
pub const MMIO: &[(usize, usize)] = &[
    (UART_BASE, 0x1000),
    (VIRTIO_GPU_MMIO_BASE, 0x1000),
    (VIRTIO_INPUT_MMIO_BASE, 0x1000),
];

/// 初始化 pingpong 的图形与键盘输入。
pub fn init() {
    init_input();
    match DisplayDriver::<KernelHal>::new(VIRTIO_GPU_MMIO_BASE) {
        Ok(display) => {
            *DISPLAY.lock() = Some(display);
            GAME.lock().render();
        }
        Err(err) => log::warn!("pingpong display unavailable: {err:?}"),
    }
}

fn init_input() {
    let Some(header) = NonNull::new(VIRTIO_INPUT_MMIO_BASE as *mut VirtIOHeader) else {
        return;
    };
    let transport = match unsafe { MmioTransport::new(header) } {
        Ok(transport) => transport,
        Err(err) => {
            log::warn!("pingpong keyboard transport unavailable: {err:?}");
            return;
        }
    };
    match VirtIOInput::<KernelHal, _>::new(transport) {
        Ok(input) => {
            *INPUT.lock() = Some(Keyboard { inner: input });
            log::info!("pingpong keyboard ready at {:#x}", VIRTIO_INPUT_MMIO_BASE);
        }
        Err(err) => log::warn!("pingpong keyboard init failed: {err:?}"),
    }
}

/// 分发 pingpong 的自定义 syscall。
pub fn handle_custom_syscall(pid: ProcId, id: usize, args: [usize; 6]) -> Option<isize> {
    let mut game = GAME.lock();
    match id {
        SYSCALL_ATTACH => Some(match args[0] {
            ROLE_HOST => game.attach_host(pid),
            ROLE_LEFT | ROLE_RIGHT => {
                game.attach_player(pid, args[0], args[1] as u8, args[2] as u8)
            }
            _ => -1,
        }),
        SYSCALL_TICK => Some(match args[0] {
            ROLE_HOST => game.tick_host(pid),
            ROLE_LEFT | ROLE_RIGHT => game.tick_player(pid, args[0]),
            _ => -1,
        }),
        _ => None,
    }
}

/// 在进程退出时清理其在 pingpong 会话中的注册状态。
pub fn detach_process(pid: ProcId) {
    GAME.lock().detach(pid);
}

fn ascii_lower(ch: u8) -> u8 {
    if ch.is_ascii_uppercase() { ch + 32 } else { ch }
}

fn ascii_to_keycode(ch: u8) -> u16 {
    match ascii_lower(ch) {
        b'q' => KEY_Q,
        b'w' => KEY_W,
        b'r' => KEY_R,
        b'o' => KEY_O,
        b's' => KEY_S,
        b'l' => KEY_L,
        _ => 0,
    }
}

fn keycode_to_ascii(code: u16) -> u8 {
    match code {
        KEY_Q => b'q',
        KEY_W => b'w',
        KEY_R => b'r',
        KEY_O => b'o',
        KEY_S => b's',
        KEY_L => b'l',
        _ => 0,
    }
}

fn now_ms() -> usize {
    time::read() / 12_500
}

fn keyboard_available() -> bool {
    INPUT.lock().is_some()
}

fn keyboard_pop_pending_event() -> Option<InputEvent> {
    let mut input = INPUT.lock();
    input.as_mut().and_then(|keyboard| keyboard.inner.pop_pending_event())
}

fn uart_getchar_nonblocking() -> Option<u8> {
    let lsr = unsafe { (UART_LSR as *const u8).read_volatile() };
    if lsr & 1 == 0 {
        None
    } else {
        Some(unsafe { (UART_BASE as *const u8).read_volatile() })
    }
}
