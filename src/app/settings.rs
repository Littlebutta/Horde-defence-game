//! The settings dialog: sound, interface and graphics. It opens over the menu or a game (pausing a
//! game played alone) and everything takes effect straight away; settings.txt is written on close.

use super::{App, Screen};
use crate::render::text;
use crate::ui::{self, pal, RectExt};
use macroquad::prelude::*;

const UI_SIZES: [(u32, &str); 4] = [(85, "Small"), (100, "Normal"), (115, "Large"), (130, "Huge")];
const RES: [(u32, &str); 3] = [(50, "50%"), (75, "75%"), (100, "Full")];

impl App {
    pub(super) fn open_settings(&mut self) {
        self.settings_open = true;
        // playing alone, the game waits while the settings are open
        if self.screen == Screen::Play && self.sess.is_none() && !self.paused {
            self.paused = true;
            self.settings_paused = true;
        }
    }
    pub(super) fn close_settings(&mut self) {
        self.settings_open = false;
        self.drag = None;
        if std::mem::take(&mut self.settings_paused) {
            self.paused = false;
        }
        self.save_settings();
    }
    pub(super) fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
        set_fullscreen(on);
        if !on {
            // coming back from fullscreen, Windows may leave the window screen-sized
            request_new_screen_size(1600.0, 900.0);
        }
        self.save_settings();
    }
    /// Drops the 3D view's render target so the next frame rebuilds it with the new settings.
    fn reset_gfx(&mut self) {
        self.depth = None;
        self.rt = None;
        self.regfx = true;
    }

    pub(super) fn settings(&mut self) {
        if is_key_pressed(KeyCode::Escape) {
            self.close_settings();
            return;
        }
        let p = pal(self.dark);
        self.backdrop();
        let r = ui::screen().centered(820.0, 600.0);
        self.panel(r);
        let mut a = r.pad(32.0, 28.0);
        let mut t = a.cut_top(36.0, 20.0);
        let close = t.cut_right(36.0, 0.0);
        text(&self.fonts.head, "Settings", t.x, t.y + 30.0, 30.0, p.text);
        if self.button(close, "×", true, false) {
            self.close_settings();
            return;
        }
        let mut foot = a.cut_bottom(44.0, 0.0);
        let cols = a.cols(2, 40.0);
        let (mut l, mut rr) = (cols[0], cols[1]);

        // ---- sound
        let y = self.heading("Sound", l.x, l.y);
        l.cut_top(y - l.y, 0.0);
        if let Some(v) = self.slider(l.cut_top(44.0, 14.0), 1, "Music", self.music_vol) {
            if v > 0 {
                self.music_prev = v;
            }
            self.music_vol = v;
            self.music.set_volume(v);
        }
        if let Some(v) = self.slider(l.cut_top(44.0, 14.0), 2, "Effects", self.sfx_vol) {
            self.set_sfx(v);
        }
        if let Some(v) = self.slider(l.cut_top(44.0, 6.0), 3, "News broadcasts", self.news_vol) {
            self.news_vol = v;
            self.music.set_news_volume(v);
        }
        let n = l.cut_top(18.0, 26.0);
        text(&self.fonts.mono, "M mutes the music during a game.", n.x, n.y + 12.0, 12.0, p.dim);

        // ---- interface
        let y = self.heading("Interface", l.x, l.y);
        l.cut_top(y - l.y, 0.0);
        let row = |l: &mut Rect, me: &App, label: &str| {
            let r = l.cut_top(22.0, 4.0);
            text(&me.fonts.head, label, r.x, r.y + 16.0, 15.0, p.text);
            l.cut_top(36.0, 14.0)
        };
        let cur = UI_SIZES.iter().position(|(v, _)| *v == ui::user_scale()).unwrap_or(1);
        let r = row(&mut l, self, "Interface size");
        if let Some(i) = self.segmented(r, &UI_SIZES.map(|s| s.1), cur) {
            ui::set_user_scale(UI_SIZES[i].0);
        }
        let r = row(&mut l, self, "HUD on ultrawide screens");
        if let Some(i) = self.segmented(r, &["Centred 21:9", "Full width"], ui::wide() as usize) {
            ui::set_wide(i == 1);
        }
        if self.hot(r) {
            self.tip = Some("Centred keeps the HUD within a 21:9 area in the middle of a 32:9 or other very wide screen, so you don't have to turn your head to read it.".into());
        }

        // ---- graphics
        let y = self.heading("Graphics", rr.x, rr.y);
        rr.cut_top(y - rr.y, 0.0);
        let r = row(&mut rr, self, "Display");
        if let Some(i) = self.segmented(r, &["Window", "Fullscreen"], self.fullscreen as usize) {
            self.set_fullscreen(i == 1);
        }
        if self.hot(r) {
            self.tip = Some("F11 switches at any time.".into());
        }
        let r = row(&mut rr, self, "Anti-aliasing");
        if let Some(i) = self.segmented(r, &["4× MSAA", "Off", "Compatible"], self.aa as usize) {
            if i as u8 != self.aa {
                self.aa = i as u8;
                self.reset_gfx();
            }
        }
        if self.hot(r) {
            self.tip = Some("4× MSAA smooths the edges of buildings. Off is faster. Compatible draws without a depth buffer, for graphics drivers where buildings show through each other or go missing.".into());
        }
        let r = row(&mut rr, self, "3D resolution");
        let cur = RES.iter().position(|(v, _)| *v == self.render_pct).unwrap_or(2);
        if let Some(i) = self.segmented(r, &RES.map(|s| s.1), cur) {
            if RES[i].0 != self.render_pct {
                self.render_pct = RES[i].0;
                self.rt = None;
            }
        }
        if self.hot(r) {
            self.tip = Some("Draws the city at a lower resolution for more frames per second on big screens. The HUD stays sharp. Not used with Compatible anti-aliasing.".into());
        }
        let r = rr.cut_top(36.0, 10.0);
        if self.toggle(r, "Show frame rate", self.show_fps) {
            self.show_fps = !self.show_fps;
        }

        // ---- footer
        let info = format!("{:.0} × {:.0} px · UI scale {:.2}", screen_width(), screen_height(), ui::scale());
        text(&self.fonts.mono, &info, foot.x, foot.y + 27.0, 12.0, p.dim);
        if self.button_primary(foot.cut_right(160.0, 0.0), "Done", true) {
            self.close_settings();
        }
    }
}
